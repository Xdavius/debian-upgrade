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
