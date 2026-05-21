#!/usr/bin/env bash
set -euo pipefail

SCRIPT_PATH="/usr/local/lib/debian-upgrade/offline-upgrade.sh"
SERVICE_PATH="/usr/lib/systemd/system/debian-upgrade-offline.service"
WANTS_LINK="/etc/systemd/system/system-update.target.wants/debian-upgrade-offline.service"
MARKER_LINK="/system-update"
MARKER_TARGET="/var/lib/system-update"

say() {
  printf '[test-offline-arm] %s\n' "$*"
}

say "1/4 Test pkexec"
pkexec /bin/sh -c 'id -u && echo PKEXEC_OK'

say "2/4 Verification fichiers requis"
[ -x "$SCRIPT_PATH" ] || { say "ERREUR: script manquant/non executable: $SCRIPT_PATH"; exit 1; }
[ -f "$SERVICE_PATH" ] || { say "ERREUR: service manquant: $SERVICE_PATH"; exit 1; }
say "OK fichiers requis"

say "3/4 Armement offline (sans reboot) via pkexec"
pkexec /bin/sh -c "
set -euo pipefail
install -d -m 0755 /var/lib/system-update
install -d -m 0755 /etc/systemd/system/system-update.target.wants
ln -snf '$SERVICE_PATH' '$WANTS_LINK'
ln -snf '$MARKER_TARGET' '$MARKER_LINK'
systemctl daemon-reload
echo ARMED_OK
"

say "4/4 Verification etat arme"
ls -l "$MARKER_LINK"
ls -l "$WANTS_LINK"
readlink -f "$MARKER_LINK" | grep -qx "$MARKER_TARGET" || { say "ERREUR: /system-update ne pointe pas vers $MARKER_TARGET"; exit 1; }
readlink -f "$WANTS_LINK" | grep -qx "$SERVICE_PATH" || { say "ERREUR: lien service offline invalide"; exit 1; }

say "SUCCESS: armement offline valide (reboot non declenche par ce test)."
