-- Approved design artifacts materialized into the registered project workspace
-- (docs/implementation/stages/ARTIFACT-MATERIALIZATION-PLAN.md, decisions M1–M4).
-- New tables only; no existing table changes.
--
-- The artifact store stays authoritative. These tables record where the application
-- repository of a project is, which files EAEH wrote there (to tell EAEH-owned files from
-- local edits) and every materialization attempt.

-- Registered application repository root of a project (written by `eaeh project init` and
-- `eaeh artifact sync`; one root per project).
CREATE TABLE project_workspaces (
    tenant_id      TEXT NOT NULL,
    project_id     TEXT NOT NULL,
    root_path      TEXT NOT NULL,
    registered_by  TEXT NOT NULL,
    registered_at  TEXT NOT NULL,
    PRIMARY KEY (tenant_id, project_id)
);

-- Files EAEH wrote into the workspace, with the hash it wrote. A file whose current hash
-- differs from `content_hash` was modified locally and is never replaced silently.
CREATE TABLE materialized_files (
    tenant_id        TEXT NOT NULL,
    project_id       TEXT NOT NULL,
    path             TEXT NOT NULL,     -- workspace-relative
    content_hash     TEXT NOT NULL,
    artifact_id      TEXT NOT NULL,     -- the file artifact
    bundle_id        TEXT NOT NULL,     -- the approved bundle it belongs to
    target_dir       TEXT NOT NULL,
    written_at       TEXT NOT NULL,
    PRIMARY KEY (tenant_id, project_id, path)
);
CREATE INDEX materialized_files_dir ON materialized_files(tenant_id, project_id, target_dir);

-- Every materialization attempt (append-only).
CREATE TABLE artifact_materializations (
    id                    TEXT PRIMARY KEY,
    tenant_id             TEXT NOT NULL,
    project_id            TEXT NOT NULL,
    bundle_id             TEXT NOT NULL,
    bundle_name           TEXT NOT NULL,
    bundle_version        INTEGER NOT NULL,
    bundle_hash           TEXT NOT NULL,
    workflow_instance_id  TEXT,
    gate                  TEXT,
    approval_id           TEXT,
    target_dir            TEXT NOT NULL,
    root_path             TEXT,
    status                TEXT NOT NULL CHECK (status IN ('succeeded', 'conflict', 'failed')),
    detail                TEXT NOT NULL,
    files                 TEXT NOT NULL,   -- JSON [{path, hash, action}]
    actor_id              TEXT NOT NULL,
    created_at            TEXT NOT NULL
);
CREATE INDEX artifact_materializations_bundle ON artifact_materializations(tenant_id, project_id, bundle_id, created_at);
CREATE INDEX artifact_materializations_dir ON artifact_materializations(tenant_id, project_id, target_dir, created_at);
