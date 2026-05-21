#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR="/var/lib/system-update"
LINK_TARGET="$(readlink -f /system-update || true)"
if [ "${LINK_TARGET}" != "${TARGET_DIR}" ]; then
  echo "System update marker invalide: ${LINK_TARGET}"
  exit 1
fi

rm -f /system-update /etc/system-update

export DEBIAN_FRONTEND=noninteractive
export DEBIAN_PRIORITY=critical
export APT_LISTCHANGES_FRONTEND=none

apt-get update
apt-get -y \
  -o Dpkg::Options::=--force-confdef \
  -o Dpkg::Options::=--force-confold \
  -o APT::Get::Always-Include-Phased-Updates=true \
  dist-upgrade
