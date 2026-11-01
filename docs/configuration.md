# Configuration

[← Back to the README](../README.md) · [All docs](README.md)

Environment variables, the application database and what Odoo needs.

## Environment variables

Every option is an environment variable (or the matching `--flag`).

| Variable | Default | Description |
|---|---|---|
| `ODB_BIND` | `0.0.0.0:8080` | Listen address |
| `ODB_DATA_DIR` | `./data` (`/data` in Docker) | SQLite database, `secret.key`, temp files |
| `ODB_DATABASE_URL` | – | `postgres://user:password@host:5432/db` to keep the application data in PostgreSQL instead of SQLite |
| `ODB_TEMP_DIR` | `<data>/tmp` | Buffer for archives in transit. Needs free space for the largest backup × parallel runs |
| `ODB_SECRET_KEY` | generated | Base64 32-byte key encrypting stored secrets |
| `ODB_MAX_CONCURRENT_JOBS` | `2` | Parallel runs (one at a time per Odoo instance in any case) |
| `ODB_PUBLIC_URL` | – | External URL, used for the OAuth redirect URI and links in notifications |
| `ODB_COOKIE_SECURE` | `false` | Set `true` when served over HTTPS |
| `ODB_SESSION_HOURS` | `168` | Session lifetime |
| `ODB_ADMIN_USERNAME` / `ODB_ADMIN_PASSWORD` | – | Create the first admin non-interactively |
| `ODB_METRICS_TOKEN` | – | Enables `GET /metrics` (`Authorization: Bearer <token>`) |
| `ODB_UPLOAD_CHUNK_MB` | `16` | Google Drive upload chunk size |
| `RUST_LOG` | `info` | Log filter |

## Application database: SQLite or PostgreSQL

By default everything the app knows (instances, destinations, jobs, run history, logs, users, settings) is kept in
`<ODB_DATA_DIR>/odoo-backup.db`, a SQLite file that needs no setup.

To keep it in PostgreSQL instead (PostgreSQL 12 or newer), set `ODB_DATABASE_URL`:

```bash
ODB_DATABASE_URL=postgres://odoo_backup:secret@db.example.com:5432/odoo_backup
# TLS: add ?sslmode=require (or verify-full&sslrootcert=/path/ca.pem)
```

A ready-to-use stack with its own PostgreSQL container:

```bash
echo "ODB_DB_PASSWORD=$(openssl rand -hex 24)" > .env
docker compose -f docker-compose.postgres.yml up -d --build
```

Notes:

- The schema is created and upgraded automatically at startup. The database must exist and the user must be
  allowed to create tables in it.
- **Run a single server per database.** The server holds a PostgreSQL advisory lock and a second one refuses to
  start, because two schedulers would run every backup twice.
- **The data directory is still needed** for temporary buffers and `secret.key`. With PostgreSQL, prefer passing the
  key with `ODB_SECRET_KEY` so the database and the key are backed up and restored together. The server refuses to
  start with a key that does not match the database.
- Use a dedicated database and role, not the ones of your Odoo server: Odoo lists every database its role can
  access, so the app's database would appear among the databases to back up.
- Switching an existing installation from SQLite to PostgreSQL does not copy the data. Start with an empty
  database and recreate the instances, destinations and jobs; old backup files stay where they are.

## Odoo requirements

| Requirement | Why |
|---|---|
| `list_db = True` (default) | Odoo blocks all database-manager functions, including backup and restore, when database listing is disabled. |
| Known **master password** (`admin_passwd`) | Required by `/web/database/backup` and `/restore`. |
| `limit_time_real` large enough (workers mode) | Odoo kills HTTP workers after `limit_time_real` seconds (default 120). Building the archive of a large database takes longer: raise it (e.g. `3600`) or backups will be cut. |
| Reverse proxy timeouts | Odoo prepares the whole archive before sending the first byte. Raise `proxy_read_timeout` (nginx) for `/web/database/`. For restores also raise `client_max_body_size`. |

Example nginx snippet:

```nginx
location /web/database/ {
    proxy_pass http://odoo;
    proxy_read_timeout 3600s;
    client_max_body_size 0;
}
```

Odoo Online (odoo.com SaaS) does not expose the database manager and cannot be backed up this way.
