//! Derived runs: restart, retry and reinitiation (PORTAL-OPERATIONS-PLAN, W1).
//!
//! A restart never modifies the source run. It creates a **new run** of the same workflow
//! that starts at a chosen stage of the definition's main path:
//!
//! * stages before it are **carried**: their outputs, quality results and the same
//!   immutable artifact versions;
//! * approval gates before it are carried only when their approval is valid for the
//!   carried artifact hash (`check_gate`), and are accepted later only through verified
//!   lineage, so a mandatory approval can never be skipped;
//! * the restart stage and everything after it run again, producing new artifact versions
//!   and needing new approvals; earlier approved versions stay untouched;
//! * an active source run is cancelled as "superseded by" the new run;
//! * changed input (for example a new BRD version) is allowed only from the first stage;
//! * the preview's fingerprint must be confirmed, so a stale confirmation is refused, and
//!   a repeated confirmation returns the same derived run (idempotent).

use super::*;
use eaeh_core::hash::sha256_hex;

/// Disposition of one stage in a restart preview.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct StagePlan {
    pub name: String,
    /// `agent_task`, `tool_step`, `approval_gate`, …
    pub kind: String,
    /// `carried`, `restart_point`, `rerun` (it has outputs in the source run that will be
    /// regenerated as new versions) or `pending` (not reached in the source run).
    pub disposition: String,
    /// The source run's artifact for this stage, if any (kept unchanged in every case).
    pub artifact: Option<ArtifactRef>,
    /// Gates: `carried` (approval remains valid), `repeat` (must be decided again),
    /// `required` (not decided in the source run).
    pub approval: Option<String>,
    pub approval_id: Option<String>,
    /// Whether the run may restart here, and if not, why.
    pub allowed: bool,
    pub reason: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestartPlan {
    pub source: WorkflowInstanceId,
    pub source_status: InstanceStatus,
    pub source_state: String,
    pub definition_id: String,
    /// Definition version the new run uses (the latest one with the same main path,
    /// otherwise the source run's version).
    pub definition_version: String,
    /// Selected restart stage (default: the failed or current stage when allowed, else the
    /// latest allowed stage).
    pub stage: String,
    pub stages: Vec<StagePlan>,
    /// Restart stages the run may use (in path order).
    pub allowed_stages: Vec<String>,
    pub carried_states: Vec<String>,
    pub carried_approvals: Vec<String>,
    pub repeat_approvals: Vec<String>,
    pub input_changed: bool,
    pub source_will_be_cancelled: bool,
    /// What happens to approved documents already in the project workspace.
    pub workspace_effect: String,
    /// Confirm with this value; any change of the source run invalidates it.
    pub fingerprint: String,
}

fn kind_name(k: &StateKind) -> &'static str {
    match k {
        StateKind::AgentTask { .. } => "agent_task",
        StateKind::ToolStep { .. } => "tool_step",
        StateKind::ApprovalGate { .. } => "approval_gate",
        StateKind::Terminal { .. } => "terminal",
        _ => "planned",
    }
}

impl WorkflowEngine {
    /// The definition's main path: from the initial state, following the success
    /// transition (`succeeded`, else `approved`) until a terminal state. Restarts are
    /// limited to this path (no arbitrary stage injection).
    pub(crate) fn main_path(def: &WorkflowDefinition) -> Vec<StateDef> {
        let mut out: Vec<StateDef> = vec![];
        let mut cur = def.initial.clone();
        while let Ok(s) = def.state(&cur) {
            if matches!(s.kind, StateKind::Terminal { .. }) || out.iter().any(|x| x.name == s.name) {
                break;
            }
            out.push(s.clone());
            let next = s
                .transitions
                .get(&Trigger::Succeeded)
                .or_else(|| s.transitions.get(&Trigger::Approved))
                .cloned();
            match next {
                Some(n) => cur = n,
                None => break,
            }
        }
        out
    }

    /// The runs this run was derived from, nearest first.
    pub async fn ancestors(&self, id: WorkflowInstanceId) -> Result<Vec<WorkflowInstanceId>> {
        let mut out = vec![];
        let mut cur = id.to_string();
        for _ in 0..100 {
            let row: Option<(String,)> =
                sqlx::query_as("SELECT source_instance_id FROM workflow_lineage WHERE instance_id = ?")
                    .bind(&cur)
                    .fetch_optional(self.db.pool())
                    .await
                    .db()?;
            match row {
                Some((s,)) => {
                    out.push(s.parse()?);
                    cur = s;
                }
                None => break,
            }
        }
        Ok(out)
    }

    /// Lineage of a run (`workflow.read`): the run it was derived from and the runs derived
    /// from it, with stage, reason and time.
    pub async fn lineage(&self, actor: &Principal, id: WorkflowInstanceId) -> Result<Value> {
        self.get_for(actor, id).await?;
        type Row = (String, String, String, String, i64, String, String);
        let from: Option<Row> = sqlx::query_as(
            "SELECT instance_id, source_instance_id, restart_stage, reason, input_changed, actor_id, created_at FROM workflow_lineage WHERE instance_id = ?",
        )
            .bind(id.to_string())
            .fetch_optional(self.db.pool())
            .await
            .db()?;
        let derived: Vec<Row> = sqlx::query_as(
            "SELECT instance_id, source_instance_id, restart_stage, reason, input_changed, actor_id, created_at FROM workflow_lineage WHERE source_instance_id = ? ORDER BY created_at",
        )
            .bind(id.to_string())
            .fetch_all(self.db.pool())
            .await
            .db()?;
        let v = |r: &Row| json!({"instance": r.0, "source": r.1, "stage": r.2, "reason": r.3, "input_changed": r.4 != 0, "actor": r.5, "at": r.6});
        Ok(
            json!({"derived_from": from.as_ref().map(v), "derived_runs": derived.iter().map(v).collect::<Vec<_>>()}),
        )
    }

    /// Preview a restart (`workflow.start`; nothing is written). `stage` `None` selects the
    /// failed or current stage when allowed. `input` replaces the run input (only from the
    /// first stage).
    pub async fn restart_plan(
        &self,
        actor: &Principal,
        id: WorkflowInstanceId,
        stage: Option<&str>,
        input: Option<&Value>,
    ) -> Result<RestartPlan> {
        let inst = self.get_for(actor, id).await?;
        self.authz
            .authorize(actor, Action::WorkflowStart, Target::Scope(inst.scope))?;
        let src_def = self
            .definition(&inst.definition_id, &inst.definition_version)
            .await?;
        let path = Self::main_path(&src_def);
        if path.is_empty() {
            return Err(HarnessError::invalid_input(
                "this workflow has no restartable stages",
            ));
        }
        // Newer definition version with the same main path (e.g. new materialization policy).
        let def = match self
            .latest_definition(&WorkflowDefinitionId::new(&inst.definition_id)?)
            .await
        {
            Ok(l)
                if Self::main_path(&l)
                    .iter()
                    .map(|s| (&s.name, kind_name(&s.kind)))
                    .eq(path.iter().map(|s| (&s.name, kind_name(&s.kind)))) =>
            {
                l
            }
            _ => src_def.clone(),
        };
        let input_changed = input.is_some_and(|i| i != &inst.input);

        // Which stages may be restart points: everything before must have run, and every
        // gate before must hold an approval valid for the carried artifact.
        let mut stages = vec![];
        let mut blocked: Option<String> = None;
        for (i, s) in path.iter().enumerate() {
            let allowed = blocked.is_none() && (!input_changed || i == 0);
            let reason = if let Some(b) = &blocked {
                Some(b.clone())
            } else if input_changed && i > 0 {
                Some("changed input requires a restart from the first stage".into())
            } else {
                None
            };
            let artifact = s.artifact_name().and(inst.data.artifacts.get(&s.name).cloned());
            let (approval, approval_id) = if matches!(s.kind, StateKind::ApprovalGate { .. }) {
                let c = self.check_gate(&inst, s).await;
                let ok = c.approval_id.is_some() && c.issues.is_empty();
                if !ok && blocked.is_none() {
                    blocked = Some(format!(
                        "gate '{}' {} – restart at or before it",
                        s.name,
                        if c.approval_id.is_none() {
                            "has no approval in the source run".to_string()
                        } else {
                            format!("approval is not valid: {}", c.issues.join("; "))
                        }
                    ));
                }
                (
                    Some(
                        if ok {
                            "valid"
                        } else if c.approval_id.is_some() {
                            "invalid"
                        } else {
                            "none"
                        }
                        .to_string(),
                    ),
                    c.approval_id,
                )
            } else {
                let ran =
                    inst.data.outputs.contains_key(&s.name) || inst.data.artifacts.contains_key(&s.name);
                if !ran && blocked.is_none() && i + 1 < path.len() {
                    blocked = Some(format!(
                        "stage '{}' has not run in the source run – restart at or before it",
                        s.name
                    ));
                }
                (None, None)
            };
            stages.push(StagePlan {
                name: s.name.clone(),
                kind: kind_name(&s.kind).into(),
                disposition: String::new(),
                artifact,
                approval,
                approval_id,
                allowed,
                reason,
            });
        }
        let allowed_stages: Vec<String> = stages
            .iter()
            .filter(|s| s.allowed)
            .map(|s| s.name.clone())
            .collect();
        if allowed_stages.is_empty() {
            return Err(HarnessError::conflict("no stage of this run can be restarted"));
        }
        // Default: the stage the run failed or stopped at, if allowed; else the last allowed.
        let failed_at = self
            .history(id)
            .await?
            .into_iter()
            .rev()
            .find(|t| t.trigger == "failed" || t.trigger == "timed_out")
            .map(|t| t.from_state);
        let chosen = match stage {
            Some(s) => s.to_string(),
            None => [failed_at.as_deref(), Some(inst.state.as_str())]
                .into_iter()
                .flatten()
                .find(|c| allowed_stages.iter().any(|a| a == c))
                .map(str::to_string)
                .unwrap_or_else(|| allowed_stages.last().cloned().unwrap_or_default()),
        };
        let r = stages.iter().position(|s| s.name == chosen).ok_or_else(|| {
            HarnessError::invalid_input(format!(
                "'{chosen}' is not a stage of this workflow's main path ({})",
                stages
                    .iter()
                    .map(|s| s.name.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        })?;
        if !stages[r].allowed {
            return Err(HarnessError::conflict(format!(
                "cannot restart at '{chosen}': {}",
                stages[r].reason.clone().unwrap_or_default()
            )));
        }
        let mut carried_states = vec![];
        let mut carried_approvals = vec![];
        let mut repeat_approvals = vec![];
        for (i, s) in stages.iter_mut().enumerate() {
            let has =
                s.artifact.is_some() || s.approval_id.is_some() || inst.data.outputs.contains_key(&s.name);
            s.disposition = if i < r {
                carried_states.push(s.name.clone());
                if s.kind == "approval_gate" {
                    carried_approvals.push(s.name.clone());
                    s.approval = Some("carried".into());
                }
                "carried"
            } else {
                if s.kind == "approval_gate" {
                    if s.approval_id.is_some() {
                        repeat_approvals.push(s.name.clone());
                        s.approval = Some("repeat".into());
                    } else {
                        s.approval = Some("required".into());
                    }
                }
                if i == r {
                    "restart_point"
                } else if has {
                    "rerun"
                } else {
                    "pending"
                }
            }
            .into();
        }
        let fp_src = json!({
            "source": id, "version": inst.version, "stage": chosen, "def": def.reference(),
            "input": input.map(eaeh_core::hash::canonical_json_hash),
        });
        Ok(RestartPlan {
            source: id,
            source_status: inst.status,
            source_state: inst.state.clone(),
            definition_id: def.id.to_string(),
            definition_version: def.version.to_string(),
            stage: chosen,
            stages,
            allowed_stages,
            carried_states,
            carried_approvals,
            repeat_approvals,
            input_changed,
            source_will_be_cancelled: !inst.status.is_terminal(),
            workspace_effect: "Approved documents already in the project workspace stay as they are; a re-run document replaces them only after its new version is approved.".into(),
            fingerprint: sha256_hex(fp_src.to_string().as_bytes()),
        })
    }

    /// Execute a previewed restart (`workflow.start`, plus `workflow.cancel` when the source
    /// run is still active). Returns the new run (not yet driven). Idempotent per source
    /// run, stage and fingerprint.
    #[allow(clippy::too_many_arguments)]
    pub async fn restart(
        &self,
        actor: &Principal,
        id: WorkflowInstanceId,
        stage: &str,
        input: Option<Value>,
        fingerprint: &str,
        reason: &str,
        trace: TraceContext,
    ) -> Result<(WorkflowInstanceId, RestartPlan)> {
        // Idempotency first: a repeated confirmation returns the run it already created (the
        // source run has since been superseded, so its fingerprint no longer matches).
        let existing: Option<(String,)> = sqlx::query_as(
            "SELECT instance_id FROM workflow_lineage WHERE source_instance_id = ? AND restart_stage = ? AND plan_fingerprint = ?",
        )
        .bind(id.to_string())
        .bind(stage)
        .bind(fingerprint)
        .fetch_optional(self.db.pool())
        .await
        .db()?;
        let plan = self.restart_plan(actor, id, Some(stage), input.as_ref()).await?;
        if let Some((n,)) = existing {
            return Ok((n.parse()?, plan));
        }
        if plan.fingerprint != fingerprint {
            return Err(HarnessError::conflict(
                "the workflow changed since the restart preview; review the new preview and confirm again",
            ));
        }
        let src = self.get(id).await?;
        if plan.source_will_be_cancelled {
            self.authz
                .authorize(actor, Action::WorkflowCancel, Target::Scope(src.scope))?;
        }
        let def = self
            .definition(&plan.definition_id, &plan.definition_version)
            .await?;
        // Carry the stages before the restart point – same immutable artifacts, outputs and
        // validated approvals – and nothing after it.
        let mut data = WorkflowData::default();
        for s in &plan.carried_states {
            for (m, src_m) in [
                (&mut data.outputs, &src.data.outputs),
                (&mut data.quality, &src.data.quality),
            ] {
                if let Some(v) = src_m.get(s) {
                    m.insert(s.clone(), v.clone());
                }
            }
            if let Some(a) = src.data.artifacts.get(s) {
                data.artifacts.insert(s.clone(), a.clone());
            }
            if let Some(a) = src.data.authors.get(s) {
                data.authors.insert(s.clone(), a.clone());
            }
            if let Some(v) = src.data.visits.get(s) {
                data.visits.insert(s.clone(), *v);
            }
            if let Some(v) = src.data.plans.get(s) {
                data.plans.insert(s.clone(), v.clone());
            }
            if let Some(v) = src.data.plan_results.get(s) {
                data.plan_results.insert(s.clone(), v.clone());
            }
        }
        for g in &plan.carried_approvals {
            let aid = *src
                .data
                .approved_gates
                .get(g)
                .ok_or_else(|| HarnessError::integrity(format!("gate '{g}' has no approval to carry")))?;
            let origin = match src.data.carried_approvals.get(g) {
                Some(c) if c.approval_id == aid => c.origin_instance,
                _ => src.id,
            };
            data.approved_gates.insert(g.clone(), aid);
            data.carried_approvals.insert(
                g.clone(),
                CarriedApproval {
                    origin_instance: origin,
                    approval_id: aid,
                },
            );
        }
        data.restarted_from = Some(RestartOrigin {
            source: id,
            stage: stage.to_string(),
        });
        let ws = self.workspaces.create(src.scope, &actor.id).await?;
        data.workspace_id = Some(ws.id);
        let new_id = WorkflowInstanceId::new();
        let new_input = input.unwrap_or_else(|| src.input.clone());
        let now = ts(self.db.now());
        let mut tx = self.db.write().await?;
        sqlx::query(
            "INSERT INTO workflow_instances (id, definition_id, definition_version, tenant_id, project_id, state, status, version, input, data, \
             started_by, correlation_id, cancel_requested, created_at, updated_at) VALUES (?, ?, ?, ?, ?, ?, 'running', 1, ?, ?, ?, ?, 0, ?, ?)",
        )
        .bind(new_id.to_string())
        .bind(def.id.as_str())
        .bind(def.version.to_string())
        .bind(src.scope.tenant.to_string())
        .bind(src.scope.project.to_string())
        .bind(stage)
        .bind(to_json(&new_input)?)
        .bind(to_json(&data)?)
        .bind(actor.id.as_str())
        .bind(trace.correlation.to_string())
        .bind(&now)
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        sqlx::query(
            "INSERT INTO workflow_lineage (instance_id, source_instance_id, tenant_id, project_id, restart_stage, reason, input_changed, \
             carried_states, carried_approvals, plan_fingerprint, actor_id, created_at) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(new_id.to_string())
        .bind(id.to_string())
        .bind(src.scope.tenant.to_string())
        .bind(src.scope.project.to_string())
        .bind(stage)
        .bind(reason)
        .bind(i64::from(plan.input_changed))
        .bind(to_json(&plan.carried_states)?)
        .bind(to_json(&data.carried_approvals)?)
        .bind(fingerprint)
        .bind(actor.id.as_str())
        .bind(&now)
        .execute(tx.conn())
        .await
        .db()?;
        self.budgets
            .init_in(&mut tx, &format!("wf:{new_id}"), &def.budget)
            .await?;
        self.db
            .audit_in(
                &mut tx,
                AuditEntry::new(
                    actor,
                    "workflow.restart",
                    "workflow_instance",
                    new_id.to_string(),
                    AuditOutcome::Succeeded,
                )
                .scope(src.scope)
                .correlation(trace.correlation)
                .details(json!({
                    "source": id, "stage": stage, "reason": reason, "definition": def.reference(),
                    "carried_states": plan.carried_states, "carried_approvals": data.carried_approvals,
                    "repeat_approvals": plan.repeat_approvals, "input_changed": plan.input_changed,
                })),
            )
            .await?;
        emit(
            &self.db,
            &mut tx,
            &WorkflowStarted {
                instance_id: new_id.to_string(),
                definition_id: def.id.to_string(),
                definition_version: def.version.to_string(),
            },
            src.scope,
            trace,
        )
        .await?;
        tx.commit().await?;
        if plan.source_will_be_cancelled {
            self.db
                .audit(
                    AuditEntry::new(
                        actor,
                        "workflow.superseded",
                        "workflow_instance",
                        id.to_string(),
                        AuditOutcome::Recorded,
                    )
                    .scope(src.scope)
                    .correlation(trace.correlation)
                    .details(json!({"superseded_by": new_id, "stage": stage})),
                )
                .await?;
            if let Err(e) = self.cancel(actor, id).await
                && e.kind != eaeh_core::ErrorKind::Conflict
            {
                return Err(e);
            }
        }
        Ok((new_id, plan))
    }
}

/// One task of a run with its persisted input, output and definition context
/// ([`WorkflowEngine::task_detail`]).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TaskDetail {
    pub task: TaskSummary,
    pub input: Value,
    pub output: Option<Value>,
    /// The stage definition (kind, task kind, capabilities, artifact, quality gate).
    pub stage: Value,
    /// Stages whose outputs this task consumed, and gates it requires.
    pub depends_on: Vec<String>,
    pub requires_approval_of: Vec<String>,
    /// Artifact this stage produced in the run, with its provenance (agent, skills, models).
    pub artifact: Option<Value>,
    pub quality: Option<Value>,
    /// Other attempts of the same stage in this run (visits, retries).
    pub attempts: Vec<TaskSummary>,
}

fn output_refs(v: &Value, out: &mut Vec<String>) {
    match v {
        Value::Object(m) => {
            if let Some(Value::String(s)) = m.get("$output")
                && !out.contains(s)
            {
                out.push(s.clone());
            }
            m.values().for_each(|x| output_refs(x, out));
        }
        Value::Array(a) => a.iter().for_each(|x| output_refs(x, out)),
        _ => {}
    }
}

impl WorkflowEngine {
    /// A task with its input, output, stage definition, dependencies and produced artifact
    /// (`workflow.read` on the task's project; unknown and foreign tasks are not found).
    pub async fn task_detail(&self, actor: &Principal, task: &str) -> Result<TaskDetail> {
        let row: Option<(String, String, Option<String>)> =
            sqlx::query_as("SELECT workflow_instance_id, input, output FROM tasks WHERE id = ?")
                .bind(task)
                .fetch_optional(self.db.pool())
                .await
                .db()?;
        let (wid, input, output) = row.ok_or_else(|| HarnessError::not_found("task not found"))?;
        let inst = self
            .get_for(actor, wid.parse()?)
            .await
            .map_err(|_| HarnessError::not_found("task not found"))?;
        let all = self.tasks_for(actor, inst.id).await?;
        let summary = all
            .iter()
            .find(|t| t.id == task)
            .cloned()
            .ok_or_else(|| HarnessError::not_found("task not found"))?;
        let def = self
            .definition(&inst.definition_id, &inst.definition_version)
            .await?;
        let st = def.state(&summary.state).ok();
        let mut depends_on = vec![];
        if let Some(s) = st {
            match &s.kind {
                StateKind::AgentTask { inputs, .. } => {
                    for v in inputs.values() {
                        output_refs(v, &mut depends_on);
                    }
                }
                StateKind::ToolStep { input, .. } => output_refs(input, &mut depends_on),
                _ => {}
            }
        }
        let artifact = match inst.data.artifacts.get(&summary.state) {
            Some(a) => self
                .artifacts
                .meta(inst.scope, a.id)
                .await
                .ok()
                .map(|m| serde_json::to_value(m).unwrap_or(Value::Null)),
            None => None,
        };
        Ok(TaskDetail {
            input: serde_json::from_str(&input).unwrap_or(Value::Null),
            output: output.and_then(|o| serde_json::from_str(&o).ok()),
            stage: st
                .map(|s| serde_json::to_value(s).unwrap_or(Value::Null))
                .unwrap_or(Value::Null),
            requires_approval_of: st.map(|s| s.requires_approval_of.clone()).unwrap_or_default(),
            depends_on,
            artifact,
            quality: inst.data.quality.get(&summary.state).cloned(),
            attempts: all
                .iter()
                .filter(|t| t.state == summary.state && t.id != summary.id)
                .cloned()
                .collect(),
            task: summary,
        })
    }
}
