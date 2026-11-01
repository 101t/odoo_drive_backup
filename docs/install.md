# Installation

[← Back to the README](../README.md) · [All docs](README.md)

Every way to run Odoo Backup: Docker, a Linux or Windows service, or from source.

## Docker in one command

```bash
docker run -d --name odoo-backup --restart unless-stopped -p 8080:8080 \
  -v odoo-backup-data:/data -v odoo-backup-files:/backups \
  ghcr.io/101t/odoo-backup:latest
```

Open `http://<server>:8080` and create the admin account. Use `/backups` as the path of a *Local folder*
destination, or replace `odoo-backup-files` with a bind mount of your NAS (owned by uid 10001), e.g.
`-v /mnt/nas/odoo:/backups`. Add `-e ODB_PUBLIC_URL=https://backup.example.com` when the app is reached through
another address (needed for Google OAuth and for links in notifications).

**Upgrade:** `docker pull ghcr.io/101t/odoo-backup:latest`, then `docker rm -f odoo-backup` and run the same
command again. The volumes keep your data.

## Docker Compose (from a clone of this repository)

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
