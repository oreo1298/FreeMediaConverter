#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the source PKGBUILD (packaging/arch/retort) against the local release
# tarball in an Arch Linux container, run namcap, install and smoke-test.
# Usage: packaging/arch/build.sh      Output: dist/arch/*.pkg.tar.zst
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "$0")/../common.sh"
ensure_dist
out="$pkg_root/dist/arch"
mkdir -p "$out"
docker run --rm "${docker_args[@]}" \
  -v "$pkg_root/dist:/dist:ro" -v "$pkg_root/packaging/arch:/pkg:ro" -v "$out:/out" \
  -e BIN="$pkg_bin" -e PKGVER="$pkg_version" archlinux:latest bash -c "$ca_prelude"'
set -euo pipefail
pacman -Syu --noconfirm --needed base-devel namcap sudo git >/dev/null
useradd -m builder
echo "builder ALL=(ALL) NOPASSWD: ALL" > /etc/sudoers.d/builder
mkdir /build && cp /pkg/retort/PKGBUILD /build/ && cp /dist/$BIN-$PKGVER.tar.xz /build/
cd /build
sum=$(sha256sum $BIN-$PKGVER.tar.xz | cut -d" " -f1)
sed -i "s|^source=.*|source=(\"$BIN-$PKGVER.tar.xz\")|; s|^sha256sums=.*|sha256sums=(\"$sum\")|" PKGBUILD
chown -R builder /build
sudo -u builder makepkg -s --noconfirm >/tmp/build.log 2>&1 || { tail -60 /tmp/build.log; exit 1; }
grep -E "^(Ok|Fail):" /tmp/build.log || true
echo "== namcap PKGBUILD"; namcap PKGBUILD || true
for p in *.pkg.tar.zst; do echo "== namcap $p"; namcap "$p" || true; done
pacman -U --noconfirm ./*.pkg.tar.zst >/dev/null
retort-cli presets | head -3
cp ./*.pkg.tar.zst /out/
ls -la /out
'
