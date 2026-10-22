#!/bin/sh
# Installs Odoo Backup as a systemd service with the native package of this Linux distribution:
# .deb (Debian, Ubuntu), .rpm (RHEL, Rocky, AlmaLinux, CentOS Stream, Fedora, openSUSE) or .pkg.tar.zst (Arch).
# Packages are checked against the release's SHA256SUMS before they are installed.
#
#   curl -fsSL https://github.com/101t/odoo_drive_backup/releases/latest/download/install.sh | sudo sh
#   sudo sh install.sh --version 0.0.2   # a specific release (run again with a newer one to upgrade)
#   sudo sh install.sh --from ./dist     # offline: the package and SHA256SUMS were downloaded already
#   sudo sh install.sh --uninstall       # remove the package; data and settings are kept
#
# Pass options through a pipe with: ... | sudo sh -s -- --version 0.0.2
set -eu

REPO="${ODB_REPO:-101t/odoo_drive_backup}"
RELEASE=1 # package release number, see packaging/linux/nfpm.yaml

version=""
from=""
action=install

die() {
    echo "install.sh: $*" >&2
    exit 1
}

while [ $# -gt 0 ]; do
    case "$1" in
        --version)
            [ $# -ge 2 ] || die "--version needs a value"
            version="${2#v}"
            shift 2
            ;;
        --from)
            [ $# -ge 2 ] || die "--from needs a directory"
            from="$2"
            shift 2
            ;;
        --uninstall)
            action=uninstall
            shift
            ;;
        -h | --help)
            echo "Usage: install.sh [--version X.Y.Z] [--from DIR] [--uninstall]"
            echo "Installs or upgrades Odoo Backup from https://github.com/$REPO/releases"
            exit 0
            ;;
        *) die "unknown option $1 (see --help)" ;;
    esac
done

[ "$(id -u)" -eq 0 ] || die "run as root, e.g. sudo sh install.sh"

if command -v apt-get >/dev/null 2>&1; then
    format=deb
elif command -v dnf >/dev/null 2>&1 || command -v yum >/dev/null 2>&1 || command -v zypper >/dev/null 2>&1; then
    format=rpm
elif command -v pacman >/dev/null 2>&1; then
    format=archlinux
else
    die "no supported package manager found (apt, dnf, yum, zypper or pacman). Download
odoo-backup-<version>-<arch>-linux.tar.gz from https://github.com/$REPO/releases: it holds a static binary
that runs on any distribution, plus the systemd unit."
fi

rpm_manager() {
    if command -v dnf >/dev/null 2>&1; then
        echo dnf
    elif command -v yum >/dev/null 2>&1; then
        echo yum
    else
        echo zypper
    fi
}

if [ "$action" = uninstall ]; then
    case "$format" in
        deb) DEBIAN_FRONTEND=noninteractive apt-get remove -y odoo-backup ;;
        rpm)
            case "$(rpm_manager)" in
                zypper) zypper --non-interactive remove odoo-backup ;;
                *) "$(rpm_manager)" remove -y odoo-backup ;;
            esac
            ;;
        archlinux) pacman -R --noconfirm odoo-backup ;;
    esac
    exit 0
fi

case "$(uname -m)" in
    x86_64 | amd64)
        deb_arch=amd64
        rpm_arch=x86_64
        ;;
    aarch64 | arm64)
        deb_arch=arm64
        rpm_arch=aarch64
        ;;
    *) die "no package for the $(uname -m) architecture (x86_64 and aarch64 are available)" ;;
esac

fetch() {
    if command -v curl >/dev/null 2>&1; then
        curl -fsSL --retry 3 -o "$2" "$1"
    elif command -v wget >/dev/null 2>&1; then
        wget -q -O "$2" "$1"
    else
        die "curl or wget is needed to download the package"
    fi
}

package_name() {
    case "$format" in
        deb) echo "odoo-backup_$1-${RELEASE}_$deb_arch.deb" ;;
        rpm) echo "odoo-backup-$1-$RELEASE.$rpm_arch.rpm" ;;
        archlinux) echo "odoo-backup-$1-$RELEASE-$rpm_arch.pkg.tar.zst" ;;
    esac
}

if [ -n "$from" ]; then
    dir=$(cd "$from" && pwd)
    if [ -n "$version" ]; then
        package=$(package_name "$version")
    else
        # The newest package for this system in the directory.
        pattern=$(package_name '*')
        package=$(cd "$dir" && for file in $pattern; do [ -f "$file" ] && echo "$file"; done | sort -V | tail -n1)
    fi
    [ -n "$package" ] && [ -f "$dir/$package" ] || die "no $format package for $rpm_arch in $dir"
    [ -f "$dir/SHA256SUMS" ] || die "$dir/SHA256SUMS is missing (download it from the release with the package)"
else
    dir=$(mktemp -d)
    trap 'rm -rf "$dir"' EXIT
    if [ -z "$version" ]; then
        fetch "https://api.github.com/repos/$REPO/releases/latest" "$dir/release.json"
        version=$(sed -n 's/.*"tag_name": *"v\{0,1\}\([^"]*\)".*/\1/p' "$dir/release.json" | head -n1)
        [ -n "$version" ] || die "cannot find the latest release of $REPO"
    fi
    package=$(package_name "$version")
    echo "Downloading $package (Odoo Backup $version)..."
    fetch "https://github.com/$REPO/releases/download/v$version/$package" "$dir/$package"
    fetch "https://github.com/$REPO/releases/download/v$version/SHA256SUMS" "$dir/SHA256SUMS"
fi

(cd "$dir" && grep " $package\$" SHA256SUMS | sha256sum -c -) || die "checksum verification of $package failed"

case "$format" in
    deb)
        export DEBIAN_FRONTEND=noninteractive
        apt-get update -qq
        # Keep the settings file when it was edited (no prompt).
        apt-get install -y -o Dpkg::Options::=--force-confdef -o Dpkg::Options::=--force-confold "$dir/$package"
        ;;
    rpm)
        case "$(rpm_manager)" in
            zypper) zypper --non-interactive install --allow-unsigned-rpm "$dir/$package" ;;
            *) "$(rpm_manager)" install -y "$dir/$package" ;;
        esac
        ;;
    archlinux) pacman -U --noconfirm "$dir/$package" ;;
esac

if [ -d /run/systemd/system ] && systemctl is-enabled --quiet odoo-backup.service; then
    sleep 2
    if ! systemctl is-active --quiet odoo-backup.service; then
        echo "The odoo-backup service did not start. Check: journalctl -u odoo-backup" >&2
        exit 1
    fi
fi
echo "Odoo Backup $(odoo-backup --version | cut -d' ' -f2) is installed."
