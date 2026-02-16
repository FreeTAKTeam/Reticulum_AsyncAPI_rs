CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    status TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    failure_reason TEXT
);

CREATE TABLE IF NOT EXISTS job_attempts (
    id INTEGER PRIMARY KEY AUTOINCREMENT,
    job_id TEXT NOT NULL,
    attempt_no INTEGER NOT NULL,
    started_at TEXT NOT NULL,
    finished_at TEXT,
    status TEXT NOT NULL,
    diagnostic TEXT,
    FOREIGN KEY(job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS job_results (
    job_id TEXT PRIMARY KEY,
    result_json TEXT NOT NULL,
    completed_at TEXT NOT NULL,
    FOREIGN KEY(job_id) REFERENCES jobs(job_id)
);

CREATE TABLE IF NOT EXISTS cached_events (
    event_id TEXT PRIMARY KEY,
    event_name TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    received_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS cached_messages (
    message_id TEXT PRIMARY KEY,
    operation TEXT NOT NULL,
    payload_json TEXT NOT NULL,
    received_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS transfers (
    transfer_id TEXT PRIMARY KEY,
    status TEXT NOT NULL,
    metadata_json TEXT NOT NULL,
    submitted_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    failure_reason TEXT
);

CREATE TABLE IF NOT EXISTS acl_allowlist (
    identity_hash TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS acl_denylist (
    identity_hash TEXT PRIMARY KEY,
    note TEXT,
    created_at TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS node_config_revisions (
    revision_id INTEGER PRIMARY KEY AUTOINCREMENT,
    config_json TEXT NOT NULL,
    created_at TEXT NOT NULL
);
