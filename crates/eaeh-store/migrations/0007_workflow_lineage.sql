-- Derived workflow runs (docs/implementation/stages/PORTAL-OPERATIONS-PLAN.md, W1).
-- A restart, retry or reinitiation creates a NEW run from a source run and a restart stage;
-- the source run is never modified (if still active it is cancelled as "superseded").
-- New table only.
CREATE TABLE workflow_lineage (
    instance_id         TEXT PRIMARY KEY REFERENCES workflow_instances(id),
    source_instance_id  TEXT NOT NULL REFERENCES workflow_instances(id),
    tenant_id           TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    restart_stage       TEXT NOT NULL,
    reason              TEXT NOT NULL,
    input_changed       INTEGER NOT NULL,
    carried_states      TEXT NOT NULL,   -- JSON [state]
    carried_approvals   TEXT NOT NULL,   -- JSON {gate: {origin_instance, approval_id}}
    plan_fingerprint    TEXT NOT NULL,
    actor_id            TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    UNIQUE (source_instance_id, restart_stage, plan_fingerprint)
);
CREATE INDEX workflow_lineage_source ON workflow_lineage(source_instance_id);
