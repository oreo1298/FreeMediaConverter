#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Portable tarball: binaries built on Ubuntu 24.04 (glibc 2.39, GTK 4.14,
# libadwaita 1.5 — the oldest supported stack), plus data files and
# install.sh / uninstall.sh. Runs on any distro with those versions or newer.
# Usage: packaging/tarball/build.sh [platform]   (e.g. linux/arm64)
# Output: dist/<bin>-<version>-<arch>-linux.tar.xz
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "$0")/../common.sh"
platform=${1:-}
ensure_dist
plat=()
[ -n "$platform" ] && plat=(--platform "$platform")
docker run --rm "${plat[@]}" "${docker_args[@]}" \
  -v "$pkg_root/dist:/dist" -v "$pkg_root/packaging/tarball:/tarball:ro" \
  -e BIN="$pkg_bin" -e PKGVER="$pkg_version" ubuntu:24.04 bash -c "$ca_prelude"'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential meson ninja-build pkg-config python3 gettext \
  appstream desktop-file-utils libglib2.0-dev-bin libgtk-4-dev libadwaita-1-dev curl ca-certificates xz-utils file >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable >/dev/null
export PATH=$HOME/.cargo/bin:$PATH
mkdir -p /build && cd /build
tar xf /dist/$BIN-$PKGVER.tar.xz && tar xf /dist/$BIN-$PKGVER-vendor.tar.xz
cd $BIN-$PKGVER
meson setup _build --prefix=/usr --buildtype=release -Doffline=true -Dupdate_check=true >/dev/null
meson compile -C _build >/tmp/build.log 2>&1 || { tail -40 /tmp/build.log; exit 1; }
meson test -C _build --print-errorlogs | tail -3
DESTDIR=/stage meson install -C _build >/dev/null
arch=$(uname -m)
name=$BIN-$PKGVER-$arch-linux
dir=/out/$name
mkdir -p $dir/bin
cp /stage/usr/bin/* $dir/bin/
cp -R /stage/usr/share $dir/share
glib-compile-schemas $dir/share/glib-2.0/schemas
cp LICENSE branding.env $dir/
cp /tarball/install.sh /tarball/uninstall.sh /tarball/README.txt $dir/
strip $dir/bin/*
file $dir/bin/$BIN | cut -c1-120
tar -C /out -cJf /dist/$name.tar.xz $name
(cd /dist && sha256sum $name.tar.xz > $name.tar.xz.sha256)
ls -la /dist/$name.tar.xz
'
