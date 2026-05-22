#!/usr/bin/env bash
set -euo pipefail

TARGET_DIR="/var/lib/system-update"
LOG_FILE="/var/log/debian-upgrade-offline.log"
THIRD_PARTY_STATE_FILE="/var/lib/debian-upgrade/third-party-actions.log"
THIRD_PARTY_TARGET_DIR="/etc/apt/sources.list.d"

log() {
  printf '[%s] %s\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "$*" >>"${LOG_FILE}"
}

plymouth_progress() {
  local percent="$1"
  if command -v plymouth >/dev/null 2>&1; then
    plymouth system-update --progress="${percent}" >/dev/null 2>&1 || true
  fi
}

plymouth_message() {
  local msg="$1"
  if command -v plymouth >/dev/null 2>&1; then
    plymouth message --text="${msg}" >/dev/null 2>&1 || true
  fi
}

report_status_line() {
  local line="$1"
  case "${line}" in
    pmstatus:*)
      local percent_raw message percent normalized
      percent_raw="$(printf '%s\n' "${line}" | cut -d: -f3)"
      message="$(printf '%s\n' "${line}" | cut -d: -f4-)"
      normalized="$(printf '%s' "${percent_raw}" | tr ',' '.' | tr -d '[:space:]')"
      percent="${normalized%%.*}"
      if [[ "${percent}" =~ ^[0-9]+$ ]]; then
        if [ "${percent}" -lt 0 ]; then percent=0; fi
        if [ "${percent}" -gt 100 ]; then percent=100; fi
        plymouth_progress "${percent}"
        if [ -n "${message}" ]; then
          plymouth_message "$(printf 'Progression: %s%%\nPaquet: %s' "${percent}" "${message}")"
        else
          plymouth_message "Progression: ${percent}%"
        fi
      elif [ -n "${message}" ]; then
        plymouth_message "$(printf 'Progression: --%%\nPaquet: %s' "${message}")"
      fi
      ;;
  esac
}

restore_third_party_repos() {
  if [ ! -d "${THIRD_PARTY_TARGET_DIR}" ]; then
    log "Aucun repertoire ${THIRD_PARTY_TARGET_DIR}: restauration depots tiers ignoree."
    return 0
  fi

  log "Reactivation automatique des depots tiers modifies par debian-upgrade"
  local restored_list=0
  local restored_sources=0

  for file_path in "${THIRD_PARTY_TARGET_DIR}"/*; do
    [ -f "${file_path}" ] || continue
    case "${file_path}" in
      *.list)
        if grep -q '^# debian-upgrade-disabled ' "${file_path}"; then
          sed -i 's/^# debian-upgrade-disabled //' "${file_path}"
          restored_list=$((restored_list + 1))
        fi
        ;;
      *.sources)
        awk '
          BEGIN { marker = "# debian-upgrade-disabled-enabled"; changed = 0; pending = 0; }
          {
            line = $0
            if (line == marker) {
              pending = 1
              changed = 1
              next
            }
            if (pending == 1 && line ~ /^[[:space:]]*Enabled[[:space:]]*:/) {
              print "Enabled: yes"
              pending = 0
              changed = 1
              next
            }
            print line
          }
          END { if (pending == 1) { changed = 1; } exit 0; }
        ' "${file_path}" > "${file_path}.debian-upgrade.tmp"
        if ! cmp -s "${file_path}" "${file_path}.debian-upgrade.tmp"; then
          mv "${file_path}.debian-upgrade.tmp" "${file_path}"
          restored_sources=$((restored_sources + 1))
        else
          rm -f "${file_path}.debian-upgrade.tmp"
        fi
        ;;
    esac
  done

  log "Reactivation depots tiers terminee: list=${restored_list}, sources=${restored_sources}."
  install -d -m 0755 "$(dirname "${THIRD_PARTY_STATE_FILE}")"
  printf '[%s] reactivate-third-party|list=%s|sources=%s\n' \
    "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
    "${restored_list}" \
    "${restored_sources}" >> "${THIRD_PARTY_STATE_FILE}"
}

install -d -m 0755 /var/log
touch "${LOG_FILE}"
chmod 0644 "${LOG_FILE}"

log "Demarrage offline-upgrade.sh"
plymouth_message "Mise a niveau Debian: preparation"
plymouth_progress 3

LINK_TARGET="$(readlink -f /system-update || true)"
if [ "${LINK_TARGET}" != "${TARGET_DIR}" ]; then
  log "Marker system-update invalide: ${LINK_TARGET}"
  plymouth_message "Erreur: marker system-update invalide"
  exit 1
fi

rm -f /system-update /etc/system-update
log "Marker system-update supprime"

export DEBIAN_FRONTEND=noninteractive
export DEBIAN_PRIORITY=critical
export APT_LISTCHANGES_FRONTEND=none

log "apt-get full-upgrade (offline)"
plymouth_message "Mise a niveau Debian: installation des paquets"
plymouth_progress 5

apt-get \
  -y \
  -o Dpkg::Use-Pty=0 \
  -o Dpkg::Progress-Fancy=0 \
  -o APT::Status-Fd=3 \
  -o Dpkg::Options::=--force-confdef \
  -o Dpkg::Options::=--force-confold \
  -o APT::Get::Always-Include-Phased-Updates=true \
  full-upgrade \
  3> >(
    while IFS= read -r status_line; do
      report_status_line "${status_line}"
      printf '%s\n' "${status_line}" >>"${LOG_FILE}"
    done
  ) >>"${LOG_FILE}" 2>&1

log "Mise a niveau terminee"
restore_third_party_repos || log "Restauration depots tiers echouee (non bloquant)"
plymouth_progress 100
plymouth_message "Mise a niveau Debian: terminee, redemarrage"
systemctl reboot
