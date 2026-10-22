#!/bin/sh
# Package script (deb, rpm, archlinux): runs before install and upgrade. Creates the service user.
set -e

if ! getent group odoo-backup >/dev/null; then
    groupadd --system odoo-backup
fi
if ! getent passwd odoo-backup >/dev/null; then
    useradd --system --gid odoo-backup --home-dir /var/lib/odoo-backup --no-create-home \
        --shell "$(command -v nologin || echo /bin/false)" --comment "Odoo Backup" odoo-backup
fi
