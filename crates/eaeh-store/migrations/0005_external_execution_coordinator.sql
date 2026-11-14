-- Automated external AI execution (ADR-0026): the External Execution Coordinator runs
-- handed-off agent tasks through an installed, externally authenticated AI client
-- (Claude Code CLI). The workflow engine stays authoritative; these tables only record
-- who is executing a hand-off, every attempt, and the workers that do it.

-- Dispatch state of a hand-off (independent of its lifecycle `status`):
--   pending     not yet dispatched
--   leased      a worker holds a lease and is executing it (lease_owner, lease_expires_at)
--   retry_wait  a bounded retry is scheduled at next_attempt_at
--   blocked     stopped on a non-retryable condition (CLI missing, auth, billing, usage limit)
--   exhausted   the retry budget is used up
--   done        accepted, failed by validation, or no longer waiting
-- `blocked` and `exhausted` are never retried automatically (`eaeh worker retry <task>`).
ALTER TABLE external_tasks ADD COLUMN dispatch_state TEXT NOT NULL DEFAULT 'pending'
    CHECK (dispatch_state IN ('pending', 'leased', 'retry_wait', 'blocked', 'exhausted', 'done'));
ALTER TABLE external_tasks ADD COLUMN lease_owner TEXT;
ALTER TABLE external_tasks ADD COLUMN lease_expires_at TEXT;
ALTER TABLE external_tasks ADD COLUMN dispatch_attempts INTEGER NOT NULL DEFAULT 0;
ALTER TABLE external_tasks ADD COLUMN next_attempt_at TEXT;
ALTER TABLE external_tasks ADD COLUMN last_failure TEXT;
CREATE INDEX external_tasks_dispatch ON external_tasks(tenant_id, project_id, dispatch_state);

-- One coordinator process per project and data directory. Liveness is the heartbeat.
CREATE TABLE external_workers (
    id              TEXT PRIMARY KEY,
    tenant_id       TEXT NOT NULL,
    project_id      TEXT NOT NULL,
    principal_id    TEXT NOT NULL,
    provider        TEXT NOT NULL,
    pid             INTEGER NOT NULL,
    host            TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('running', 'stopped', 'failed')),
    detail          TEXT,
    stop_requested  INTEGER NOT NULL DEFAULT 0,
    started_at      TEXT NOT NULL,
    heartbeat_at    TEXT NOT NULL,
    stopped_at      TEXT
);
CREATE INDEX external_workers_scope ON external_workers(tenant_id, project_id, status);

-- One row per execution attempt of a hand-off. No credentials are stored: auth_status is a
-- summary such as "claude.ai subscription (pro)". notional_cost_usd is the client's own
-- estimate; for a subscription login it is not a separate charge.
CREATE TABLE external_executions (
    id                    TEXT PRIMARY KEY,
    task_id               TEXT NOT NULL REFERENCES external_tasks(id),
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    workflow_instance_id  TEXT NOT NULL,
    state_name            TEXT NOT NULL,
    agent_definition      TEXT NOT NULL,
    worker_id             TEXT NOT NULL,
    provider              TEXT NOT NULL,
    client_version        TEXT,
    execution_mode        TEXT NOT NULL,
    auth_status           TEXT NOT NULL,
    attempt               INTEGER NOT NULL,
    outcome               TEXT NOT NULL CHECK (outcome IN ('running', 'succeeded', 'rejected', 'retry', 'blocked', 'exhausted', 'cancelled', 'interrupted', 'superseded')),
    failure_class         TEXT,
    detail                TEXT,
    task_status_after     TEXT,
    submissions_after     INTEGER,
    num_turns             INTEGER,
    duration_ms           INTEGER,
    notional_cost_usd     REAL,
    started_at            TEXT NOT NULL,
    ended_at              TEXT
);
CREATE INDEX external_executions_task ON external_executions(task_id, attempt);
CREATE INDEX external_executions_scope ON external_executions(tenant_id, project_id, started_at);
