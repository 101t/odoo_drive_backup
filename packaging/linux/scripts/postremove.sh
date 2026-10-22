#!/bin/sh
# Package script: runs after removal. Arguments: deb "remove", "purge" or "upgrade"; rpm 0 (remove) or 1 (upgrade);
# archlinux <version> (removal only). The data, the settings and the odoo-backup user are kept.
set -e

case "$1" in
    upgrade | failed-upgrade | abort-install | abort-upgrade | disappear | 1) exit 0 ;;
esac

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload || true
fi
echo "Odoo Backup was removed. Its data in /var/lib/odoo-backup and /var/backups/odoo-backup was kept."
