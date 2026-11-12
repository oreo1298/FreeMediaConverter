#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# AppImage: built on Ubuntu 24.04 (needs glibc >= 2.39 on the host), bundles
# GTK 4 / libadwaita and a redistributable FFmpeg (packaging/ffmpeg/build-ffmpeg.sh).
# VA-API, Vulkan and DRM libraries come from the host so its GPU drivers work.
# Usage: packaging/appimage/build.sh [platform]   (e.g. linux/arm64)
# Output: dist/<App>-<version>-<arch>.AppImage (+ .zsync)
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "$0")/../common.sh"
platform=${1:-}
ensure_dist
plat=()
[ -n "$platform" ] && plat=(--platform "$platform")
app_name=$(sed -n 's/^APP_NAME=//p' "$pkg_root/branding.env")
owner=$(sed -n 's/^APP_REPO_OWNER=//p' "$pkg_root/branding.env")
repo=$(sed -n 's/^APP_REPO_NAME=//p' "$pkg_root/branding.env")
docker run --rm "${plat[@]}" "${docker_args[@]}" \
  -v "$pkg_root/dist:/dist" -v "$pkg_root/packaging/ffmpeg:/ffbuild:ro" \
  -e BIN="$pkg_bin" -e PKGVER="$pkg_version" -e APP_ID="$pkg_id" -e APP_NAME="$app_name" \
  -e OWNER="$owner" -e REPO="$repo" -e APPIMAGE_EXTRACT_AND_RUN=1 ubuntu:24.04 bash -c "$ca_prelude"'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential meson ninja-build pkg-config python3 gettext \
  appstream desktop-file-utils libglib2.0-dev-bin libgtk-4-dev libadwaita-1-dev curl ca-certificates xz-utils \
  file git cmake nasm clang llvm python3-jinja2 liblcms2-dev libunwind-dev libx264-dev libx265-dev \
  libsvtav1enc-dev libdav1d-dev libaom-dev libvpx-dev libopus-dev libvorbis-dev libmp3lame-dev libwebp-dev \
  libzimg-dev libass-dev libsoxr-dev libfreetype-dev libfontconfig-dev libharfbuzz-dev libvulkan-dev \
  glslang-dev glslang-tools spirv-headers libva-dev libdrm-dev libvpl-dev libgnutls28-dev zsync \
  librsvg2-common gstreamer1.0-tools >/dev/null
arch=$(uname -m)
# FFmpeg (latest stable) built for this base.
WORK=/tmp/ffbuild bash /ffbuild/build-ffmpeg.sh /opt/ffmpeg
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable >/dev/null
export PATH=$HOME/.cargo/bin:$PATH
mkdir -p /build && cd /build
tar xf /dist/$BIN-$PKGVER.tar.xz && tar xf /dist/$BIN-$PKGVER-vendor.tar.xz
cd $BIN-$PKGVER
meson setup _build --prefix=/usr --buildtype=release -Doffline=true -Dupdate_check=true >/dev/null
meson compile -C _build >/tmp/build.log 2>&1 || { tail -40 /tmp/build.log; exit 1; }
DESTDIR=/AppDir meson install -C _build >/dev/null
glib-compile-schemas /AppDir/usr/share/glib-2.0/schemas
cp /opt/ffmpeg/bin/ffmpeg /opt/ffmpeg/bin/ffprobe /AppDir/usr/bin/
cd /tmp
base=https://github.com/linuxdeploy
curl -sSfLO $base/linuxdeploy/releases/download/continuous/linuxdeploy-$arch.AppImage
curl -sSfLO $base/linuxdeploy-plugin-appimage/releases/download/continuous/linuxdeploy-plugin-appimage-$arch.AppImage
curl -sSfLO https://raw.githubusercontent.com/linuxdeploy/linuxdeploy-plugin-gtk/master/linuxdeploy-plugin-gtk.sh
chmod +x linuxdeploy-*.AppImage linuxdeploy-plugin-gtk.sh
export LD_LIBRARY_PATH=/opt/ffmpeg/lib
export DEPLOY_GTK_VERSION=4
export LDAI_UPDATE_INFORMATION="gh-releases-zsync|$OWNER|$REPO|latest|$APP_NAME-*-$arch.AppImage.zsync"
export LDAI_OUTPUT="$APP_NAME-$PKGVER-$arch.AppImage"
./linuxdeploy-$arch.AppImage --appdir /AppDir --plugin gtk --output appimage \
  -d /AppDir/usr/share/applications/$APP_ID.desktop \
  -i /AppDir/usr/share/icons/hicolor/scalable/apps/$APP_ID.svg \
  -e /AppDir/usr/bin/$BIN -e /AppDir/usr/bin/$BIN-cli -e /AppDir/usr/bin/ffmpeg -e /AppDir/usr/bin/ffprobe \
  --exclude-library "libva*" --exclude-library "libvulkan*" --exclude-library "libdrm*" \
  --exclude-library "libvpl*" >/tmp/ld.log 2>&1 || { tail -40 /tmp/ld.log; exit 1; }
# Smoke test: the bundled CLI finds the bundled FFmpeg.
./$LDAI_OUTPUT --appimage-extract >/dev/null
APPDIR=/tmp/squashfs-root /tmp/squashfs-root/usr/bin/$BIN-cli diagnose > /tmp/diag.txt; head -8 /tmp/diag.txt
mv $LDAI_OUTPUT* /dist/
ls -la /dist/*.AppImage*
'
