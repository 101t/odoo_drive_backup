# Security policy

## Reporting a vulnerability

Please do not open a public issue for security problems. Report them privately through GitHub's
[private vulnerability reporting](https://github.com/101t/odoo_drive_backup/security/advisories/new) for this
repository, with the steps to reproduce and the affected version.

You can expect an acknowledgement within a few days. Fixes are released as a new version, and the advisory credits
the reporter unless they prefer otherwise.

## Supported versions

Only the latest release receives security fixes.

## Scope

Odoo Backup stores Odoo master passwords and Google credentials (encrypted at rest). Reports about credential
handling, authentication, session management or access to backup files are especially welcome. See "Security notes"
in the README for the design.
