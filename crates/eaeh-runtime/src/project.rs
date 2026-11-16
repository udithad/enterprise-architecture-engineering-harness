//! Developer experience: the repository-level project manifest (`.eaeh/project.toml`),
//! required-skill discovery and synchronisation, project readiness checks and engineering
//! activity planning. Used by the CLI (`eaeh project …`, `eaeh skill …`,
//! `eaeh workflow run`) and the MCP server. Nothing here bypasses governance: every
//! operation goes through the same services, authorization and approval manager.

use crate::harness::Harness;
use eaeh_agents::registry::CapabilityQuery;
use eaeh_core::{
    HarnessError, Principal, PrincipalId, ProjectId, Result, Role, Scope, TenantId, TraceContext,
};
use eaeh_skills::{SkillDefinition, SkillRef, SkillStatus};
use eaeh_workflow::{StateKind, WorkflowDefinition};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

pub const MANIFEST_DIR: &str = ".eaeh";
pub const MANIFEST_FILE: &str = ".eaeh/project.toml";

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectManifest {
    pub project: ProjectSection,
    pub workspace: WorkspaceSection,
    pub stack: StackSection,
    #[serde(default)]
    pub brd: Option<BrdSection>,
    #[serde(default)]
    pub knowledge: Vec<KnowledgeEntry>,
    pub workflows: Vec<String>,
    pub roles: RolesSection,
    pub provider: ProviderSection,
    /// Expected execution mode and the AI clients configured for this repository (ADR-0025).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub execution: Option<ExecutionSection>,
    /// Software delivery settings (build/test command, verification policy).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub delivery: Option<DeliverySection>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExecutionSection {
    /// `external_ai`, `direct_provider` or `deterministic`: what the project expects. The
    /// installation's `[execution] mode` is authoritative; readiness warns on a mismatch.
    pub mode: String,
    /// Generated client integrations: `claude-code`, `github-copilot`.
    #[serde(default)]
    pub clients: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeliverySection {
    /// `required`: EAEH runs the build and tests (isolated `exec.run`) before the delivery
    /// approval. `deferred`: EAEH does not run them; the bundle is marked unverified.
    pub build_verification: String,
    /// Program and arguments of the build/test command (run in the workspace, isolated).
    #[serde(default)]
    pub build_program: String,
    #[serde(default)]
    pub build_args: Vec<String>,
}

/// Delivery workflow variant for a manifest.
pub fn delivery_workflow(m: &ProjectManifest) -> &'static str {
    match &m.delivery {
        Some(d) if d.build_verification == "deferred" => DELIVERY_UNVERIFIED,
        _ => DELIVERY,
    }
}

pub const DELIVERY: &str = "engineering.delivery";
pub const DELIVERY_UNVERIFIED: &str = "engineering.delivery-unverified";
pub const DESIGN: &str = "engineering.design";

fn is_delivery(w: &str) -> bool {
    w == DELIVERY || w == DELIVERY_UNVERIFIED
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProjectSection {
    pub id: ProjectId,
    pub tenant: TenantId,
    pub name: String,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct WorkspaceSection {
    /// Absolute path of the repository root at bootstrap time.
    pub root: String,
    #[serde(default)]
    pub git_remote: Option<String>,
    #[serde(default)]
    pub git_head: Option<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StackSection {
    #[serde(default)]
    pub language: String,
    #[serde(default)]
    pub framework: String,
    #[serde(default)]
    pub build_tool: String,
    #[serde(default)]
    pub constraints: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BrdSection {
    /// Repository-relative path of the BRD file.
    pub path: String,
    /// Governed knowledge citation `source/subject@vN`.
    pub citation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEntry {
    pub path: String,
    pub citation: String,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RolesSection {
    /// Principal that runs workflows (role `operator`).
    pub developer: String,
    /// Principal that curates knowledge and registers skills (role `knowledge_steward`).
    pub steward: String,
    /// Principals that decide approval gates (role `approver`; never the developer).
    pub approvers: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ProviderSection {
    /// `anthropic`, `replay` (deterministic, not AI) or `none`.
    pub mode: String,
}

impl ProjectManifest {
    pub fn scope(&self) -> Scope {
        Scope::new(self.project.tenant, self.project.id)
    }

    pub fn load(root: &Path) -> Result<Self> {
        let p = root.join(MANIFEST_FILE);
        let s = std::fs::read_to_string(&p)
            .map_err(|e| HarnessError::not_found(format!("{}: {e}", p.display())))?;
        toml::from_str(&s).map_err(|e| HarnessError::invalid_input(format!("{}: {e}", p.display())))
    }

    pub fn save(&self, root: &Path) -> Result<PathBuf> {
        std::fs::create_dir_all(root.join(MANIFEST_DIR))?;
        let p = root.join(MANIFEST_FILE);
        let body = toml::to_string_pretty(self).map_err(|e| HarnessError::internal(e.to_string()))?;
        std::fs::write(
            &p,
            format!(
                "# EAEH project manifest (generated by `eaeh project init`; safe to commit – contains no secrets)\n{body}"
            ),
        )?;
        Ok(p)
    }
}

/// Walk up from `start` to the directory containing `.eaeh/project.toml`.
pub fn find_root(start: &Path) -> Option<PathBuf> {
    let mut cur = std::fs::canonicalize(start).ok()?;
    loop {
        if cur.join(MANIFEST_FILE).is_file() {
            return Some(cur);
        }
        if !cur.pop() {
            return None;
        }
    }
}

/// Git association read from `.git` without running git: (`origin` URL, HEAD ref/commit).
pub fn git_info(root: &Path) -> (Option<String>, Option<String>) {
    let git = root.join(".git");
    let remote = std::fs::read_to_string(git.join("config")).ok().and_then(|c| {
        let mut in_origin = false;
        for line in c.lines() {
            let l = line.trim();
            if l.starts_with('[') {
                in_origin = l == "[remote \"origin\"]";
            } else if in_origin && let Some(u) = l.strip_prefix("url") {
                return Some(u.trim_start_matches([' ', '=']).trim().to_string());
            }
        }
        None
    });
    let head = std::fs::read_to_string(git.join("HEAD")).ok().map(|h| {
        h.trim()
            .strip_prefix("ref: ")
            .map(str::to_string)
            .unwrap_or_else(|| h.trim().to_string())
    });
    (remote, head)
}

// ------------------------------------------------------------------ skills

/// A skill required by an agent used by an enabled workflow.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RequiredSkill {
    pub reference: String,
    pub agents: BTreeSet<String>,
    pub workflows: BTreeSet<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SkillState {
    Approved {
        version: String,
    },
    PendingApproval {
        version: String,
        approval_id: Option<String>,
        digest: String,
    },
    Unusable {
        version: String,
        status: String,
    },
    NotRegistered {
        definition_file: Option<String>,
    },
}

/// Agents that would execute the agent states of a workflow (same discovery as the executor).
pub async fn workflow_agents(
    h: &Harness,
    def: &WorkflowDefinition,
) -> Result<Vec<(String, Option<eaeh_agents::AgentDefinition>)>> {
    let mut out = vec![];
    for s in &def.states {
        if let StateKind::AgentTask {
            capabilities,
            max_autonomy,
            ..
        } = &s.kind
        {
            let found = h
                .agent_registry
                .discover(&CapabilityQuery {
                    required: capabilities.clone(),
                    max_autonomy: Some(*max_autonomy),
                    ids: None,
                })
                .await?;
            out.push((s.name.clone(), found.into_iter().next()));
        }
    }
    Ok(out)
}

pub async fn workflow_definition(h: &Harness, id: &str) -> Result<WorkflowDefinition> {
    h.engine
        .definitions()
        .await?
        .into_iter()
        .filter(|d| d.id.as_str() == id)
        .max_by(|a, b| a.version.cmp(&b.version))
        .ok_or_else(|| HarnessError::not_found(format!("workflow '{id}' is not registered")))
}

pub async fn required_skills(h: &Harness, workflows: &[String]) -> Result<Vec<RequiredSkill>> {
    let mut m: BTreeMap<String, RequiredSkill> = BTreeMap::new();
    for w in workflows {
        let def = workflow_definition(h, w).await?;
        for (_, agent) in workflow_agents(h, &def).await? {
            let Some(a) = agent else { continue };
            for s in &a.skills {
                let e = m.entry(s.clone()).or_insert_with(|| RequiredSkill {
                    reference: s.clone(),
                    agents: BTreeSet::new(),
                    workflows: BTreeSet::new(),
                });
                e.agents.insert(a.id.to_string());
                e.workflows.insert(w.clone());
            }
        }
    }
    Ok(m.into_values().collect())
}

/// A skill file applied to workflow layers by its `task_kinds` (dynamic selection), in
/// addition to the agents' own skills (MARKDOWN-SKILLS-PLAN, K2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct LayerSkill {
    /// `id@=version` – the exact version in the skill file.
    pub reference: String,
    pub file: String,
    pub description: String,
    /// `workflow/stage (task kind)` the skill applies to.
    pub layers: BTreeSet<String>,
}

/// Task kinds of the agent stages of `workflows`, as `(workflow, stage, task kind)`.
pub async fn workflow_layers(h: &Harness, workflows: &[String]) -> Result<Vec<(String, String, String)>> {
    let mut out = vec![];
    for w in workflows {
        let def = workflow_definition(h, w).await?;
        for s in &def.states {
            if let StateKind::AgentTask { task_kind, .. } = &s.kind {
                out.push((w.clone(), s.name.clone(), task_kind.clone()));
            }
        }
    }
    Ok(out)
}

/// Skill files in `definitions.skills_dir` that apply to a layer of `workflows` and are not
/// already an agent's own skill (those are [`required_skills`]).
pub async fn layer_skills(h: &Harness, workflows: &[String]) -> Result<Vec<LayerSkill>> {
    let layers = workflow_layers(h, workflows).await?;
    let explicit: Vec<SkillRef> = required_skills(h, workflows)
        .await?
        .iter()
        .filter_map(|r| SkillRef::parse(&r.reference).ok())
        .collect();
    let mut out = vec![];
    let dirs: Vec<(PathBuf, SkillDefinition)> = skill_directories(h)?
        .into_iter()
        .filter_map(|(p, r)| r.ok().map(|d| (p, d)))
        .collect();
    let files = repository_skills(h)?;
    for (file, d) in files.into_iter().chain(dirs) {
        if explicit.iter().any(|r| r.id == d.id) {
            continue;
        }
        let hits: BTreeSet<String> = layers
            .iter()
            .filter(|(_, _, k)| d.task_kinds.contains(k))
            .map(|(w, s, k)| format!("{w}/{s} ({k})"))
            .collect();
        if !hits.is_empty() {
            out.push(LayerSkill {
                // A directory may be registered as a later patch (O2): match the major.
                reference: if d.package.is_some() {
                    format!("{}@^{}", d.id, d.version.major)
                } else {
                    format!("{}@={}", d.id, d.version)
                },
                file: file.display().to_string(),
                description: d.description.clone(),
                layers: hits,
            });
        }
    }
    Ok(out)
}

/// Every skill reference a project's workflows use: the agents' own skills and the layer
/// skills. `project init` and `skill sync` register these.
pub async fn project_skill_references(h: &Harness, workflows: &[String]) -> Result<Vec<String>> {
    let mut refs: Vec<String> = required_skills(h, workflows)
        .await?
        .into_iter()
        .map(|r| r.reference)
        .collect();
    // Skill directories are registered by discovery (versions may be later patches).
    refs.extend(
        layer_skills(h, workflows)
            .await?
            .into_iter()
            .filter(|s| !s.reference.contains("@^"))
            .map(|s| s.reference),
    );
    Ok(refs)
}

/// Repository-approved skill definitions (`definitions.skills_dir`).
pub fn repository_skills(h: &Harness) -> Result<Vec<(PathBuf, SkillDefinition)>> {
    let dir = h.config.resolve(&h.config.definitions.skills_dir);
    let mut out = vec![];
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(out);
    };
    let mut files: Vec<PathBuf> = rd
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x == "json" || x == "md"))
        .filter(|p| {
            // README.md and other Markdown without front matter are documentation, not skills.
            p.extension().is_some_and(|x| x == "json")
                || std::fs::read_to_string(p)
                    .is_ok_and(|t| t.trim_start_matches('\u{feff}').starts_with("+++"))
        })
        .collect();
    files.sort();
    for f in files {
        let def = SkillDefinition::from_file_text(&f, &std::fs::read_to_string(&f)?).map_err(|e| {
            HarnessError::invalid_input(format!("skill definition {}: {}", f.display(), e.message))
        })?;
        if let Some((other, _)) = out
            .iter()
            .find(|(_, d): &&(PathBuf, SkillDefinition)| d.id == def.id && d.version == def.version)
        {
            return Err(HarnessError::invalid_input(format!(
                "skill {} is defined twice: {} and {}",
                def.reference(),
                other.display(),
                f.display()
            )));
        }
        out.push((f, def));
    }
    Ok(out)
}

/// Skill directories (`<skills_dir>/<name>/SKILL.md` + nested files), each loaded or with
/// the reason it is invalid (SKILL-ONBOARDING-PLAN).
pub fn skill_directories(h: &Harness) -> Result<Vec<(PathBuf, Result<SkillDefinition>)>> {
    let dir = h.config.resolve(&h.config.definitions.skills_dir);
    let Ok(rd) = std::fs::read_dir(&dir) else {
        return Ok(vec![]);
    };
    let mut dirs: Vec<PathBuf> = rd
        .filter_map(|e| e.ok())
        .filter(|e| !e.file_name().to_string_lossy().starts_with('.'))
        .filter(|e| e.file_type().is_ok_and(|t| t.is_dir()))
        .map(|e| e.path())
        .filter(|p| p.join(eaeh_skills::package::MANIFEST).is_file())
        .collect();
    dirs.sort();
    Ok(dirs
        .into_iter()
        .map(|d| {
            let r = SkillDefinition::from_directory(&d);
            (d, r)
        })
        .collect())
}

/// What automatic discovery did with one skill directory.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DiscoveryOutcome {
    pub dir: String,
    pub skill: Option<String>,
    /// Registered version (may be a later patch of the manifest version).
    pub version: Option<String>,
    /// `new`, `changed`, `unchanged` or `invalid`.
    pub action: String,
    pub detail: String,
    pub tree_hash: Option<String>,
    pub approval_id: Option<String>,
}

/// Governance scope of discovered skill approvals: the given project, else the project
/// where skills were last governed, else the first project of the installation.
async fn discovery_scope(h: &Harness, project: Option<ProjectId>) -> Result<Option<Scope>> {
    use eaeh_store::DbResultExt;
    if let Some(p) = project {
        let row: Option<(String,)> =
            eaeh_store::sqlx::query_as("SELECT tenant_id FROM projects WHERE id = ?")
                .bind(p.to_string())
                .fetch_optional(h.db.pool())
                .await
                .db()?;
        if let Some((t,)) = row {
            return Ok(Some(Scope::new(t.parse()?, p)));
        }
    }
    let last: Option<(String, String)> = eaeh_store::sqlx::query_as(
        "SELECT tenant_id, project_id FROM approvals WHERE subject_type = 'skill' ORDER BY created_at DESC LIMIT 1",
    )
    .fetch_optional(h.db.pool())
    .await
    .db()?;
    let pick = match last {
        Some(r) => Some(r),
        None => eaeh_store::sqlx::query_as(
            "SELECT tenant_id, id FROM projects ORDER BY CASE WHEN name = 'platform' THEN 1 ELSE 0 END, created_at LIMIT 1",
        )
        .fetch_optional(h.db.pool())
        .await
        .db()?,
    };
    Ok(match pick {
        Some((t, p)) => Some(Scope::new(t.parse()?, p.parse()?)),
        None => None,
    })
}

/// Automatic skill discovery (O2/O3): register every new or changed skill directory as
/// *pending approval*, attributed to `system:skill-discovery`. An unchanged directory
/// (its tree hash is already registered) is left alone. Changed content whose manifest
/// version is already registered becomes the next free patch version; the approved version
/// stays in force until a human approves the new one. Idempotent.
pub async fn discover_skills(h: &Harness, project: Option<ProjectId>) -> Result<Vec<DiscoveryOutcome>> {
    let dirs = skill_directories(h)?;
    if dirs.is_empty() {
        return Ok(vec![]);
    }
    let Some(gov) = discovery_scope(h, project).await? else {
        return Ok(vec![]); // not bootstrapped yet
    };
    let mut out = vec![];
    for (path, def) in dirs {
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
        let mut def = match def {
            Ok(d) => d,
            Err(e) => {
                out.push(DiscoveryOutcome {
                    dir: name,
                    skill: None,
                    version: None,
                    action: "invalid".into(),
                    detail: e.message,
                    tree_hash: None,
                    approval_id: None,
                });
                continue;
            }
        };
        let tree = def.tree_hash().unwrap_or_default().to_string();
        let all = h.skills.all_records().await?;
        let same: Vec<&eaeh_skills::SkillRecord> = all.iter().filter(|r| r.definition.id == def.id).collect();
        if let Some(r) = same
            .iter()
            .find(|r| r.definition.tree_hash() == Some(tree.as_str()))
        {
            out.push(DiscoveryOutcome {
                dir: name,
                skill: Some(def.id.to_string()),
                version: Some(r.definition.version.to_string()),
                action: "unchanged".into(),
                detail: format!("registered ({})", r.status.as_str()),
                tree_hash: Some(tree),
                approval_id: r.approval_id.clone(),
            });
            continue;
        }
        let source = def.version.clone();
        let taken = |v: &semver::Version| same.iter().any(|r| &r.definition.version == v);
        let changed = taken(&source)
            || same.iter().any(|r| {
                r.definition
                    .package
                    .as_ref()
                    .is_some_and(|p| p.source_version == source)
            });
        if taken(&source) {
            let mut next = same
                .iter()
                .map(|r| r.definition.version.clone())
                .filter(|v| v.major == source.major && v.minor == source.minor && v >= &source)
                .max()
                .unwrap_or_else(|| source.clone());
            next.patch += 1;
            next.pre = semver::Prerelease::EMPTY;
            while taken(&next) {
                next.patch += 1;
            }
            def.version = next;
        }
        let revision = 1 + same
            .iter()
            .filter(|r| {
                r.definition
                    .package
                    .as_ref()
                    .is_some_and(|p| p.source_version == source)
            })
            .count() as u32;
        if let Some(p) = def.package.as_mut() {
            p.revision = revision;
        }
        match h.skills.register_discovered(&def, gov, TraceContext::new_root()).await {
            Ok(rec) => out.push(DiscoveryOutcome {
                dir: name,
                skill: Some(def.id.to_string()),
                version: Some(def.version.to_string()),
                action: if changed { "changed".into() } else { "new".into() },
                detail: if def.version != source {
                    format!(
                        "files changed: registered as {} (from manifest {source}, revision {revision}); awaiting human approval",
                        def.version
                    )
                } else {
                    "registered; awaiting human approval".into()
                },
                tree_hash: Some(tree),
                approval_id: rec.approval_id,
            }),
            Err(e) => out.push(DiscoveryOutcome {
                dir: name,
                skill: Some(def.id.to_string()),
                version: None,
                action: "invalid".into(),
                detail: e.message,
                tree_hash: Some(tree),
                approval_id: None,
            }),
        }
    }
    Ok(out)
}

/// Scan interval of periodic discovery: 30 s, or `EAEH_SKILL_DISCOVERY_INTERVAL_SECS` (≥ 1).
pub fn skill_discovery_interval() -> std::time::Duration {
    let secs = std::env::var("EAEH_SKILL_DISCOVERY_INTERVAL_SECS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(30);
    std::time::Duration::from_secs(secs)
}

/// Periodic discovery for long-running processes (`eaeh portal`, `eaeh serve`).
pub fn spawn_skill_discovery(
    h: std::sync::Arc<Harness>,
    every: std::time::Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match discover_skills(&h, None).await {
                Ok(r) => {
                    for o in r.iter().filter(|o| o.action != "unchanged") {
                        tracing::info!(dir = %o.dir, action = %o.action, version = ?o.version, detail = %o.detail, "skill discovery");
                    }
                }
                Err(e) => tracing::warn!(error = %e, "skill discovery failed"),
            }
            tokio::time::sleep(every).await;
        }
    })
}

pub async fn skill_state(h: &Harness, actor: &Principal, reference: &str) -> Result<SkillState> {
    let r = SkillRef::parse(reference)?;
    let all = h.skills.list(actor).await?;
    let mut matching: Vec<_> = all
        .into_iter()
        .filter(|s| s.definition.id == r.id && r.version.matches(&s.definition.version))
        .collect();
    matching.sort_by(|a, b| a.definition.version.cmp(&b.definition.version));
    if let Some(a) = matching.iter().rev().find(|s| s.status == SkillStatus::Approved) {
        return Ok(SkillState::Approved {
            version: a.definition.version.to_string(),
        });
    }
    if let Some(p) = matching
        .iter()
        .rev()
        .find(|s| s.status == SkillStatus::PendingApproval)
    {
        return Ok(SkillState::PendingApproval {
            version: p.definition.version.to_string(),
            approval_id: p.approval_id.clone(),
            digest: p.digest.clone(),
        });
    }
    if let Some(u) = matching.last() {
        return Ok(SkillState::Unusable {
            version: u.definition.version.to_string(),
            status: u.status.as_str().into(),
        });
    }
    let file = repository_skills(h)?
        .into_iter()
        .filter(|(_, d)| d.id == r.id && r.version.matches(&d.version))
        .max_by(|a, b| a.1.version.cmp(&b.1.version))
        .map(|(f, _)| f.display().to_string());
    Ok(SkillState::NotRegistered {
        definition_file: file,
    })
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SyncOutcome {
    pub reference: String,
    /// `registered`, `already_registered`, `missing_definition` or `failed`.
    pub action: String,
    pub detail: String,
    pub approval_id: Option<String>,
}

/// Register required skills (and their dependencies) that are not yet registered, from
/// repository definitions. Registration creates an approval request; nothing is approved.
pub async fn sync_skills(
    h: &Harness,
    actor: &Principal,
    project: ProjectId,
    references: &[String],
) -> Result<Vec<SyncOutcome>> {
    let repo = repository_skills(h)?;
    let mut queue: Vec<String> = references.to_vec();
    let mut seen = BTreeSet::new();
    let mut out = vec![];
    while let Some(reference) = queue.pop() {
        if !seen.insert(reference.clone()) {
            continue;
        }
        let r = SkillRef::parse(&reference)?;
        match skill_state(h, actor, &reference).await? {
            SkillState::NotRegistered { .. } => {
                let Some((file, def)) = repo
                    .iter()
                    .filter(|(_, d)| d.id == r.id && r.version.matches(&d.version))
                    .max_by(|a, b| a.1.version.cmp(&b.1.version))
                else {
                    out.push(SyncOutcome {
                        reference,
                        action: "missing_definition".into(),
                        detail: format!(
                            "no definition in {}",
                            h.config.resolve(&h.config.definitions.skills_dir).display()
                        ),
                        approval_id: None,
                    });
                    continue;
                };
                for d in &def.dependencies {
                    queue.push(format!("{}@{}", d.id, d.version));
                }
                match h
                    .skills
                    .register(actor, def, project, TraceContext::new_root())
                    .await
                {
                    Ok(rec) => out.push(SyncOutcome {
                        reference,
                        action: "registered".into(),
                        detail: format!(
                            "{} from {} – awaiting human approval",
                            def.reference(),
                            file.display()
                        ),
                        approval_id: rec.approval_id,
                    }),
                    Err(e) => out.push(SyncOutcome {
                        reference,
                        action: "failed".into(),
                        detail: e.to_string(),
                        approval_id: None,
                    }),
                }
            }
            other => out.push(SyncOutcome {
                reference,
                action: "already_registered".into(),
                detail: serde_json::to_string(&other)?,
                approval_id: None,
            }),
        }
    }
    out.sort_by(|a, b| a.reference.cmp(&b.reference));
    Ok(out)
}

// ------------------------------------------------------------------ readiness

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    Ok,
    Warning,
    Blocking,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Check {
    pub area: String,
    pub status: CheckStatus,
    pub message: String,
    /// Exact command or action that resolves the issue.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub fix: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Readiness {
    pub project: String,
    pub ready: bool,
    pub blocking: usize,
    pub warnings: usize,
    pub checks: Vec<Check>,
}

struct Report(Vec<Check>);

impl Report {
    fn push(&mut self, area: &str, status: CheckStatus, message: impl Into<String>, fix: Option<String>) {
        self.0.push(Check {
            area: area.into(),
            status,
            message: message.into(),
            fix,
        });
    }
    fn ok(&mut self, area: &str, m: impl Into<String>) {
        self.push(area, CheckStatus::Ok, m, None);
    }
    fn warn(&mut self, area: &str, m: impl Into<String>, fix: impl Into<String>) {
        self.push(area, CheckStatus::Warning, m, Some(fix.into()));
    }
    fn block(&mut self, area: &str, m: impl Into<String>, fix: impl Into<String>) {
        self.push(area, CheckStatus::Blocking, m, Some(fix.into()));
    }
}

async fn principal_with(
    h: &Harness,
    id: &str,
    role: Role,
    scope: Scope,
) -> std::result::Result<Principal, String> {
    let pid = PrincipalId::new(id).map_err(|e| e.message)?;
    let (p, active) = h
        .identity
        .get_principal(&pid)
        .await
        .map_err(|_| format!("principal '{id}' does not exist"))?;
    if !active {
        return Err(format!("principal '{id}' is disabled"));
    }
    if p.tenant != scope.tenant {
        return Err(format!("principal '{id}' does not exist"));
    }
    if !p.has_role(role) {
        return Err(format!("principal '{id}' lacks role {}", role.as_str()));
    }
    if !p.can_access(&scope) {
        return Err(format!("principal '{id}' has no access to this project"));
    }
    Ok(p)
}

/// Project readiness for the given workflows (all enabled workflows when `workflows` is
/// `None`). `caller` is the authenticated identity running the check.
pub async fn readiness(
    h: &Harness,
    caller: &Principal,
    m: &ProjectManifest,
    root: &Path,
    workflows: Option<&[String]>,
) -> Result<Readiness> {
    let scope = m.scope();
    let mut r = Report(vec![]);
    let workflows: Vec<String> = workflows
        .map(<[String]>::to_vec)
        .unwrap_or_else(|| m.workflows.clone());

    // Project and workspace.
    let registered = h
        .identity
        .projects(scope.tenant)
        .await?
        .into_iter()
        .any(|(id, _)| id == scope.project);
    if !registered || caller.tenant != scope.tenant {
        r.block(
            "project",
            format!(
                "project '{}' ({}) is not registered in your tenant",
                m.project.name, m.project.id
            ),
            "eaeh project init … (as an administrator)",
        );
        return Ok(finish(m, r));
    }
    r.ok(
        "project",
        format!("project '{}' registered ({})", m.project.name, m.project.id),
    );
    let here = std::fs::canonicalize(root).unwrap_or_else(|_| root.to_path_buf());
    if Path::new(&m.workspace.root) == here {
        r.ok("workspace", format!("workspace associated: {}", m.workspace.root));
    } else {
        r.warn(
            "workspace",
            format!(
                "manifest workspace is {}, current repository is {}",
                m.workspace.root,
                here.display()
            ),
            "re-run `eaeh project init` in this repository to re-associate it",
        );
    }
    if root.join(".git").exists() {
        r.ok("workspace", "git repository detected");
    } else {
        r.warn(
            "workspace",
            "workspace is not a git repository",
            "git init (recommended for traceability of later phases)",
        );
    }

    // Identity and permissions.
    if caller.can_access(&scope) {
        r.ok(
            "identity",
            format!("authenticated as '{}' with access to the project", caller.id),
        );
    } else {
        r.block(
            "identity",
            format!("'{}' has no access to this project", caller.id),
            format!(
                "eaeh principal grant --id {} --project {} (administrator)",
                caller.id, scope.project
            ),
        );
    }
    let developer = match principal_with(h, &m.roles.developer, Role::Operator, scope).await {
        Ok(p) => {
            r.ok(
                "roles",
                format!("developer '{}' can run workflows (operator)", p.id),
            );
            Some(p)
        }
        Err(e) => {
            r.block(
                "roles",
                format!("developer: {e}"),
                "eaeh project init --developer <id> … (administrator)",
            );
            None
        }
    };
    let mut approvers = 0;
    for a in &m.roles.approvers {
        match principal_with(h, a, Role::Approver, scope).await {
            Ok(p) if !p.is_human() => r.block(
                "roles",
                format!("approver '{a}' is not a human principal"),
                "assign a human approver",
            ),
            Ok(_) if a == &m.roles.developer => r.block(
                "roles",
                format!("approver '{a}' is also the developer (separation of duties)"),
                "assign a different approver",
            ),
            Ok(_) => approvers += 1,
            Err(e) => r.block(
                "roles",
                format!("approver: {e}"),
                "eaeh project init --approver <id> … (administrator)",
            ),
        }
    }
    if approvers == 0 {
        r.block(
            "roles",
            "no approver can decide this project's gates",
            "eaeh project init --approver <id> … (administrator)",
        );
    } else {
        r.ok(
            "roles",
            format!("{approvers} approver(s) responsible for approval gates"),
        );
    }
    match principal_with(h, &m.roles.steward, Role::KnowledgeSteward, scope).await {
        Ok(p) => r.ok("roles", format!("knowledge steward '{}'", p.id)),
        Err(e) => r.warn(
            "roles",
            format!("steward: {e}"),
            "eaeh project init --steward <id> … (administrator)",
        ),
    }

    // Workflows, agents, tools, gates.
    let gates: BTreeSet<String> = h
        .engine
        .quality_gates()
        .into_iter()
        .map(|g| g.id.clone())
        .collect();
    let mut needs_model = false;
    let mut planned_workflows: Vec<String> = vec![];
    for w in &workflows {
        let def = match workflow_definition(h, w).await {
            Ok(d) => d,
            Err(e) => {
                r.block(
                    "workflows",
                    e.message,
                    format!("add workflows/{w}.json and restart"),
                );
                continue;
            }
        };
        let approvals = def
            .states
            .iter()
            .filter(|s| matches!(s.kind, StateKind::ApprovalGate { .. }))
            .count();
        if def
            .states
            .iter()
            .any(|s| matches!(s.kind, StateKind::Planned { .. }))
        {
            needs_model = true;
            planned_workflows.push(w.clone());
        }
        r.ok(
            "workflows",
            format!(
                "{w}@{} registered ({approvals} human approval gate(s))",
                def.version
            ),
        );
        for s in &def.states {
            if let Some(g) = &s.quality_gate
                && !gates.contains(&g.gate)
            {
                r.block(
                    "workflows",
                    format!("{w}: quality gate '{}' is not configured", g.gate),
                    "platform configuration (eaeh-engineering default gates)",
                );
            }
        }
        for (state, agent) in workflow_agents(h, &def).await? {
            needs_model = true;
            match agent {
                None => r.block(
                    "agents",
                    format!("{w}: no active agent for state '{state}'"),
                    "add the agent definition under config/agents and restart",
                ),
                Some(a) => {
                    let missing: Vec<String> = a
                        .allowed_tools
                        .iter()
                        .filter(|t| h.tools.descriptor(t).is_err())
                        .map(|t| t.to_string())
                        .collect();
                    if missing.is_empty() {
                        r.ok(
                            "agents",
                            format!("{w}/{state}: agent {} with its tools", a.reference()),
                        );
                    } else {
                        r.block(
                            "agents",
                            format!(
                                "{w}/{state}: agent {} needs unregistered tools {missing:?}",
                                a.reference()
                            ),
                            "register the tools (composition root / plugin)",
                        );
                    }
                }
            }
        }
    }

    // Skills.
    for s in required_skills(h, &workflows).await? {
        let st = skill_state(h, caller, &s.reference).await;
        let used_by = s.agents.iter().cloned().collect::<Vec<_>>().join(", ");
        match st {
            Ok(SkillState::Approved { version }) => r.ok("skills", format!("{} approved (v{version}; used by {used_by})", s.reference)),
            Ok(SkillState::PendingApproval { version, approval_id, .. }) => r.block(
                "skills",
                format!(
                    "{} v{version} awaits human approval (approval {}; used by {used_by})",
                    s.reference,
                    approval_id.unwrap_or_default()
                ),
                format!("eaeh approval review --as <approver-profile>   # an approver reviews each skill and confirms; approvers: {}", m.roles.approvers.join(", ")),
            ),
            Ok(SkillState::Unusable { version, status }) => r.block(
                "skills",
                format!("{} v{version} is {status} and cannot be used", s.reference),
                "a steward registers a new version (eaeh skill sync), then an approver runs `eaeh approval review`",
            ),
            Ok(SkillState::NotRegistered { definition_file: Some(f) }) => r.block(
                "skills",
                format!("{} is not registered (definition available: {f})", s.reference),
                "eaeh skill sync   # as the steward or administrator; then an approver runs `eaeh approval review`",
            ),
            Ok(SkillState::NotRegistered { definition_file: None }) => r.block(
                "skills",
                format!("{} is not registered and no repository definition exists", s.reference),
                format!("add a definition to {}", h.config.resolve(&h.config.definitions.skills_dir).display()),
            ),
            Err(e) => r.block("skills", format!("{}: {}", s.reference, e.message), "eaeh auth status"),
        }
    }
    // Additional layer skills (skill files matched by task kind): applied once approved;
    // a pending one does not block the project (K2).
    for s in layer_skills(h, &workflows).await? {
        let at = s.layers.iter().cloned().collect::<Vec<_>>().join(", ");
        match skill_state(h, caller, &s.reference).await {
            Ok(SkillState::Approved { .. }) => {
                r.ok("skills", format!("{} approved; applied at {at}", s.reference))
            }
            Ok(SkillState::PendingApproval { approval_id, .. }) => r.warn(
                "skills",
                format!(
                    "{} (for {at}) awaits approval {}; it is not applied until approved",
                    s.reference,
                    approval_id.unwrap_or_default()
                ),
                "eaeh approval review --as <approver-profile>   # or the portal: Skills → Review",
            ),
            Ok(SkillState::NotRegistered { .. }) => r.warn(
                "skills",
                format!("{} ({}) for {at} is not registered", s.reference, s.file),
                "eaeh skill sync   # or the portal: Skills → Register skill files",
            ),
            Ok(SkillState::Unusable { status, .. }) => r.warn(
                "skills",
                format!("{} for {at} is {status} and is not applied", s.reference),
                "register a new version of the skill file (bump `version`), then approve it",
            ),
            Err(e) => r.warn(
                "skills",
                format!("{}: {}", s.reference, e.message),
                "eaeh auth status",
            ),
        }
    }

    // Knowledge.
    if let Some(dev) = &developer {
        let ctx = h.knowledge.context_for_human(dev, scope.project).await?;
        match &m.brd {
            None => r.block(
                "knowledge",
                "no BRD is associated with the project",
                "eaeh brd ingest --file <BRD.md> (in the repository, as the steward)",
            ),
            Some(b) => match h
                .knowledge
                .read_document(&ctx, &b.citation, TraceContext::new_root())
                .await
            {
                Ok(c) => r.ok(
                    "knowledge",
                    format!(
                        "BRD {} readable by the developer ({} sections)",
                        b.citation,
                        c.len()
                    ),
                ),
                Err(e) => r.block(
                    "knowledge",
                    format!("BRD {} is not readable: {}", b.citation, e.message),
                    "eaeh brd ingest --file <BRD.md>",
                ),
            },
        }
        match h
            .knowledge
            .search(
                &ctx,
                &eaeh_knowledge::SearchQuery::new(
                    "requirements constraints",
                    eaeh_knowledge::SearchMode::Hybrid,
                    1,
                ),
                TraceContext::new_root(),
            )
            .await
        {
            Ok(_) => r.ok("knowledge", "retrieval available"),
            Err(e) => r.block(
                "knowledge",
                format!("retrieval failed: {}", e.message),
                "eaeh health",
            ),
        }
        let standards = h.knowledge.mandatory_standards(&ctx).await?;
        if standards.is_empty() {
            r.warn(
                "knowledge",
                "no approved mandatory enterprise standards are visible; designs will not be checked against standards",
                "eaeh project init … --standard <file> (or steward: eaeh knowledge ingest --mandatory …); then an approver runs `eaeh approval review`",
            );
        } else {
            r.ok(
                "knowledge",
                format!("{} mandatory standard(s) in force", standards.len()),
            );
        }
    }

    // Execution mode: who reasons, who pays, and whether the agent stages can run.
    let mode = h.execution_mode;
    let models = h.models.models();
    let replay = models.iter().any(|m| m.provider.as_str() == "replay");
    r.ok("execution", format!("mode {mode}: {}", mode.billing()));
    if let Some(x) = &m.execution
        && x.mode != mode.as_str()
    {
        r.warn(
            "execution",
            format!(
                "the project expects mode '{}' but this installation runs '{mode}'",
                x.mode
            ),
            format!(
                "an administrator sets [execution] mode = \"{}\" in the EAEH configuration file (in {}), or re-run `eaeh project init --mode {mode}`",
                x.mode,
                h.config.base_dir.display()
            ),
        );
    }
    if !needs_model {
        r.ok("provider", "selected workflows need no AI reasoning");
    } else if mode.is_external() {
        if !h.ignored_providers.is_empty() {
            r.ok(
                "execution",
                format!(
                    "enabled model providers are not registered in this mode (no fallback): {}",
                    h.ignored_providers.join(", ")
                ),
            );
        }
        if planned_workflows.is_empty() {
            r.ok(
                "execution",
                "agent stages are handed off to your AI client through EAEH: the background worker runs them automatically (or an assistant via eaeh_task_get / eaeh_task_submit)",
            );
        }
        for w in &planned_workflows {
            r.block(
                "execution",
                format!("{w} uses a model-driven planner, which external_ai mode cannot run"),
                "use direct_provider mode for this workflow, or a workflow without planned states",
            );
        }
        let clients = m
            .execution
            .as_ref()
            .map(|x| x.clients.clone())
            .unwrap_or_default();
        if clients.is_empty() {
            r.warn(
                "client",
                "no AI client integration is configured for this repository",
                "eaeh project init … --client claude-code (and/or --client github-copilot)",
            );
        }
        for c in &clients {
            let (file, label) = match c.as_str() {
                "claude-code" => (".mcp.json", "Claude Code"),
                "github-copilot" => (".vscode/mcp.json", "GitHub Copilot (VS Code agent mode)"),
                _ => continue,
            };
            let ok = std::fs::read_to_string(root.join(file))
                .map(|t| t.contains("\"eaeh\""))
                .unwrap_or(false);
            if ok {
                r.ok("client", format!("{label}: EAEH MCP server configured in {file}"));
            } else {
                r.warn(
                    "client",
                    format!("{label}: {file} has no 'eaeh' MCP server"),
                    format!("re-run `eaeh project init` (it writes {file})"),
                );
            }
        }
        // Automated execution (ADR-0026): can the background worker run AI tasks through
        // Claude Code on the user's subscription? Not blocking: an interactive assistant can
        // still do a task; nothing ever falls back to a billed provider.
        match crate::coordinator::executor_from_config(&h.config.execution.coordinator) {
            Err(e) => r.warn(
                "automation",
                e.message,
                "set [execution.coordinator] provider = \"claude_code\"",
            ),
            Ok(x) => {
                let pf = x.preflight().await;
                let withheld = crate::coordinator::claude::ClaudeCodeCli::billing_env_present();
                match pf.usable() {
                    Ok(()) => r.ok(
                        "automation",
                        format!(
                            "automated AI execution ready: {}; AI tasks run automatically in the background worker{}",
                            pf.detail,
                            if withheld.is_empty() {
                                String::new()
                            } else {
                                format!(" ({} withheld from Claude Code)", withheld.join(", "))
                            }
                        ),
                    ),
                    Err((c, d)) => r.warn(
                        "automation",
                        format!("automated AI execution unavailable [{}]: {d}", c.as_str()),
                        c.remedy(),
                    ),
                }
            }
        }
    } else if models.is_empty() {
        r.block(
            "provider",
            format!("mode {mode}: no model provider is registered; agent stages cannot run"),
            "use [execution] mode = \"external_ai\" (your Claude Code / Copilot subscription), or enable [providers.anthropic] (key via EAEH_SECRET_ANTHROPIC_API_KEY) for direct_provider, or [providers.replay] for deterministic",
        );
    } else if replay {
        r.warn(
            "provider",
            "deterministic replay provider enabled: outputs are recorded fixtures, NOT AI-generated",
            "for AI reasoning use [execution] mode = \"external_ai\" (or direct_provider with [providers.anthropic])",
        );
    } else {
        r.ok(
            "provider",
            format!(
                "model(s) available: {} (billed by the provider)",
                models
                    .iter()
                    .map(|m| format!("{}/{}", m.provider, m.id))
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
        );
    }

    // Software delivery prerequisites.
    for w in workflows.iter().filter(|w| is_delivery(w)) {
        let only_delivery = workflows.iter().all(|w| is_delivery(w));
        match latest_design(h, caller, scope).await {
            Ok(Some(d)) => r.ok("delivery", format!("approved design available (run {})", d.id)),
            Ok(None) if only_delivery => r.block(
                "delivery",
                "no approved design: delivery starts from a succeeded design run",
                "eaeh workflow run design --yes   # then approve requirements, HLD and LLD",
            ),
            Ok(None) => r.ok(
                "delivery",
                "deliver becomes available after a design run succeeds",
            ),
            Err(e) => r.warn(
                "delivery",
                format!("design runs not readable: {}", e.message),
                "eaeh auth status",
            ),
        }
        if w == DELIVERY_UNVERIFIED {
            r.warn(
                "delivery",
                "build and test verification is DEFERRED: EAEH will not run the build or the tests; the delivery bundle is marked unverified",
                "set up isolated build execution, then re-run `eaeh project init --build-verification required --build-program <program> --build-arg …`",
            );
            continue;
        }
        let d = m.delivery.clone().unwrap_or(DeliverySection {
            build_verification: "required".into(),
            build_program: String::new(),
            build_args: vec![],
        });
        let container = h
            .isolation_status
            .iter()
            .find(|(k, _)| k == "container")
            .map(|(_, v)| v.clone())
            .unwrap_or_else(|| "disabled".into());
        // Build prerequisites block only the `deliver` activity itself.
        let mut need = |msg: String, fix: String| {
            if only_delivery {
                r.block("delivery", msg, fix);
            } else {
                r.warn("delivery", format!("deliver: {msg}"), fix);
            }
        };
        if d.build_program.is_empty() {
            need(
                "no build/test command is configured".into(),
                "eaeh project init … --build-program <program> --build-arg <arg> … (or --build-verification deferred)".into(),
            );
        } else if !h
            .config
            .isolation
            .exec_allowed_programs
            .contains(&d.build_program)
        {
            need(
                format!(
                    "build program '{}' is not in [isolation] exec_allowed_programs",
                    d.build_program
                ),
                format!(
                    "an administrator adds \"{}\" to exec_allowed_programs (and a container image that provides it)",
                    d.build_program
                ),
            );
        }
        if container != "available" {
            need(
                format!("isolated build execution unavailable (container: {container})"),
                "start the container runtime (podman machine start) with an image providing the build toolchain, or choose --build-verification deferred".into(),
            );
        }
        if !d.build_program.is_empty()
            && h.config
                .isolation
                .exec_allowed_programs
                .contains(&d.build_program)
            && container == "available"
        {
            r.ok(
                "delivery",
                format!(
                    "build/test command {} {} runs isolated (container image {})",
                    d.build_program,
                    d.build_args.join(" "),
                    h.config.isolation.container_image
                ),
            );
        }
    }

    // Governance.
    let audit = h.db.verify_audit_chain().await?;
    if audit.valid {
        r.ok(
            "governance",
            format!("audit chain valid ({} records)", audit.records_checked),
        );
    } else {
        r.block(
            "governance",
            "audit chain verification failed",
            "eaeh audit verify (auditor) and investigate",
        );
    }
    if m.brd.is_some() && needs_model {
        if mode.is_external() {
            r.warn(
                "governance",
                "the BRD (confidential) is handed to your AI client, which sends it to its model provider under your subscription's terms",
                "confirm this is allowed for the project (A-33); classify the BRD as restricted to block the hand-off",
            );
        } else if models.iter().any(|x| x.provider.as_str() == "anthropic") {
            r.warn(
                "governance",
                "the BRD (confidential) will be sent to an external model provider",
                "confirm the data-protection review (A-33) or classify the BRD as restricted to block egress",
            );
        }
    }
    Ok(finish(m, r))
}

fn finish(m: &ProjectManifest, r: Report) -> Readiness {
    let blocking = r.0.iter().filter(|c| c.status == CheckStatus::Blocking).count();
    let warnings = r.0.iter().filter(|c| c.status == CheckStatus::Warning).count();
    Readiness {
        project: m.project.name.clone(),
        ready: blocking == 0,
        blocking,
        warnings,
        checks: r.0,
    }
}

// ------------------------------------------------------------------ activities

/// Engineering activities a developer can request by name.
pub const ACTIVITIES: &[(&str, &str, &str)] = &[
    (
        "requirements",
        "requirements.analysis",
        "Analyse the project BRD and prepare traceable, approved requirements",
    ),
    (
        "design",
        "engineering.design",
        "BRD → requirements → HLD → LLD, each approved by a human",
    ),
    (
        "deliver",
        "engineering.delivery",
        "Approved design → implementation plan (approved) → code and tests in a governed workspace → build/test → approved source bundle to export",
    ),
];

/// Activities that are recognised but not available in this release.
pub const UNAVAILABLE: &[(&str, &str)] = &[
    (
        "release",
        "release management is Phase 7 scope and is not implemented",
    ),
    ("deploy", "deployment is Phase 7 scope and is not implemented"),
];

/// Names that select the `deliver` activity.
const DELIVER_ALIASES: &[&str] = &["plan", "implement", "test", "develop", "implementation"];

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagePlan {
    pub state: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent: Option<String>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub skills: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub detail: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ActivityPlan {
    pub activity: String,
    pub description: String,
    pub workflow: String,
    pub input: serde_json::Value,
    pub stages: Vec<StagePlan>,
    pub approvals: Vec<String>,
    pub readiness: Readiness,
}

pub fn activity_workflow(activity: &str) -> Result<(&'static str, &'static str)> {
    let activity = if DELIVER_ALIASES.contains(&activity) || activity == DELIVERY_UNVERIFIED {
        "deliver"
    } else {
        activity
    };
    if let Some((_, w, d)) = ACTIVITIES
        .iter()
        .find(|(a, w, _)| *a == activity || *w == activity)
    {
        return Ok((w, d));
    }
    if let Some((_, why)) = UNAVAILABLE.iter().find(|(a, _)| *a == activity) {
        return Err(HarnessError::not_supported(format!(
            "activity '{activity}': {why}"
        )));
    }
    Err(HarnessError::invalid_input(format!(
        "unknown activity '{activity}'; available: {}",
        ACTIVITIES
            .iter()
            .map(|(a, _, d)| format!("{a} ({d})"))
            .collect::<Vec<_>>()
            .join("; ")
    )))
}

/// The most recent succeeded `engineering.design` run of the project (all its gates approved).
pub async fn latest_design(
    h: &Harness,
    caller: &Principal,
    scope: Scope,
) -> Result<Option<eaeh_workflow::WorkflowInstance>> {
    Ok(h.engine
        .list(caller, scope, None, 200)
        .await?
        .into_iter()
        .find(|i| i.definition_id == DESIGN && i.status == eaeh_workflow::InstanceStatus::Succeeded))
}

/// Delivery input from an approved design run: the approved requirement set, HLD and LLD
/// models, the project stack and the build/test command.
pub fn delivery_input(
    m: &ProjectManifest,
    design: &eaeh_workflow::WorkflowInstance,
) -> Result<serde_json::Value> {
    let get =
        |k: &str| {
            design.data.outputs.get(k).cloned().ok_or_else(|| {
                HarnessError::integrity(format!("design run {} has no '{k}' output", design.id))
            })
        };
    let d = m.delivery.clone().unwrap_or(DeliverySection {
        build_verification: "required".into(),
        build_program: String::new(),
        build_args: vec![],
    });
    Ok(serde_json::json!({
        "design_run": design.id.to_string(),
        "requirements": get("refine")?,
        "hld": get("hld")?,
        "lld": get("lld")?,
        "project": {
            "name": m.project.name, "description": m.project.description,
            "language": m.stack.language, "framework": m.stack.framework,
            "build_tool": m.stack.build_tool, "constraints": m.stack.constraints,
        },
        "build": {"program": d.build_program, "args": d.build_args},
    }))
}

/// Resolve an activity into a concrete, checked workflow run (no execution).
pub async fn plan_activity(
    h: &Harness,
    caller: &Principal,
    m: &ProjectManifest,
    root: &Path,
    activity: &str,
) -> Result<ActivityPlan> {
    let (workflow, description) = activity_workflow(activity)?;
    let workflow = if workflow == DELIVERY {
        delivery_workflow(m)
    } else {
        workflow
    };
    let def = workflow_definition(h, workflow).await?;
    let mut readiness = readiness(h, caller, m, root, Some(&[workflow.to_string()])).await?;
    let can_start = caller.roles().iter().any(|r| {
        eaeh_governance::authz::RbacAuthorizer::role_permits(
            *r,
            eaeh_governance::authz::Action::WorkflowStart,
        )
    });
    if !can_start {
        readiness.checks.push(Check {
            area: "identity".into(),
            status: CheckStatus::Blocking,
            message: format!("'{}' cannot start workflows (needs role operator)", caller.id),
            fix: Some(format!(
                "run as the developer: --as developer (principal '{}')",
                m.roles.developer
            )),
        });
        readiness.blocking += 1;
        readiness.ready = false;
    }
    let agents: BTreeMap<String, Option<eaeh_agents::AgentDefinition>> =
        workflow_agents(h, &def).await?.into_iter().collect();
    let mut stages = vec![];
    let mut approvals = vec![];
    for s in &def.states {
        let (kind, agent, skills, detail) = match &s.kind {
            StateKind::AgentTask { task_kind, .. } => {
                let a = agents.get(&s.name).cloned().flatten();
                (
                    "agent".to_string(),
                    a.as_ref().map(|a| a.reference()),
                    a.map(|a| a.skills).unwrap_or_default(),
                    Some(format!(
                        "task {task_kind}{}",
                        s.quality_gate
                            .as_ref()
                            .map(|g| format!("; quality gate '{}'", g.gate))
                            .unwrap_or_default()
                    )),
                )
            }
            StateKind::ToolStep { tool, .. } => (
                "tool".into(),
                None,
                vec![],
                Some(format!("deterministic tool {tool}")),
            ),
            StateKind::ApprovalGate {
                required_role,
                max_revisions,
                ..
            } => {
                approvals.push(format!(
                    "{} – human {} (approve / request changes ≤{max_revisions} / reject)",
                    s.name,
                    required_role.as_str()
                ));
                (
                    "approval".into(),
                    None,
                    vec![],
                    Some(format!("approvers: {}", m.roles.approvers.join(", "))),
                )
            }
            StateKind::Planned { .. } => ("planned".into(), None, vec![], None),
            StateKind::Terminal { .. } => continue,
        };
        stages.push(StagePlan {
            state: s.name.clone(),
            kind,
            agent,
            skills,
            detail,
        });
    }
    let input = if is_delivery(workflow) {
        match latest_design(h, caller, m.scope()).await? {
            Some(d) => delivery_input(m, &d)?,
            None => serde_json::Value::Null,
        }
    } else {
        let brd = m.brd.as_ref().ok_or_else(|| {
            HarnessError::invalid_input(
                "no BRD is associated with this project; run `eaeh brd ingest --file <BRD.md>` in the repository",
            )
        })?;
        serde_json::json!({"brd": brd.citation})
    };
    Ok(ActivityPlan {
        activity: activity.into(),
        description: description.into(),
        workflow: workflow.into(),
        input,
        stages,
        approvals,
        readiness,
    })
}
