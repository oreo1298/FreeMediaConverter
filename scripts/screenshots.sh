#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Render every major UI state, light and dark, to docs/screenshots/ (or $1).
# Needs Xvfb and ffmpeg. Uses a simulated hardware profile (RETORT_DEMO_PROFILE).
set -euo pipefail
cd "$(dirname "$0")/.."
out=$(realpath -m "${1:-docs/screenshots}")
mkdir -p "$out"
cargo build -p retort
export RETORT_SCREENSHOT_DIR="$out"
export GSK_RENDERER=${GSK_RENDERER:-cairo}
export GTK_A11Y=none
export NO_AT_BRIDGE=1
xvfb-run -a -s "-screen 0 2560x1600x24" dbus-run-session -- target/debug/retort
rm -rf "${out:?}/media"
ls "$out"
