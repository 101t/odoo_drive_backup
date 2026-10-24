-- Pinned (legal hold) backups, backup sanity warnings and the archive manifest of each run.

ALTER TABLE artifacts ADD COLUMN pinned BOOLEAN NOT NULL DEFAULT FALSE;
ALTER TABLE runs ADD COLUMN warning TEXT;
ALTER TABLE runs ADD COLUMN manifest_json TEXT;
