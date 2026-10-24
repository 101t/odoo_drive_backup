# Odoo Backup

[![CI](https://github.com/101t/odoo_drive_backup/actions/workflows/ci.yml/badge.svg)](https://github.com/101t/odoo_drive_backup/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/license-MIT-blue.svg)](LICENSE)
[![Release](https://img.shields.io/github/v/release/101t/odoo_drive_backup)](https://github.com/101t/odoo_drive_backup/releases)

Automated, verified backups of any number of Odoo instances to Google Drive and local/NAS storage,
with a web dashboard. One self-contained binary (Rust + embedded React UI), one Docker image, and the application
data in a SQLite file or in PostgreSQL.

- **Any Odoo, no addon required.** Uses Odoo's own database manager (`/web/database/backup`), the same
  endpoint as the *Backup* button. Works with Odoo 12 to 18, Community and Enterprise, on-premise or Odoo.sh-style
  self-hosting. Databases are discovered automatically.
- **Safe transfer.** The archive is streamed to a local buffer file while SHA-256 and MD5 are computed. Nothing is
  held in memory.
- **Verified before shipping.** Every ZIP is opened and every entry's CRC is checked. `dump.sql` must be present and
  `manifest.json` is read (Odoo version, modules).
- **Google Drive done right.**
  - Resumable chunked uploads survive dropped connections: they resume at the exact byte, and an expired session
    restarts the upload.
  - Drive's MD5 is compared with the local one, so the copy is verified end to end.
  - Supports a service account in a Shared Drive (Workspace) or OAuth with a personal account (in-app
    *Connect Google account* flow).
- **Several destinations per job** (e.g. Google Drive + NAS), uploaded in parallel. Each job shows a
  **3-2-1-1-0 score** (3 copies, 2 media, 1 off-site, 1 immutable, 0 errors) with hints for what is missing.
- **Sanity checks**: every archive's contents (dump size, filestore files, modules) are recorded and compared
  with the previous backup. A backup that shrank suspiciously is flagged and alerted, even though it "succeeded".
- **Schedules** with cron expressions and time zones. A run missed while the service was down is executed at startup.
- **GFS retention**: keep the last *N*, plus *N* daily, weekly, monthly and yearly backups. Pruning is applied per
  destination, and the newest backup is never deleted. **Pin** a backup (e.g. "before upgrade") to keep it forever.
- **One-click restore** of any stored backup into a new database on any instance, with "copy" (new UUID) and
  *neutralize* options. Use it to test your backups.
- **Live dashboard.** Database health (stale/failing detection), per-phase progress bars with throughput via
  Server-Sent Events, full logs per run, history, downloads.
- **Alerts** through Slack/Mattermost/Google Chat/Discord webhooks, Telegram or e-mail (SMTP), plus **heartbeat**
  pings to healthchecks.io / Uptime Kuma so a dead server is noticed too. Also a Prometheus `/metrics` endpoint and
  `/api/health`.
- **Multilingual UI**: English, Arabic (full right-to-left layout), French, Turkish and Spanish.
- **SQLite or PostgreSQL** for the application data: zero setup by default, or a managed PostgreSQL server.
- **Secure by default.**
  - Master passwords, keys and tokens are encrypted at rest (AES-256-GCM).
  - Login uses Argon2id hashes with lockout on repeated failures.
  - Sessions use HttpOnly cookies, and secrets are never sent back to the browser.

## How it works

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

## Quick start (Docker)

```bash
docker compose up -d --build
open http://localhost:8080        # create the admin account on first visit
```

Released images are published to GitHub Container Registry: `ghcr.io/101t/odoo-backup:<version>` (e.g. `0.0.2`),
`latest` for the newest release and `edge` for the current `main`. To run one instead of building locally, replace
`build: .` and `image: odoo-backup:latest` in the compose file with `image: ghcr.io/101t/odoo-backup:0.0.2`.

Then, in the UI:

1. **Instances → Add instance**: Odoo URL and master password. Use *Test connection* to check the version, the
   database list and the master password.
2. **Destinations → Add destination**: Google Drive (see below) or *Local / NAS folder* with path `/backups`.
   Click *Test*: it writes and deletes a small file to prove the setup works.
3. **Backup jobs → New job**: pick databases (or *All databases*), a schedule, destinations and retention.
   Use ▶ to run it immediately.

> **Back up the `/data` volume, or at least `/data/secret.key`.** It contains the key that decrypts the stored
> master passwords and Google credentials. Alternatively pass your own key with `ODB_SECRET_KEY`
> (`docker compose run --rm odoo-backup generate-key`).

## Install on a server (without Docker)

Each [release](https://github.com/101t/odoo_drive_backup/releases) ships native bundles that install Odoo Backup
as a system service. The Linux binary is static, so it does not depend on the distribution's libraries.

### Linux

One command picks the right package for the distribution: `.deb` (Debian, Ubuntu), `.rpm` (RHEL, Rocky,
AlmaLinux, CentOS Stream, Fedora, openSUSE) or `.pkg.tar.zst` (Arch). It checks the package against the release's
`SHA256SUMS` and installs it with apt, dnf/yum/zypper or pacman:

```bash
curl -fsSL https://github.com/101t/odoo_drive_backup/releases/latest/download/install.sh | sudo sh
```

Then open `http://<server>:8080` and create the admin account.

| What | Where |
|------|-------|
| Service | `systemctl status odoo-backup`, runs as the unprivileged `odoo-backup` user with systemd hardening |
| Settings | `/etc/odoo-backup/odoo-backup.env` (then `sudo systemctl restart odoo-backup`) |
| Data, `secret.key` | `/var/lib/odoo-backup` (back it up) |
| Local backups | `/var/backups/odoo-backup`: use this path for a *Local folder* destination |
| Logs | `journalctl -u odoo-backup` |

- **Upgrade:** run the same command again. Your settings and data are kept.
- **Specific version:** `curl -fsSL .../install.sh | sudo sh -s -- --version 0.0.2`
- **Offline servers:** copy the package, `SHA256SUMS` and `install.sh` to the server, then run
  `sudo sh install.sh --from <that folder>`.
- **Uninstall:** `sudo sh install.sh --uninstall`. The data and settings are kept.
- **Install the package directly:** `sudo apt install ./odoo-backup_<version>-1_amd64.deb`,
  `sudo dnf install ./odoo-backup-<version>-1.x86_64.rpm` or `sudo pacman -U odoo-backup-<version>-1-x86_64.pkg.tar.zst`.
- **Other distributions** (Alpine, Gentoo, NixOS…): `odoo-backup-<version>-<arch>-linux.tar.gz` holds the static
  binary, the systemd unit and the settings template.

The service may write only to its data directory and to folders it owns outside `/usr`, `/etc`, `/boot` and
`/home`. Before using a NAS mount or another folder as a destination, give it to the service user:
`sudo chown odoo-backup: /mnt/nas/odoo`.

To check that a download was built by this repository's CI: `gh attestation verify <file> --repo 101t/odoo_drive_backup`.

### Windows Server

Download `odoo-backup-<version>-x86_64-windows.zip`, extract it, and in PowerShell opened as Administrator run:

```powershell
powershell -ExecutionPolicy Bypass -File .\install.ps1 -OpenFirewall
```

This installs the `OdooBackup` service, which starts automatically and restarts after a crash. It runs as the
virtual account `NT SERVICE\OdooBackup`, not as LocalSystem.

- **Program:** `C:\Program Files\OdooBackup`.
- **Settings, data and logs:** `C:\ProgramData\OdooBackup`. Only Administrators, SYSTEM and the service can open
  this folder.
- **Settings file:** `odoo-backup.env` in that folder; run `Restart-Service OdooBackup` after a change.
- **Upgrade:** run `install.ps1` from a newer zip.
- **Uninstall:** `install.ps1 -Uninstall`. The data is kept.
- **Local folder destinations:** grant the service access first, for example
  `icacls D:\OdooBackups /grant "NT SERVICE\OdooBackup:(OI)(CI)M"`.

### From source

```bash
make build                      # needs Node 22+ and Rust 1.88+
./target/release/odoo-backup --data-dir /var/lib/odoo-backup serve
```

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

## Languages

The UI is available in **English**, **Arabic** (العربية), **French** (Français), **Turkish** (Türkçe) and
**Spanish** (Español); in Arabic the whole layout is right-to-left. The language is detected from the browser and
can be chosen from the drop-down in the sidebar or on the sign-in page; the choice is remembered per browser.

Technical values (URLs, paths, database names, cron expressions, hashes) always read left to right. Run logs and
error messages are stored in English, so they stay searchable and easy to share. They are translated when displayed,
for Arabic only; the other languages show them in English. Notification messages (e-mail, chat) are sent in English.

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

## Google Drive setup

### Option A: Service account + Shared Drive (Google Workspace)

Service accounts have **no Drive storage of their own**. Uploads to a regular "My Drive" folder fail with
`storageQuotaExceeded`. Use a Shared Drive:

1. In Google Cloud console, create a project, enable the **Google Drive API**, and create a **service account**.
   Under *Keys → Add key → JSON*, download the key file.
2. In Google Drive, create a **Shared Drive** (or a folder inside one) and add the service account e-mail as a member
   with the **Content manager** role.
3. Create a *Google Drive* destination with *Service account*: upload the JSON key and paste the folder link.

Advanced: with domain-wide delegation you can instead *impersonate* a Workspace user (files then count against
that user's quota).

### Option B: Google account via OAuth (personal Gmail or Workspace)

1. In Google Cloud console, enable the **Google Drive API**. Under *APIs & Services → Credentials*, create an
   **OAuth client ID** of type **Web application**.
2. Add the authorized redirect URI shown in the destination form: `<ODB_PUBLIC_URL>/api/oauth/google/callback`
   (e.g. `http://localhost:8080/api/oauth/google/callback`).
3. On the OAuth consent screen, set the publishing status to **In production**. Apps left in *Testing* receive
   refresh tokens that expire after 7 days. An unverified app works for your own account after clicking through
   the warning.
4. Create a *Google Drive* destination with *Google account (OAuth)*, enter the client ID and secret, then click
   **Save & connect Google account**. Leave the folder empty to use `My Drive/Odoo Backups`.

Files are organised as `<folder>/<instance>/<database>/<instance>_<database>_<YYYY-MM-DD_HH-MM-SS>.zip`. The
sub-folders can be turned off per destination.

## Retention

Each job has a grandfather-father-son policy, applied after every successful run, per destination, in the job's
time zone:

| Setting | Keeps |
|---|---|
| Last *N* | the *N* most recent backups |
| Daily *N* | the newest backup of each of the last *N* days that have backups |
| Weekly *N* | the newest backup of each of the last *N* ISO weeks |
| Monthly *N* | the newest backup of each of the last *N* months |
| Yearly *N* | the newest backup of each of the last *N* years |

A backup is kept if any rule keeps it, and the newest backup is always kept. All zeros means "keep everything".
Manual (*Back up now*) backups are never pruned automatically.

**Pinned** copies (pin icon next to a stored copy in the run page) are outside the policy: retention never deletes
them and they do not use up its slots. A pinned copy cannot be deleted until it is unpinned.

## Restore

Open a run in **History**. Next to each stored copy, use **Restore…** and pick the target instance and a **new**
database name. Existing databases are never overwritten. Options:

- **This database is a copy**: generates a new database UUID. Keep it on for test restores next to production.
  Turn it off when rebuilding a lost server.
- **Neutralize** (Odoo 16+): disables mail servers, crons, payment providers and other integrations in the
  restored copy.

Backups can also be downloaded from the UI. They are standard Odoo archives, so you can restore them manually with
Odoo's database manager.

## Configuration

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

## Monitoring

- **Notifications** (Settings): on failure (default) and/or success. Webhook payloads are Slack-compatible
  (`{"text": …}`), Discord, or generic JSON with the full run details. Failure channels also receive backups that
  succeeded with a sanity warning (shrank by more than the threshold in Settings, 30% by default).
- **Heartbeat** channel (dead-man's switch): pings a [healthchecks.io](https://healthchecks.io) URL (`/fail` on
  failure), an Uptime Kuma push monitor (`status=up|down`), or a plain URL on success only. Set the monitor's
  period to the job schedule: if pings stop, the monitor alerts you even when this server is down.
- **Prometheus**: set `ODB_METRICS_TOKEN` and scrape `/metrics`. Available series:
  - `odoo_backup_last_success_timestamp_seconds{instance,database}`
  - `odoo_backup_last_run_ok`
  - `odoo_backup_stale`
  - `odoo_backup_last_size_bytes`
  - `odoo_backup_destination_bytes`
  - `odoo_backup_active_runs`
  - `odoo_backup_last_run_warning{instance,database}`: 1 when the last backup raised a sanity warning
  - `odoo_backup_policy_score{job}`: 3-2-1-1-0 rules met (0 to 5)

  Example alert: `time() - odoo_backup_last_success_timestamp_seconds > 26*3600`.
- **Health**: `GET /api/health` (no authentication), or `odoo-backup healthcheck` inside the container.

## Security notes

- Put the service behind HTTPS (reverse proxy) and set `ODB_COOKIE_SECURE=true`. The UI holds the keys to your
  databases.
- Secrets (master passwords, service-account keys, OAuth tokens, webhook URLs, SMTP passwords) are encrypted with
  AES-256-GCM. The API never returns them, only whether they are set.
- Sessions use random 256-bit tokens. Only their SHA-256 is stored, and cookies are `HttpOnly; SameSite=Lax`.
- Five failed logins on a username trigger an exponential lockout.
- Lost the admin password? Run `odoo-backup reset-password <username>` (`docker compose exec -it odoo-backup
  odoo-backup reset-password admin`).

## CLI

```text
odoo-backup [serve]                   run the web server and the scheduler
odoo-backup reset-password <user>     set a password (read from stdin), creates the user if needed
odoo-backup generate-key              print a new ODB_SECRET_KEY
odoo-backup healthcheck               exit 0 if the local server is healthy
odoo-backup --help                    all options
```

## Development

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
[CONTRIBUTING.md](CONTRIBUTING.md)). Server-generated texts (run logs, API errors) are translated for Arabic only
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

## Contributing

Bug reports, feature requests and pull requests are welcome. Read [CONTRIBUTING.md](CONTRIBUTING.md) first; it
covers the development setup, the checks a pull request must pass and how to add a translation. To report a
vulnerability, follow [SECURITY.md](SECURITY.md) instead of opening a public issue.

## License

Released under the [MIT License](LICENSE).
