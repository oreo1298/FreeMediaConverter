# SPDX-License-Identifier: GPL-3.0-or-later
# Build environment for packaging/ffmpeg/build-ffmpeg.sh
FROM debian:trixie
RUN apt-get update && DEBIAN_FRONTEND=noninteractive apt-get install -y --no-install-recommends \
      build-essential nasm pkg-config git curl ca-certificates xz-utils meson ninja-build cmake clang llvm python3 python3-jinja2 liblcms2-dev libunwind-dev \
      libx264-dev libx265-dev libsvtav1enc-dev libdav1d-dev libaom-dev libvpx-dev \
      libopus-dev libvorbis-dev libmp3lame-dev libwebp-dev libzimg-dev libass-dev \
      libsoxr-dev libfreetype-dev libfontconfig-dev libharfbuzz-dev \
      libplacebo-dev libvulkan-dev libshaderc-dev glslang-dev glslang-tools spirv-headers \
      libva-dev libdrm-dev libvpl-dev libgnutls28-dev \
    && rm -rf /var/lib/apt/lists/*
COPY build-ffmpeg.sh /usr/local/bin/build-ffmpeg.sh
