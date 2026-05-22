#!/usr/bin/env bash
set -euo pipefail

ROOT_PREFIX="${OFFLINE_UPGRADE_ROOT_PREFIX:-}"
TARGET_DIR="${OFFLINE_UPGRADE_TARGET_DIR:-${ROOT_PREFIX}/var/lib/system-update}"
STATE_DIR="${OFFLINE_UPGRADE_STATE_DIR:-${ROOT_PREFIX}/var/lib/debian-upgrade}"
PHASE_FILE="${OFFLINE_UPGRADE_PHASE_FILE:-${STATE_DIR}/offline-phase}"
PHASE1_OK_FILE="${OFFLINE_UPGRADE_PHASE1_OK_FILE:-${STATE_DIR}/phase1.ok}"
PHASE2_DONE_FILE="${OFFLINE_UPGRADE_PHASE2_DONE_FILE:-${STATE_DIR}/phase2.done}"
DKMS_REINSTALL_FILE="${OFFLINE_UPGRADE_DKMS_REINSTALL_FILE:-${STATE_DIR}/dkms-reinstall.list}"
LOG_FILE="${OFFLINE_UPGRADE_LOG_FILE:-${ROOT_PREFIX}/var/log/debian-upgrade-offline.log}"
THIRD_PARTY_STATE_FILE="${OFFLINE_UPGRADE_THIRD_PARTY_STATE_FILE:-${STATE_DIR}/third-party-actions.log}"
THIRD_PARTY_REACTIVATE_FILE="${OFFLINE_UPGRADE_THIRD_PARTY_REACTIVATE_FILE:-${STATE_DIR}/third-party-reactivate.list}"
THIRD_PARTY_TARGET_DIR="${OFFLINE_UPGRADE_THIRD_PARTY_TARGET_DIR:-${ROOT_PREFIX}/etc/apt/sources.list.d}"
SYSTEM_UPDATE_LINK="${OFFLINE_UPGRADE_SYSTEM_UPDATE_LINK:-${ROOT_PREFIX}/system-update}"
ETC_SYSTEM_UPDATE_LINK="${OFFLINE_UPGRADE_ETC_SYSTEM_UPDATE_LINK:-${ROOT_PREFIX}/etc/system-update}"
OFFLINE_SIMULATE="${OFFLINE_UPGRADE_SIMULATE:-0}"

log() {
  printf '[%s] %s\n' "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" "$*" >>"${LOG_FILE}"
}

run_reboot() {
  if [ "${OFFLINE_SIMULATE}" = "1" ]; then
    log "SIMULATE: reboot demande"
    return 0
  fi
  systemctl reboot
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
  if [ ! -f "${THIRD_PARTY_REACTIVATE_FILE}" ]; then
    log "Aucune liste de reactivation (${THIRD_PARTY_REACTIVATE_FILE}): restauration ignoree."
    return 0
  fi

  log "Reactivation automatique des depots tiers selectionnes par l'utilisateur"
  local restored_list=0
  local restored_sources=0

  while IFS= read -r repo_name; do
    repo_name="$(printf '%s' "${repo_name}" | xargs)"
    [ -n "${repo_name}" ] || continue
    case "${repo_name}" in
      */*|*'..'*)
        log "Nom de depot invalide ignore: ${repo_name}"
        continue
        ;;
    esac
    file_path="${THIRD_PARTY_TARGET_DIR}/${repo_name}"
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
          BEGIN { marker = "# debian-upgrade-disabled-enabled"; pending = 0; }
          {
            line = $0
            if (line == marker) {
              pending = 1
              next
            }
            if (pending == 1 && line ~ /^[[:space:]]*Enabled[[:space:]]*:/) {
              print "Enabled: yes"
              pending = 0
              next
            }
            print line
          }
          END { exit 0; }
        ' "${file_path}" > "${file_path}.debian-upgrade.tmp"
        if ! cmp -s "${file_path}" "${file_path}.debian-upgrade.tmp"; then
          mv "${file_path}.debian-upgrade.tmp" "${file_path}"
          restored_sources=$((restored_sources + 1))
        else
          rm -f "${file_path}.debian-upgrade.tmp"
        fi
        ;;
    esac
  done < "${THIRD_PARTY_REACTIVATE_FILE}"

  log "Reactivation depots tiers terminee: list=${restored_list}, sources=${restored_sources}."
  install -d -m 0755 "$(dirname "${THIRD_PARTY_STATE_FILE}")"
  printf '[%s] reactivate-third-party|list=%s|sources=%s\n' \
    "$(date -u +'%Y-%m-%dT%H:%M:%SZ')" \
    "${restored_list}" \
    "${restored_sources}" >> "${THIRD_PARTY_STATE_FILE}"
}

arm_second_phase_and_reboot() {
  install -d -m 0755 "${STATE_DIR}" "${TARGET_DIR}" "${ROOT_PREFIX}/etc/systemd/system/system-update.target.wants"
  printf 'dkms\n' > "${PHASE_FILE}"
  ln -snf /usr/lib/systemd/system/debian-upgrade-offline.service \
    "${ROOT_PREFIX}/etc/systemd/system/system-update.target.wants/debian-upgrade-offline.service"
  ln -snf "${TARGET_DIR}" "${SYSTEM_UPDATE_LINK}"
  if [ "${OFFLINE_SIMULATE}" != "1" ]; then
    systemctl daemon-reload
  fi
  log "Phase DKMS armee via second system-update, redemarrage."
  plymouth_message "Mise a niveau Debian: phase DKMS planifiee"
  run_reboot
}

has_dkms_entries() {
  [ -f "${DKMS_REINSTALL_FILE}" ] || return 1
  grep -q '[^[:space:]]' "${DKMS_REINSTALL_FILE}"
}

run_phase_upgrade() {
  log "Phase offline: upgrade"
  plymouth_message "Mise a niveau Debian: installation des paquets"
  plymouth_progress 5

  export DEBIAN_FRONTEND=noninteractive
  export DEBIAN_PRIORITY=critical
  export APT_LISTCHANGES_FRONTEND=none

  if [ "${OFFLINE_SIMULATE}" = "1" ]; then
    log "SIMULATE: apt-get full-upgrade"
    report_status_line "pmstatus:simulated:35.0000:Installing simulated-package"
    report_status_line "pmstatus:simulated:78.0000:Configuring simulated-package"
  else
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
  fi

  touch "${PHASE1_OK_FILE}"
  log "Phase upgrade terminee avec succes."

  if has_dkms_entries; then
    log "Liste DKMS detectee: planification phase 2."
    arm_second_phase_and_reboot
    return 0
  fi

  log "Aucune phase DKMS requise, finalisation immediate."
  restore_third_party_repos || log "Restauration depots tiers echouee (non bloquant)"
  rm -f "${PHASE_FILE}" "${PHASE1_OK_FILE}" "${PHASE2_DONE_FILE}"
  plymouth_progress 100
  plymouth_message "Mise a niveau Debian: terminee, redemarrage"
  run_reboot
}

run_phase_dkms() {
  log "Phase offline: dkms"
  if [ ! -f "${PHASE1_OK_FILE}" ]; then
    log "Phase DKMS refusee: phase1.ok absent."
    rm -f "${PHASE_FILE}"
    plymouth_message "DKMS ignore: phase 1 non validee"
    run_reboot
    return 0
  fi

  if [ ! -f "${DKMS_REINSTALL_FILE}" ]; then
    log "Phase DKMS: aucune liste a traiter."
    restore_third_party_repos || log "Restauration depots tiers echouee (non bloquant)"
    touch "${PHASE2_DONE_FILE}"
    rm -f "${PHASE_FILE}" "${PHASE1_OK_FILE}"
    run_reboot
    return 0
  fi

  plymouth_message "Mise a niveau Debian: reconstruction DKMS"
  plymouth_progress 20

  local total=0
  local ok=0
  local ko=0
  while IFS=, read -r module version; do
    module="$(printf '%s' "${module}" | xargs)"
    version="$(printf '%s' "${version}" | xargs)"
    [ -n "${module}" ] || continue
    [ -n "${version}" ] || continue
    total=$((total + 1))
    log "DKMS install ${module}/${version}"
    if [ "${OFFLINE_SIMULATE}" = "1" ]; then
      log "SIMULATE: dkms install -m ${module} -v ${version}"
      ok=$((ok + 1))
      continue
    fi
    if dkms install -m "${module}" -v "${version}" >>"${LOG_FILE}" 2>&1; then
      ok=$((ok + 1))
      log "DKMS OK ${module}/${version}"
    else
      ko=$((ko + 1))
      log "DKMS ECHEC ${module}/${version}"
    fi
  done < "${DKMS_REINSTALL_FILE}"

  log "Phase DKMS terminee: total=${total}, ok=${ok}, ko=${ko}."
  restore_third_party_repos || log "Restauration depots tiers echouee (non bloquant)"
  touch "${PHASE2_DONE_FILE}"
  rm -f "${PHASE_FILE}" "${PHASE1_OK_FILE}"
  plymouth_progress 100
  plymouth_message "Mise a niveau Debian: DKMS termine, redemarrage"
  run_reboot
  return 0
}

install -d -m 0755 "$(dirname "${LOG_FILE}")" "${STATE_DIR}"
touch "${LOG_FILE}"
chmod 0644 "${LOG_FILE}"

log "Demarrage offline-upgrade.sh"
plymouth_message "Mise a niveau Debian: preparation"
plymouth_progress 3

LINK_TARGET="$(readlink -f "${SYSTEM_UPDATE_LINK}" || true)"
if [ "${LINK_TARGET}" != "${TARGET_DIR}" ]; then
  log "Marker system-update invalide: ${LINK_TARGET}"
  plymouth_message "Erreur: marker system-update invalide"
  exit 1
fi

rm -f "${SYSTEM_UPDATE_LINK}" "${ETC_SYSTEM_UPDATE_LINK}"
log "Marker system-update supprime"

phase="upgrade"
if [ -f "${PHASE_FILE}" ]; then
  phase="$(head -n1 "${PHASE_FILE}" | tr -d '[:space:]')"
  [ -n "${phase}" ] || phase="upgrade"
fi
log "Phase demandee: ${phase}"

case "${phase}" in
  upgrade)
    run_phase_upgrade
    ;;
  dkms)
    run_phase_dkms
    ;;
  *)
    log "Phase inconnue '${phase}', fallback upgrade."
    run_phase_upgrade
    ;;
esac
