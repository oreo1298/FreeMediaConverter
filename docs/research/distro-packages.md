# Distro packages (verified 2026-11-12)

Produced by `scripts/research/verify-packages.sh` (one container per distro,
querying the real repositories with RPM Fusion / Packman enabled) plus
follow-up simulated installs (`dnf -y install …`, `apt-get install -s …`).
Re-run before each release: `DOCKER_ARGS=… scripts/research/verify-packages.sh`.

## GTK / libadwaita / FFmpeg per release

| Release | GTK 4 | libadwaita | FFmpeg (repo) | Native package? |
|---|---|---|---|---|
| Debian 13 "trixie" | 4.18.6 | 1.7.6 | 7.1.5 (main) | ✅ .deb |
| Debian 12 "bookworm" | 4.8.3 | 1.2.2 | 5.1.9 | ❌ Flatpak/AppImage only |
| Ubuntu 24.04 LTS | 4.14.5 | 1.5.0 | 6.1.1 (universe) | ✅ .deb (baseline) |
| Ubuntu 26.04 LTS | 4.22.4 | 1.9.1 | 8.0.1 (universe) | ✅ .deb |
| Ubuntu 22.04 LTS | 4.6.9 | 1.1.7 | 4.4.2 | ❌ Flatpak/AppImage only |
| Fedora 45 | 4.24.1 | 1.10.0 | ffmpeg-free 9.0.2 (fedora) / ffmpeg 9.0.2 (RPM Fusion) | ✅ .rpm |
| AlmaLinux / RHEL 10 | 4.16.7 | 1.6.10 | ffmpeg-free 7.1.5 (EPEL) / ffmpeg 7.1.5 (RPM Fusion) | ✅ .rpm |
| openSUSE Tumbleweed | 4.24.1 | 1.10.0 | ffmpeg 9.0.2 (Packman only) | ✅ .rpm |
| openSUSE Leap 16.0 | 4.18.6 | 1.7.5 | ffmpeg-8 8.1.3 (Packman only) | ✅ .rpm |
| Arch Linux | 4.24.1 | 1.10.0 | 9.0.2 (extra) | ✅ PKGBUILD |

## Hardware-acceleration packages

| Purpose | Debian 13 | Ubuntu 24.04 / 26.04 | Fedora 45 | EL10 | openSUSE TW / Leap 16 | Arch |
|---|---|---|---|---|---|---|
| Intel VA-API (iHD) with encode | `intel-media-va-driver-non-free` (non-free) | `intel-media-va-driver-non-free` (multiverse) | `intel-media-driver` (RPM Fusion nonfree) | `intel-media-driver` (RPM Fusion nonfree) | `intel-media-driver` (oss) | `intel-media-driver` |
| Intel legacy i965 | `i965-va-driver` | `i965-va-driver` | `libva-intel-driver` (RPM Fusion free) | same | — (missing) | `libva-intel-driver` |
| Mesa VA-API (AMD) | `mesa-va-drivers` | `mesa-va-drivers` (26.04: virtual, provided by `mesa-libgallium`) | built into `mesa-dri-drivers` **without H.264/HEVC encode**; `mesa-va-drivers-freeworld` (RPM Fusion free) adds encode (coexists, preferred via ld.so.conf.d) | none in base repos | Packman `Mesa-libva` / `Mesa-dri` (oss Mesa has encode disabled) | inside `mesa` (no separate libva-mesa-driver) |
| QSV runtime (oneVPL GPU) | `libmfx-gen1.2` + `libvpl2` | `libmfx-gen1.2` + `libvpl2` | `intel-vpl-gpu-rt` + `libvpl` | `intel-vpl-gpu-rt` (EPEL) | `libmfx-gen1_2` + `libvpl2` | `vpl-gpu-rt` + `libvpl` |
| NVENC/CUDA libs | `libnvidia-encode1` + `libcuda1` (non-free, 550) | `libnvidia-encode-<N>`, `libnvidia-compute-<N>`, `libnvidia-decode-<N>` (N=580 on both) | `xorg-x11-drv-nvidia-cuda-libs` (RPM Fusion nonfree, 620) | NVIDIA repo `cuda-rhel10.repo`: `nvidia-driver-cuda-libs`, `nvidia-driver-libs` (620) | NVIDIA repo: `nvidia-compute-G06/G07`, `nvidia-video-G06/G07` (G06 = 580, G07 = 595+) | `nvidia-utils` (615) |
| NVIDIA VA-API (decode only) | `nvidia-vaapi-driver` | `nvidia-vaapi-driver` | `libva-nvidia-driver` | `libva-nvidia-driver` (EPEL) | — | `libva-nvidia-driver` |
| vainfo | `vainfo` | `vainfo` | `libva-utils` | — (missing) | `libva-utils` | `libva-utils` |
| vulkaninfo | `vulkan-tools` | `vulkan-tools` | `vulkan-tools` | `vulkan-tools` | `vulkan-tools` | `vulkan-tools` |

## Verified fix commands

- Fedora 45: `sudo dnf swap ffmpeg-free ffmpeg --allowerasing` (after enabling
  RPM Fusion free+nonfree) — completed in a container.
- Fedora 45: `sudo dnf install mesa-va-drivers-freeworld` — completes; the old
  `dnf swap mesa-va-drivers …` no longer works because `mesa-va-drivers` was
  folded into `mesa-dri-drivers`.
- Ubuntu 26.04: `apt-get install -s mesa-va-drivers` resolves to `mesa-libgallium`.
- openSUSE: Packman URLs `…/openSUSE_Tumbleweed/` and `…/openSUSE_Leap_16.0/`
  both refresh; Tumbleweed's Packman package is `ffmpeg` (9.0.2), Leap 16's is `ffmpeg-8`.
- NVIDIA repos: `https://download.nvidia.com/opensuse/{tumbleweed,leap/16.0}`
  and `https://developer.download.nvidia.com/compute/cuda/repos/rhel{9,10}/x86_64/cuda-rhel{9,10}.repo` return 200.
