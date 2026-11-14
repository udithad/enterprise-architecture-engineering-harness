-- EAEH Phase 1 initial schema.
-- Ownership: eaeh-store owns the schema and migrations; domain crates own their queries.
-- Conventions: ids are TEXT (UUIDv7 or slug), timestamps are RFC3339 TEXT (UTC),
-- JSON documents are TEXT. Every scoped table carries tenant_id + project_id.

-- ---------------------------------------------------------------- identity
CREATE TABLE tenants (
    id          TEXT PRIMARY KEY,
    name        TEXT NOT NULL UNIQUE,
    created_at  TEXT NOT NULL
);

CREATE TABLE projects (
    id          TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL REFERENCES tenants(id),
    name        TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);

CREATE TABLE principals (
    id            TEXT PRIMARY KEY,
    kind          TEXT NOT NULL CHECK (kind IN ('human', 'service')),
    tenant_id     TEXT NOT NULL REFERENCES tenants(id),
    display_name  TEXT NOT NULL,
    roles         TEXT NOT NULL,          -- JSON array of roles
    projects      TEXT NOT NULL,          -- JSON ProjectAccess
    status        TEXT NOT NULL CHECK (status IN ('active', 'disabled')),
    created_at    TEXT NOT NULL,
    created_by    TEXT
);

CREATE TABLE api_tokens (
    id            TEXT PRIMARY KEY,
    principal_id  TEXT NOT NULL REFERENCES principals(id),
    token_hash    TEXT NOT NULL UNIQUE,   -- SHA-256 of the token; plaintext is never stored
    label         TEXT NOT NULL,
    created_at    TEXT NOT NULL,
    expires_at    TEXT NOT NULL,
    revoked_at    TEXT,
    replaced_by   TEXT,
    last_used_at  TEXT
);
CREATE INDEX api_tokens_principal ON api_tokens(principal_id);

-- ---------------------------------------------------------------- audit
CREATE TABLE audit_log (
    seq             INTEGER PRIMARY KEY,  -- gapless, assigned by the writer (prev + 1)
    id              TEXT NOT NULL UNIQUE,
    occurred_at     TEXT NOT NULL,
    tenant_id       TEXT,
    project_id      TEXT,
    actor_id        TEXT NOT NULL,
    actor_kind      TEXT NOT NULL,
    action          TEXT NOT NULL,
    resource_type   TEXT NOT NULL,
    resource_id     TEXT NOT NULL,
    outcome         TEXT NOT NULL,
    correlation_id  TEXT,
    details         TEXT NOT NULL,
    prev_hash       TEXT NOT NULL,
    hash            TEXT NOT NULL
);
CREATE INDEX audit_log_correlation ON audit_log(correlation_id);

-- ---------------------------------------------------------------- events
CREATE TABLE outbox (
    rowid_seq       INTEGER PRIMARY KEY AUTOINCREMENT,  -- delivery order
    event_id        TEXT NOT NULL UNIQUE,               -- duplicate publish protection
    event_type      TEXT NOT NULL,
    schema_version  INTEGER NOT NULL,
    occurred_at     TEXT NOT NULL,
    tenant_id       TEXT,
    project_id      TEXT,
    correlation_id  TEXT NOT NULL,
    causation_id    TEXT,
    producer        TEXT NOT NULL,
    payload         TEXT NOT NULL,
    status          TEXT NOT NULL CHECK (status IN ('pending', 'dispatched'))
);
CREATE INDEX outbox_status ON outbox(status, rowid_seq);

CREATE TABLE event_deliveries (
    event_id         TEXT NOT NULL REFERENCES outbox(event_id),
    consumer         TEXT NOT NULL,
    status           TEXT NOT NULL CHECK (status IN ('pending', 'done', 'dead')),
    attempts         INTEGER NOT NULL DEFAULT 0,
    next_attempt_at  TEXT NOT NULL,
    last_error       TEXT,
    updated_at       TEXT NOT NULL,
    PRIMARY KEY (event_id, consumer)
);

-- ---------------------------------------------------------------- governance
CREATE TABLE policies (
    id          TEXT NOT NULL,
    version     TEXT NOT NULL,
    level       TEXT NOT NULL,
    document    TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('active', 'retired')),
    created_at  TEXT NOT NULL,
    PRIMARY KEY (id, version)
);

CREATE TABLE approvals (
    id                    TEXT PRIMARY KEY,
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    subject_type          TEXT NOT NULL,      -- workflow_gate | tool_invocation
    subject_ref           TEXT NOT NULL,      -- e.g. workflow instance id + gate, or tool fingerprint
    workflow_instance_id  TEXT,
    gate                  TEXT,
    artifact_id           TEXT,
    artifact_hash         TEXT,
    authors               TEXT NOT NULL,      -- JSON array of principal ids (separation of duties)
    required_role         TEXT NOT NULL,
    requested_by          TEXT NOT NULL,
    status                TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected', 'superseded')),
    created_at            TEXT NOT NULL,
    decided_at            TEXT
);
CREATE INDEX approvals_subject ON approvals(subject_type, subject_ref);
CREATE INDEX approvals_status ON approvals(tenant_id, project_id, status);

CREATE TABLE approval_decisions (
    id             TEXT PRIMARY KEY,
    approval_id    TEXT NOT NULL REFERENCES approvals(id),
    approver_id    TEXT NOT NULL,
    approver_role  TEXT NOT NULL,
    decision       TEXT NOT NULL CHECK (decision IN ('approve', 'reject')),
    comments       TEXT NOT NULL,
    artifact_hash  TEXT,
    decided_at     TEXT NOT NULL,
    correlation_id TEXT
);

CREATE TABLE budget_usage (
    owner_id    TEXT NOT NULL,                -- workflow instance / agent instance / task
    resource    TEXT NOT NULL,
    used        INTEGER NOT NULL,
    max_allowed INTEGER NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (owner_id, resource)
);

-- ---------------------------------------------------------------- extensibility
CREATE TABLE plugins (
    id             TEXT NOT NULL,
    version        TEXT NOT NULL,
    kind           TEXT NOT NULL,
    manifest       TEXT NOT NULL,
    digest         TEXT NOT NULL,
    signed_by      TEXT,
    state          TEXT NOT NULL,
    granted        TEXT NOT NULL,   -- JSON array of granted capabilities
    registered_at  TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    PRIMARY KEY (id, version)
);

-- ---------------------------------------------------------------- intelligence
CREATE TABLE prompt_templates (
    id          TEXT NOT NULL,
    version     TEXT NOT NULL,
    document    TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('draft', 'approved', 'retired')),
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL,
    PRIMARY KEY (id, version)
);

CREATE TABLE model_usage (
    id                 TEXT PRIMARY KEY,
    tenant_id          TEXT NOT NULL,
    project_id         TEXT NOT NULL,
    provider_id        TEXT NOT NULL,
    model_id           TEXT NOT NULL,
    model_version      TEXT NOT NULL,
    input_tokens       INTEGER NOT NULL,
    output_tokens      INTEGER NOT NULL,
    cost_micros        INTEGER NOT NULL,
    outcome            TEXT NOT NULL,
    agent_instance_id  TEXT,
    correlation_id     TEXT,
    context_snapshot   TEXT,
    prompt_refs        TEXT NOT NULL,   -- JSON array of prompt id@version
    occurred_at        TEXT NOT NULL
);

CREATE TABLE context_snapshots (
    hash        TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    items       TEXT NOT NULL,     -- JSON provenance of included items (not content)
    created_at  TEXT NOT NULL
);

-- ---------------------------------------------------------------- agents
CREATE TABLE agent_definitions (
    id          TEXT NOT NULL,
    version     TEXT NOT NULL,
    document    TEXT NOT NULL,
    digest      TEXT NOT NULL,
    status      TEXT NOT NULL CHECK (status IN ('active', 'deprecated', 'retired')),
    created_at  TEXT NOT NULL,
    PRIMARY KEY (id, version)
);

CREATE TABLE agent_instances (
    id                  TEXT PRIMARY KEY,
    definition_id       TEXT NOT NULL,
    definition_version  TEXT NOT NULL,
    tenant_id           TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    task_id             TEXT,
    root_instance_id    TEXT NOT NULL,
    parent_instance_id  TEXT,
    delegation_depth    INTEGER NOT NULL,
    lineage             TEXT NOT NULL,   -- JSON array of definition ids from root
    state               TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (definition_id, definition_version) REFERENCES agent_definitions(id, version)
);
CREATE INDEX agent_instances_state ON agent_instances(tenant_id, project_id, state);
CREATE INDEX agent_instances_parent ON agent_instances(parent_instance_id);

CREATE TABLE agent_messages (
    id              TEXT PRIMARY KEY,
    from_instance   TEXT NOT NULL,
    to_instance     TEXT,
    kind            TEXT NOT NULL,
    task_fingerprint TEXT,
    root_instance_id TEXT NOT NULL,
    payload_hash    TEXT NOT NULL,
    correlation_id  TEXT NOT NULL,
    created_at      TEXT NOT NULL
);
CREATE INDEX agent_messages_fingerprint ON agent_messages(root_instance_id, task_fingerprint);

-- ---------------------------------------------------------------- workflows
CREATE TABLE workflow_definitions (
    id          TEXT NOT NULL,
    version     TEXT NOT NULL,
    document    TEXT NOT NULL,
    digest      TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (id, version)
);

CREATE TABLE workflow_instances (
    id                  TEXT PRIMARY KEY,
    definition_id       TEXT NOT NULL,
    definition_version  TEXT NOT NULL,
    tenant_id           TEXT NOT NULL,
    project_id          TEXT NOT NULL,
    state               TEXT NOT NULL,
    status              TEXT NOT NULL CHECK (status IN ('running', 'waiting_approval', 'succeeded', 'failed', 'cancelled', 'escalated')),
    version             INTEGER NOT NULL,     -- optimistic concurrency
    input               TEXT NOT NULL,
    data                TEXT NOT NULL,        -- JSON workflow memory (state outputs, plan)
    started_by          TEXT NOT NULL,
    correlation_id      TEXT NOT NULL,
    cancel_requested    INTEGER NOT NULL DEFAULT 0,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY (definition_id, definition_version) REFERENCES workflow_definitions(id, version)
);
CREATE INDEX workflow_instances_status ON workflow_instances(status);
CREATE INDEX workflow_instances_scope ON workflow_instances(tenant_id, project_id, id);

CREATE TABLE workflow_transitions (
    instance_id  TEXT NOT NULL REFERENCES workflow_instances(id),
    seq          INTEGER NOT NULL,
    from_state   TEXT NOT NULL,
    to_state     TEXT NOT NULL,
    trigger      TEXT NOT NULL,
    actor_id     TEXT NOT NULL,
    checkpoint   TEXT NOT NULL,   -- JSON snapshot of instance data after the transition
    occurred_at  TEXT NOT NULL,
    PRIMARY KEY (instance_id, seq)
);

CREATE TABLE tasks (
    id                   TEXT PRIMARY KEY,
    workflow_instance_id TEXT REFERENCES workflow_instances(id),
    tenant_id            TEXT NOT NULL,
    project_id           TEXT NOT NULL,
    state_name           TEXT NOT NULL,
    kind                 TEXT NOT NULL,
    idempotency_key      TEXT NOT NULL UNIQUE,
    status               TEXT NOT NULL CHECK (status IN ('pending', 'running', 'succeeded', 'failed', 'interrupted', 'escalated', 'cancelled')),
    attempt              INTEGER NOT NULL,
    input                TEXT NOT NULL,
    output               TEXT,
    error                TEXT,
    agent_instance_id    TEXT,
    heartbeat_at         TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL
);
CREATE INDEX tasks_status ON tasks(status);
CREATE INDEX tasks_instance ON tasks(workflow_instance_id);

-- ---------------------------------------------------------------- workspaces & artifacts
CREATE TABLE workspaces (
    id          TEXT PRIMARY KEY,
    tenant_id   TEXT NOT NULL,
    project_id  TEXT NOT NULL,
    root_path   TEXT NOT NULL,
    owner_id    TEXT NOT NULL,
    state       TEXT NOT NULL CHECK (state IN ('active', 'cleaned')),
    created_at  TEXT NOT NULL,
    cleaned_at  TEXT
);

CREATE TABLE workspace_locks (
    workspace_id  TEXT NOT NULL REFERENCES workspaces(id),
    path          TEXT NOT NULL,     -- relative path, or '*' for the whole workspace
    holder        TEXT NOT NULL,
    acquired_at   TEXT NOT NULL,
    expires_at    TEXT NOT NULL,
    PRIMARY KEY (workspace_id, path)
);

CREATE TABLE artifacts (
    id                    TEXT PRIMARY KEY,
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    name                  TEXT NOT NULL,
    version               INTEGER NOT NULL,
    content_hash          TEXT NOT NULL,
    size_bytes            INTEGER NOT NULL,
    media_type            TEXT NOT NULL,
    provenance            TEXT NOT NULL,
    created_at            TEXT NOT NULL,
    UNIQUE (tenant_id, project_id, name, version)
);

-- ---------------------------------------------------------------- reliability
CREATE TABLE idempotency_keys (
    key         TEXT PRIMARY KEY,
    result      TEXT NOT NULL,
    created_at  TEXT NOT NULL
);
