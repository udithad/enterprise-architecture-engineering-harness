//! Retrieval: lexical (FTS5/BM25), vector (cosine), hybrid (reciprocal rank fusion),
//! deterministic reranking, visibility filtering, freshness and conflict flags,
//! mandatory standards with precedence.

use crate::model::*;
use crate::service::{KnowledgeService, emit};
use eaeh_core::{Classification, HarnessError, Result, TraceContext, Untrusted};
use eaeh_events::catalog::KnowledgeRetrieved;
use eaeh_extensibility::{HookContext, HookPayload, HookPoint};
use eaeh_intelligence::embedding::{cosine, from_bytes};
use eaeh_intelligence::{ContextItem, MemoryKind};
use eaeh_store::audit::{AuditEntry, AuditOutcome};
use eaeh_store::{DbResultExt, sqlx};
use serde_json::json;
use sqlx::Row;
use std::collections::{BTreeMap, BTreeSet, HashMap};

/// Visibility predicate over `knowledge_documents d`. Binds, in order: tenant,
/// max classification rank, domain, project, teams (JSON array), workflow.
macro_rules! visible {
    () => {
        "d.tenant_id = ? AND d.status = 'active' AND d.classification_rank <= ? AND ( \
         d.scope_level = 'enterprise' \
         OR (d.scope_level = 'domain' AND d.scope_ref = ?) \
         OR (d.scope_level = 'project' AND d.scope_ref = ?) \
         OR (d.scope_level = 'team' AND d.scope_ref IN (SELECT value FROM json_each(?))) \
         OR (d.scope_level = 'workflow' AND d.scope_ref = ?))"
    };
}

macro_rules! bind_visible {
    ($q:expr, $ctx:expr) => {
        $q.bind($ctx.tenant.to_string())
            .bind($ctx.max_classification.rank())
            .bind($ctx.domain.clone().unwrap_or_else(|| "\u{0}".into()))
            .bind(
                $ctx.project
                    .map(|p| p.to_string())
                    .unwrap_or_else(|| "\u{0}".into()),
            )
            .bind(serde_json::to_string(&$ctx.teams).unwrap_or_else(|_| "[]".into()))
            .bind(
                $ctx.workflow
                    .map(|w| w.to_string())
                    .unwrap_or_else(|| "\u{0}".into()),
            )
    };
}

/// Turn free text into a safe FTS5 query: alphanumeric terms only, each quoted, OR-ed.
/// FTS5 operators, column filters and quotes in the input can never reach `MATCH`.
pub fn sanitize_fts_query(text: &str) -> Option<String> {
    let mut seen = BTreeSet::new();
    let terms: Vec<String> = text
        .split(|c: char| !c.is_alphanumeric())
        .map(str::to_lowercase)
        .filter(|t| t.chars().count() >= 2 && seen.insert(t.clone()))
        .take(32)
        .map(|t| format!("\"{t}\""))
        .collect();
    if terms.is_empty() {
        None
    } else {
        Some(terms.join(" OR "))
    }
}

/// Reciprocal rank fusion of ranked id lists.
pub fn rrf(lists: &[Vec<String>], k: f64) -> Vec<(String, f64)> {
    let mut scores: HashMap<String, f64> = HashMap::new();
    for list in lists {
        for (rank, id) in list.iter().enumerate() {
            *scores.entry(id.clone()).or_default() += 1.0 / (k + rank as f64 + 1.0);
        }
    }
    let mut v: Vec<(String, f64)> = scores.into_iter().collect();
    v.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
    v
}

impl KnowledgeService {
    async fn lexical(&self, ctx: &RetrievalContext, text: &str) -> Result<Vec<String>> {
        let Some(q) = sanitize_fts_query(text) else {
            return Ok(vec![]);
        };
        let sql = concat!(
            "SELECT c.id FROM knowledge_chunks_fts f JOIN knowledge_chunks c ON c.rowid = f.rowid ",
            "JOIN knowledge_documents d ON d.id = c.document_id WHERE knowledge_chunks_fts MATCH ? AND ",
            visible!(),
            " ORDER BY bm25(knowledge_chunks_fts) LIMIT ?"
        );
        let q = sqlx::query(sql).bind(q);
        let q = bind_visible!(q, ctx).bind(i64::try_from(self.cfg.candidates).unwrap_or(50));
        let rows = q.fetch_all(self.db.pool()).await.db()?;
        rows.iter().map(|r| r.try_get::<String, _>(0).db()).collect()
    }

    async fn vector(&self, ctx: &RetrievalContext, text: &str) -> Result<Vec<String>> {
        let qv = self
            .embedder
            .embed(&[text.to_string()])
            .await?
            .into_iter()
            .next()
            .unwrap_or_default();
        let sql = concat!(
            "SELECT c.id, e.vector FROM knowledge_chunks c JOIN knowledge_documents d ON d.id = c.document_id ",
            "JOIN knowledge_embeddings e ON e.chunk_id = c.id WHERE e.embedder = ? AND ",
            visible!()
        );
        let q = sqlx::query(sql).bind(self.embedder.id().to_string());
        let rows = bind_visible!(q, ctx).fetch_all(self.db.pool()).await.db()?;
        let mut scored = vec![];
        for r in &rows {
            let id: String = r.try_get(0).db()?;
            let v = from_bytes(&r.try_get::<Vec<u8>, _>(1).db()?)?;
            let s = cosine(&qv, &v);
            if s > 0.0 {
                scored.push((id, s));
            }
        }
        scored.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        scored.truncate(self.cfg.candidates);
        Ok(scored.into_iter().map(|(id, _)| id).collect())
    }

    async fn hit(&self, chunk_id: &str, score: f64) -> Result<SearchHit> {
        let r = sqlx::query(
            "SELECT c.id, c.ordinal, c.heading, c.text, d.id AS doc_id, d.subject, d.version, d.title, d.content_hash, d.classification, \
             d.mandatory, d.review_by, d.tenant_id, s.name, s.kind, \
             (SELECT COUNT(*) FROM knowledge_conflicts k WHERE k.tenant_id = d.tenant_id AND k.subject = d.subject) AS n_conflicts \
             FROM knowledge_chunks c JOIN knowledge_documents d ON d.id = c.document_id JOIN knowledge_sources s ON s.id = d.source_id WHERE c.id = ?",
        )
        .bind(chunk_id)
        .fetch_one(self.db.pool())
        .await
        .db()?;
        let source: String = r.try_get("name").db()?;
        let subject: String = r.try_get("subject").db()?;
        let version: i64 = r.try_get("version").db()?;
        let ordinal: i64 = r.try_get("ordinal").db()?;
        let review_by: Option<String> = r.try_get("review_by").db()?;
        let stale = match &review_by {
            Some(t) => eaeh_store::parse_ts(t)? < self.db.now(),
            None => false,
        };
        Ok(SearchHit {
            citation: format!("{}#{ordinal}", citation_base(&source, &subject, version)),
            chunk_id: r.try_get("id").db()?,
            document_id: r.try_get("doc_id").db()?,
            source,
            source_kind: SourceKind::parse(&r.try_get::<String, _>("kind").db()?)?,
            subject,
            version,
            title: r.try_get("title").db()?,
            heading: r.try_get("heading").db()?,
            text: r.try_get("text").db()?,
            content_hash: r.try_get("content_hash").db()?,
            classification: Classification::parse(&r.try_get::<String, _>("classification").db()?)?,
            mandatory: r.try_get::<i64, _>("mandatory").db()? != 0,
            stale,
            conflict: r.try_get::<i64, _>("n_conflicts").db()? > 0,
            score,
        })
    }

    /// Governed search within a retrieval context. Results are cited, ranked
    /// deterministically, scope- and classification-filtered, and audited.
    pub async fn search(
        &self,
        ctx: &RetrievalContext,
        q: &SearchQuery,
        trace: TraceContext,
    ) -> Result<Vec<SearchHit>> {
        if q.text.len() > 4096 {
            return Err(HarnessError::invalid_input("query too long"));
        }
        let k = q.k.clamp(1, 50);
        let hook = |point| HookContext {
            point,
            scope: ctx.project.map(|p| eaeh_core::Scope::new(ctx.tenant, p)),
            trace,
            actor: Some(ctx.principal_id.clone()),
            subject: "knowledge.search".into(),
            payload: HookPayload::Knowledge {
                query: q.text.chars().take(200).collect(),
            },
        };
        self.hooks
            .dispatch(&hook(HookPoint::BeforeKnowledgeRetrieval))
            .await
            .into_result()?;
        let fused: Vec<(String, f64)> = match q.mode {
            SearchMode::Lexical => rrf(&[self.lexical(ctx, &q.text).await?], self.cfg.rrf_k),
            SearchMode::Vector => rrf(&[self.vector(ctx, &q.text).await?], self.cfg.rrf_k),
            SearchMode::Hybrid => {
                let l = self.lexical(ctx, &q.text).await?;
                let v = self.vector(ctx, &q.text).await?;
                rrf(&[l, v], self.cfg.rrf_k)
            }
        };
        let terms: BTreeSet<String> = q
            .text
            .split(|c: char| !c.is_alphanumeric())
            .filter(|t| t.len() >= 3)
            .map(str::to_lowercase)
            .collect();
        let mut hits = vec![];
        for (id, base) in fused.into_iter().take(self.cfg.candidates) {
            let mut h = self.hit(&id, base).await?;
            if !q.kinds.is_empty() && !q.kinds.contains(&h.source_kind) {
                continue;
            }
            if h.stale && !q.include_stale {
                continue;
            }
            let heading = h.heading.to_lowercase();
            let heading_match = terms.iter().any(|t| heading.contains(t.as_str()));
            // Relevance dominates: a heading match is a small nudge and staleness halves
            // the score; source authority (standard > adr > spec > reference) and the
            // mandatory flag only break ties (mandatory standards are injected into agent
            // context separately, so search need not favour them).
            let mut factor = if heading_match { 1.01 } else { 1.0 };
            if h.stale {
                factor *= 0.5;
            }
            h.score = (base * factor * 1e9).round() / 1e9;
            hits.push(h);
        }
        hits.sort_by(|a, b| {
            b.score
                .total_cmp(&a.score)
                .then(b.source_kind.authority().total_cmp(&a.source_kind.authority()))
                .then(b.mandatory.cmp(&a.mandatory))
                .then(a.citation.cmp(&b.citation))
        });
        hits.truncate(k);
        self.hooks
            .dispatch(&hook(HookPoint::AfterKnowledgeRetrieval))
            .await;
        let citations: Vec<&str> = hits.iter().map(|h| h.citation.as_str()).collect();
        let mut tx = self.db.write().await?;
        let mut entry = AuditEntry::system(
            "knowledge",
            "knowledge.search",
            "knowledge",
            ctx.principal_id.clone(),
            AuditOutcome::Recorded,
        )
        .correlation(trace.correlation)
        .details(json!({
            "principal": ctx.principal_id,
            "mode": q.mode.as_str(),
            "query_sha256": eaeh_core::hash::sha256_hex(q.text.as_bytes()),
            "max_classification": ctx.max_classification,
            "citations": citations,
        }));
        entry.tenant = Some(ctx.tenant);
        entry.project = ctx.project;
        self.db.audit_in(&mut tx, entry).await?;
        emit(
            &self.db,
            &mut tx,
            &KnowledgeRetrieved {
                hits: u32::try_from(hits.len()).unwrap_or(u32::MAX),
                mode: q.mode.as_str().into(),
                principal_id: ctx.principal_id.clone(),
            },
            None,
            trace,
        )
        .await?;
        tx.commit().await?;
        Ok(hits)
    }

    /// All chunks (in order) of one document `source/subject@vN` visible in `ctx` (Phase 3:
    /// BRD analysis needs the whole document, not a ranked subset). Invisible or unknown
    /// documents return `not_found` (no existence oracle across scopes). Audited.
    pub async fn read_document(
        &self,
        ctx: &RetrievalContext,
        citation: &str,
        trace: TraceContext,
    ) -> Result<Vec<SearchHit>> {
        let base = citation.split('#').next().unwrap_or(citation);
        let parsed = base.rsplit_once("@v").and_then(|(path, v)| {
            let (src, subject) = path.split_once('/')?;
            Some((src.to_string(), subject.to_string(), v.parse::<i64>().ok()?))
        });
        let Some((source, subject, version)) = parsed else {
            return Err(HarnessError::invalid_input(
                "document must be a citation 'source/subject@vN'",
            ));
        };
        let sql = concat!(
            "SELECT c.id FROM knowledge_chunks c JOIN knowledge_documents d ON d.id = c.document_id ",
            "JOIN knowledge_sources s ON s.id = d.source_id WHERE s.name = ? AND d.subject = ? AND d.version = ? AND ",
            visible!(),
            " ORDER BY c.ordinal LIMIT 500"
        );
        let rows: Vec<(String,)> = bind_visible!(
            sqlx::query_as(sql).bind(&source).bind(&subject).bind(version),
            ctx
        )
        .fetch_all(self.db.pool())
        .await
        .db()?;
        if rows.is_empty() {
            return Err(HarnessError::not_found(format!("document {base} not found")));
        }
        let mut hits = vec![];
        for (id,) in rows {
            hits.push(self.hit(&id, 1.0).await?);
        }
        let mut tx = self.db.write().await?;
        let mut entry = AuditEntry::system(
            "knowledge",
            "knowledge.read",
            "knowledge",
            ctx.principal_id.clone(),
            AuditOutcome::Recorded,
        )
        .correlation(trace.correlation)
        .details(
            json!({"principal": ctx.principal_id, "document": base, "chunks": hits.len(),
                        "max_classification": ctx.max_classification}),
        );
        entry.tenant = Some(ctx.tenant);
        entry.project = ctx.project;
        self.db.audit_in(&mut tx, entry).await?;
        emit(
            &self.db,
            &mut tx,
            &KnowledgeRetrieved {
                hits: u32::try_from(hits.len()).unwrap_or(u32::MAX),
                mode: "document".into(),
                principal_id: ctx.principal_id.clone(),
            },
            None,
            trace,
        )
        .await?;
        tx.commit().await?;
        Ok(hits)
    }

    /// Active mandatory standards visible in `ctx`. Precedence per subject: enterprise
    /// over domain; within a level the newest version. Returns (citation, title, content,
    /// classification, stale).
    pub async fn mandatory_standards(
        &self,
        ctx: &RetrievalContext,
    ) -> Result<Vec<(String, String, String, Classification, bool)>> {
        let sql = concat!(
            "SELECT d.subject, d.version, d.title, d.content, d.classification, d.scope_level, d.review_by, s.name ",
            "FROM knowledge_documents d JOIN knowledge_sources s ON s.id = d.source_id ",
            "WHERE d.mandatory = 1 AND s.kind = 'standard' AND d.scope_level IN ('enterprise', 'domain') AND ",
            visible!(),
            " ORDER BY d.subject, d.version"
        );
        let rows = bind_visible!(sqlx::query(sql), ctx)
            .fetch_all(self.db.pool())
            .await
            .db()?;
        let mut best: BTreeMap<String, (u8, String, String, String, Classification, bool)> = BTreeMap::new();
        for r in &rows {
            let subject: String = r.try_get(0).db()?;
            let version: i64 = r.try_get(1).db()?;
            let level: String = r.try_get(5).db()?;
            let review_by: Option<String> = r.try_get(6).db()?;
            let stale = match &review_by {
                Some(t) => eaeh_store::parse_ts(t)? < self.db.now(),
                None => false,
            };
            if stale && self.cfg.exclude_stale_standards {
                continue;
            }
            let prec = KnowledgeScope::precedence(&level);
            let entry = (
                prec,
                citation_base(&r.try_get::<String, _>(7).db()?, &subject, version),
                r.try_get::<String, _>(2).db()?,
                r.try_get::<Option<String>, _>(3).db()?.unwrap_or_default(),
                Classification::parse(&r.try_get::<String, _>(4).db()?)?,
                stale,
            );
            match best.get(&subject) {
                Some(cur) if cur.0 > prec => {}
                _ => {
                    best.insert(subject, entry);
                }
            }
        }
        Ok(best
            .into_values()
            .map(|(_, c, t, body, cl, st)| (c, t, body, cl, st))
            .collect())
    }

    /// Mandatory standards as `enterprise_policy` instructions (the only governed
    /// producer of such context items).
    pub async fn standards_context(&self, ctx: &RetrievalContext) -> Result<Vec<ContextItem>> {
        Ok(self
            .mandatory_standards(ctx)
            .await?
            .into_iter()
            .map(|(citation, title, body, classification, stale)| {
                let note = if stale { " (REVIEW OVERDUE)" } else { "" };
                ContextItem::enterprise_standard(
                    &format!("standard:{citation}"),
                    format!("MANDATORY ENTERPRISE STANDARD [{citation}] {title}{note}\n{body}"),
                    &citation,
                    classification,
                )
            })
            .collect())
    }

    /// Search hits as untrusted, cited context excerpts.
    pub fn excerpts(hits: &[SearchHit], scope: eaeh_core::Scope) -> Vec<ContextItem> {
        hits.iter()
            .map(|h| {
                let memory = if h.source_kind == SourceKind::Standard {
                    MemoryKind::Enterprise
                } else {
                    MemoryKind::Project
                };
                let flags = format!(
                    "{}{}",
                    if h.stale { " [STALE]" } else { "" },
                    if h.conflict { " [CONFLICTING SOURCES]" } else { "" }
                );
                ContextItem::knowledge_excerpt(
                    &h.citation,
                    Untrusted::new(format!(
                        "[{}] {} — {}{flags}\n{}",
                        h.citation, h.title, h.heading, h.text
                    )),
                    memory,
                    h.classification,
                    scope,
                )
            })
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fts_query_is_neutralised() {
        assert_eq!(
            sanitize_fts_query("error format"),
            Some("\"error\" OR \"format\"".into())
        );
        let evil = sanitize_fts_query("text:* OR \"x\" NEAR(a b) AND -c heading: \u{0}").unwrap();
        assert!(!evil.contains(':') && !evil.contains('*') && !evil.contains('('));
        assert_eq!(sanitize_fts_query("!!! ?"), None);
    }

    #[test]
    fn rrf_rewards_agreement() {
        let fused = rrf(
            &[vec!["a".into(), "b".into()], vec!["b".into(), "c".into()]],
            60.0,
        );
        assert_eq!(fused[0].0, "b");
    }
}
