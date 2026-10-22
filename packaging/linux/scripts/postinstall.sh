#!/bin/sh
# Package script: runs after install and upgrade. Arguments: deb "configure" [old version];
# rpm 1 (install) or 2 (upgrade); archlinux <new version> [old version].
set -e

case "$1" in
    1) upgrade=false ;;
    [0-9]) upgrade=true ;;
    *) if [ -n "$2" ]; then upgrade=true; else upgrade=false; fi ;;
esac

# The settings may hold secrets: readable by root and the service only.
chown root:odoo-backup /etc/odoo-backup /etc/odoo-backup/odoo-backup.env
chmod 0750 /etc/odoo-backup
chmod 0640 /etc/odoo-backup/odoo-backup.env
# The data directory (systemd also creates it) and a ready-made target for "Local folder" destinations.
install -d -m 0750 -o odoo-backup -g odoo-backup /var/lib/odoo-backup /var/backups/odoo-backup

if [ -d /run/systemd/system ]; then
    systemctl daemon-reload
    if [ "$upgrade" = true ]; then
        systemctl try-restart odoo-backup.service
    else
        systemctl enable --now odoo-backup.service
    fi
fi

if [ "$upgrade" = false ]; then
    cat <<'EOF'

Odoo Backup is installed as the odoo-backup service.
  Web interface: http://<this server>:8080 (create the admin account on the first visit)
  Settings:      /etc/odoo-backup/odoo-backup.env, then: systemctl restart odoo-backup
  Data:          /var/lib/odoo-backup (back up secret.key!)
  Local backups: /var/backups/odoo-backup (use this path for a "Local folder" destination)
  Logs:          journalctl -u odoo-backup

EOF
fi
