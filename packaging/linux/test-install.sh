#!/bin/sh
# CI check, run inside a distribution container (which has no systemd): installs the package from DIR with
# install.sh, starts the server as the service user, then uninstalls.
#   docker run --rm -v "$PWD:/src:ro" -w /src almalinux:9 sh packaging/linux/test-install.sh dist
set -eu

dist="$1"

sh packaging/linux/install.sh --from "$dist"

odoo-backup --version
getent passwd odoo-backup
test "$(stat -c '%U:%G %a' /etc/odoo-backup/odoo-backup.env)" = "root:odoo-backup 640"
test "$(stat -c '%U:%G' /var/lib/odoo-backup /var/backups/odoo-backup)" = "$(printf 'odoo-backup:odoo-backup\nodoo-backup:odoo-backup')"

# Started as the systemd unit would. Minimal Fedora/RHEL images lack runuser.
command -v runuser >/dev/null || dnf install -y -q util-linux
runuser -u odoo-backup -- env ODB_DATA_DIR=/var/lib/odoo-backup odoo-backup serve >/tmp/odoo-backup.log 2>&1 &
server=$!
for _ in $(seq 1 30); do
    odoo-backup healthcheck 2>/dev/null && break
    sleep 1
done
odoo-backup healthcheck || {
    cat /tmp/odoo-backup.log
    exit 1
}
kill "$server"
wait "$server" || true
test -f /var/lib/odoo-backup/secret.key

sh packaging/linux/install.sh --uninstall
if [ -e /usr/bin/odoo-backup ]; then
    echo "odoo-backup is still installed" >&2
    exit 1
fi
test -f /var/lib/odoo-backup/secret.key # data is kept
echo "Install, start and removal work."
