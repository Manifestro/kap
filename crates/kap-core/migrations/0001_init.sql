-- Initial schema for the KAP record of work.
--
-- History is append-only: corrections are new rows, never updates to old ones.

CREATE TABLE attempts (
    id          TEXT PRIMARY KEY,
    label       TEXT NOT NULL,
    task        TEXT,
    state       TEXT NOT NULL CHECK (state IN ('open', 'closed')),
    branch      TEXT NOT NULL UNIQUE,
    worktree    TEXT NOT NULL,
    base_commit TEXT NOT NULL,
    created_at  TEXT NOT NULL,
    closed_at   TEXT
);

CREATE INDEX attempts_created ON attempts(created_at);

CREATE TABLE runs (
    id          TEXT PRIMARY KEY,
    attempt_id  TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    agent       TEXT NOT NULL,
    status      TEXT NOT NULL DEFAULT 'running'
                CHECK (status IN ('running', 'completed', 'failed', 'abandoned')),
    started_at  TEXT NOT NULL,
    finished_at TEXT
);

CREATE INDEX runs_attempt ON runs(attempt_id);

CREATE TABLE tool_events (
    id          TEXT PRIMARY KEY,
    attempt_id  TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    run_id      TEXT REFERENCES runs(id) ON DELETE CASCADE,
    kind        TEXT NOT NULL,
    command     TEXT,
    exit_code   INTEGER,
    output_hash TEXT,
    observed    INTEGER NOT NULL DEFAULT 1 CHECK (observed IN (0, 1)),
    occurred_at TEXT NOT NULL
);

CREATE INDEX tool_events_attempt ON tool_events(attempt_id, occurred_at);

CREATE TABLE decisions (
    id           TEXT PRIMARY KEY,
    attempt_id   TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    summary      TEXT NOT NULL,
    rationale    TEXT,
    alternatives TEXT,
    source       TEXT NOT NULL CHECK (source IN ('observed', 'agent_report')),
    created_at   TEXT NOT NULL
);

CREATE INDEX decisions_attempt ON decisions(attempt_id, created_at);

CREATE TABLE evidence (
    id            TEXT PRIMARY KEY,
    attempt_id    TEXT NOT NULL REFERENCES attempts(id) ON DELETE CASCADE,
    decision_id   TEXT REFERENCES decisions(id) ON DELETE SET NULL,
    source        TEXT NOT NULL
                  CHECK (source IN ('tool_execution', 'file_change', 'test_result',
                                    'agent_report', 'external_artifact')),
    command       TEXT,
    exit_code     INTEGER,
    artifact_hash TEXT,
    observed      INTEGER NOT NULL DEFAULT 1 CHECK (observed IN (0, 1)),
    complete      INTEGER NOT NULL DEFAULT 1 CHECK (complete IN (0, 1)),
    recorded_at   TEXT NOT NULL
);

CREATE INDEX evidence_attempt ON evidence(attempt_id, recorded_at);
