#!/bin/sh
# Package script: runs before removal. Arguments: deb "remove" or "upgrade"; rpm 0 (remove) or 1 (upgrade);
# archlinux <version> (removal only).
set -e

case "$1" in
    upgrade | failed-upgrade | deconfigure | 1) exit 0 ;;
esac

if [ -d /run/systemd/system ]; then
    systemctl disable --now odoo-backup.service || true
fi
