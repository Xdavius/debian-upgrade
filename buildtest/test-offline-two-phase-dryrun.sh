#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
OFFLINE_SCRIPT="$REPO_ROOT/packaging/assets/bin/offline-upgrade.sh"

ROOT="$(mktemp -d /tmp/debian-upgrade-offline-sim.XXXXXX)"
trap 'rm -rf "$ROOT"' EXIT

mkdir -p "$ROOT/var/lib/debian-upgrade" "$ROOT/var/lib/system-update" "$ROOT/etc/apt/sources.list.d"
ln -snf "$ROOT/var/lib/system-update" "$ROOT/system-update"

cat > "$ROOT/var/lib/debian-upgrade/dkms-reinstall.list" <<'DKMS'
nvidia-current,550.90.12
virtualbox,7.0.20
DKMS

cat > "$ROOT/var/lib/debian-upgrade/third-party-reactivate.list" <<'REPOS'
antigravity.list
nvidia.list
REPOS

cat > "$ROOT/etc/apt/sources.list.d/antigravity.list" <<'APT'
# debian-upgrade-disabled deb [arch=amd64] https://example.invalid/antigravity stable main
APT
cat > "$ROOT/etc/apt/sources.list.d/nvidia.list" <<'APT'
# debian-upgrade-disabled deb https://example.invalid/nvidia stable main
APT

echo "== Dry-run phase 1 (upgrade) =="
OFFLINE_UPGRADE_ROOT_PREFIX="$ROOT" \
OFFLINE_UPGRADE_SIMULATE=1 \
"$OFFLINE_SCRIPT"

printf '\n== Etat apres phase 1 ==\n'
ls -la "$ROOT/var/lib/debian-upgrade" || true
printf 'offline-phase: '; cat "$ROOT/var/lib/debian-upgrade/offline-phase" || true
printf 'phase1.ok exists: '; test -f "$ROOT/var/lib/debian-upgrade/phase1.ok" && echo yes || echo no
printf 'system-update link: '; readlink -f "$ROOT/system-update" || true

echo "\n== Dry-run phase 2 (dkms) =="
OFFLINE_UPGRADE_ROOT_PREFIX="$ROOT" \
OFFLINE_UPGRADE_SIMULATE=1 \
"$OFFLINE_SCRIPT"

printf '\n== Etat final ==\n'
ls -la "$ROOT/var/lib/debian-upgrade" || true
printf 'phase2.done exists: '; test -f "$ROOT/var/lib/debian-upgrade/phase2.done" && echo yes || echo no
printf '\n== Log offline ==\n'
cat "$ROOT/var/log/debian-upgrade-offline.log"
