# Maintainer: Retort contributors <https://github.com/oreo1298/FreeMediaConverter>
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Build and install from a clone of this repository:
#   git clone https://github.com/oreo1298/FreeMediaConverter.git
#   cd FreeMediaConverter
#   makepkg -si
# Update: `git pull`, then `makepkg -sif` (-f rebuilds even when pkgver has not
# changed; without it makepkg reinstalls the package it built last time).

pkgbase=retort
pkgname=('retort' 'retort-cli')
pkgver=0.1.0
pkgrel=1
pkgdesc="Convert video and audio files with FFmpeg, using verified hardware acceleration"
arch=('x86_64' 'aarch64')
url="https://github.com/oreo1298/FreeMediaConverter"
license=('GPL-3.0-or-later')
# Release binaries are stripped by Cargo (profile.release strip).
options=('!debug')
makedepends=('cargo' 'meson' 'ffmpeg' 'appstream' 'desktop-file-utils' 'gtk4' 'libadwaita')
source=()
sha256sums=()

# The PKGBUILD lives in the repository root; find the checkout from wherever
# makepkg was started.
_srcroot() {
  local dir="$startdir"
  while [[ ! -f "$dir/branding.env" && "$dir" != / ]]; do
    dir="$(dirname "$dir")"
  done
  if [[ ! -f "$dir/branding.env" ]]; then
    echo "Run makepkg inside the FreeMediaConverter source folder." >&2
    return 1
  fi
  printf '%s\n' "$dir"
}

prepare() {
  local root
  root="$(_srcroot)" || return 1
  # Start clean: makepkg keeps src/ between runs.
  rm -rf "${srcdir:?}/$pkgbase" "$srcdir/build" "$srcdir/install"
  mkdir -p "$srcdir/$pkgbase"
  # Copy the checkout, leaving out git data, build trees and makepkg's own folders.
  tar -C "$root" \
      --exclude=./.git --exclude=./src --exclude=./pkg --exclude='*.pkg.tar*' \
      --exclude=./target --exclude=./_build --exclude=./_install --exclude=./_package \
      --exclude=./dist \
      -cf - . | tar -C "$srcdir/$pkgbase" -xf -
  cd "$srcdir/$pkgbase" || return 1
  export RUSTUP_TOOLCHAIN=stable
  cargo fetch --locked --target "$(rustc -vV | sed -n 's/host: //p')"
}

build() {
  export RUSTUP_TOOLCHAIN=stable
  arch-meson "$pkgbase" build -Doffline=true
  meson compile -C build
}

check() {
  meson test -C build --print-errorlogs
}

_install() {
  [ -d "$srcdir/install" ] || meson install -C build --no-rebuild --destdir "$srcdir/install"
}

package_retort() {
  depends=('gtk4' 'libadwaita' 'glib2' 'glibc' 'libgcc' 'cairo' 'graphene' 'hicolor-icon-theme' 'ffmpeg')
  optdepends=('retort-cli: command-line companion'
              'intel-media-driver: Intel VA-API / Quick Sync encode'
              'vpl-gpu-rt: Intel Quick Sync runtime'
              'nvidia-utils: NVENC / NVDEC'
              'libva-utils: vainfo for System Check'
              'vulkan-tools: vulkaninfo for System Check')
  _install
  cp -a "$srcdir/install/." "$pkgdir/"
  rm "$pkgdir/usr/bin/retort-cli"
}

package_retort-cli() {
  pkgdesc="Convert video and audio files with FFmpeg (command line, no GTK)"
  depends=('glibc' 'libgcc' 'ffmpeg')
  _install
  install -Dm755 "$srcdir/install/usr/bin/retort-cli" "$pkgdir/usr/bin/retort-cli"
  install -Dm644 "$srcdir/$pkgbase/LICENSE" "$pkgdir/usr/share/licenses/retort-cli/LICENSE"
}
