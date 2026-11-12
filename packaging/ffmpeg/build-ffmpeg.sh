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
LIBPLACEBO_VERSION=${LIBPLACEBO_VERSION:-v7.360.1}
VULKAN_HEADERS_VERSION=${VULKAN_HEADERS_VERSION:-v1.4.368}
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

# Vulkan headers: FFmpeg 9.0 needs >= 1.4.317 for av1_vulkan encode and VP9
# Vulkan decode. Header-only; the loader/drivers come from the host.
if [ ! -d Vulkan-Headers ]; then
  git clone -q --depth 1 --branch "$VULKAN_HEADERS_VERSION" https://github.com/KhronosGroup/Vulkan-Headers.git
fi
cmake -S Vulkan-Headers -B Vulkan-Headers/build -DCMAKE_INSTALL_PREFIX="$prefix" -DVULKAN_HEADERS_ENABLE_MODULE=OFF >/dev/null
cmake --install Vulkan-Headers/build >/dev/null

# libplacebo: FFmpeg 9.0 needs >= 7.351 (newer than most distros ship).
if [ ! -d libplacebo ]; then
  git clone -q --depth 1 --recurse-submodules --shallow-submodules --branch "$LIBPLACEBO_VERSION" \
    https://code.videolan.org/videolan/libplacebo.git
fi
meson setup libplacebo/build libplacebo --prefix="$prefix" --libdir=lib --buildtype=release \
  -Dvulkan=enabled -Dglslang=enabled -Dshaderc=disabled -Dopengl=disabled -Dd3d11=disabled \
  -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false --wipe >/dev/null 2>&1 \
  || meson setup libplacebo/build libplacebo --prefix="$prefix" --libdir=lib --buildtype=release \
  -Dvulkan=enabled -Dglslang=enabled -Dshaderc=disabled -Dopengl=disabled -Dd3d11=disabled \
  -Ddemos=false -Dtests=false -Dbench=false -Dfuzz=false >/dev/null
ninja -C libplacebo/build install >/dev/null

if [ ! -d "ffmpeg-$FFMPEG_VERSION" ]; then
  curl -fsSL "https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz" -o "ffmpeg-$FFMPEG_VERSION.tar.xz"
  tar xf "ffmpeg-$FFMPEG_VERSION.tar.xz"
fi
cd "ffmpeg-$FFMPEG_VERSION"

# FFmpeg <= 8 links libshaderc for runtime GLSL; 9.0+ compiles shaders at
# build time with glslc/glslang (from glslang-tools).
shader_flag=""
if ./configure --help | grep -q -- "--enable-libshaderc"; then shader_flag="--enable-libshaderc"; fi

# Intel oneVPL (Quick Sync) and AMD AMF exist only on x86-64.
x86_flags=""
if [ "$(uname -m)" = x86_64 ]; then x86_flags="--enable-libvpl --enable-amf"; fi

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
  --enable-libplacebo --enable-vulkan $shader_flag \
  --enable-vaapi --enable-libdrm $x86_flags \
  --enable-ffnvcodec --enable-nvenc --enable-nvdec --enable-cuvid --enable-cuda-llvm \
  --enable-v4l2-m2m \
  --enable-gnutls
make -j"$jobs" >/dev/null
make install >/dev/null
export LD_LIBRARY_PATH="$prefix/lib${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"
"$prefix/bin/ffmpeg" -hide_banner -version | head -1
