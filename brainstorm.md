# Odoo Backup: implementation plan

Status: in progress. P0 is partly done and the small P2 items have shipped (see [Progress](#progress-2026-10-24)). Items are grouped into phases that ship independently. Each phase lists scope, design, files, schema changes and acceptance criteria. Ideas that are not scheduled are in the [Backlog](#9-backlog).

## Progress (2026-10-24)

Shipped in this pass. Tests: 52 Rust tests (44 before) pass on SQLite and PostgreSQL; the frontend typechecks and builds. An end-to-end run against a fake Odoo exercised parallel uploads, the sanity warning, pinning with retention, metrics and the heartbeat channel.

| Item | Phase | Where |
|---|---|---|
| `Hashes { md5, sha256 }` replaces `UploadRequest.md5`. `Storage::download` returns both digests from one pass, and restore checks SHA-256 (MD5 as fallback). The local destination now checks SHA-256 after copying. | P0.4 | `storage/mod.rs`, `storage/local.rs`, `engine/restore.rs` |
| `Capabilities { hash, medium, off_site, immutable, can_delete }`, derived from kind + public config (no secret needed). Exposed on `GET /api/destinations`. Local destinations have an **off-site** switch (NAS / remote mount). | P0.3 | `storage/mod.rs`, `api/destinations.rs`, `DestinationForm.tsx` |
| **Pinning** (legal hold): `artifacts.pinned`, `PUT /api/artifacts/:id/pin`. Retention ignores pinned copies (they don't use up GFS slots), and manual or run deletion refuses them. | P0 / P4 | `engine/retention.rs`, `api/runs.rs`, `RunDetail.tsx` |
| **Archive manifest** stored per run (`runs.manifest_json`, the `VerifyReport`), shown as "Contents" on the run page. | P2.1 | `engine/verify.rs`, `engine/backup.rs` |
| **Sanity check** against the previous successful backup of the same database and format: size drop, filestore count drop (≥ 20 files), fewer modules. Stored in `runs.warning`, logged, shown as a banner, sent to failure channels, and exported as `odoo_backup_last_run_warning`. Threshold in Settings (`size_drop_warn_percent`, default 30, 0 = off). | P2.1 | `engine/verify.rs::compare`, `api/settings.rs` |
| **Restore version warning**: the run log warns on an Odoo major-version mismatch. The restore dialog's check was fixed (`saas~17.2` vs `17.0` used to be flagged). | P2.1 | `engine/restore.rs::version_mismatch`, `RestoreDialog.tsx` |
| **Policy score 3-2-1-1-0**, computed on the fly. Returned as `policy` on every job (no separate endpoint needed), shown as a badge with fix hints on the Jobs page, exported as `odoo_backup_policy_score{job}`. | P2.3 | `engine/policy.rs`, `api/jobs.rs`, `Jobs.tsx` |
| **Heartbeat channel** with three styles: `healthchecks` (`/fail` on failure), `uptime_kuma` (`status=up|down&msg=`), `plain` (success only). | P2.4 | `notify.rs`, `NotificationForm.tsx` |
| **Optimization: parallel uploads.** All destinations of a run upload at once, so a run takes as long as its slowest destination, not the sum. Progress is aggregated across destinations; each artifact's status is recorded as soon as its own upload ends. | existing | `engine/backup.rs` |
| Small cleanups: job validation checks all destination ids in one `IN` query; `settings::load` takes `&Db` so the engine and scheduler share one loader and its defaults. | existing | `api/jobs.rs`, `api/settings.rs`, `scheduler.rs` |

Schema: `migrations/{sqlite,postgres}/0002_safety.sql` adds `artifacts.pinned`, `runs.warning` and `runs.manifest_json`.

### Decisions taken while implementing

- **No `Destination` trait yet.** With two backends, the trait would only duplicate the `enum Storage` match. `Capabilities` is a plain function of `(kind, config_json)`, which is what the API, policy score and future retention rules need (they never hold a live client). Add the trait together with the third backend (S3), when it pays for itself.
- **`runs.warning` instead of a new status `success_with_warnings`.** Every status filter (dashboard SQL, `status IN ('success','partial')`, run filters, badges, metrics) would have needed changing, and a suspicious backup is still a usable backup for retention and restore. The warning is orthogonal: status says *did it work*, warning says *does it look right*. Notification routing treats "success + warning" as a problem.
- **Pinned copies are excluded from GFS selection** rather than filtered out of the delete list afterwards. A "before upgrade" pin should not use up a daily slot and push a regular backup out.
- **The policy score sits on the job view**, not on a separate `GET /api/jobs/:id/policy-score`. The Jobs page needs it for every row, and three grouped queries cover all jobs.
- **Parallel uploads are unbounded per run** (one task per destination). Jobs have 1 to 3 destinations in practice. Add a cap together with the P4 bandwidth limit.

### Corrections to the starting point

- A disk-space guard already existed (`check_free_space`, last size × 1.2, warns under 512 MB without history). 4.1's "disk-space guard" item is therefore only a tuning question (1.2 vs 1.5).
- The restore dialog already warned about an Odoo version mismatch. Only the major-version parsing was wrong.

### Next steps, in order

1. Show `last_warning` on the dashboard health list and the history list (the API already returns it).
2. Retention safety floor: keep at least N stored copies per destination (today, the newest one plus pinned copies).
3. Retention honors `Capabilities.can_delete` (mark as expired instead of deleting). It becomes testable once S3 append-only exists.
4. `artifacts.availability` + the `ColdStorage` error, then the per-destination `keep_*` override (P0.5, P0.6). These are the only P0 items left, and both can land with P1.
5. Spike: `aws-sdk-s3` vs a minimal SigV4 client on musl (open question 1), then P1.
6. P2.2 scheduled restore tests against a scratch instance: reuse `engine/restore.rs`, add `kind = 'restore_test'` and drop the database afterwards.
7. Global scheduler-alive heartbeat ("ping every N minutes"): a ticker in `scheduler.rs` that reuses the heartbeat channels with `event = "heartbeat.alive"`.


## 0. Starting point (verified against the code)

| Area | Today | Where |
|---|---|---|
| Destinations | Closed `enum Storage { Gdrive, Local }` with `match` dispatch. Not a trait. | `backend/src/storage/mod.rs` |
| Destination config | `destinations(kind, config_json, secret_enc)`. Secrets are AES-GCM encrypted. | `db/mod.rs`, `crypto.rs`, `api/destinations.rs` |
| Integrity | ✅ `Hashes { md5, sha256 }` everywhere. Drive checks MD5, local checks SHA-256, restore checks SHA-256. | `storage/mod.rs`, `runs` table |
| Retention | GFS (`keep_last/daily/weekly/monthly/yearly`) set **per job**, applied across its destinations. | `engine/retention.rs`, `jobs` table |
| Artifacts | One row per (run, destination), with `remote_id`, `deleted_at`, `delete_reason`, ✅ `pinned`. | `artifacts` table |
| Verify / restore | ZIP CRC and `dump.sql` check, restore into an Odoo instance. No restore test on scratch infrastructure. | `engine/verify.rs`, `engine/restore.rs` |
| Alerts | `notification_channels` with on_success / on_failure. ✅ `heartbeat` kind. A warning counts as a failure. | `notify.rs` |
| Metrics | `/metrics`, enabled by `ODB_METRICS_TOKEN`. | `api/dashboard.rs` |
| Auth | Single user type, session tokens. No roles, no API tokens. | `auth.rs`, `api/auth.rs` |
| Migrations | Two parallel sets: `migrations/sqlite` and `migrations/postgres`. Every schema change needs both. | `backend/migrations/` |

Conventions that apply to every phase:

- New migration files go in **both** `sqlite/` and `postgres/`, numbered sequentially.
- New user-facing strings go through `i18n.rs` (English and Arabic).
- New destination code needs a mock-server test like `gdrive_mock_tests.rs`, so CI never touches a real cloud.
- Pin SDK/crate versions. Keep the static musl build working (`rustls`, no OpenSSL).

## 1. Roadmap

| Phase | Theme | Depends on | Size | Status |
|---|---|---|---|---|
| P0 | Destination abstraction + hash generalization | none | M | Hashes, capabilities, pinning done. Availability and per-destination retention left. |
| P1 | S3-compatible + Glacier destination | P0 | L | not started |
| P2 | Restore tests + policy score + heartbeat | none (P1 for S3 targets) | L | 4.1 (mostly), 4.3 and 4.4 done. 4.2 restore tests left. |
| P3 | OneDrive / SharePoint destination | P0 | L | not started |
| P4 | Safety hardening (encryption, immutability, audit, API tokens) | P0 | L | pinning done (API trigger with `pin` left) |
| P5 | Policy as code, roles, SSO | P4 (audit, tokens) | L | not started |
| Parallel | Repo, docs, launch (section 8) | none | ongoing | README updated for the new features |

P2 does not depend on P1 and can run in parallel with it. P0 is the only hard blocker.

---

## 2. P0: Destination abstraction

**Goal.** Adding a destination should mean adding one file and one match arm, not editing five places.

**Design decisions**

1. **Keep enum dispatch, add a trait.** *(Deferred to P1: see Decisions.)* Async traits with `dyn` cost an allocation and complicate lifetimes (`UploadRequest<'a>`). Define a `Destination` trait that every backend implements, and keep `enum Storage` as the thin dispatcher. The enum stays exhaustive, so the compiler flags missing arms. Revisit `Box<dyn>` only if third-party plugins become a goal.
2. **Trait surface** (derived from the methods already on `Storage`):
   - `upload(&UploadRequest, ProgressFn, &CancellationToken) -> StoredObject`
   - `delete(remote_id)`
   - `open_stream(remote_id) -> (ByteStream, Option<u64>)`
   - `test() -> TestReport`
   - `capabilities() -> Capabilities`, new (below).
   - `download` and `local_path` stay as default methods on the dispatcher.
3. ✅ **Capabilities struct** so the UI and retention engine can adapt without matching on `kind`:
   ```rust
   struct Capabilities {
       can_delete: bool,            // false for append-only / least-privilege setups
       immutable: bool,             // object lock active
       supports_resume: bool,
       hash: HashKind,              // Md5 | Sha256 | QuickXor | Crc32c
       min_retention_days: Option<u32>, // Glacier / Object Lock minimum
       cold_storage: bool,          // needs a retrieval step before download
   }
   ```
4. ✅ **Generalize the hash.** `UploadRequest.md5` and `Storage::download -> md5` bake in Drive's hash. Replace with `hashes: LocalHashes { md5, sha256, crc32c?, quickxor? }`, computed in one pass over the file by the backup engine. Each destination verifies the one it supports. The `runs` table already stores `sha256` and `md5`.
5. **Cold objects.** Add `artifacts.availability` (`online | archived | restoring`). `open_stream` on an archived object returns a typed `ColdStorage` error instead of a generic failure. This is the hook for P1 retrieval.
6. **Destination-level retention override.** Add nullable `keep_*` columns to `job_destinations`. When null, use the job's values (current behavior). `engine/retention.rs` then groups artifacts per (job, destination).

**Schema.** Migration 0002 shipped `pinned` (plus `runs.warning` and `runs.manifest_json`). The rest moves to migration 0003:

```sql
ALTER TABLE artifacts ADD COLUMN availability TEXT NOT NULL DEFAULT 'online';
ALTER TABLE artifacts ADD COLUMN pinned INTEGER NOT NULL DEFAULT 0;      -- legal hold
ALTER TABLE artifacts ADD COLUMN storage_class TEXT;
ALTER TABLE artifacts ADD COLUMN retain_until TEXT;                      -- object-lock / min-duration
ALTER TABLE job_destinations ADD COLUMN keep_last INTEGER;               -- NULL = inherit job
-- same for keep_daily, keep_weekly, keep_monthly, keep_yearly
```

**Files:** `storage/mod.rs` (trait, `Capabilities`), `storage/gdrive.rs` and `storage/local.rs` (implement trait), `engine/backup.rs` (compute hashes once), `engine/retention.rs` (skip `pinned`, honor per-destination policy, respect `can_delete`), `api/destinations.rs` (validate `kind` through a registry rather than a `match` on strings), `db/models.rs`.

**Acceptance**
- Existing Drive and local tests pass unchanged.
- A fake in-memory destination in tests exercises upload, delete, download and retention through the trait only.
- ✅ Pinned artifacts are never pruned, even when the GFS policy says so.
- Retention never prunes below the safety floor: keep at least N **verified** artifacts per destination (default 1, configurable). The newest artifact is already protected today.

---

## 3. P1: S3-compatible and Glacier destination

**Goal.** One destination type, `s3`, covering AWS S3, Cloudflare R2, Backblaze B2, MinIO, Wasabi and Hetzner through a custom endpoint.

**Design**

- **Client:** `aws-sdk-s3` with `rustls`. Confirm the musl build and binary size impact before committing. The fallback is a small SigV4 client over the existing `reqwest`.
- **Config (`config_json`):** `endpoint` (optional), `region`, `bucket`, `prefix`, `storage_class`, `part_size_mib` (default 64), `parallelism` (default 4), `path_style` (needed for MinIO), `sse` (`none | aes256 | kms`), `object_lock_mode` + `retention_days` (optional).
- **Secret (`secret_enc`):** access key + secret, or `auth = instance_profile | env`. OIDC / Roles Anywhere is deferred to the Backlog.
- **Upload:** multipart with `part_size` and `parallelism`. Send `x-amz-checksum-sha256` per part and compare with the local hash. Resume = persist `upload_id` in the run, list parts, upload only the missing ones. Abort stale multipart uploads at the start of a run (and recommend an `AbortIncompleteMultipartUpload` lifecycle rule in docs).
- **Key layout:** `{prefix}/{instance}/{database}/{file_name}`. `remote_id` is the object key. S3 has no path or character problems, but still sanitize `:`.
- **Storage classes:** `STANDARD`, `STANDARD_IA`, `GLACIER_IR`, `GLACIER`, `DEEP_ARCHIVE`, selected per destination. Set on upload, and fill `artifacts.storage_class` and `availability`.
- **Tiering:** two options, and v1 ships only the first.
  1. *Generated lifecycle config.* The UI shows the S3 lifecycle JSON for the user to apply. No extra IAM rights are needed.
  2. *App-managed transitions* (`CopyObject` to a new class). Deferred.
- **Glacier retrieval:** `POST /api/artifacts/:id/retrieve {tier: expedited|standard|bulk, days}` calls `RestoreObject`. Poll `HeadObject` (`x-amz-restore`) on a timer and set `availability` to `restoring`, then `online`. Fire a notification when ready. Restore from an `archived` artifact starts retrieval and then waits.
- **Min-duration guard:** `Capabilities.min_retention_days` is 90 for GLACIER, 180 for DEEP_ARCHIVE, 30 for IA classes. The retention planner shows a warning when a rule would delete earlier, and records `retain_until`.
- **Immutability:** if Object Lock is on, upload with the retention mode and date. `Capabilities.immutable = true`, and delete is not attempted before `retain_until`.
- **IAM templates** (shipped in `docs/`): (a) full access; (b) **append-only**: `s3:PutObject`, `s3:GetObject`, `s3:ListBucket`, `s3:AbortMultipartUpload`, no `s3:DeleteObject`. With (b), `can_delete = false` and expiry is handled by a bucket lifecycle rule. The destination form has a "no delete rights" switch, and retention then only *marks* artifacts as expired instead of failing.
- **`test()` steps:** credentials valid, bucket reachable, write + read + (delete if allowed) a probe object, report Object Lock / versioning status, warn if the class has a minimum duration.

**Files:** new `storage/s3.rs`, `storage/s3_mock_tests.rs` (a local S3 mock, e.g. `s3s` or a hand-rolled axum stub), `api/destinations.rs` (validation), frontend destination form, `docs/s3.md`, `docs/iam-policies/`.

**Acceptance**
- Upload, resume after a killed process, download, delete and `test()` work against MinIO in CI (service container).
- A 5 GB file uploads with bounded memory (`part_size × parallelism`).
- Checksum mismatch on a part fails the upload and aborts the multipart.
- An archived artifact shows "archived", retrieval can be requested, and the status moves to `online`.
- With the append-only policy, a run completes and retention marks expired items without erroring.

---

## 4. P2: Restore confidence

**Goal.** Make "verified" real. This is the product's headline feature.

### 4.1 Backup sanity checks (small, do first)
Done except the new status (replaced by `runs.warning`), per-job thresholds (global for now) and the *Test connection* diagnostics.
- ✅ Manifest inside the run: Odoo version, PostgreSQL version, module list, filestore entry count, `dump.sql` size. Store as `runs.manifest_json`.
- ✅ Compare with the previous successful run of the same database. Warn when size drops more than X% (default 30%, per-job setting) or the filestore count drops sharply. A warning marks the run `success_with_warnings` (new status; check how the UI and `notify.rs` treat unknown statuses).
- ✅ (log + dialog; no confirmation step yet) Restore-time warning when the target Odoo version differs from the manifest (a major version mismatch is a hard warning that needs confirmation).
- Improve *Test connection* in `odoo/mod.rs`: distinguish `list_db = False`, wrong master password, `dbfilter`, proxy timeout. Return a stable error code so the UI can link to a doc.
- Timeout diagnostic: when a connection closes at around N seconds, suggest `proxy_read_timeout` in the error message.
- Already existed (×1.2): disk-space guard. Before a run, compare free space on the temp dir with the last backup size × 1.5 (or the DB size estimate). Fail early with a clear message.

### 4.2 Scheduled restore tests
- **New run kind** `restore_test` (the `runs.kind` column already exists). Settings per job: `restore_test_cron`, `restore_test_destination_id`.
- **Flow:** take the latest artifact, download and verify it, restore into a throwaway target, run checks, record the result, tear down.
- **Target options** (implement in this order):
  1. *Scratch Odoo instance* the user already runs (reuses the current restore path). Fastest to ship.
  2. *Docker*: start `postgres` + `odoo:<version>` containers from the manifest version, restore, check, remove. Needs a Docker socket and is opt-in.
- **Checks:** restore finishes, `SELECT count(*)` on `res_partner` and `account_move` is greater than 0 (configurable table list), `/web/login` returns 200, module-load errors in the Odoo log are zero.
- **Results:** `runs` row with duration (measured RTO), plus `jobs.last_verified_restore_at`. Dashboard shows "last verified restore" per database, and a notification fires on failure or on staleness ("no verified restore in N days").

### 4.3 Policy score (3-2-1-1-0) ✅
- Computed on the fly from existing data, no new table needed. Inputs: destinations per job, destination kinds and providers, `Capabilities.immutable`, last verified restore.
- Rules: ≥3 copies, ≥2 media types, ≥1 off-site, ≥1 immutable or offline, 0 errors in the latest verification.
- Output: `policy: {score, max, rules: [{id, ok}]}` on each job in `GET /api/jobs`. Hints are translated in the UI from the rule id. The UI shows a badge on the job.
- Copies are counted as 1 (live database) + destinations. "Media" comes from `Capabilities.medium` (disk or cloud). "Immutable" can't pass until S3 Object Lock exists, and the hint says so.
- Not done: the "all destinations on the same provider" warning (needs a provider field once S3 endpoints exist).

### 4.4 Heartbeat (dead-man's switch) ✅ (except the global ping)
- ✅ New notification channel kind `heartbeat`: `GET`/`POST` to a healthchecks.io / Uptime Kuma push URL after each successful run (and `/fail` on failure).
- Still to do: a global "ping every N minutes" option so a silently dead scheduler is caught.
- Reuses `notification_channels`; mostly changes to `notify.rs` and a form in the UI.

### 4.5 Metrics additions
Add to `/metrics`: `odb_last_verified_restore_timestamp`, `odb_restore_test_duration_seconds`, `odb_policy_score`, `odb_artifact_bytes{destination,class}`.
Shipped so far: `odoo_backup_policy_score{job}` and `odoo_backup_last_run_warning{instance,database}`. Keep the existing `odoo_backup_` prefix for consistency rather than `odb_`.

**Acceptance for P2:** a fixture ZIP that is structurally valid but holds an empty DB passes the CRC check yet is flagged by the restore test. A job with one local destination scores below 3-2-1 with specific hints. A missed heartbeat triggers an alert in the external monitor.

---

## 5. P3: OneDrive / SharePoint destination

**Goal.** A `onedrive` destination using Microsoft Graph, covering personal OneDrive, OneDrive for Business and SharePoint libraries.

**Design**

- **Auth modes:** (a) delegated auth-code flow with refresh token, matching the existing Drive OAuth flow in `api/destinations.rs`; (b) Entra app with client credentials, scoped by `Sites.Selected` (preferred) or `Files.ReadWrite.All`.
- **Target:** `drive_id` or `site_id + library`, plus a root folder path.
- **Upload:** `createUploadSession`, chunk size a multiple of 320 KiB (default 10 MiB, which is 32 × 320 KiB). Resume by querying `nextExpectedRanges`. Maximum file size is 250 GB.
- **Integrity:** implement **QuickXorHash** locally (small, well documented) and compare with `file.hashes.quickXorHash`. Uses `HashKind::QuickXor` from P0. Test with known vectors.
- **Throttling:** honor `429` and `503` with `Retry-After`. Add a per-destination rate limiter.
- **Names:** sanitize forbidden characters (`" * : < > ? / \ |`), trailing dots and spaces, and enforce path length limits. Reuse the sanitizer for any future destination.
- **Token expiry:** refresh tokens expire after 90 days of inactivity. Store `token_expires_hint`, notify 7 days before, and show a "reconnect" state on the destination card.
- **Docs:** Entra app registration walkthrough; which mode to choose.

**Files:** new `storage/onedrive.rs`, `storage/quickxor.rs`, `storage/onedrive_mock_tests.rs`, `api/destinations.rs`, UI form.

**Acceptance:** upload with a forced mid-session failure resumes; QuickXorHash matches Graph's value for a 1 GiB test file in a manual test run (documented, not in CI); a 429 with `Retry-After` is honored in a mock test.

---

## 6. P4: Safety hardening

| Item | Design | Notes |
|---|---|---|
| **Client-side encryption** | Encrypt the artifact before upload with `age` (supports recipients and passphrase, one crate) or streaming AES-GCM in chunks. Store `encryption: {scheme, key_id}` in the manifest. Stored name gets an `.age` suffix. | **Breaks "the backup is the stock Odoo ZIP".** Make it per destination and opt-in, and document that restore must decrypt first. Needs key escrow docs and a `decrypt` CLI subcommand so a backup can be opened without the app. Verification (`engine/verify.rs`) runs on the pre-encryption local file. |
| **Audit log** | New `audit_log(id, ts, actor, action, target_type, target_id, detail_json, ip)`. Write from the API layer on policy changes, restore, download, delete, login. `GET /api/audit` with filters, CSV export. | Never store secrets in `detail_json`. |
| **API tokens** | `api_tokens(id, name, token_hash, scopes, created_at, last_used_at, expires_at)`. Bearer auth next to sessions. Scopes: `jobs:run`, `runs:read`. Show the token once. | Hash with SHA-256, like `sessions.token_hash`. |
| **Event trigger** | `POST /api/jobs/:id/run` accepts `{label, pin: bool}`. A pinned run sets `artifacts.pinned` (e.g. "before upgrade"). | Uses P0 `pinned` and API tokens. |
| **Maintenance windows** | `jobs.blackout_json`: list of cron-like ranges. The scheduler delays a run until the window ends, and logs why. | Edit `scheduler.rs`. |
| **Concurrency limit** | Per-instance max concurrent runs (default 1) and a global limit. | `engine/mod.rs`. |
| **Bandwidth limit** | Global and per-destination upload rate cap in the upload loop, with an optional daytime schedule. | |

Order inside P4: audit log, API tokens, pinning and trigger, encryption, then the rest. Pinning shipped; the trigger can set it once API tokens exist.
Also add a **per-run upload concurrency cap** next to the bandwidth limit, since uploads now run in parallel.

---

## 7. P5: Policy as code, roles, SSO

- **Policy as code:** `odoo-backup.yaml` (or TOML) describing instances, destinations, jobs and notification channels. Applied at startup and with `odoo-backup apply -f file`. Resources are matched by `name`, and secrets come from environment variables (`${ENV:NAME}`), never inline. Ship `odoo-backup export` to generate a file from an existing database. Include `--dry-run` that prints a diff.
- **Roles:** `users.role` = `admin | operator | viewer`. Enforce in the router as middleware per route group. Operators can run and restore. Viewers are read-only.
- **SSO:** OIDC (Google, Entra, Keycloak) via auth code + PKCE, mapping a claim or group to a role. Local login stays as break-glass.
- **Compliance presets:** named templates in the UI (GDPR, 7-year financial, etc.) that fill retention, encryption and immutability. These are presets over existing settings, so they ship last and carry a disclaimer that they are not legal advice.

---

## 8. Parallel track: repo and launch

Do not wait for the engineering phases.

1. **Rename and re-pitch.** New tagline: *"Verified, off-site, tamper-resistant Odoo backups in one binary."* The repo folder is `odoo_drive_backup` while the product is "Odoo Backup", and "drive" misleads once S3 and OneDrive exist. Update the `Cargo.toml` description (still says "Google Drive, local storage"), README, Docker image names and packaging metadata together.
2. **First-5-minutes experience:** a `docker-compose.demo.yml` with sample Odoo + Postgres + MinIO + the tool, seeded so the dashboard has data. README gets a dashboard screenshot, a 3-command quick start, and a "why not cron + pg_dump" section.
3. **Trust files:** `SECURITY.md` (disclosure process; mention existing signed attestations, SHA256SUMS, static binary, hardened systemd unit), `CONTRIBUTING.md`, `CODE_OF_CONDUCT.md`, issue and PR templates, `CHANGELOG.md`, a threat-model page, `cargo audit` + `cargo deny` in CI, Dependabot. LICENSE (MIT) exists.
4. **Contributor funnel:** after P0, document how to add a destination (`docs/adding-a-destination.md`). Seed 10 to 15 issues labelled `good first issue`, `help wanted`, `destination`, `docs`, `translation`. Candidate destinations: SFTP, WebDAV/Nextcloud, Azure Blob, Dropbox, Backblaze native. Open translations (French, Spanish, Portuguese, German, Turkish, Indonesian, Chinese), starting from the Arabic work in `i18n.rs`. Enable GitHub Discussions and a public roadmap.
5. **Launch content** (after P1 and P2 land): "Why your Odoo backup probably doesn't restore", "How we verify backups end to end", "Odoo to S3 Deep Archive for a few dollars a month". Channels: Odoo forum, OCA, r/Odoo, r/selfhosted, Show HN, awesome-selfhosted, awesome-rust, integrator groups first (multi-instance is their use case).
6. **Packaging later:** Helm chart, Homebrew tap, Unraid/TrueNAS/Proxmox templates, Docker Hub mirror, Windows MSI.

---

## 9. Backlog

Valuable, but not scheduled. Each needs a design pass before it is promoted to a phase.

- **Large-database mode:** an agent or SSH + `pg_dump` mode, since the HTTP `/web/database/backup` endpoint is synchronous and does not scale past roughly 20 GB. Likely the biggest functional gap for big customers.
- **Selective restore:** filestore only, or a single table.
- **Backup diff:** row counts per table and module changes between runs.
- **Point-in-time recovery:** WAL archiving for self-hosted Postgres.
- **Anonymized dev copies:** scrub emails, phones, payment tokens after restore, beyond Odoo's neutralize.
- **Cost estimator** in the UI and tier-aware retention (needs P1 data first).
- **Adaptive schedule,** job dependencies and priorities.
- **App-managed storage-class transitions** (instead of lifecycle JSON).
- **zstd recompression** or a dedup store. This trades away the stock-ZIP property, so decide together with client-side encryption.
- **OIDC / Roles Anywhere** for S3 auth.
- **Odoo addon** that shows "Backup status" inside Odoo.
- **Odoo.sh / Odoo Online:** document what is and is not supported (docs-only task, can be done any time).
- **Per-job sanity thresholds** and checks on specific tables (row counts need a restore test, so they belong to P2.2).
- **Per-destination progress in the UI:** the run page shows aggregated upload progress. Expose one progress bar per artifact.
- **Restore confirmation step** when the major version differs (today it's a hint plus a log line).
- **Sustainability:** GitHub Sponsors / Open Collective; any paid tier must leave core backup and restore fully open.

## 10. Open questions

1. Is `aws-sdk-s3` acceptable in the musl static build in terms of binary size, or should S3 use a minimal SigV4 client? Decide with a spike before P1.
2. Should a restore test run only against Docker, or is "a scratch Odoo you already run" enough for v1? This plan assumes the scratch instance first.
3. Is changing the repo name acceptable before the first public release? It touches image names, packaging and docs, so it is cheapest now.
4. Do we accept breaking the "stock Odoo ZIP" property for encrypted destinations (P4)? This plan assumes opt-in per destination.
