//! Durable execution budgets.
//!
//! Budgets are attached to owners (workflow instance, agent instance, …). Consumption is
//! atomic and all-or-nothing across several owners (e.g. an agent's tool call is charged
//! to both the agent instance and its workflow). Consumption for a resource that has no
//! configured limit **fails closed**.

use eaeh_core::{HarnessError, Result};
use eaeh_store::{Db, DbResultExt, WriteTx, sqlx, ts};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BudgetResource {
    ToolCalls,
    ModelCalls,
    Tokens,
    CostMicros,
    WallClockMs,
    DelegationDepth,
    ConcurrentAgents,
    CorrectionAttempts,
    AgentSteps,
}

impl BudgetResource {
    pub fn as_str(self) -> &'static str {
        match self {
            BudgetResource::ToolCalls => "tool_calls",
            BudgetResource::ModelCalls => "model_calls",
            BudgetResource::Tokens => "tokens",
            BudgetResource::CostMicros => "cost_micros",
            BudgetResource::WallClockMs => "wall_clock_ms",
            BudgetResource::DelegationDepth => "delegation_depth",
            BudgetResource::ConcurrentAgents => "concurrent_agents",
            BudgetResource::CorrectionAttempts => "correction_attempts",
            BudgetResource::AgentSteps => "agent_steps",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct BudgetLimits(pub BTreeMap<BudgetResource, u64>);

impl BudgetLimits {
    pub fn with(mut self, r: BudgetResource, limit: u64) -> Self {
        self.0.insert(r, limit);
        self
    }

    pub fn get(&self, r: BudgetResource) -> Option<u64> {
        self.0.get(&r).copied()
    }

    /// Conservative platform defaults used when a workflow/agent does not specify limits.
    pub fn platform_default() -> Self {
        Self::default()
            .with(BudgetResource::ToolCalls, 200)
            .with(BudgetResource::ModelCalls, 100)
            .with(BudgetResource::Tokens, 2_000_000)
            .with(BudgetResource::CostMicros, 50_000_000)
            .with(BudgetResource::WallClockMs, 3_600_000)
            .with(BudgetResource::DelegationDepth, 3)
            .with(BudgetResource::ConcurrentAgents, 8)
            .with(BudgetResource::CorrectionAttempts, 3)
            .with(BudgetResource::AgentSteps, 50)
    }

    /// Element-wise minimum (a child budget can never exceed its parent's limits).
    pub fn min(&self, other: &BudgetLimits) -> BudgetLimits {
        let mut out = self.clone();
        for (k, v) in &other.0 {
            out.0.entry(*k).and_modify(|x| *x = (*x).min(*v)).or_insert(*v);
        }
        out
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BudgetUsage {
    pub resource: String,
    pub used: u64,
    pub limit: u64,
}

#[derive(Debug, Clone)]
pub struct BudgetLedger {
    db: Db,
}

impl BudgetLedger {
    pub fn new(db: Db) -> Self {
        Self { db }
    }

    /// Initialise limits for an owner (idempotent: existing rows are kept).
    pub async fn init_in(&self, tx: &mut WriteTx, owner: &str, limits: &BudgetLimits) -> Result<()> {
        for (r, l) in &limits.0 {
            sqlx::query(
                "INSERT OR IGNORE INTO budget_usage (owner_id, resource, used, max_allowed, updated_at) VALUES (?, ?, 0, ?, ?)",
            )
            .bind(owner)
            .bind(r.as_str())
            .bind(i64::try_from(*l).unwrap_or(i64::MAX))
            .bind(ts(self.db.now()))
            .execute(tx.conn())
            .await
            .db()?;
        }
        Ok(())
    }

    pub async fn init(&self, owner: &str, limits: &BudgetLimits) -> Result<()> {
        let mut tx = self.db.write().await?;
        self.init_in(&mut tx, owner, limits).await?;
        tx.commit().await
    }

    /// Atomically consume `amount` from every owner, or from none.
    pub async fn consume(&self, owners: &[&str], resource: BudgetResource, amount: u64) -> Result<()> {
        let mut tx = self.db.write().await?;
        self.consume_in(&mut tx, owners, resource, amount).await?;
        tx.commit().await
    }

    pub async fn consume_in(
        &self,
        tx: &mut WriteTx,
        owners: &[&str],
        resource: BudgetResource,
        amount: u64,
    ) -> Result<()> {
        let amt =
            i64::try_from(amount).map_err(|_| HarnessError::invalid_input("budget amount too large"))?;
        for owner in owners {
            let r = sqlx::query(
                "UPDATE budget_usage SET used = used + ?, updated_at = ? WHERE owner_id = ? AND resource = ? AND used + ? <= max_allowed",
            )
            .bind(amt)
            .bind(ts(self.db.now()))
            .bind(*owner)
            .bind(resource.as_str())
            .bind(amt)
            .execute(tx.conn())
            .await
            .db()?;
            if r.rows_affected() == 0 {
                let exists: Option<(i64, i64)> = sqlx::query_as(
                    "SELECT used, max_allowed FROM budget_usage WHERE owner_id = ? AND resource = ?",
                )
                .bind(*owner)
                .bind(resource.as_str())
                .fetch_optional(tx.conn())
                .await
                .db()?;
                // The caller's transaction is dropped (rolled back) on error, so earlier
                // owners in this loop are not charged.
                return Err(match exists {
                    Some((used, max)) => HarnessError::budget_exceeded(format!(
                        "{} budget exhausted for {owner}: used {used} + {amount} > {max}",
                        resource.as_str()
                    )),
                    None => HarnessError::budget_exceeded(format!(
                        "no {} budget configured for {owner} (fail closed)",
                        resource.as_str()
                    )),
                });
            }
        }
        Ok(())
    }

    /// Return previously consumed capacity (e.g. a concurrency slot).
    pub async fn release(&self, owner: &str, resource: BudgetResource, amount: u64) -> Result<()> {
        let mut tx = self.db.write().await?;
        sqlx::query("UPDATE budget_usage SET used = MAX(used - ?, 0), updated_at = ? WHERE owner_id = ? AND resource = ?")
            .bind(i64::try_from(amount).unwrap_or(i64::MAX))
            .bind(ts(self.db.now()))
            .bind(owner)
            .bind(resource.as_str())
            .execute(tx.conn())
            .await
            .db()?;
        tx.commit().await
    }

    pub async fn usage(&self, owner: &str) -> Result<Vec<BudgetUsage>> {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT resource, used, max_allowed FROM budget_usage WHERE owner_id = ? ORDER BY resource",
        )
        .bind(owner)
        .fetch_all(self.db.pool())
        .await
        .db()?;
        Ok(rows
            .into_iter()
            .map(|(r, u, l)| BudgetUsage {
                resource: r,
                used: u64::try_from(u).unwrap_or(0),
                limit: u64::try_from(l).unwrap_or(0),
            })
            .collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn consumption_is_bounded_atomic_and_fail_closed() {
        let d = tempfile::tempdir().unwrap();
        let db = Db::open(&d.path().join("b.db")).await.unwrap();
        let l = BudgetLedger::new(db);
        l.init("wf", &BudgetLimits::default().with(BudgetResource::ToolCalls, 10))
            .await
            .unwrap();
        l.init(
            "agent",
            &BudgetLimits::default().with(BudgetResource::ToolCalls, 2),
        )
        .await
        .unwrap();
        l.consume(&["wf", "agent"], BudgetResource::ToolCalls, 1)
            .await
            .unwrap();
        l.consume(&["wf", "agent"], BudgetResource::ToolCalls, 1)
            .await
            .unwrap();
        let e = l
            .consume(&["wf", "agent"], BudgetResource::ToolCalls, 1)
            .await
            .unwrap_err();
        assert_eq!(e.kind, eaeh_core::ErrorKind::BudgetExceeded);
        // All-or-nothing: workflow was not charged for the rejected call.
        let wf = l.usage("wf").await.unwrap();
        assert_eq!(wf[0].used, 2);
        // Unconfigured resource fails closed.
        assert_eq!(
            l.consume(&["wf"], BudgetResource::Tokens, 1)
                .await
                .unwrap_err()
                .kind,
            eaeh_core::ErrorKind::BudgetExceeded
        );
        l.release("agent", BudgetResource::ToolCalls, 1).await.unwrap();
        l.consume(&["agent"], BudgetResource::ToolCalls, 1).await.unwrap();
    }

    #[test]
    fn child_limits_never_exceed_parent() {
        let parent = BudgetLimits::default().with(BudgetResource::ToolCalls, 5);
        let child = BudgetLimits::default()
            .with(BudgetResource::ToolCalls, 50)
            .with(BudgetResource::Tokens, 9);
        let m = child.min(&parent);
        assert_eq!(m.get(BudgetResource::ToolCalls), Some(5));
        assert_eq!(m.get(BudgetResource::Tokens), Some(9));
    }
}
