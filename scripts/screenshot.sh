#!/usr/bin/env bash
# Screenshots the playground on a private Xvfb (never the user's desktop).
# Usage: scripts/screenshot.sh <out.png> [demo args…]
# RUSTEXT_INPUT=<file> replays scripted input (see the demo's --script option) before the shot.
set -euo pipefail
out="$(realpath -m "$1")"; shift
cd "$(dirname "$0")/.."
cargo build -q -p rustext-demo --release
env -u WAYLAND_DISPLAY LIBGL_ALWAYS_SOFTWARE=1 EFRAME_SCREENSHOT_TO="$out" \
  xvfb-run -a -s "-screen 0 1400x900x24 -ac +extension GLX +render" \
  target/release/rustext-demo "$@"
