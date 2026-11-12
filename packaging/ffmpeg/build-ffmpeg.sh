#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a redistributable (GPL, no --enable-nonfree) FFmpeg with every
# hardware backend Retort supports. Used by CI (integration tests against the
# latest release), the AppImage and the tarball's optional bundled ffmpeg.
#
# Runs inside a Debian 13 (trixie) container; see packaging/ffmpeg/Dockerfile.
# Usage: build-ffmpeg.sh <prefix>
set -euo pipefail

FFMPEG_VERSION=${FFMPEG_VERSION:-9.0.2}
NV_CODEC_HEADERS_VERSION=${NV_CODEC_HEADERS_VERSION:-n12.2.72.0}
AMF_VERSION=${AMF_VERSION:-v1.5.3}
prefix=${1:?usage: build-ffmpeg.sh <prefix>}
work=${WORK:-/tmp/ffbuild}
jobs=$(nproc)

mkdir -p "$work" "$prefix"
cd "$work"

# nv-codec-headers: NVENC/NVDEC/CUDA loader headers (MIT, redistributable).
if [ ! -d nv-codec-headers ]; then
  git clone -q --depth 1 --branch "$NV_CODEC_HEADERS_VERSION" https://github.com/FFmpeg/nv-codec-headers.git
fi
make -C nv-codec-headers PREFIX="$prefix" install >/dev/null

# AMF headers (MIT) — headers only, the runtime comes from AMD's driver.
if [ ! -d AMF ]; then
  git clone -q --depth 1 --branch "$AMF_VERSION" --filter=blob:none --sparse https://github.com/GPUOpen-LibrariesAndSDKs/AMF.git
  git -C AMF sparse-checkout set amf/public/include
fi
mkdir -p "$prefix/include/AMF"
cp -r AMF/amf/public/include/* "$prefix/include/AMF/"

if [ ! -d "ffmpeg-$FFMPEG_VERSION" ]; then
  curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" -o "ffmpeg-$FFMPEG_VERSION.tar.xz"
  tar xf "ffmpeg-$FFMPEG_VERSION.tar.xz"
fi
cd "ffmpeg-$FFMPEG_VERSION"

export PKG_CONFIG_PATH="$prefix/lib/pkgconfig:$prefix/share/pkgconfig:${PKG_CONFIG_PATH:-}"
./configure \
  --prefix="$prefix" \
  --extra-cflags="-I$prefix/include" \
  --enable-gpl --enable-version3 --disable-nonfree \
  --disable-debug --disable-doc --disable-static --enable-shared \
  --enable-libx264 --enable-libx265 --enable-libsvtav1 --enable-libdav1d \
  --enable-libaom --enable-libvpx --enable-libopus --enable-libvorbis \
  --enable-libmp3lame --enable-libwebp --enable-libzimg --enable-libass \
  --enable-libsoxr --enable-libfreetype --enable-libfontconfig --enable-libharfbuzz \
  --enable-libplacebo --enable-vulkan --enable-libshaderc \
  --enable-vaapi --enable-libdrm --enable-libvpl \
  --enable-ffnvcodec --enable-nvenc --enable-nvdec --enable-cuvid \
  --enable-amf --enable-v4l2-m2m \
  --enable-gnutls
make -j"$jobs" >/dev/null
make install >/dev/null
"$prefix/bin/ffmpeg" -hide_banner -version | head -1
