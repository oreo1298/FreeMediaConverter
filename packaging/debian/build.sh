#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the .deb packages inside a container of the target release, then
# install them there and smoke-test.
# Usage: packaging/debian/build.sh <image> [platform]
#   e.g. packaging/debian/build.sh ubuntu:24.04
#        packaging/debian/build.sh debian:trixie linux/arm64
# Output: dist/deb/<codename>/*.deb
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "$0")/../common.sh"
image=${1:?usage: build.sh <image> [platform]}
platform=${2:-}
ensure_dist
out="$pkg_root/dist/deb"
mkdir -p "$out"
plat=()
[ -n "$platform" ] && plat=(--platform "$platform")

docker run --rm "${plat[@]}" "${docker_args[@]}" \
  -v "$pkg_root/dist:/dist:ro" -v "$pkg_root/packaging/debian/debian:/debian:ro" -v "$out:/out" \
  -e BIN="$pkg_bin" -e PKGVER="$pkg_version" "$image" bash -c "$ca_prelude"'
set -euo pipefail
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq
apt-get install -y -qq --no-install-recommends build-essential debhelper fakeroot meson ninja-build pkg-config \
  python3 gettext appstream desktop-file-utils libglib2.0-dev-bin libgtk-4-dev libadwaita-1-dev \
  curl ca-certificates xz-utils >/dev/null
# Distro rustc is older than the MSRV (1.92): use rustup.
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable >/dev/null
export PATH=$HOME/.cargo/bin:$PATH
# debhelper (compat 13) runs with its own HOME: point rustup at its install.
export RUSTUP_HOME=$HOME/.rustup
. /etc/os-release
codename=${VERSION_CODENAME:-unstable}
mkdir -p /build && cd /build
tar xf /dist/$BIN-$PKGVER.tar.xz
tar xf /dist/$BIN-$PKGVER-vendor.tar.xz
cd $BIN-$PKGVER
cp -r /debian debian
cat > debian/changelog <<CHANGELOG
$BIN ($PKGVER-1~${ID}${VERSION_ID}) $codename; urgency=medium

  * Release $PKGVER (see CHANGELOG.md).

 -- Retort contributors <noreply@users.noreply.github.com>  $(date -R)
CHANGELOG
dpkg-buildpackage -us -uc -b >/tmp/build.log 2>&1 || { tail -60 /tmp/build.log; exit 1; }
grep -E "^(OK|Ok:|Fail:)|validate" /tmp/build.log | tail -5 || true
mkdir -p /out/$codename
cp ../*.deb /out/$codename/
# Smoke test: install with dependencies and run.
apt-get install -y -qq ../*.deb >/dev/null
retort-cli presets > /tmp/presets.txt && head -3 /tmp/presets.txt
retort-cli list-formats > /tmp/formats.txt && head -3 /tmp/formats.txt
test -x /usr/bin/'"$pkg_bin"'
dpkg -L '"$pkg_bin"' | grep -E "\.desktop$|metainfo|gschema"
ls -la /out/$codename
'
