#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Install Retort the best way for this system:
#   Debian 13 / Ubuntu 24.04+      -> signed APT repository (updates via apt)
#   Fedora / EL 10 / openSUSE      -> signed RPM repository (updates via dnf/zypper)
#   Arch Linux                     -> AUR (retort-bin) with your AUR helper
#   anything else / older          -> Flatpak (if installed), else the portable tarball
# Usage: curl -fsSL https://oreo1298.github.io/FreeMediaConverter/install.sh | sh
#        sh install.sh [--flatpak | --tarball] [--cli-only] [--dry-run]
set -eu
APP_NAME=Retort
BIN=retort
OWNER=oreo1298
REPO=FreeMediaConverter
# Overridable for testing against a local mirror (scripts/test-docs.sh).
PAGES=${RETORT_INSTALL_BASE:-"https://$OWNER.github.io/$REPO"}
RELEASES=${RETORT_INSTALL_RELEASES:-"https://github.com/$OWNER/$REPO/releases/latest/download"}

mode="" cli_only=0 dry=0
for a in "$@"; do
  case "$a" in
    --flatpak) mode=flatpak ;;
    --tarball) mode=tarball ;;
    --cli-only) cli_only=1 ;;
    --dry-run) dry=1 ;;
    -h|--help) sed -n '2,9p' "$0"; exit 0 ;;
    *) echo "unknown option: $a" >&2; exit 2 ;;
  esac
done

say() { printf '%s\n' "$*"; }
run() {
  say "+ $*"
  [ "$dry" = 1 ] || "$@"
}
need_root() {
  if [ "$(id -u)" = 0 ]; then SUDO=""; elif command -v sudo >/dev/null; then SUDO=sudo; else
    say "This needs root: run it as root or install sudo." >&2; exit 1
  fi
}
pkgs() { if [ "$cli_only" = 1 ]; then echo "$BIN-cli"; else echo "$BIN $BIN-cli"; fi; }

. /etc/os-release 2>/dev/null || true
id_like=" ${ID:-} ${ID_LIKE:-} "
ver=${VERSION_ID:-0}
major=${ver%%.*}

apt_repo() {
  need_root
  keyring=/etc/apt/keyrings/$BIN-archive-keyring.gpg
  run $SUDO install -d -m 0755 /etc/apt/keyrings
  if [ "$dry" = 1 ]; then say "+ curl -fsSL $PAGES/$BIN-archive-keyring.gpg > $keyring"; else
    curl -fsSL "$PAGES/$BIN-archive-keyring.gpg" | $SUDO tee "$keyring" >/dev/null; fi
  line="deb [signed-by=$keyring] $PAGES/apt $VERSION_CODENAME main"
  if [ "$dry" = 1 ]; then say "+ echo '$line' > /etc/apt/sources.list.d/$BIN.list"; else
    echo "$line" | $SUDO tee "/etc/apt/sources.list.d/$BIN.list" >/dev/null; fi
  run $SUDO apt-get update
  # shellcheck disable=SC2046
  run $SUDO apt-get install -y $(pkgs)
}

rpm_repo() {
  need_root
  repo="$PAGES/rpm/$BIN-$1.repo"
  case "$ID" in
    opensuse*)
      run $SUDO zypper --non-interactive addrepo --refresh "$repo"
      run $SUDO zypper --gpg-auto-import-keys refresh "$BIN"
      # shellcheck disable=SC2046
      run $SUDO zypper --non-interactive install $(pkgs) ;;
    *)
      if [ "$dry" = 1 ]; then say "+ curl -fsSL $repo > /etc/yum.repos.d/$BIN.repo"; else
        curl -fsSL "$repo" | $SUDO tee "/etc/yum.repos.d/$BIN.repo" >/dev/null; fi
      if [ "${1#el-}" != "$1" ]; then run $SUDO dnf install -y epel-release; fi
      # shellcheck disable=SC2046
      run $SUDO dnf install -y $(pkgs) ;;
  esac
  if ! command -v ffmpeg >/dev/null; then
    say "Note: install FFmpeg with full codecs from RPM Fusion (Fedora/EL) or Packman (openSUSE)"
    say "      for H.264/HEVC; see $PAGES (INSTALL)."
  fi
}

flatpak_install() {
  run flatpak remote-add --user --if-not-exists flathub https://dl.flathub.org/repo/flathub.flatpakrepo
  tmp=$(mktemp -d)
  run curl -fsSL -o "$tmp/$BIN.flatpak" "$RELEASES/$APP_NAME-$(uname -m).flatpak"
  run flatpak install --user -y "$tmp/$BIN.flatpak"
  rm -rf "$tmp"
}

tarball_install() {
  tmp=$(mktemp -d)
  arch=$(uname -m)
  run curl -fsSL -o "$tmp/t.tar.xz" "$RELEASES/$BIN-linux-$arch.tar.xz"
  run tar -C "$tmp" -xJf "$tmp/t.tar.xz"
  run sh "$tmp"/"$BIN"-*-linux/install.sh
  rm -rf "$tmp"
}

if [ -z "$mode" ]; then
  case "$id_like" in
    *" debian "*|*" ubuntu "*)
      if { [ "${ID:-}" = debian ] && [ "$major" -ge 13 ]; } || { [ "${ID:-}" = ubuntu ] && [ "$major" -ge 24 ]; }; then
        mode=apt; fi ;;
    *" fedora "*) [ "${ID:-}" = fedora ] && mode=fedora ;;
    *" rhel "*|*" centos "*) [ "$major" -ge 10 ] && mode=el ;;
    *" suse "*|*" opensuse "*) case "${ID:-}" in opensuse-tumbleweed|opensuse-slowroll) mode=tw ;; opensuse-leap) [ "$major" -ge 16 ] && mode=leap ;; esac ;;
    *" arch "*) mode=arch ;;
  esac
fi
if [ -z "$mode" ]; then
  if command -v flatpak >/dev/null; then mode=flatpak; else mode=tarball; fi
fi

say "Installing $APP_NAME ($mode)…"
case "$mode" in
  apt) apt_repo ;;
  fedora) rpm_repo "fedora-$ver" ;;
  el) rpm_repo "el-$major" ;;
  tw) rpm_repo "opensuse-tumbleweed" ;;
  leap) rpm_repo "opensuse-leap-$ver" ;;
  arch)
    for h in paru yay; do
      if command -v $h >/dev/null; then run $h -S --needed "$BIN-bin"; exit 0; fi
    done
    say "Install from the AUR: retort-bin (prebuilt), retort (source) or retort-git."
    say "With an AUR helper: yay -S retort-bin" ;;
  flatpak) flatpak_install ;;
  tarball) tarball_install ;;
esac
say "Done. Start $APP_NAME from your applications menu, or run '$BIN-cli diagnose' to check hardware support."
