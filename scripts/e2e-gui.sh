#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# End-to-end GUI check under Xvfb: open generated files in the real app, press
# Convert, and verify that every output exists and is readable by ffprobe.
# Settings/cache/queue are isolated in a temporary directory.
# Usage: scripts/e2e-gui.sh [path/to/retort]   (default: cargo build)
set -euo pipefail
cd "$(dirname "$0")/.."
if [ $# -ge 1 ]; then
  bin=$(realpath "$1")
else
  cargo build -p retort
  bin=$PWD/target/debug/retort
fi
work=$(mktemp -d)
trap 'rm -rf "${work:?}"' EXIT
media="$work/in put's [media]"
mkdir -p "$media" "$work/home"
ffmpeg -v error -nostdin -f lavfi -i testsrc2=s=640x360:d=2 -f lavfi -i sine=d=2 \
  -c:v libx264 -preset ultrafast -c:a aac "file:$media/clip one.mkv"
ffmpeg -v error -nostdin -f lavfi -i "sine=f=330:d=3" -c:a flac "file:$media/tone #2.flac"

# A Meson build tree has the compiled schema in data/ (Meson builds carry no
# build-tree paths; `meson devenv` sets the same variable).
schemas="$(dirname "$bin")/data"
if [ -f "$schemas/gschemas.compiled" ]; then
  export GSETTINGS_SCHEMA_DIR="$schemas"
fi
export HOME="$work/home" XDG_CONFIG_HOME="$work/home/.config" XDG_DATA_HOME="$work/home/.local/share" \
  XDG_CACHE_HOME="$work/home/.cache" GSETTINGS_BACKEND=memory RETORT_E2E=1 GSK_RENDERER=cairo \
  GTK_A11Y=none NO_AT_BRIDGE=1
status=0
xvfb-run -a dbus-run-session -- "$bin" "$media/clip one.mkv" "$media/tone #2.flac" || status=$?
if [ "$status" -ne 0 ]; then
  echo "e2e: app exited with $status" >&2
  exit 1
fi
for f in "$media/clip one.mp4" "$media/tone #2.mp4"; do
  ffprobe -v error -show_entries format=format_name,duration -of csv=p=0 "file:$f" \
    || { echo "e2e: missing or unreadable output: $f" >&2; ls -la "$media" >&2; exit 1; }
done
# Inputs untouched, no temp files left behind.
shopt -s nullglob dotglob
leftovers=("$media"/*.part.*)
if [ ${#leftovers[@]} -gt 0 ]; then
  echo "e2e: temporary files left behind: ${leftovers[*]}" >&2
  exit 1
fi
# Advanced mode through its widgets: rotate 90°, trim to 1.5 s, mono audio.
adv="$work/adv"
mkdir -p "$adv"
cp "$media/clip one.mkv" "$adv/clip.mkv"
# Fresh profile: the first run's queue must not be restored here.
mkdir -p "$work/home2"
export HOME="$work/home2" XDG_CONFIG_HOME="$work/home2/.config" XDG_DATA_HOME="$work/home2/.local/share" \
  XDG_CACHE_HOME="$work/home2/.cache"
status=0
RETORT_E2E_ADVANCED=1 xvfb-run -a dbus-run-session -- "$bin" "$adv/clip.mkv" || status=$?
[ "$status" -eq 0 ] || { echo "e2e: advanced run exited with $status" >&2; exit 1; }
dims=$(ffprobe -v error -select_streams v:0 -show_entries stream=width,height -of csv=p=0 "file:$adv/clip.mp4")
chans=$(ffprobe -v error -select_streams a:0 -show_entries stream=channels -of csv=p=0 "file:$adv/clip.mp4")
dur=$(ffprobe -v error -show_entries format=duration -of csv=p=0 "file:$adv/clip.mp4")
echo "e2e: advanced output ${dims} channels=${chans} duration=${dur}"
[ "$dims" = "360,640" ] || { echo "e2e: expected 360x640 after rotation, got $dims" >&2; exit 1; }
[ "$chans" = "1" ] || { echo "e2e: expected mono, got $chans channels" >&2; exit 1; }
awk -v d="$dur" 'BEGIN { exit !(d > 1.2 && d < 1.9) }' || { echo "e2e: expected ~1.5 s, got $dur" >&2; exit 1; }
echo "e2e: ok"
