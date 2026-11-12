#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a native package from this checkout and install it:
#   Debian / Ubuntu          -> .deb  (dpkg-buildpackage), installed with apt
#   Fedora / EL / openSUSE   -> .rpm  (rpmbuild), installed with dnf or zypper
#   Arch Linux               -> use `makepkg -si` (PKGBUILD in this folder)
#
# Run it as your normal user from the FreeMediaConverter folder; it asks for
# your password when it installs. Updating = `git pull`, then run it again.
#
# Usage: ./build-package.sh [--no-install] [--yes]
#   --no-install  only build; the packages are left in _package/
#   --yes         don't ask before installing (also: RETORT_BUILD_ASSUME_YES=1)
set -euo pipefail

root=$(cd "$(dirname "$0")" && pwd)
install=1
yes=${RETORT_BUILD_ASSUME_YES:-0}
for a in "$@"; do
  case "$a" in
    --no-install) install=0 ;;
    --yes|-y) yes=1 ;;
    -h|--help) sed -n '2,15p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
die() { printf 'error: %s\n' "$*" >&2; exit 1; }

[ -f "$root/branding.env" ] || die "run this from the FreeMediaConverter folder"
[ "$(id -u)" -ne 0 ] || die "run this as your normal user, not root (it uses sudo to install)"
bin=$(sed -n 's/^APP_BIN=//p' "$root/branding.env")
version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$root/Cargo.toml" | head -1)
# shellcheck disable=SC1091
. /etc/os-release
like=" ${ID:-} ${ID_LIKE:-} "
case "$like" in
  *" arch "*) die "on Arch Linux run: makepkg -si" ;;
  *" debian "*|*" ubuntu "*) family=deb ;;
  *" fedora "*|*" rhel "*|*" centos "*|*" suse "*|*" opensuse "*) family=rpm ;;
  *) die "no native package for ${PRETTY_NAME:-this system}; see docs/INSTALL.md (Flatpak, AppImage or tarball)" ;;
esac

# Rust >= 1.92. Ubuntu's versioned toolchains (cargo-1.NN) live outside PATH.
newest=$(printf '%s\n' /usr/lib/rust-1.*/bin | sort -V -r | head -1)
if [ -d "$newest" ]; then PATH="$newest:$PATH"; fi
export PATH
command -v cargo >/dev/null || die "cargo not found: install Rust as shown in docs/INSTALL.md for your distribution"
rust=$(cargo --version | sed -n 's/^cargo \([0-9]*\.[0-9]*\).*/\1/p')
if [ "$(printf '%s\n1.92\n' "$rust" | sort -V | head -1)" != "1.92" ]; then
  die "Rust $rust is too old (1.92 or newer needed); see docs/INSTALL.md"
fi

# Every build gets a newer package version than the last one, so installing it
# replaces what is there even when Cargo.toml's version has not changed.
stamp=$(date -u +%Y%m%d%H%M%S)

out="$root/_package"
work=$(mktemp -d)
trap 'rm -rf "$work"' EXIT
src="$work/$bin-$version"
mkdir -p "$src" "$out"

say "Copying the source"
tar -C "$root" \
    --exclude=./.git --exclude=./target --exclude=./_build --exclude=./_install \
    --exclude=./_package --exclude=./dist --exclude=./src --exclude=./pkg --exclude='*.pkg.tar*' \
    -cf - . | tar -C "$src" -xf -

say "Fetching Rust crates (cargo vendor)"
mkdir -p "$src/.cargo"
(cd "$src" && cargo vendor --locked --versioned-dirs vendor >/dev/null)
cat > "$src/.cargo/config.toml" <<'TOML'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
TOML

case "$family" in
  deb)
    say "Building the .deb packages"
    cp -r "$root/packaging/debian/debian" "$src/debian"
    cat > "$src/debian/changelog" <<CHANGELOG
$bin ($version-0local$stamp) ${VERSION_CODENAME:-unstable}; urgency=medium

  * Built from a local checkout with build-package.sh.

 -- Retort contributors <noreply@users.noreply.github.com>  $(date -R)
CHANGELOG
    # debhelper runs with its own HOME: keep rustup's toolchains reachable.
    export RUSTUP_HOME="${RUSTUP_HOME:-$HOME/.rustup}"
    (cd "$src" && dpkg-buildpackage -us -uc -b) > "$out/build.log" 2>&1 \
      || { tail -40 "$out/build.log"; die "the build failed (full log: _package/build.log)"; }
    rm -f "$out"/*.deb
    mv "$work"/*.deb "$out/"
    pkgs=("$out/${bin}_"*.deb "$out/${bin}-cli_"*.deb)
    ;;
  rpm)
    say "Building the .rpm packages"
    top="$work/rpmbuild"
    mkdir -p "$top/SOURCES" "$top/SPECS"
    tar -C "$work" --exclude="$bin-$version/vendor" --exclude="$bin-$version/.cargo" \
        -cJf "$top/SOURCES/$bin-$version.tar.xz" "$bin-$version"
    tar -C "$work" -cJf "$top/SOURCES/$bin-$version-vendor.tar.xz" "$bin-$version/vendor" "$bin-$version/.cargo"
    sed -e "s/^Version: .*/Version:        $version/" -e "s/^Release: .*/Release:        0.local$stamp%{?dist}/" \
        "$root/packaging/rpm/$bin.spec" > "$top/SPECS/$bin.spec"
    rpmbuild --define "_topdir $top" -bb "$top/SPECS/$bin.spec" > "$out/build.log" 2>&1 \
      || { tail -40 "$out/build.log"; die "the build failed (full log: _package/build.log)"; }
    rm -f "$out"/*.rpm
    find "$top/RPMS" -name '*.rpm' -exec mv {} "$out/" \;
    pkgs=("$out/$bin-$version"-*.rpm "$out/$bin-cli-$version"-*.rpm)
    ;;
esac

say "Built: ${pkgs[*]##*/}"
if [ "$install" = 0 ]; then
  exit 0
fi

say "Installing (sudo)"
case "$family" in
  deb)
    if [ "$yes" = 1 ]; then sudo apt-get install -y "${pkgs[@]}"; else sudo apt install "${pkgs[@]}"; fi ;;
  rpm)
    if command -v zypper >/dev/null; then
      if [ "$yes" = 1 ]; then sudo zypper -n install --allow-unsigned-rpm "${pkgs[@]}"
      else sudo zypper install --allow-unsigned-rpm "${pkgs[@]}"; fi
    else
      if [ "$yes" = 1 ]; then sudo dnf install -y "${pkgs[@]}"; else sudo dnf install "${pkgs[@]}"; fi
    fi ;;
esac
say "Done. Start Retort from your applications menu, or run '$bin-cli diagnose' to check hardware support."
