-- Task-scoped capabilities for external AI attempts, and richer delivery traces.
-- Additive.

-- One capability per execution attempt (its id is the execution id): read of exactly this
-- task's own context, bound to tenant, project, workflow, task and package version.
CREATE TABLE external_capabilities (
    id                    TEXT PRIMARY KEY,
    task_id               TEXT NOT NULL REFERENCES external_tasks(id),
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    workflow_instance_id  TEXT NOT NULL,
    state_name            TEXT NOT NULL,
    package_hash          TEXT NOT NULL,
    issued_to             TEXT NOT NULL,
    issued_at             TEXT NOT NULL,
    expires_at            TEXT NOT NULL,
    revoked_at            TEXT,
    revoked_reason        TEXT
);
CREATE INDEX external_capabilities_task ON external_capabilities(task_id);

ALTER TABLE external_deliveries ADD COLUMN denied_calls INTEGER NOT NULL DEFAULT 0;
-- access_denied | integrity | submission_failed
ALTER TABLE external_deliveries ADD COLUMN failure_kind TEXT;

ALTER TABLE external_executions ADD COLUMN denied_calls INTEGER;
ALTER TABLE external_executions ADD COLUMN sections_total INTEGER;
ALTER TABLE external_executions ADD COLUMN sections_retrieved INTEGER;
ALTER TABLE external_executions ADD COLUMN acknowledged_at TEXT;
