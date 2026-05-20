#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
BUILD_DIR="${ROOT_DIR}/buildtest"

printf '[build] root: %s\n' "$ROOT_DIR"
printf '[build] cleaning %s\n' "$BUILD_DIR"
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/bin" "$BUILD_DIR/logs"

printf '[build] checking frontend-gui (priority)\n'
cargo check -p frontend-gui

printf '[build] checking backend-cli\n'
cargo check -p backend-cli

printf '[build] running backend-cli tests\n'
cargo test -p backend-cli

printf '[build] building frontend-gui (release)\n'
cargo build -p frontend-gui --release --bin debian-upgrade

printf '[build] building backend-cli (release)\n'
cargo build -p backend-cli --release --bin debian-upgrade-backend

cp "$ROOT_DIR/target/release/debian-upgrade" "$BUILD_DIR/bin/debian-upgrade"
cp "$ROOT_DIR/target/release/debian-upgrade-backend" "$BUILD_DIR/bin/debian-upgrade-backend"

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

"$SCRIPT_DIR/bin/debian-upgrade" "$@"
GUIEOF

chmod +x "$BUILD_DIR/run-dry-run-demo.sh" "$BUILD_DIR/run-gui.sh"

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

## Lancer une démo dry-run CLI seule

```bash
./run-dry-run-demo.sh
```
EOF2

printf '[build] done. test bundle available in %s\n' "$BUILD_DIR"
