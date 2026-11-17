-- Context delivery of external AI task packages (sections, acknowledgement, traces).
-- Additive: no existing row or column changes meaning.

-- One record per (task, execution attempt): what the AI client retrieved and acknowledged.
CREATE TABLE external_deliveries (
    id                 TEXT PRIMARY KEY,
    task_id            TEXT NOT NULL REFERENCES external_tasks(id),
    execution_id       TEXT NOT NULL,
    package_hash       TEXT NOT NULL,
    package_bytes      INTEGER NOT NULL,
    mandatory_bytes    INTEGER NOT NULL,
    -- JSON: { "<section>": { "class": "mandatory|reference", "total": n, "covered": [[start,end],...] } }
    sections           TEXT NOT NULL,
    manifest_requests  INTEGER NOT NULL DEFAULT 0,
    section_requests   INTEGER NOT NULL DEFAULT 0,
    bytes_served       INTEGER NOT NULL DEFAULT 0,
    status             TEXT NOT NULL CHECK (status IN ('pending', 'partial', 'complete', 'acknowledged', 'failed')),
    failure_reason     TEXT,
    created_at         TEXT NOT NULL,
    updated_at         TEXT NOT NULL,
    acknowledged_at    TEXT,
    UNIQUE (task_id, execution_id)
);
CREATE INDEX external_deliveries_task ON external_deliveries(task_id);

-- Large tool results, stored once (content-addressed per task) so they can be read in pages.
CREATE TABLE external_blobs (
    id          TEXT NOT NULL,
    task_id     TEXT NOT NULL REFERENCES external_tasks(id),
    sha256      TEXT NOT NULL,
    bytes       INTEGER NOT NULL,
    content     TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    PRIMARY KEY (task_id, id)
);

-- Delivery failures are bounded separately from execution attempts and correction attempts.
ALTER TABLE external_tasks ADD COLUMN context_failures INTEGER NOT NULL DEFAULT 0;

-- Execution traces.
ALTER TABLE external_executions ADD COLUMN package_bytes INTEGER;
ALTER TABLE external_executions ADD COLUMN delivery_status TEXT;
ALTER TABLE external_executions ADD COLUMN retrieval_status TEXT;
