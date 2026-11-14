-- Phase 3: revision decisions on approval gates (ADR-0020) and the traceability store
-- (ADR-0022).
--
-- SQLite cannot alter CHECK constraints, so the two approval tables are rebuilt. The child
-- table is rebuilt against the new parent before the old tables are dropped, which keeps
-- foreign-key enforcement on throughout (no PRAGMA changes inside the migration).

CREATE TABLE approvals_v3 (
    id                    TEXT PRIMARY KEY,
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    subject_type          TEXT NOT NULL,
    subject_ref           TEXT NOT NULL,
    workflow_instance_id  TEXT,
    gate                  TEXT,
    artifact_id           TEXT,
    artifact_hash         TEXT,
    authors               TEXT NOT NULL,
    required_role         TEXT NOT NULL,
    requested_by          TEXT NOT NULL,
    status                TEXT NOT NULL CHECK (status IN ('pending', 'approved', 'rejected', 'changes_requested', 'superseded')),
    created_at            TEXT NOT NULL,
    decided_at            TEXT
);
INSERT INTO approvals_v3 SELECT id, tenant_id, project_id, subject_type, subject_ref, workflow_instance_id, gate,
    artifact_id, artifact_hash, authors, required_role, requested_by, status, created_at, decided_at FROM approvals;

CREATE TABLE approval_decisions_v3 (
    id             TEXT PRIMARY KEY,
    approval_id    TEXT NOT NULL REFERENCES approvals_v3(id),
    approver_id    TEXT NOT NULL,
    approver_role  TEXT NOT NULL,
    decision       TEXT NOT NULL CHECK (decision IN ('approve', 'reject', 'request_changes')),
    comments       TEXT NOT NULL,
    artifact_hash  TEXT,
    decided_at     TEXT NOT NULL,
    correlation_id TEXT
);
INSERT INTO approval_decisions_v3 SELECT id, approval_id, approver_id, approver_role, decision, comments,
    artifact_hash, decided_at, correlation_id FROM approval_decisions;

DROP TABLE approval_decisions;
DROP TABLE approvals;
ALTER TABLE approvals_v3 RENAME TO approvals;
ALTER TABLE approval_decisions_v3 RENAME TO approval_decisions;
CREATE INDEX approvals_subject ON approvals(subject_type, subject_ref);
CREATE INDEX approvals_status ON approvals(tenant_id, project_id, status);

-- Traceability links computed by the platform from approved engineering artifacts.
CREATE TABLE trace_links (
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    from_kind             TEXT NOT NULL,
    from_id               TEXT NOT NULL,
    relation              TEXT NOT NULL,
    to_kind               TEXT NOT NULL,
    to_id                 TEXT NOT NULL,
    artifact_id           TEXT NOT NULL,
    artifact_name         TEXT NOT NULL,
    artifact_version      INTEGER NOT NULL,
    artifact_hash         TEXT NOT NULL,
    workflow_instance_id  TEXT NOT NULL,
    approval_id           TEXT NOT NULL,
    recorded_at           TEXT NOT NULL,
    PRIMARY KEY (tenant_id, project_id, from_kind, from_id, relation, to_kind, to_id, artifact_id)
);
CREATE INDEX trace_links_from ON trace_links(tenant_id, project_id, from_id);
CREATE INDEX trace_links_to ON trace_links(tenant_id, project_id, to_id);
