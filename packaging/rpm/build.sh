#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build the RPMs inside a container of the target distro, then install them
# there (with the distro's own FFmpeg) and smoke-test.
# Usage: packaging/rpm/build.sh <image> [platform]
#   e.g. fedora:45 | almalinux:10 | opensuse/tumbleweed | opensuse/leap:16.0
# Output: dist/rpm/<distro>/*.rpm
set -euo pipefail
# shellcheck source=../common.sh
. "$(dirname "$0")/../common.sh"
image=${1:?usage: build.sh <image> [platform]}
platform=${2:-}
ensure_dist
out="$pkg_root/dist/rpm"
mkdir -p "$out"
plat=()
[ -n "$platform" ] && plat=(--platform "$platform")

docker run --rm "${plat[@]}" "${docker_args[@]}" \
  -v "$pkg_root/dist:/dist:ro" -v "$pkg_root/packaging/rpm:/spec:ro" -v "$out:/out" \
  -e BIN="$pkg_bin" -e PKGVER="$pkg_version" "$image" bash -c "$ca_prelude"'
set -euo pipefail
. /etc/os-release
deps="rpm-build meson gcc python3 gettext desktop-file-utils tar xz curl gtk4-devel libadwaita-devel glib2-devel"
case "$ID" in
  fedora)
    dnf -y -q install $deps ninja-build appstream >/dev/null ;;
  almalinux|rocky|rhel|centos)
    dnf -y -q install dnf-plugins-core epel-release >/dev/null
    dnf config-manager --set-enabled crb
    dnf -y -q install $deps ninja-build appstream >/dev/null ;;
  opensuse*)
    zypper -n -q install rpm-build meson gcc python3 gettext-tools desktop-file-utils tar xz curl \
      gtk4-devel libadwaita-devel glib2-devel ninja AppStream >/dev/null ;;
esac
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --default-toolchain stable >/dev/null
export PATH=$HOME/.cargo/bin:$PATH
# debhelper (compat 13) runs with its own HOME: point rustup at its install.
export RUSTUP_HOME=$HOME/.rustup
top=$HOME/rpmbuild
mkdir -p $top/SOURCES $top/SPECS
cp /dist/$BIN-$PKGVER.tar.xz /dist/$BIN-$PKGVER-vendor.tar.xz $top/SOURCES/
cp /spec/retort.spec $top/SPECS/
rpmbuild -ba $top/SPECS/retort.spec >/tmp/build.log 2>&1 || { tail -80 /tmp/build.log; exit 1; }
grep -E "^(Ok|Fail):" /tmp/build.log || true
# Stable repository names: fedora-45, el-10 (AlmaLinux/RHEL/Rocky), opensuse-tumbleweed, opensuse-leap-16.0
case "$ID" in
  almalinux|rocky|rhel|centos) repo=el-${VERSION_ID%%.*} ;;
  opensuse-tumbleweed) repo=opensuse-tumbleweed ;;
  *) repo=$ID-$VERSION_ID ;;
esac
dest=/out/$repo
mkdir -p $dest
cp $top/RPMS/*/*.rpm $top/SRPMS/*.rpm $dest/
# Smoke test with the distro FFmpeg.
case "$ID" in
  fedora|almalinux|rocky|rhel|centos) dnf -y -q install $top/RPMS/*/*.rpm >/dev/null ;;
  opensuse*) zypper -n -q --no-gpg-checks install $top/RPMS/*/*.rpm >/dev/null ;;
esac
rpm -q --whatprovides /usr/bin/ffmpeg
retort-cli presets | head -3
rpm -ql '"$pkg_bin"' | grep -E "\.desktop$|metainfo"
ls -la $dest
'
