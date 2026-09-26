#!/usr/bin/env bash
set -euo pipefail

DEB=${1:?usage: install-local.sh libatk-bridge2.0-0t64_...deb}
test -f "$DEB"

. /etc/os-release
test "${ID:-}" = ubuntu
test "${VERSION_ID:-}" = 24.04

EXPECTED=2.52.0-1build1+semwright1
PKG=$(dpkg-deb -f "$DEB" Package)
VER=$(dpkg-deb -f "$DEB" Version)
test "$PKG" = libatk-bridge2.0-0t64
test "$VER" = "$EXPECTED"

CURRENT=$(dpkg-query -W -f='${Version}' libatk-bridge2.0-0t64)
printf 'current=%s\ninstalling=%s\n' "$CURRENT" "$VER"

sudo dpkg -i "$DEB"
sudo ldconfig

echo "Installed local AT-SPI bridge backport."
echo "A full logout/login is required before GNOME Shell loads the patched library."
echo "Rollback: sudo apt-get install --reinstall libatk-bridge2.0-0t64=2.52.0-1build1"
