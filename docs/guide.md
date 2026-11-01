# User guide

[← Back to the README](../README.md) · [All docs](README.md)

Retention, restore, monitoring, languages, security and the command line.

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

## Languages

The UI is available in **English**, **Arabic** (العربية), **French** (Français), **Turkish** (Türkçe) and
**Spanish** (Español); in Arabic the whole layout is right-to-left. The language is detected from the browser and
can be chosen from the drop-down in the sidebar or on the sign-in page; the choice is remembered per browser.

Technical values (URLs, paths, database names, cron expressions, hashes) always read left to right. Run logs and
error messages are stored in English, so they stay searchable and easy to share. They are translated when displayed,
for Arabic only; the other languages show them in English. Notification messages (e-mail, chat) are sent in English.

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
