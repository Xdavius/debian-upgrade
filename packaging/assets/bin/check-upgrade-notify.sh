#!/usr/bin/env bash
set -euo pipefail

APP_NAME="Debian-Upgrade"
APP_ICON="system-software-update"
BACKEND_BIN="${DEBIAN_UPGRADE_BACKEND:-/usr/libexec/debian-upgrade-backend}"
GUI_BIN="${DEBIAN_UPGRADE_GUI:-/usr/bin/debian-upgrade}"
STATE_DIR="/var/lib/debian-upgrade/notify"

log() {
  logger -t debian-upgrade-notify -- "$*"
}

ensure_state_dir() {
  install -d -m 0755 "$STATE_DIR"
}

has_network() {
  timeout 8 getent hosts deb.debian.org >/dev/null 2>&1
}

is_sudo_or_root_user() {
  local user="$1"
  if [ "$user" = "root" ]; then
    return 0
  fi
  id -nG "$user" 2>/dev/null | tr ' ' '\n' | grep -qx "sudo"
}

next_notify_file_for_uid() {
  local uid="$1"
  printf '%s/%s.next-notify-epoch\n' "$STATE_DIR" "$uid"
}

should_skip_due_to_defer() {
  local uid="$1"
  local file now next
  file="$(next_notify_file_for_uid "$uid")"
  if [ ! -f "$file" ]; then
    return 1
  fi
  now="$(date +%s)"
  next="$(cat "$file" 2>/dev/null || true)"
  if [ -z "$next" ]; then
    return 1
  fi
  if [ "$now" -lt "$next" ]; then
    return 0
  fi
  rm -f "$file"
  return 1
}

defer_uid() {
  local uid="$1"
  local seconds="$2"
  local now next file
  file="$(next_notify_file_for_uid "$uid")"
  now="$(date +%s)"
  next="$((now + seconds))"
  printf '%s\n' "$next" > "$file"
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

notify_for_session() {
  local session="$1"
  local stable_message="$2"

  local user uid active state remote leader
  user="$(get_session_prop "$session" Name)"
  uid="$(get_session_prop "$session" User)"
  active="$(get_session_prop "$session" Active)"
  state="$(get_session_prop "$session" State)"
  remote="$(get_session_prop "$session" Remote)"
  leader="$(get_session_prop "$session" Leader)"

  [ -n "$user" ] || return 0
  [ -n "$uid" ] || return 0
  [ "$active" = "yes" ] || return 0
  [ "$state" = "active" ] || return 0
  [ "$remote" = "no" ] || return 0
  is_sudo_or_root_user "$user" || return 0
  should_skip_due_to_defer "$uid" && return 0

  local xdg_runtime_dir dbus_bus
  xdg_runtime_dir="$(read_env_from_leader "$leader" XDG_RUNTIME_DIR)"
  [ -n "$xdg_runtime_dir" ] || xdg_runtime_dir="/run/user/${uid}"
  dbus_bus="$(read_env_from_leader "$leader" DBUS_SESSION_BUS_ADDRESS)"
  [ -n "$dbus_bus" ] || dbus_bus="unix:path=${xdg_runtime_dir}/bus"
  if [ ! -S "${xdg_runtime_dir}/bus" ]; then
    log "session ${session}: bus DBus absent pour ${user} (${xdg_runtime_dir}/bus)"
    return 0
  fi

  local action
  action="$(
    sudo -u "$user" env \
      XDG_RUNTIME_DIR="$xdg_runtime_dir" \
      DBUS_SESSION_BUS_ADDRESS="$dbus_bus" \
      notify-send \
        --app-name="$APP_NAME" \
        --icon="$APP_ICON" \
        --urgency=critical \
        --expire-time=0 \
        --action="open=Lancer la mise a niveau" \
        --action="defer_day=Reporter 1 jour" \
        --action="defer_week=Reporter 1 semaine" \
        --action="defer_month=Reporter 1 mois" \
        "Debian-Upgrade" \
        "$stable_message" \
      || true
  )"

  case "$action" in
    open)
      sudo -u "$user" env \
        XDG_RUNTIME_DIR="$xdg_runtime_dir" \
        DBUS_SESSION_BUS_ADDRESS="$dbus_bus" \
        nohup "$GUI_BIN" >/dev/null 2>&1 &
      ;;
    defer_day)
      "$BACKEND_BIN" defer day >/dev/null 2>&1 || true
      defer_uid "$uid" $((24 * 60 * 60))
      ;;
    defer_week)
      "$BACKEND_BIN" defer week >/dev/null 2>&1 || true
      defer_uid "$uid" $((7 * 24 * 60 * 60))
      ;;
    defer_month)
      "$BACKEND_BIN" defer month >/dev/null 2>&1 || true
      defer_uid "$uid" $((30 * 24 * 60 * 60))
      ;;
    *)
      ;;
  esac
}

main() {
  ensure_state_dir

  if [ ! -x "$BACKEND_BIN" ]; then
    log "backend introuvable: $BACKEND_BIN"
    exit 0
  fi

  if [ ! -x "$GUI_BIN" ]; then
    log "gui introuvable: $GUI_BIN"
    exit 0
  fi

  if ! command -v notify-send >/dev/null 2>&1; then
    log "notify-send indisponible (libnotify-bin manquant)"
    exit 0
  fi

  if ! has_network; then
    exit 0
  fi

  local check_output success_line message
  check_output="$(timeout 30 "$BACKEND_BIN" check-new-release 2>/dev/null || true)"
  [ -n "$check_output" ] || exit 0

  success_line="$(
    printf '%s\n' "$check_output" \
      | grep '"step":"check-new-release"' \
      | grep '"level":"success"' \
      | tail -n 1 || true
  )"
  [ -n "$success_line" ] || exit 0

  message="$(
    printf '%s\n' "$success_line" \
      | sed -n 's/.*"message":"\([^"]*\)".*/\1/p' \
      | sed 's/\\"/"/g' \
      | head -n1
  )"
  if [ -z "$message" ]; then
    message="Une nouvelle version majeure Debian est disponible."
  fi

  while IFS= read -r session_id; do
    [ -n "$session_id" ] || continue
    notify_for_session "$session_id" "$message"
  done < <(loginctl list-sessions --no-legend 2>/dev/null | awk '{print $1}')
}

main "$@"
