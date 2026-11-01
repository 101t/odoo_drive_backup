# Development

[← Back to the README](../README.md) · [All docs](README.md)

Architecture, local setup, tests, releases and the REST API.

## Architecture

```text
              ┌────────────── odoo-backup (single binary) ──────────────┐
 Browser ───► │ Axum HTTP API + embedded React UI ◄── SSE live events    │
              │        │                                                 │
              │   Scheduler (cron, tz) ──► Engine (queue, per-instance   │
              │                             lock, N parallel runs)       │
              │                               │                          │
              │  1. POST /web/database/backup ─┼──► Odoo instance(s)      │
              │     stream ► temp file + SHA-256/MD5                     │
              │  2. verify ZIP (CRC of every entry, dump.sql, manifest)  │
              │  3. upload ► Google Drive (resumable, MD5 check)         │
              │           ► local / NAS folder (atomic rename)           │
              │  4. GFS retention, 5. notifications, 6. delete temp file │
              │                                                          │
              │  SQLite or PostgreSQL: instances, jobs, runs, logs…      │
              └──────────────────────────────────────────────────────────┘
```

## Local setup

```bash
make dev-backend     # API on :8080 (data in ./data)
make dev-frontend    # Vite on :5173 with hot reload, proxies /api to :8080
make test            # cargo test (unit + mock Google Drive + mock storage) and the TS type-check
make lint
```

The database tests run on SQLite, and also on PostgreSQL when `ODB_TEST_POSTGRES_URL` points at a server where the
user may create databases (a throwaway database is created and dropped):

```bash
ODB_TEST_POSTGRES_URL=postgres://postgres:postgres@127.0.0.2:5432/postgres cargo test db::
```

```text
backend/            Rust (Axum, SQLx, Tokio)
  migrations/       SQL schema, one folder per engine (sqlite/, postgres/)
  src/db/           SQLite/PostgreSQL access: queries are written once and run on both
  src/api/          REST handlers, SSE, metrics
  src/engine/       queue, backup and restore pipelines, verification, GFS retention
  src/odoo/         database-manager client
  src/storage/      Google Drive (resumable upload) and local destinations
  src/scheduler.rs  cron scheduling and housekeeping
frontend/           React 19, TypeScript, Tailwind CSS 4, TanStack Query
  src/i18n/         en.ts (source), ar.ts / fr.ts / tr.ts / es.ts (typed: a missing key fails the build), server.ts (Arabic server messages)
```

To add a language, copy `fr.ts`, translate it and register it in `src/i18n/index.ts` (see
[CONTRIBUTING.md](../CONTRIBUTING.md)). Server-generated texts (run logs, API errors) are translated for Arabic only
(`server.ts`); other languages show them in English.

The release build embeds `frontend/dist` into the binary (`rust-embed`), so `make build` produces a single file
to deploy.

### CI and releases

`.github/workflows/ci.yml` runs on every pull request and push to `main`: rustfmt, clippy, the tests (SQLite and
PostgreSQL), the TS type-check, then it builds the Docker image and checks that the container starts and reports
the expected version on `/api/health`. Pushes to `main` publish `ghcr.io/101t/odoo-backup:edge`.

The same run calls `.github/workflows/bundles.yml`, which builds the server bundles from `packaging/` and installs
each one before it can be released:

- **Linux:** static musl binaries for x86_64 and aarch64, packaged with nfpm (`packaging/linux/nfpm.yaml`). They
  are installed with `install.sh` on the systemd runner and on Ubuntu, Debian, AlmaLinux, Fedora, openSUSE and Arch
  containers (`packaging/linux/test-install.sh`).
- **Windows:** the service bundle is installed, restarted and removed on Windows Server 2025.

Download the artifacts from the run's summary page to try a build before it is released.

The version lives in the root `Cargo.toml` (the binary, `--version` and `/api/health` report it). To release:

```bash
# set version = "0.0.2" in Cargo.toml, then:
cargo check                       # updates Cargo.lock
git commit -am "chore: release 0.0.2"
git tag v0.0.2
git push origin main v0.0.2
```

The tag publishes `ghcr.io/101t/odoo-backup:0.0.2`, `:0.0` and `:latest`. It also creates the GitHub release with
the server bundles, `install.sh`, `SHA256SUMS` and build provenance attestations. The run fails if the tag does not
match the version in `Cargo.toml`.

### API overview

All endpoints are under `/api`, return JSON, and require the session cookie except `health` and `auth/*`.

| Endpoint | Purpose |
|---|---|
| `GET /dashboard` | Health overview |
| `GET /events` | SSE stream of run updates, progress and logs |
| `GET, POST /instances`, `GET, PUT, DELETE /instances/{id}`, `POST /instances/{id}/refresh`, `POST /instances/test` | Odoo instances |
| `GET, POST /destinations`, `PUT, DELETE /destinations/{id}`, `POST /destinations/{id}/test`, `POST /destinations/{id}/oauth/start` | Storage destinations |
| `GET, POST /jobs`, `PUT, DELETE /jobs/{id}`, `POST /jobs/{id}/run`, `POST /cron/preview` | Scheduled jobs |
| `POST /backups` | Ad-hoc backup `{instance_id, databases, destination_ids, format}` |
| `GET /runs?status=&instance_id=&job_id=&kind=&search=`, `GET, DELETE /runs/{id}`, `POST /runs/{id}/cancel` | History |
| `GET /artifacts/{id}/download`, `POST /artifacts/{id}/restore`, `PUT /artifacts/{id}/pin {pinned}`, `DELETE /artifacts/{id}` | Stored files |
| `GET, POST /notifications`, `PUT, DELETE /notifications/{id}`, `POST /notifications/{id}/test` | Alerts |
| `GET, PUT /settings` | Preferences and system information |
