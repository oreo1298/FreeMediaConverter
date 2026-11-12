#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Regenerate the tiny clips used to probe hardware *decoders*. They are
# shipped inside the binary so decode probing never depends on the user's
# ffmpeg having software encoders (e.g. Fedora's ffmpeg-free).
# 320x240 satisfies every hardware decoder's minimum size; 4 frames each.
set -euo pipefail
out=${1:-"$(dirname "$0")/../data/probe-clips"}
mkdir -p "$out"
src="testsrc2=s=320x240:r=25:d=0.16"
enc() { # name pix_fmt encoder [extra args...]
  local name=$1 pix=$2 encoder=$3; shift 3
  ffmpeg -v error -nostdin -y -f lavfi -i "$src" -frames:v 4 -pix_fmt "$pix" -c:v "$encoder" "$@" \
    -an -map_metadata -1 -fflags +bitexact -flags:v +bitexact -f matroska "$out/$name.mkv"
}
enc h264_8   yuv420p     libx264    -preset veryfast -crf 35 -x264-params log-level=error
enc hevc_8   yuv420p     libx265    -preset veryfast -crf 35 -x265-params log-level=error
enc hevc_10  yuv420p10le libx265    -preset veryfast -crf 35 -x265-params log-level=error
enc av1_8    yuv420p     libaom-av1 -cpu-used 8 -crf 50 -row-mt 1
enc av1_10   yuv420p10le libaom-av1 -cpu-used 8 -crf 50 -row-mt 1
enc vp9_8    yuv420p     libvpx-vp9 -deadline realtime -cpu-used 8 -crf 50 -b:v 0
enc vp9_10   yuv420p10le libvpx-vp9 -deadline realtime -cpu-used 8 -crf 50 -b:v 0 -profile:v 2
enc vp8_8    yuv420p     libvpx     -deadline realtime -cpu-used 8 -crf 50 -b:v 200k
enc mpeg2_8  yuv420p     mpeg2video -q:v 20
ls -l "$out"
