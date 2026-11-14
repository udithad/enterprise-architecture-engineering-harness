-- EAEH Phase 2: enterprise knowledge, skills, ABAC attributes, traceability columns.

-- ---------------------------------------------------------------- ABAC attributes
ALTER TABLE projects ADD COLUMN domain TEXT;
ALTER TABLE principals ADD COLUMN teams TEXT NOT NULL DEFAULT '[]';

-- ---------------------------------------------------------------- traceability
ALTER TABLE model_usage ADD COLUMN skill_refs TEXT NOT NULL DEFAULT '[]';
ALTER TABLE model_usage ADD COLUMN knowledge_refs TEXT NOT NULL DEFAULT '[]';

-- ---------------------------------------------------------------- knowledge
CREATE TABLE knowledge_sources (
    id                      TEXT PRIMARY KEY,
    tenant_id               TEXT NOT NULL REFERENCES tenants(id),
    name                    TEXT NOT NULL,
    kind                    TEXT NOT NULL CHECK (kind IN ('standard', 'reference', 'api_spec', 'adr', 'process')),
    description             TEXT NOT NULL,
    owner_id                TEXT NOT NULL,
    created_at              TEXT NOT NULL,
    UNIQUE (tenant_id, name)
);

CREATE TABLE knowledge_documents (
    id                  TEXT PRIMARY KEY,
    tenant_id           TEXT NOT NULL,
    source_id           TEXT NOT NULL REFERENCES knowledge_sources(id),
    subject             TEXT NOT NULL,
    version             INTEGER NOT NULL,
    title               TEXT NOT NULL,
    format              TEXT NOT NULL CHECK (format IN ('markdown', 'text', 'json', 'openapi')),
    content             TEXT,                    -- NULL after deletion (purged)
    content_hash        TEXT NOT NULL,
    scope_level         TEXT NOT NULL CHECK (scope_level IN ('enterprise', 'domain', 'project', 'team', 'workflow')),
    scope_ref           TEXT NOT NULL,           -- '' for enterprise; domain/team name; project/workflow id
    classification      TEXT NOT NULL CHECK (classification IN ('public', 'internal', 'confidential', 'restricted')),
    classification_rank INTEGER NOT NULL,
    tags                TEXT NOT NULL,           -- JSON array
    mandatory           INTEGER NOT NULL DEFAULT 0,
    review_by           TEXT,
    status              TEXT NOT NULL CHECK (status IN ('quarantined', 'pending_approval', 'active', 'rejected', 'superseded', 'retired', 'deleted')),
    validation          TEXT NOT NULL,           -- JSON findings
    approval_id         TEXT,
    governance_project  TEXT NOT NULL,           -- project used as the approval scope
    created_by          TEXT NOT NULL,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    UNIQUE (source_id, subject, version)
);
CREATE INDEX knowledge_documents_visibility ON knowledge_documents(tenant_id, status, scope_level, scope_ref);
CREATE INDEX knowledge_documents_subject ON knowledge_documents(tenant_id, subject);

CREATE TABLE knowledge_chunks (
    rowid           INTEGER PRIMARY KEY AUTOINCREMENT,
    id              TEXT NOT NULL UNIQUE,
    document_id     TEXT NOT NULL REFERENCES knowledge_documents(id),
    tenant_id       TEXT NOT NULL,
    ordinal         INTEGER NOT NULL,
    heading         TEXT NOT NULL,
    text            TEXT NOT NULL,
    token_estimate  INTEGER NOT NULL,
    UNIQUE (document_id, ordinal)
);

-- Full-text index (BM25) over chunk text and headings; maintained explicitly by eaeh-knowledge.
CREATE VIRTUAL TABLE knowledge_chunks_fts USING fts5(
    text, heading, content='knowledge_chunks', content_rowid='rowid', tokenize='porter unicode61'
);

CREATE TABLE knowledge_embeddings (
    chunk_id    TEXT PRIMARY KEY REFERENCES knowledge_chunks(id),
    embedder    TEXT NOT NULL,
    dims        INTEGER NOT NULL,
    vector      BLOB NOT NULL
);

CREATE TABLE knowledge_conflicts (
    id           TEXT PRIMARY KEY,
    tenant_id    TEXT NOT NULL,
    subject      TEXT NOT NULL,
    document_a   TEXT NOT NULL REFERENCES knowledge_documents(id),
    document_b   TEXT NOT NULL REFERENCES knowledge_documents(id),
    detected_at  TEXT NOT NULL,
    resolution   TEXT NOT NULL      -- precedence decision recorded at detection time
);
CREATE INDEX knowledge_conflicts_subject ON knowledge_conflicts(tenant_id, subject);

-- ---------------------------------------------------------------- skills
CREATE TABLE skills (
    id             TEXT NOT NULL,
    version        TEXT NOT NULL,
    document       TEXT NOT NULL,
    digest         TEXT NOT NULL,
    status         TEXT NOT NULL CHECK (status IN ('pending_approval', 'approved', 'rejected', 'deprecated', 'retired')),
    approval_id    TEXT,
    registered_by  TEXT NOT NULL,
    created_at     TEXT NOT NULL,
    updated_at     TEXT NOT NULL,
    PRIMARY KEY (id, version)
);
