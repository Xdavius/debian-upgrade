#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${ROOT_DIR}/buildtest"

version_ge() {
  # Returns 0 if $1 >= $2
  [ "$(printf '%s\n' "$2" "$1" | sort -V | head -n1)" = "$2" ]
}

RUSTC_VERSION_RAW="$(rustc --version | awk '{print $2}')"
GUI_MIN_RUSTC="1.64.0"
GUI_BUILD_ENABLED=1
if ! version_ge "$RUSTC_VERSION_RAW" "$GUI_MIN_RUSTC"; then
  GUI_BUILD_ENABLED=0
fi

printf '[build] root: %s\n' "$ROOT_DIR"
printf '[build] cleaning %s\n' "$BUILD_DIR"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/bin" "$BUILD_DIR/logs"

if [ "$GUI_BUILD_ENABLED" -eq 1 ]; then
  printf '[build] checking frontend-gui (priority)\n'
  cargo check -p frontend-gui
else
  printf '[build][warn] rustc=%s < %s, GUI skipped (deps require newer toolchain)\n' "$RUSTC_VERSION_RAW" "$GUI_MIN_RUSTC"
fi

printf '[build] checking backend-cli\n'
cargo check -p backend-cli

printf '[build] running backend-cli tests\n'
cargo test -p backend-cli

if [ "$GUI_BUILD_ENABLED" -eq 1 ]; then
  printf '[build] building frontend-gui (release)\n'
  cargo build -p frontend-gui --release --bin debian-upgrade
fi

printf '[build] building backend-cli (release)\n'
cargo build -p backend-cli --release --bin debian-upgrade-backend

cp "$ROOT_DIR/target/release/debian-upgrade-backend" "$BUILD_DIR/bin/debian-upgrade-backend"
if [ "$GUI_BUILD_ENABLED" -eq 1 ]; then
  cp "$ROOT_DIR/target/release/debian-upgrade" "$BUILD_DIR/bin/debian-upgrade"
fi

cat > "$BUILD_DIR/run-dry-run-demo.sh" << 'RUNEOF'
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI="${SCRIPT_DIR}/bin/debian-upgrade-backend"
LOG_FILE="${SCRIPT_DIR}/logs/dry-run-demo.jsonl"

"$CLI" --dry-run --debug run-all | tee "$LOG_FILE"
printf '\nDemo terminée. Logs enregistrés dans: %s\n' "$LOG_FILE"
RUNEOF

cat > "$BUILD_DIR/run-gui.sh" << 'GUIEOF'
#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# The GUI resolves ./buildtest/bin/debian-upgrade-cli automatically from repo root,
# but here we run from buildtest so we export PATH with local bin to be explicit.
export PATH="$SCRIPT_DIR/bin:$PATH"
if [ -d /usr/share/X11/xkb ]; then
  export XKB_CONFIG_ROOT="${XKB_CONFIG_ROOT:-/usr/share/X11/xkb}"
fi
export WINIT_UNIX_BACKEND="${WINIT_UNIX_BACKEND:-x11}"
export WINIT_X11_SCALE_FACTOR="${WINIT_X11_SCALE_FACTOR:-1}"
export LIBGL_ALWAYS_SOFTWARE="${LIBGL_ALWAYS_SOFTWARE:-1}"

if [ ! -x "$SCRIPT_DIR/bin/debian-upgrade" ]; then
  echo "GUI binaire absent dans buildtest/bin."
  echo "Cause probable: rustc local trop ancien pour les deps GUI (minimum pratique >= 1.64)."
  exit 1
fi

"$SCRIPT_DIR/bin/debian-upgrade" "$@"
GUIEOF

cat > "$BUILD_DIR/test-offline-two-phase-dryrun.sh" << 'OFFEOF'
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
OFFEOF

chmod +x "$BUILD_DIR/run-dry-run-demo.sh" "$BUILD_DIR/run-gui.sh" "$BUILD_DIR/test-offline-two-phase-dryrun.sh"

cat > "$BUILD_DIR/README.md" << 'EOF2'
# buildtest

Contenu de test local généré par `./build.sh`.

## Lancer la GUI (prioritaire)

```bash
./run-gui.sh
```

Mode debug:

```bash
./run-gui.sh --debug
```

Note: si `rustc` est trop ancien (ex: 1.63), la GUI peut etre ignoree au build et seul le backend est produit.

## Lancer une démo dry-run CLI seule

```bash
./run-dry-run-demo.sh
```

## Tester l'offline upgrade en 2 phases (simulation)

```bash
./test-offline-two-phase-dryrun.sh
```
EOF2

printf '[build] done. test bundle available in %s\n' "$BUILD_DIR"
