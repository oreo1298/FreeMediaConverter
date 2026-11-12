#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Dump the AVOption names of the hardware/software encoders we map, for several
# FFmpeg release tags, so docs/research/encoders.md can be re-verified when a
# new FFmpeg ships.
#
# Usage: scripts/research/ffmpeg-options.sh <workdir> [tag ...]
# Example: scripts/research/ffmpeg-options.sh /tmp/ffopts n6.1.1 n7.1.5 n8.0.3 n9.0.2
set -euo pipefail

workdir=${1:?usage: $0 <workdir> [tag ...]}
shift
tags=("$@")
[ ${#tags[@]} -eq 0 ] && tags=(n6.1.1 n7.1.5 n8.0.3 n9.0.2)

files=(
  nvenc.c nvenc_h264.c nvenc_hevc.c nvenc_av1.c
  vaapi_encode.h vaapi_encode_h264.c vaapi_encode_h265.c vaapi_encode_av1.c vaapi_encode_vp9.c
  hw_base_encode.h
  qsvenc.h qsvenc_h264.c qsvenc_hevc.c qsvenc_av1.c
  vulkan_encode.h vulkan_encode_h264.c vulkan_encode_h265.c vulkan_encode_av1.c
  amfenc.h amfenc_h264.c amfenc_hevc.c amfenc_av1.c
  v4l2_m2m_enc.c libsvtav1.c libx264.c libx265.c libvpxenc.c libaomenc.c
)

mkdir -p "$workdir"
if [ ! -d "$workdir/ffmpeg/.git" ]; then
  git clone -q --filter=blob:none --no-checkout https://github.com/FFmpeg/FFmpeg.git "$workdir/ffmpeg"
fi

for tag in "${tags[@]}"; do
  echo "### $tag"
  for f in "${files[@]}"; do
    if src=$(git -C "$workdir/ffmpeg" show "$tag:libavcodec/$f" 2>/dev/null); then
      names=$(printf '%s\n' "$src" | grep -oE '^\s*\{ *"[a-z0-9_-]+"' | tr -d ' {"' | sort -u | tr '\n' ' ')
      printf '%-24s %s\n' "$f" "$names"
    else
      printf '%-24s (absent)\n' "$f"
    fi
  done
  echo
done
