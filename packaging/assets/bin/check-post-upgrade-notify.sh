#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Debian-Upgrade"
APP_ICON="system-software-update"
LOG_PATH="/var/log/debian-upgrade-offline.log"
STATE_DIR="/var/lib/debian-upgrade"
STATUS_FILE="${STATE_DIR}/post-upgrade-status.env"
PENDING_FILE="${STATE_DIR}/post-upgrade-notify.pending"

log() {
  logger -t debian-upgrade-post-notify -- "$*"
}

is_sudo_or_root_user() {
  local user="$1"
  if [ "$user" = "root" ]; then
    return 0
  fi
  id -nG "$user" 2>/dev/null | tr ' ' '\n' | grep -qx "sudo"
}

get_session_prop() {
  local session="$1"
  local prop="$2"
  loginctl show-session "$session" -p "$prop" --value 2>/dev/null || true
}

read_env_from_leader() {
  local leader="$1"
  local key="$2"
  tr '\0' '\n' < "/proc/${leader}/environ" 2>/dev/null | sed -n "s/^${key}=//p" | head -n1
}

load_status() {
  result="unknown"
  phase="unknown"
  dkms_total="0"
  dkms_ok="0"
  dkms_ko="0"
  dkms_remove_ok="0"
  dkms_remove_ko="0"
  dkms_failed_modules=""
  dkms_obsolete_skipped="0"
  dkms_obsolete_modules=""

  [ -f "${STATUS_FILE}" ] || return 1
  while IFS='=' read -r key value; do
    case "$key" in
      result|phase|dkms_total|dkms_ok|dkms_ko|dkms_remove_ok|dkms_remove_ko|dkms_failed_modules|dkms_obsolete_skipped|dkms_obsolete_modules)
        printf -v "$key" '%s' "$value"
        ;;
    esac
  done < "${STATUS_FILE}"
  return 0
}

build_message() {
  urgency="critical"
  local modules_lines=""
  local obsolete_lines=""
  case "${result}" in
    success)
      title="Mise a niveau Debian terminee"
      body="Mise a niveau terminee avec succes. Cliquez pour lire le journal detaille si besoin."
      ;;
    partial_dkms)
      title="Mise a niveau terminee avec avertissement DKMS"
      if [ -n "${dkms_failed_modules}" ]; then
        modules_lines="$(printf '%s' "${dkms_failed_modules}" | tr ',' '\n')"
        if [ "${dkms_obsolete_skipped}" != "0" ] && [ -n "${dkms_obsolete_modules}" ]; then
          obsolete_lines="$(printf '%s' "${dkms_obsolete_modules}" | tr ',' '\n')"
          body="$(printf 'DKMS: %s echec(s) sur %s.\nModules en echec :\n%s\nEntrees obsoletes ignorees: %s\n%s\nNettoyage auto (dkms remove): %s ok, %s echec(s).\nVerifiez les pilotes et lisez le journal detaille.' \
            "${dkms_ko}" "${dkms_total}" "${modules_lines}" "${dkms_obsolete_skipped}" "${obsolete_lines}" "${dkms_remove_ok}" "${dkms_remove_ko}")"
        else
          body="$(printf 'DKMS: %s echec(s) sur %s.\nModules en echec :\n%s\nNettoyage auto (dkms remove): %s ok, %s echec(s).\nVerifiez les pilotes et lisez le journal detaille.' \
            "${dkms_ko}" "${dkms_total}" "${modules_lines}" "${dkms_remove_ok}" "${dkms_remove_ko}")"
        fi
      else
        if [ "${dkms_obsolete_skipped}" != "0" ] && [ -n "${dkms_obsolete_modules}" ]; then
          obsolete_lines="$(printf '%s' "${dkms_obsolete_modules}" | tr ',' '\n')"
          body="$(printf 'DKMS: %s echec(s) sur %s.\nEntrees obsoletes ignorees: %s\n%s\nNettoyage auto (dkms remove): %s ok, %s echec(s).\nVerifiez les pilotes et lisez le journal detaille.' \
            "${dkms_ko}" "${dkms_total}" "${dkms_obsolete_skipped}" "${obsolete_lines}" "${dkms_remove_ok}" "${dkms_remove_ko}")"
        else
          body="DKMS: ${dkms_ko} echec(s) sur ${dkms_total}. Nettoyage auto (dkms remove): ${dkms_remove_ok} ok, ${dkms_remove_ko} echec(s). Verifiez les pilotes et lisez le journal detaille."
        fi
      fi
      ;;
    success_dkms)
      title="Mise a niveau Debian terminee"
      if [ "${dkms_obsolete_skipped}" != "0" ] && [ -n "${dkms_obsolete_modules}" ]; then
        obsolete_lines="$(printf '%s' "${dkms_obsolete_modules}" | tr ',' '\n')"
        body="$(printf 'Mise a niveau terminee avec succes.\nEntrees DKMS obsoletes ignorees: %s\n%s\nCliquez pour lire le journal detaille si besoin.' \
          "${dkms_obsolete_skipped}" "${obsolete_lines}")"
      else
        body="Mise a niveau terminee avec succes. Cliquez pour lire le journal detaille si besoin."
      fi
      ;;
    failed_upgrade)
      title="Echec de la mise a niveau Debian"
      body="La phase offline a echoue. Lisez le journal detaille pour diagnostiquer."
      ;;
    *)
      title="Resultat de mise a niveau Debian"
      body="Un resultat post-upgrade est disponible (${result}). Lisez le journal detaille si necessaire."
      ;;
  esac
}

notify_for_session() {
  local session="$1"

  local user uid active state remote leader
  user="$(get_session_prop "$session" Name)"
  uid="$(get_session_prop "$session" User)"
  active="$(get_session_prop "$session" Active)"
  state="$(get_session_prop "$session" State)"
  remote="$(get_session_prop "$session" Remote)"
  leader="$(get_session_prop "$session" Leader)"

  [ -n "$user" ] || return 1
  [ -n "$uid" ] || return 1
  [ "$active" = "yes" ] || return 1
  [ "$state" = "active" ] || return 1
  [ "$remote" = "no" ] || return 1
  is_sudo_or_root_user "$user" || return 1

  local xdg_runtime_dir dbus_bus
  xdg_runtime_dir="$(read_env_from_leader "$leader" XDG_RUNTIME_DIR)"
  [ -n "$xdg_runtime_dir" ] || xdg_runtime_dir="/run/user/${uid}"
  dbus_bus="$(read_env_from_leader "$leader" DBUS_SESSION_BUS_ADDRESS)"
  [ -n "$dbus_bus" ] || dbus_bus="unix:path=${xdg_runtime_dir}/bus"
  [ -S "${xdg_runtime_dir}/bus" ] || return 1

  local action
  action="$(
    sudo -u "$user" env \
    XDG_RUNTIME_DIR="$xdg_runtime_dir" \
    DBUS_SESSION_BUS_ADDRESS="$dbus_bus" \
    notify-send \
      --app-name="$APP_NAME" \
      --icon="$APP_ICON" \
      --urgency="$urgency" \
      --expire-time=0 \
      --wait \
      --action="open_log=Lire le journal detaille" \
      "$title" \
      "$body" \
      || true
  )"
  log "session ${session}: action post-upgrade recue='${action:-<none>}' pour ${user}."

  if [ "${action}" = "open_log" ]; then
    if sudo -u "$user" env \
      XDG_RUNTIME_DIR="$xdg_runtime_dir" \
      DBUS_SESSION_BUS_ADDRESS="$dbus_bus" \
      systemd-run --user --quiet --collect \
        --unit "debian-upgrade-open-log-${session}" \
        xdg-open "${LOG_PATH}"; then
      log "session ${session}: ouverture journal demandee par ${user}."
      return 0
    else
      log "session ${session}: echec ouverture journal pour ${user}."
      return 1
    fi
  fi

  log "session ${session}: notification post-upgrade envoyee sans action de traitement."
  return 2
}

main() {
  if [ ! -f "${PENDING_FILE}" ]; then
    exit 0
  fi
  if ! command -v notify-send >/dev/null 2>&1; then
    log "notify-send indisponible (libnotify-bin manquant)"
    exit 0
  fi
  load_status || exit 0
  build_message

  treated=0
  while IFS= read -r session_id; do
    [ -n "$session_id" ] || continue
    if notify_for_session "$session_id"; then
      treated=1
      break
    fi
  done < <(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}')

  if [ "$treated" -eq 1 ]; then
    rm -f "${PENDING_FILE}"
    log "Notification post-upgrade traitee, pending supprime."
  else
    log "Notification post-upgrade non traitee (pending conserve)."
  fi
}

main "$@"
