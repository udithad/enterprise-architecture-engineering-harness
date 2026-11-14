-- External AI execution (ADR-0025): agent tasks handed off to a connected AI client
-- (Claude Code, GitHub Copilot) instead of an EAEH-managed model provider.
--
-- Adds the `waiting_external` instance status and the `awaiting_external` task status.
-- SQLite cannot alter CHECK constraints, so the three workflow tables are rebuilt; children
-- are rebuilt against the new parent before the old tables are dropped (same pattern as 0003).

CREATE TABLE workflow_instances_v4 (
    id                  TEXT PRIMARY KEY,
    definition_id       TEXT NOT NULL,
    definition_version  TEXT NOT NULL,
    tenant_id           TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    state               TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('running', 'waiting_approval', 'waiting_external', 'succeeded', 'failed', 'cancelled', 'escalated')),
    version             INTEGER NOT NULL,
    input               TEXT NOT NULL,
    data                TEXT NOT NULL,
    started_by          TEXT NOT NULL,
    correlation_id      TEXT NOT NULL,
    cancel_requested    INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (definition_id, definition_version) REFERENCES workflow_definitions(id, version)
);
INSERT INTO workflow_instances_v4 SELECT id, definition_id, definition_version, tenant_id, project_id, state, status,
    version, input, data, started_by, correlation_id, cancel_requested, created_at, updated_at FROM workflow_instances;

CREATE TABLE workflow_transitions_v4 (
    instance_id  TEXT NOT NULL REFERENCES workflow_instances_v4(id),
    seq          INTEGER NOT NULL,
    from_state   TEXT NOT NULL,
    to_state     TEXT NOT NULL,
    trigger      TEXT NOT NULL,
    actor_id     TEXT NOT NULL,
    checkpoint   TEXT NOT NULL,
    occurred_at  TEXT NOT NULL,
    PRIMARY KEY (instance_id, seq)
);
INSERT INTO workflow_transitions_v4 SELECT instance_id, seq, from_state, to_state, trigger, actor_id, checkpoint,
    occurred_at FROM workflow_transitions;

CREATE TABLE tasks_v4 (
    id                   TEXT PRIMARY KEY,
    workflow_instance_id TEXT REFERENCES workflow_instances_v4(id),
    tenant_id            TEXT NOT NULL,
    project_id           TEXT NOT NULL,
    state_name           TEXT NOT NULL,
    kind                 TEXT NOT NULL,
    idempotency_key      TEXT NOT NULL UNIQUE,
    status               TEXT NOT NULL CHECK (status IN ('pending', 'running', 'awaiting_external', 'succeeded', 'failed', 'interrupted', 'escalated', 'cancelled')),
    attempt              INTEGER NOT NULL,
    input                TEXT NOT NULL,
    output               TEXT,
    error                TEXT,
    agent_instance_id    TEXT,
    heartbeat_at         TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
INSERT INTO tasks_v4 SELECT id, workflow_instance_id, tenant_id, project_id, state_name, kind, idempotency_key, status,
    attempt, input, output, error, agent_instance_id, heartbeat_at, created_at, updated_at FROM tasks;

DROP TABLE tasks;
DROP TABLE workflow_transitions;
DROP TABLE workflow_instances;
ALTER TABLE workflow_instances_v4 RENAME TO workflow_instances;
ALTER TABLE workflow_transitions_v4 RENAME TO workflow_transitions;
ALTER TABLE tasks_v4 RENAME TO tasks;
CREATE INDEX workflow_instances_status ON workflow_instances(status);
CREATE INDEX workflow_instances_scope ON workflow_instances(tenant_id, project_id, id);
CREATE INDEX tasks_status ON tasks(status);
CREATE INDEX tasks_instance ON tasks(workflow_instance_id);

-- One row per workflow agent task handed off to an external AI client. `package` is the
-- complete, platform-assembled task specification; `retrieved` the citations the task may
-- cite (package excerpts plus governed task-tool results). Submissions are untrusted.
CREATE TABLE external_tasks (
    id                    TEXT PRIMARY KEY REFERENCES tasks(id),
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    workflow_instance_id  TEXT NOT NULL REFERENCES workflow_instances(id),
    state_name            TEXT NOT NULL,
    task_kind             TEXT NOT NULL,
    agent_instance_id     TEXT NOT NULL,
    agent_definition      TEXT NOT NULL,
    workspace_id          TEXT,
    trace                 TEXT NOT NULL,
    budget_owners         TEXT NOT NULL,
    status                TEXT NOT NULL CHECK (status IN ('awaiting', 'submitted', 'accepted', 'failed', 'cancelled')),
    package               TEXT NOT NULL,
    package_hash          TEXT NOT NULL,
    retrieved             TEXT NOT NULL,
    tool_invocations      TEXT NOT NULL,
    submission            TEXT,
    submitted_by          TEXT,
    client                TEXT,
    submitted_at          TEXT,
    feedback              TEXT NOT NULL,
    submissions           INTEGER NOT NULL DEFAULT 0,
    created_at            TEXT NOT NULL,
    updated_at            TEXT NOT NULL
);
CREATE INDEX external_tasks_scope ON external_tasks(tenant_id, project_id, status);
CREATE INDEX external_tasks_instance ON external_tasks(workflow_instance_id);
