#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
CLI="${SCRIPT_DIR}/bin/debian-upgrade-backend"
LOG_FILE="${SCRIPT_DIR}/logs/dry-run-demo.jsonl"

"$CLI" --dry-run --debug run-all | tee "$LOG_FILE"
printf '\nDemo terminée. Logs enregistrés dans: %s\n' "$LOG_FILE"
