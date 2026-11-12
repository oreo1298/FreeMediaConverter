#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Verify that the package names Retort's System Check and INSTALL.md
# recommend exist in each distro's real repositories (incl. RPM Fusion and
# Packman), and record GTK / libadwaita / FFmpeg versions per release.
#
# Runs one container per distro. Output: one "distro<TAB>package<TAB>version|MISSING"
# line per query on stdout. Usage: scripts/research/verify-packages.sh [distro...]
# Extra docker args (e.g. a CA bundle mount) can be passed via $DOCKER_ARGS.
set -uo pipefail

# shellcheck disable=SC2206
docker_args=(${DOCKER_ARGS:-})

# If a CA bundle is mounted at /ca.crt (TLS-intercepting proxy), trust it.
ca_prelude='
if [ -f /ca.crt ]; then
  { cp /ca.crt /etc/pki/ca-trust/source/anchors/proxy.crt && update-ca-trust; } >/dev/null 2>&1 ||
  { mkdir -p /etc/pki/trust/anchors && cp /ca.crt /etc/pki/trust/anchors/proxy.crt && update-ca-certificates; } >/dev/null 2>&1 ||
  { cp /ca.crt /etc/ca-certificates/trust-source/anchors/proxy.crt && trust extract-compat; } >/dev/null 2>&1 ||
  { cp /ca.crt /usr/local/share/ca-certificates/proxy.crt && update-ca-certificates; } >/dev/null 2>&1 || true
fi
'

run() { # image script
  docker run --rm "${docker_args[@]}" "$1" bash -c "$ca_prelude
$2" 2>/dev/null
}

DEB_PKGS="ffmpeg libgtk-4-1 libadwaita-1-0 intel-media-va-driver intel-media-va-driver-non-free i965-va-driver mesa-va-drivers vainfo vulkan-tools mesa-vulkan-drivers libvpl2 libmfx-gen1.2 libmfx-gen1 nvidia-driver nvidia-vaapi-driver libnvidia-encode1 libcuda1 nvidia-cuda-dev"
UBU_EXTRA="libnvidia-encode-580 libnvidia-compute-580 libnvidia-decode-580 nvidia-driver-580"

apt_query='
export DEBIAN_FRONTEND=noninteractive
CODENAME=$(. /etc/os-release; echo ${VERSION_CODENAME})
if [ -f /etc/apt/sources.list.d/debian.sources ]; then
  sed -i "s/^Components: main$/Components: main contrib non-free non-free-firmware/" /etc/apt/sources.list.d/debian.sources
fi
if [ -f /etc/apt/sources.list.d/ubuntu.sources ]; then
  sed -i "s/^Components: main restricted$/Components: main restricted universe multiverse/; s/^Components: main$/Components: main restricted universe multiverse/" /etc/apt/sources.list.d/ubuntu.sources
fi
apt-get update -qq >/dev/null 2>&1
for p in $PKGS; do
  v=$(apt-cache policy "$p" 2>/dev/null | sed -n "s/^ *Candidate: *//p")
  if [ -n "$v" ] && [ "$v" != "(none)" ]; then echo -e "$NAME\t$p\t$v"; else echo -e "$NAME\t$p\tMISSING"; fi
done'

dnf_query='
. /etc/os-release
dnf -y -q install "https://mirrors.rpmfusion.org/free/fedora/rpmfusion-free-release-${VERSION_ID}.noarch.rpm" "https://mirrors.rpmfusion.org/nonfree/fedora/rpmfusion-nonfree-release-${VERSION_ID}.noarch.rpm" >/dev/null 2>&1
dnf -q makecache >/dev/null 2>&1
for p in $PKGS; do
  v=$(dnf -q repoquery --latest-limit 1 --qf "%{version}-%{release} [%{repoid}]\n" "$p" 2>/dev/null | head -1)
  if [ -n "$v" ]; then echo -e "$NAME\t$p\t$v"; else echo -e "$NAME\t$p\tMISSING"; fi
done'

el_query='
. /etc/os-release
MAJ=${VERSION_ID%%.*}
dnf -y -q install epel-release >/dev/null 2>&1 || dnf -y -q install "https://dl.fedoraproject.org/pub/epel/epel-release-latest-${MAJ}.noarch.rpm" >/dev/null 2>&1
dnf config-manager --set-enabled crb >/dev/null 2>&1 || true
dnf -y -q install "https://mirrors.rpmfusion.org/free/el/rpmfusion-free-release-${MAJ}.noarch.rpm" "https://mirrors.rpmfusion.org/nonfree/el/rpmfusion-nonfree-release-${MAJ}.noarch.rpm" >/dev/null 2>&1
dnf -q makecache >/dev/null 2>&1
for p in $PKGS; do
  v=$(dnf -q repoquery --latest-limit 1 --qf "%{version}-%{release} [%{repoid}]\n" "$p" 2>/dev/null | head -1)
  if [ -n "$v" ]; then echo -e "$NAME\t$p\t$v"; else echo -e "$NAME\t$p\tMISSING"; fi
done'

FED_PKGS="ffmpeg-free ffmpeg gtk4 libadwaita mesa-va-drivers mesa-va-drivers-freeworld intel-media-driver libva-intel-driver libva-utils vulkan-tools mesa-vulkan-drivers libvpl intel-vpl-gpu-rt xorg-x11-drv-nvidia-cuda-libs xorg-x11-drv-nvidia-cuda libva-nvidia-driver nvidia-vaapi-driver"

zypper_query='
. /etc/os-release
case "$ID" in
  opensuse-tumbleweed|opensuse-slowroll) PACK=https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Tumbleweed/ ;;
  *) PACK="https://ftp.gwdg.de/pub/linux/misc/packman/suse/openSUSE_Leap_${VERSION_ID}/" ;;
esac
zypper -n -q ar -cfp 90 "$PACK" packman >/dev/null 2>&1
zypper -n -q --gpg-auto-import-keys ref >/dev/null 2>&1
for p in $PKGS; do
  info=$(zypper -n -q info "$p" 2>/dev/null)
  v=$(printf "%s\n" "$info" | sed -n "s/^Version *: *//p" | head -1)
  r=$(printf "%s\n" "$info" | sed -n "s/^Repository *: *//p" | head -1)
  if [ -n "$v" ]; then echo -e "$NAME\t$p\t$v [$r]"; else echo -e "$NAME\t$p\tMISSING"; fi
done'
SUSE_PKGS="ffmpeg-7 ffmpeg-8 ffmpeg-9 ffmpeg libgtk-4-1 libadwaita-1-0 intel-media-driver libva-intel-driver Mesa-dri Mesa-libva libva-utils vulkan-tools libvpl2 libmfx-gen1_2 nvidia-compute-G06 nvidia-video-G06 nvidia-vaapi-driver"

pacman_query='
pacman -Sy --noconfirm >/dev/null 2>&1
for p in $PKGS; do
  info=$(pacman -Si "$p" 2>/dev/null)
  v=$(printf "%s\n" "$info" | sed -n "s/^Version *: *//p" | head -1)
  r=$(printf "%s\n" "$info" | sed -n "s/^Repository *: *//p" | head -1)
  if [ -n "$v" ]; then echo -e "$NAME\t$p\t$v [$r]"; else echo -e "$NAME\t$p\tMISSING"; fi
done'
ARCH_PKGS="ffmpeg gtk4 libadwaita intel-media-driver libva-intel-driver libva-mesa-driver mesa libva-utils vulkan-tools vulkan-intel vulkan-radeon nvidia-utils libva-nvidia-driver vpl-gpu-rt libvpl intel-media-sdk"

targets=("$@")
[ ${#targets[@]} -eq 0 ] && targets=(debian13 ubuntu2404 ubuntu2604 fedora rhel10 tumbleweed leap16 arch)

for t in "${targets[@]}"; do
  case "$t" in
    debian13)   run debian:trixie "NAME=debian13 PKGS='$DEB_PKGS'; $apt_query" ;;
    ubuntu2404) run ubuntu:24.04 "NAME=ubuntu2404 PKGS='$DEB_PKGS $UBU_EXTRA'; $apt_query" ;;
    ubuntu2604) run ubuntu:26.04 "NAME=ubuntu2604 PKGS='$DEB_PKGS $UBU_EXTRA'; $apt_query" ;;
    fedora)     run fedora:latest "NAME=fedora\$(. /etc/os-release; echo \$VERSION_ID) PKGS='$FED_PKGS'; $dnf_query" ;;
    rhel10)     run almalinux:10 "NAME=alma10 PKGS='$FED_PKGS'; $el_query" ;;
    tumbleweed) run opensuse/tumbleweed "NAME=tumbleweed PKGS='$SUSE_PKGS'; $zypper_query" ;;
    leap16)     run opensuse/leap:16.0 "NAME=leap16 PKGS='$SUSE_PKGS'; $zypper_query" ;;
    arch)       run archlinux:latest "NAME=arch PKGS='$ARCH_PKGS'; $pacman_query" ;;
  esac
done
