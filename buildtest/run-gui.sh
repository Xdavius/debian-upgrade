#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# The GUI resolves ./buildtest/bin/debian-upgrade-cli automatically from repo root,
# but here we run from buildtest so we export PATH with local bin to be explicit.
export PATH="$SCRIPT_DIR/bin:$PATH"

"$SCRIPT_DIR/bin/debian-upgrade" "$@"
