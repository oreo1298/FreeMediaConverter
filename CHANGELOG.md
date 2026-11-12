# Changelog

All notable changes are listed here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/) and versions follow
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-11-12

First release.

### Added
- Convert any audio or video file into any format FFmpeg can write, with
  Simple presets (MP4 that plays everywhere, Smaller MP4, MKV archive, AV1,
  WebM, Fit under a size limit, Apple-friendly, ProRes/DNxHR, GIF/WebP, audio
  formats, extract audio, remux) and an Advanced mode for almost every option.
- Hardware acceleration (NVENC/NVDEC, VA-API, Quick Sync, Vulkan Video, AMF,
  V4L2 M2M) used only where a real test encode/decode passed on this computer,
  with an automatic fallback to other GPUs and then the CPU.
- HDR10/HLG/Dolby Vision detection, HDR preservation where possible and tone
  mapping to SDR (libplacebo, tonemap_vaapi or zscale).
- Queue with thumbnails/waveforms, progress, pause/resume, cancel,
  notifications and suspend inhibition; the queue survives restarts.
- 10-second preview with size and time estimates; target-size encodes.
- System Check with distro-specific, copyable fix commands.
- `retort-cli`: diagnose, probe-hw, list-formats, analyze, presets, convert
  (with `--dry-run` and `--preview`).
- Packages: Flatpak, .deb, .rpm, AUR, AppImage, portable tarball, Nix flake,
  Gentoo ebuild, Void template; signed APT/RPM repositories.
