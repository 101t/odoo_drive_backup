-- Initial schema for odoo-backup.
-- All timestamps are RFC 3339 strings in UTC written by the application.

CREATE TABLE users (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    username      TEXT    NOT NULL UNIQUE COLLATE NOCASE,
    password_hash TEXT    NOT NULL,
    created_at    TEXT    NOT NULL
);

CREATE TABLE sessions (
    token_hash TEXT    PRIMARY KEY,
    user_id    INTEGER NOT NULL REFERENCES users(id) ON DELETE CASCADE,
    created_at TEXT    NOT NULL,
    expires_at TEXT    NOT NULL
);

CREATE TABLE instances (
    id                  INTEGER PRIMARY KEY AUTOINCREMENT,
    name                TEXT    NOT NULL UNIQUE,
    url                 TEXT    NOT NULL,
    master_password_enc TEXT,
    verify_tls          INTEGER NOT NULL DEFAULT 1,
    timeout_secs        INTEGER NOT NULL DEFAULT 3600,
    notes               TEXT    NOT NULL DEFAULT '',
    odoo_version        TEXT,
    databases_json      TEXT    NOT NULL DEFAULT '[]',
    last_checked_at     TEXT,
    last_check_error    TEXT,
    created_at          TEXT    NOT NULL,
    updated_at          TEXT    NOT NULL
);

CREATE TABLE destinations (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL UNIQUE,
    kind        TEXT    NOT NULL,
    config_json TEXT    NOT NULL DEFAULT '{}',
    secret_enc  TEXT,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE jobs (
    id             INTEGER PRIMARY KEY AUTOINCREMENT,
    name           TEXT    NOT NULL,
    instance_id    INTEGER NOT NULL REFERENCES instances(id) ON DELETE CASCADE,
    all_databases  INTEGER NOT NULL DEFAULT 0,
    databases_json TEXT    NOT NULL DEFAULT '[]',
    format         TEXT    NOT NULL DEFAULT 'zip',
    cron           TEXT    NOT NULL,
    timezone       TEXT    NOT NULL DEFAULT 'UTC',
    enabled        INTEGER NOT NULL DEFAULT 1,
    keep_last      INTEGER NOT NULL DEFAULT 7,
    keep_daily     INTEGER NOT NULL DEFAULT 7,
    keep_weekly    INTEGER NOT NULL DEFAULT 4,
    keep_monthly   INTEGER NOT NULL DEFAULT 6,
    keep_yearly    INTEGER NOT NULL DEFAULT 0,
    retries        INTEGER NOT NULL DEFAULT 2,
    verify_archive INTEGER NOT NULL DEFAULT 1,
    next_run_at    TEXT,
    last_run_at    TEXT,
    created_at     TEXT    NOT NULL,
    updated_at     TEXT    NOT NULL
);

CREATE TABLE job_destinations (
    job_id         INTEGER NOT NULL REFERENCES jobs(id) ON DELETE CASCADE,
    destination_id INTEGER NOT NULL REFERENCES destinations(id) ON DELETE CASCADE,
    PRIMARY KEY (job_id, destination_id)
);

CREATE TABLE runs (
    id                 INTEGER PRIMARY KEY AUTOINCREMENT,
    kind               TEXT    NOT NULL DEFAULT 'backup',
    job_id             INTEGER REFERENCES jobs(id) ON DELETE SET NULL,
    instance_id        INTEGER REFERENCES instances(id) ON DELETE SET NULL,
    instance_name      TEXT    NOT NULL,
    database           TEXT    NOT NULL,
    format             TEXT    NOT NULL,
    trigger            TEXT    NOT NULL,
    status             TEXT    NOT NULL,
    phase              TEXT,
    error              TEXT,
    file_name          TEXT,
    size_bytes         INTEGER,
    sha256             TEXT,
    md5                TEXT,
    odoo_version       TEXT,
    source_artifact_id INTEGER,
    restore_options    TEXT,
    attempt            INTEGER NOT NULL DEFAULT 0,
    created_at         TEXT    NOT NULL,
    started_at         TEXT,
    finished_at        TEXT
);
CREATE INDEX idx_runs_created     ON runs(created_at DESC);
CREATE INDEX idx_runs_status      ON runs(status);
CREATE INDEX idx_runs_job_db      ON runs(job_id, database);
CREATE INDEX idx_runs_instance_db ON runs(instance_id, database, status);

CREATE TABLE artifacts (
    id               INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id           INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    destination_id   INTEGER REFERENCES destinations(id) ON DELETE SET NULL,
    destination_name TEXT    NOT NULL,
    destination_kind TEXT    NOT NULL,
    status           TEXT    NOT NULL,
    remote_id        TEXT,
    remote_path      TEXT,
    web_url          TEXT,
    size_bytes       INTEGER,
    error            TEXT,
    created_at       TEXT    NOT NULL,
    stored_at        TEXT,
    deleted_at       TEXT,
    delete_reason    TEXT
);
CREATE INDEX idx_artifacts_run  ON artifacts(run_id);
CREATE INDEX idx_artifacts_dest ON artifacts(destination_id, status);

CREATE TABLE run_logs (
    id      INTEGER PRIMARY KEY AUTOINCREMENT,
    run_id  INTEGER NOT NULL REFERENCES runs(id) ON DELETE CASCADE,
    ts      TEXT    NOT NULL,
    level   TEXT    NOT NULL,
    message TEXT    NOT NULL
);
CREATE INDEX idx_run_logs_run ON run_logs(run_id, id);

CREATE TABLE notification_channels (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    name        TEXT    NOT NULL,
    kind        TEXT    NOT NULL,
    config_json TEXT    NOT NULL DEFAULT '{}',
    secret_enc  TEXT,
    on_success  INTEGER NOT NULL DEFAULT 0,
    on_failure  INTEGER NOT NULL DEFAULT 1,
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT    NOT NULL,
    updated_at  TEXT    NOT NULL
);

CREATE TABLE settings (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
);
