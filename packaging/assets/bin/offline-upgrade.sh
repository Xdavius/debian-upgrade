#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR="/var/lib/system-update"
LOG_FILE="/var/log/debian-upgrade-offline.log"

log() {
  printf '[%s] %s\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "$*" >>"${LOG_FILE}"
}

plymouth_msg() {
  if command -v plymouth >/dev/null 2>&1; then
    plymouth display-message --text="$1" >/dev/null 2>&1 || true
  fi
}

plymouth_pct() {
  if command -v plymouth >/dev/null 2>&1; then
    plymouth system-update --progress="$1" >/dev/null 2>&1 || true
  fi
}

install -d -m 0755 /var/log
touch "${LOG_FILE}"
chmod 0644 "${LOG_FILE}"

log "Demarrage offline-upgrade.sh"
plymouth_msg "Mise a niveau Debian: preparation..."
plymouth_pct 5

LINK_TARGET="$(readlink -f /system-update || true)"
if [ "${LINK_TARGET}" != "${TARGET_DIR}" ]; then
  log "Marker system-update invalide: ${LINK_TARGET}"
  plymouth_msg "Erreur: marker system-update invalide"
  exit 1
fi

rm -f /system-update /etc/system-update
log "Marker system-update supprime"
plymouth_pct 10

export DEBIAN_FRONTEND=noninteractive
export DEBIAN_PRIORITY=critical
export APT_LISTCHANGES_FRONTEND=none

plymouth_msg "Mise a niveau Debian: application des paquets prepares..."
plymouth_pct 25
log "apt-get dist-upgrade"
apt-get -y \
  -o Dpkg::Options::=--force-confdef \
  -o Dpkg::Options::=--force-confold \
  -o APT::Get::Always-Include-Phased-Updates=true \
  dist-upgrade >>"${LOG_FILE}" 2>&1

plymouth_pct 95
plymouth_msg "Mise a niveau Debian: finalisation..."
log "Mise a niveau terminee"
plymouth_pct 100
