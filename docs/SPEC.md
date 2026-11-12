# Project: a modern FFmpeg converter GUI for Linux (x86-64 + arm64)

You are building a complete, production-quality, free and open-source desktop app from an empty repo. Read this whole spec before starting. Work autonomously through every milestone below. I'm not watching live and I'll review your commits later.

---

## 0. First steps (do these before anything else)

1. Save this entire prompt verbatim to `docs/SPEC.md` and commit it. It's the source of truth across sessions.
2. Create `CLAUDE.md` with the repo conventions and the exact build, test and lint commands.
3. Create `docs/PROGRESS.md` with the milestone checklist, a decisions log (decision, why, alternatives rejected) and open questions. Update it at the end of every milestone so a fresh session can pick up exactly where you left off.
4. Check your environment (`check-tools`, `cat /etc/os-release`, `nproc`, `free -h`) and note what you have.

---

## 1. What we're building

A desktop app that converts **any audio or video file FFmpeg can read into any format FFmpeg can write**. It has a beautiful, modern GUI and uses **hardware acceleration automatically whenever it actually works on the user's machine**. When it doesn't, the app falls back to software cleanly.

- Linux only. x86-64 and arm64, both first-class.
- License: GPL-3.0-or-later.
- Packaged for every popular Linux distro, with install, update and uninstall instructions for each one.
- Users range from "make this an MP4 for my phone" to "HEVC 10-bit via VAAPI on my second GPU with custom x265-params". Simple mode must be dead simple. Advanced mode must expose almost everything.

---

## 2. Context: your sandbox and my hardware

**Your environment:** a Claude Code cloud VM running Ubuntu 24.04 on x86-64, with 4 vCPU, 16 GB RAM and 30 GB disk.
- It has no GPU and no display.
- Rust/cargo, GCC/Clang, Docker and `apt` are available.
- Package registries, GitHub and Docker Hub are reachable. Other sites (for example Flathub) may not be.
- If something is blocked, don't stall. Work around it or move that step into CI, and log it in PROGRESS.md.
- You can't test hardware acceleration here, so design for testing with recorded fixtures plus hardware reports from me (§10).

**My test machines.** I'll do the real-hardware testing:
1. **Desktop:** CachyOS (Arch-based) with KDE Plasma on Wayland, Ryzen 9 9950X3D, RTX 4090. This covers NVENC/NVDEC, including AV1 encode. If the CPU's small Radeon iGPU is enabled, it's also an AMD VAAPI target.
2. **Dell Precision 5570 laptop:** CachyOS, Intel 12th-gen iGPU plus an NVIDIA dGPU (hybrid graphics). This tests multi-GPU selection, VAAPI/QSV on Intel and NVENC on NVIDIA.
3. **Proxmox home server:** Intel iGPU (QSV already works there for Jellyfin) plus a GTX 1660 Ti (Turing NVENC). It's headless, so I'll test it through the CLI.
4. **M1 MacBook Air:** Asahi Linux, arm64. Expect no usable hardware video encode. The arm64 build and the software fallback must be flawless here.

At the end of each milestone, add to PROGRESS.md a short list of what I should test on which machine.

---

## 3. Non-negotiables

- **No shell, ever.** Invoke ffmpeg/ffprobe with argv arrays. Use the `file:` protocol prefix (or absolute paths) for inputs and outputs so filenames with leading dashes, colons, quotes, newlines or Unicode never get misparsed.
- **Hardware support is data-driven and verified.** Discover candidates from FFmpeg itself, then confirm each one with a real test encode or decode on this machine. Never assume support from the GPU vendor. Nothing appears as "available" in the UI unless a probe passed.
- **Every hardware job has an automatic fallback.** A job must never fail only because the GPU path failed. The exception is when the user pinned a specific encoder: then show a clear error with a "Retry with software" button.
- **The exact ffmpeg command for every job is visible and copyable.**
- Never modify inputs. Never overwrite outputs silently.
- No telemetry. The only network use is an optional, opt-in update check, and only for AppImage and tarball installs.
- **Every command in the docs is tested** (§9).
- **Distributed builds must be redistributable.** No `--enable-nonfree` FFmpeg builds, so no fdk-aac and no libnpp. NVENC/NVDEC/CUDA through nv-codec-headers is fine.

---

## 4. Tech stack

Use this stack. Deviate only with a written reason in PROGRESS.md.

- **Rust (stable) + GTK 4 + libadwaita** via gtk4-rs and libadwaita-rs. That gives a native, fast app that follows the system light/dark preference and accent color, adapts its layout, and is first-class on Flathub.
- Build with **Meson wrapping Cargo**, following GNOME Rust app conventions. Meson installs the desktop file, AppStream metainfo, icons, GSettings schema and translations.
- **Cargo workspace:**
  - `core`: no GTK dependency. Holds FFmpeg discovery, capability probing, the format/codec catalog, compatibility rules, the preset model, the command builder, the job runner, progress parsing and fallback logic. Most of the tests live here.
  - `app`: the GTK UI.
  - `cli`: the same engine, headless. Subcommands: `diagnose [--json]`, `probe-hw`, `list-formats`, `convert <in> --preset … -o …`. I'll use it on headless boxes and in bug reports. A single binary with subcommands is fine.
- **Minimum versions for native packages:** GTK ≥ 4.14 and libadwaita ≥ 1.5. That's the Ubuntu 24.04 baseline, which also covers Mint 22, Pop!_OS 24.04, elementary OS 8 and Zorin OS 18.
  - Use newer libadwaita widgets only behind feature gates or runtime version checks, with graceful fallbacks. Newer distros have much newer versions; Ubuntu 26.04 ships libadwaita 1.9.
  - Distros below the minimum (Debian 12, Ubuntu 22.04 and so on) get Flatpak/AppImage only. Say so plainly in the docs.
  - Verify each distro's real GTK and libadwaita versions rather than trusting this list.
- **FFmpeg:**
  - Native packages use the system ffmpeg. Support 6.1 and newer (6.1 is Ubuntu 24.04's version) and adapt features to whatever is present.
  - Flatpak and AppImage bundle the latest stable release, which is 9.0.x at the time of writing. Check for newer.
  - Preferences has an option to point at any ffmpeg binary (jellyfin-ffmpeg, a static build, a custom build).

---

## 5. Features

### 5.1 Input
- Add files by drag-and-drop of files and folders, the file chooser (through the portal), "Open with" from file managers (`.desktop` MimeType `audio/*;video/*`), and CLI arguments.
- Recursive folder import with an extension filter.
- Show ffprobe analysis in a clean layout:
  - File: container, duration, size and overall bitrate.
  - Per stream: codec/profile/level, resolution, frame rate (with VFR detection), pixel format and bit depth, color space and HDR type (HDR10, HLG, Dolby Vision flags), audio channels/layout/sample rate, and subtitle formats.
  - Also: chapters, attachments and metadata.
- Show a thumbnail for video and a waveform for audio.
- Batch queue with per-file overrides. The queue persists across restarts.

### 5.2 Output
- **Format picker:** every muxer the user's FFmpeg reports.
  - Group them (Popular, Video, Audio, Images/Animation, Professional/Broadcast, Other) and make them searchable.
  - A "Show all formats" toggle reveals exotic ones.
  - Hide non-file outputs by default: devices like alsa/pulse/v4l2/fbdev, `null`, and network-only muxers.
- **Codec pickers per stream type**, filtered to what the chosen container can hold and what this FFmpeg can encode.
  - Use a curated compatibility table for popular containers.
  - Validate everything else empirically with a tiny lavfi test mux, and cache the result.
  - Never offer a combination that will fail.
- Smart defaults per container, and "Copy (no re-encode)" per stream. When a pure remux is possible, detect it and suggest it ("Instant, lossless").
- **Simple-mode preset cards**, for example:
  - MP4 that plays everywhere (H.264/AAC); MP4 that's smaller (HEVC)
  - MKV at archive quality; AV1 for the smallest file; WebM for the web
  - Fit under N MB for Discord or email (target size with a computed bitrate, 2-pass in software)
  - Apple-friendly (HEVC with the `hvc1` tag, plus AAC)
  - Editing intermediate (ProRes / DNxHR)
  - GIF / animated WebP
  - Audio: MP3, AAC/M4A, Opus, FLAC, WAV, ALAC
  - Extract audio only; remux only
- **Speed vs. efficiency choice:** Fastest (GPU) / Balanced / Smallest file (CPU, using slow x265/SVT-AV1 presets).
- **Advanced mode, video:**
  - Encoder (Auto or a specific one), rate control (CRF/CQ/QP/VBR/CBR/target size), preset/tune/profile/level, pixel format and bit depth, GOP and B-frames.
  - Scaling (keep the aspect ratio and even dimensions), frame rate, crop (with auto cropdetect), trim, rotate/flip.
  - Deinterlace (auto-detected with `idet`).
  - HDR→SDR tonemapping: GPU through libplacebo/Vulkan or tonemap_vaapi when available, with a zscale/tonemap software fallback.
  - Basic denoise and sharpen.
- **Advanced mode, audio:** codec, bitrate/quality, channels/downmix, sample rate, EBU R128 two-pass loudness normalization, and track selection with language and default flags.
- **Advanced mode, subtitles:** copy, convert (for example SRT → mov_text for MP4), burn in, or drop.
- **Advanced mode, other:** copy or strip metadata and chapters, plus custom extra input and output args with warnings.
- **Unified quality slider** that maps to each encoder's native control: `crf` for x264/x265/SVT-AV1/libvpx, `cq`/`qp` for NVENC, `global_quality`/ICQ for QSV, `qp`/`rc_mode` for VAAPI and Vulkan, and so on.
  - Research the real options of each encoder in each supported FFmpeg version.
  - Keep the mapping in one table-driven module with thorough tests.
- **Output naming template** (`{name}`, `{ext}`, `{codec}`, `{res}`, `{date}`), output folder (next to the source or a custom folder), conflict policy (rename/skip/ask/overwrite) and an option to preserve file timestamps.
- **Preview:** encode a 10-second sample from the middle of the file to check quality and estimate the final size.
- Show the estimated output size and time.

### 5.3 Jobs
- Read progress through `-progress pipe:` plus `-nostats`. Show %, fps, speed, ETA and current size, per job and in total.
- Pause/resume with SIGSTOP/SIGCONT. Cancel gracefully: send `q`, then SIGTERM, then SIGKILL. Clean up partial outputs on cancel or failure (configurable).
- Concurrency: use a sensible default (for example one job per hardware encode engine plus one CPU job), configurable. Handle NVENC session limits by queueing instead of failing.
- Inhibit suspend while converting (portal). Send a desktop notification on completion, with "Open folder" and "Show in file manager".
- Keep a per-job log with the full ffmpeg stderr, and a "Copy diagnostics" button for bug reports.

---

## 6. Hardware acceleration (the heart of this project)

### 6.1 Backends to handle first-class
- **NVIDIA:** NVENC (`h264/hevc/av1_nvenc`), NVDEC via `-hwaccel cuda`, and CUDA filters (`scale_cuda` and others).
  - FFmpeg 9.0 removed deprecated NVENC options and support for NVENC SDKs older than 11.1.
  - Gate NVENC options on the detected FFmpeg version and driver.
- **VAAPI:** Intel and AMD through Mesa, plus NVIDIA decode through nvidia-vaapi-driver. Covers `*_vaapi` encoders, VAAPI decode, and `scale_vaapi`, `tonemap_vaapi` and similar filters.
- **Intel QSV (oneVPL/libvpl):** `*_qsv` encoders and decoders, plus `vpp_qsv`.
- **Vulkan Video:**
  - Decode: H.264/HEVC/AV1 since FFmpeg 6.1, VP9 since 8.0.
  - Encode: `h264_vulkan`/`hevc_vulkan` since 7.1, `av1_vulkan` since 8.0.
  - Filters: libplacebo and `scale_vulkan`.
  - Works on NVIDIA, RADV and ANV when the driver exposes the video extensions.
- **AMD AMF (`*_amf`):** only if present and a probe passes. On Linux it needs AMD's proprietary stack, which is rare, so don't push users toward it.
- **ARM:**
  - V4L2 M2M (`*_v4l2m2m`, for example Raspberry Pi 4 H.264).
  - Rockchip MPP (`*_rkmpp`, only in forks such as ffmpeg-rockchip).
  - Any other hardware codec the user's FFmpeg exposes.
  - Verify Raspberry Pi 5's encoder situation (I believe it has no H.264 hardware encoder). Either way, the app just falls back.
- **General rule:** discover candidates from FFmpeg (`-encoders`, `-decoders`, `-hwaccels`, and `ffmpeg -h encoder=X` for supported hw devices and pixel formats), not from a hardcoded list. The list above exists so each family gets proper device selection, filters and rate-control mapping.

### 6.2 Device discovery
- Enumerate:
  - DRM render nodes (`/dev/dri/renderD*`), with vendor, driver and PCI ID from sysfs.
  - NVIDIA GPUs, through `nvidia-smi` if present or `/proc/driver/nvidia` otherwise.
  - Vulkan devices, through `-init_hw_device vulkan:N` probes or `vulkaninfo` if present.
  - V4L2 M2M devices.
- **Multi-GPU:** map each backend to the physical GPU it runs on. For example, the laptop should show "Intel Iris Xe — QSV/VAAPI" and "NVIDIA RTX … — NVENC". Let the user pick a device or leave it on Auto. Use explicit `-init_hw_device` and `-filter_hw_device` with device paths or indices.
- **Permissions:** detect inaccessible `/dev/dri` or `/dev/nvidia*` (user not in the render/video group, or a missing Flatpak permission) and explain the fix.

### 6.3 Capability probing
- For each (backend, device, encoder) candidate, run a tiny real encode in 8-bit and in 10-bit.
  - Use lavfi `testsrc2` at a size every encoder accepts, for example 640×360, for a fraction of a second.
  - Record: works or fails (with the reason), accepted pixel formats, B-frame support, rate-control modes, and resolution limits where discoverable.
- Probe hardware decoders the same way. Ship tiny clips we generate ourselves (a few KB each) so decode probing doesn't depend on the user's FFmpeg having software encoders. A limited build like Fedora's ffmpeg-free may lack them.
- Run probes in parallel with timeouts, in the background on first launch.
- Show the results in Preferences → Hardware as a clean matrix: device × codec × encode/decode × 8/10-bit, with a "Re-test" button.
- Cache the results, keyed on: ffmpeg path, version and buildconf hash; kernel version; GPU driver versions; and the device list. Re-probe automatically when any of them change.

### 6.4 Pipeline planner
For every job, pick the best plan and explain it in the UI, for example "Decode: NVDEC · Filters: CUDA · Encode: NVENC AV1 on RTX 4090".
- **Prefer full on-GPU pipelines** (decode → filters → encode on one device, zero-copy, `-hwaccel_output_format`) when every needed filter has a hardware equivalent on that device.
- **Otherwise use a hybrid path:** hardware decode → hwdownload → CPU filters → hwupload → hardware encode, or software decode → hardware encode. Be correct first and fast second.
- **Respect encoder limits:** bit depth and pixel formats (never send 10-bit to an encoder that can't take it; convert, or choose another encoder and say so), resolution limits, odd dimensions, and B-frames.
- **HDR:** preserve HDR10/HLG signaling: color primaries, transfer and matrix, plus mastering display and content light level side data where the encoder supports it. Warn when an encoder or container will drop HDR or Dolby Vision, and offer tonemapping.
- **Fallback chain:** chosen hardware → another working hardware backend for the same codec → software.
  - If a hardware path fails at runtime (session limit, driver error), retry down the chain automatically.
  - Mark the job "Completed (software fallback)" and show the reason.
- **Ranking under "Fastest":** a full GPU path on a device that can also decode the input, then hardware encode only, then software. Users can pin a preferred backend in Preferences.

### 6.5 System check ("Doctor")
Build an in-app System Check page, also exposed as `cli diagnose`. It explains problems in plain language, with copyable fix commands tailored to the distro (read `ID`/`ID_LIKE` from `/etc/os-release`). Cover at least:
- ffmpeg missing, or a limited build. For example, Fedora's `ffmpeg-free` lacks the x264/x265 encoders: the fix is enabling RPM Fusion, then `sudo dnf swap ffmpeg-free ffmpeg --allowerasing`.
- VAAPI driver missing or decode-only. Fedora AMD users need `mesa-va-drivers-freeworld`, and Intel users need `intel-media-driver`. Debian/Ubuntu have similar non-free/multiverse driver packages.
- NVIDIA driver present but `libnvidia-encode`/`libcuda` missing.
- Vulkan driver without the video extensions.
- User not in the `render` group.
- Flatpak missing a device permission.

Verify every fix command for every distro. Don't guess package names.

---

## 7. UI/UX design

**Goal:** it should look like a polished, current GNOME Circle–quality app and still feel at home on KDE Plasma.

- Follow libadwaita and the GNOME HIG. Follow the system light/dark preference and accent color.
- Adaptive layout down to about 360 px wide with `AdwBreakpoint`, so it works on small laptops and Linux phones.
- **Empty state:** a big, friendly drop target (`AdwStatusPage` with a custom illustration) saying "Drop files to convert".
- **Queue items are cards**, each with:
  - A thumbnail or waveform, and the filename.
  - Key specs as small pills (`1080p · HEVC 10-bit · HDR10 · 5.1 · 2:13:04`).
  - A source → target arrow with a target format pill.
  - Progress with speed and ETA.
  - A hardware badge (`⚡ NVENC · RTX 4090` or `CPU`) and status icons.
- **Format picker:** a searchable grid of format cards with clear icons and one-line descriptions, with Simple presets at the top.
- **Settings for the selected item or batch** live in a side panel or bottom sheet (`AdwNavigationSplitView`/`AdwOverlaySplitView`), with a Simple/Advanced toggle.
- **Actions and feedback:** one prominent "Convert" pill button as the primary action. Toasts for non-blocking info. Dialogs only when truly needed.
- **Polish:** subtle, smooth animations. Keyboard shortcuts (Ctrl+O, Ctrl+Return, Delete, Ctrl+, , Ctrl+?, Ctrl+Q) with a shortcuts window. Full keyboard navigation and screen-reader labels.
- **Icon:** an original app icon in the current GNOME style, full-color plus symbolic, drawn by you as SVG.
- i18n-ready with gettext, English first.

**Visual self-review is mandatory.** You can't see a screen, so render the app headlessly (Xvfb, or a headless Wayland compositor with a software GSK renderer). Screenshot every major state:
- empty
- queue with items
- running
- completed
- errors
- format picker
- advanced settings
- preferences / hardware matrix
- system check
- narrow width
- light and dark

Look at the screenshots and iterate until the result is genuinely polished: spacing, alignment, hierarchy, contrast. Save the final set to `docs/screenshots/`; the README and the Flathub metainfo need them.

---

## 8. Packaging and distribution (every artifact for x86-64 and arm64)

### Tier 1
- **Flatpak (the primary universal format, Flathub-ready):**
  - Runtime: the latest stable `org.gnome.Platform`.
  - FFmpeg: bundle the latest stable release, built in the manifest with: x264, x265, SVT-AV1, dav1d, libaom, libvpx, Opus, Vorbis, LAME, libplacebo, Vulkan, VAAPI, nv-codec-headers (NVENC/NVDEC/CUDA), libvpl plus the Intel GPU runtime QSV needs, zimg, libass, libsoxr and libwebp.
  - Device permissions: as minimal as possible while still allowing NVENC/CUDA, VAAPI, Vulkan and QSV. Study OBS Studio's and HandBrake's Flathub manifests; they solve the same problems.
  - Vendor Rust deps with flatpak-cargo-generator.
  - Must pass `flatpak-builder-lint` and `appstreamcli validate`.
  - Also publish a `.flatpak` bundle on GitHub Releases.
> **Changed by the owner (2026-11-12, `docs/PROGRESS.md` D40):** native
> packages are built by the user from a clone of the repository and installed
> with the package manager, EZP2019Linux-style (`build-package.sh` for
> .deb/.rpm, a repository-root `PKGBUILD` with `makepkg -si` on Arch). There
> are no signed APT/RPM repositories, no AUR packages and no
> `scripts/install.sh`; the items below that describe them are superseded.

- **Native .deb and .rpm:**
  - Targets: .deb for Debian 13+ and Ubuntu 24.04+ (and their derivatives); .rpm for Fedora, openSUSE Tumbleweed/Leap 16, and RHEL-family releases whose GTK and libadwaita are new enough.
  - Build each package inside the target release's own container so library dependencies are correct.
  - Depend on the system ffmpeg. RPMs use a file dependency on `/usr/bin/ffmpeg` so either ffmpeg-free or RPM Fusion's ffmpeg satisfies it.
  - Recommend the appropriate VAAPI driver packages.
- **Arch:** PKGBUILDs for `-bin` and `-git` (plus a plain source build), with `arch=('x86_64' 'aarch64')`. They must pass namcap and be ready to push to the AUR.
- **Signed package repos so updates arrive through the package manager:**
  - An apt repo and an rpm repo (dnf/zypper), generated by CI and hosted on GitHub Pages.
  - Signed with a GPG key kept in GitHub secrets.
  - apt instructions use the modern deb822 `.sources` format with `Signed-By` and a keyring in `/etc/apt/keyrings`.

### Tier 2
- **AppImage:** embedded update information for AppImageUpdate/zsync, and a bundled FFmpeg. Use the host's GPU drivers: bundle the loaders, never the drivers. Document the limitations honestly.
- **Tarball:** with `install.sh`/`uninstall.sh`, for a user install (`~/.local`) or a system install (`/usr/local`). It uses the system GTK, libadwaita and ffmpeg.
- **Nix flake:** package, overlay, and NixOS/home-manager usage. Also prepare a nixpkgs-ready derivation.
- **Gentoo:** an ebuild in an overlay.
- **Void:** an xbps-src template.
- **Snap:** not planned. Explain in the docs that Flatpak covers it, unless Snap turns out to be trivial *and* hardware acceleration works inside it.

### Universal helper: `scripts/install.sh`
- Detects the distro (from `ID`/`ID_LIKE` in os-release) and the architecture, then recommends and runs the best method.
- Flags: `--method`, `--update`, `--uninstall`, `--purge`, `--dry-run`, `--yes`.
- Verifies checksums and signatures, asks before using sudo, and stays short and readable so people can audit it before running it.

### Releases
- SemVer, with a CHANGELOG.md in Keep a Changelog format.
- Pushing a `vX.Y.Z` tag triggers CI to:
  - build every artifact for both architectures
  - write `SHA256SUMS` plus its GPG signature
  - create a draft GitHub Release with notes
  - update the apt/rpm repos and the AUR/Flatpak manifests, automatically or with one documented command
- **In-app update notifier:** only for AppImage and tarball installs, opt-in, and compiled out of Flatpak and distro packages.

---

## 9. Install / update / uninstall documentation

**README.md:** what the app is, screenshots, features, a short install table that links to the full guide, a hardware-support summary and a FAQ.

**docs/INSTALL.md** (or a GitHub Pages docs site, your call):
- Start with a "Which method should I use?" decision table.
- Then one section per distro or family. Every section has the same sub-sections:
  1. **Install.** Include any repos needed for full codecs: RPM Fusion on Fedora/RHEL-family, Packman on openSUSE, non-free/multiverse components on Debian/Ubuntu where needed.
  2. **Enable hardware acceleration** for Intel, AMD and NVIDIA (and ARM SoCs where relevant): exact packages, plus how to verify with `vainfo`, `nvidia-smi`, `vulkaninfo --summary` and our `diagnose`.
  3. **Update.**
  4. **Uninstall**, plus **Purge** (config, cache and data dirs, and removing the repo and its key).
  5. **Troubleshooting.**
- Distros to cover. Verify which releases are currently supported when you write this, and state which ones you actually tested:
  - **Debian family:** Debian 13 (Debian 12 through Flatpak), Ubuntu (current LTS releases plus the current interim), Linux Mint, Pop!_OS, elementary OS, Zorin OS, KDE neon, MX Linux, Raspberry Pi OS (64-bit), Armbian.
  - **Fedora/RHEL family:** Fedora Workstation and spins, Nobara, Fedora Asahi Remix (arm64), RHEL/AlmaLinux/Rocky (EPEL + RPM Fusion). Fedora Atomic desktops (Silverblue, Kinoite, Bazzite, Bluefin, Aurora): Flatpak first, with rpm-ostree layering as the alternative.
  - **openSUSE:** Tumbleweed, Slowroll, Leap 16 (with Packman), Aeon/Kalpa (Flatpak).
  - **Arch family:** Arch, CachyOS, EndeavourOS, Manjaro, Garuda, Arch Linux ARM; SteamOS through Flatpak.
  - **Others:** NixOS/Nix, Gentoo, Void, Alpine/postmarketOS (Flatpak, with a musl note), Solus (Flatpak), Slackware (tarball).
  - **Universal:** Flatpak, AppImage, tarball.
- **Every documented command must be exercised.**
  - Write `scripts/test-docs.sh`. For each package-based method it runs install → verify → update → uninstall → purge inside the matching distro container: x86-64 Docker in your sandbox, and both architectures in CI.
  - Keep the docs and the tests from drifting: generate the doc snippets from one source of truth, or extract and test them.

---

## 10. Testing (there's no GPU here, so be clever)

- **Unit tests in `core`:**
  - parsers for `-encoders`, `-decoders`, `-hwaccels`, `-muxers`, `-h encoder=`, `-buildconf` and `-progress` output
  - device discovery against fake sysfs and `/dev` trees
  - planner decisions
  - rate-control mapping
  - compatibility rules
  - filename edge cases
  - snapshot tests (for example with `insta`) of generated command lines across inputs × presets × simulated hardware profiles
- **Simulated hardware profiles:** fixture sets for NVIDIA-only, Intel-only, AMD-only, a hybrid Intel + NVIDIA laptop, Raspberry Pi, Asahi (no hardware), and "drivers installed but broken".
- **Write `scripts/collect-hw-report.sh` for me to run on each of my machines.** It saves everything the parsers need into a folder I'll commit to `tests/fixtures/hw/<machine>/`:
  - FFmpeg: `-encoders`, `-decoders`, `-hwaccels`, `-buildconf`, and `-h encoder=` for every hardware encoder
  - `vainfo` for every render node, `nvidia-smi -q`, `vulkaninfo --summary`
  - `lspci -nnk`, the `/dev/dri` listing, os-release and `uname -a`
  - our `diagnose --json`

  As my real fixtures arrive, replace the synthetic ones.
- **Integration tests with real software FFmpeg:** Ubuntu's 6.1 in the sandbox, plus the bundled latest build in CI.
  - Generate media with lavfi: odd dimensions, VFR, 10-bit, HDR-tagged, 5.1/7.1 audio, subtitles, chapters, and nasty filenames.
  - Run it through every Simple preset and a broad sample of container/codec combinations.
  - Verify the outputs with ffprobe: streams, codecs, duration within tolerance, and metadata.
- **GUI smoke tests:** launch the app headlessly, drive the basic flows and take screenshots (§7).
- **`TESTING.md`:** a manual checklist for my machines in §2: what to click, what should happen, and which logs to send back.

---

## 11. CI (GitHub Actions)

- **On every push and PR:**
  - `cargo fmt --check`, clippy with warnings denied, tests, and cargo-deny/audit
  - builds on `ubuntu-24.04` (x86-64) and `ubuntu-24.04-arm` (arm64; free for public repos)
  - the Flatpak build for both architectures
  - the docs-test matrix
  - AppStream and desktop-file validation
- **On a tag:** the full release pipeline from §8.
- **Caching:** cache aggressively, especially the FFmpeg builds.

---

## 12. Repo hygiene

- **Repo files:** LICENSE (GPL-3.0-or-later), CONTRIBUTING.md, CODE_OF_CONDUCT.md, SECURITY.md, and issue templates (the bug template asks for `diagnose` output).
- **Name and app ID:**
  - App ID: `io.github.<owner>.<AppName>`, with `<owner>` taken from the git remote.
  - Choose a short, distinctive name that isn't already used by a Flathub app, an AUR/Debian/Fedora package or a well-known GitHub project. Check before committing to it.
  - Keep the name and ID defined in one place so a rename is trivial. Log your top three candidates in PROGRESS.md.
- **docs/MAINTAINER.md:** step-by-step instructions for everything only I can do. I'll follow it literally, so be precise:
  - create the GPG signing key and add the GitHub secrets
  - enable GitHub Pages
  - set up an AUR account and SSH key
  - submit to Flathub
  - open the nixpkgs PR
  - (optional) set up COPR, OBS or a PPA if we ever add them

---

## 13. How to work

- **Plan, then build.** Before writing app code, write `docs/ARCHITECTURE.md` and the milestone plan in PROGRESS.md.
  - Then build without waiting for me. Make reasonable decisions and log them.
  - Stop only for something genuinely destructive or irreversible.
- **Verify facts against current upstream sources instead of memory:**
  - FFmpeg docs and source for each encoder's and filter's options in every supported version
  - distro package names and versions
  - GTK/libadwaita APIs
  - Flathub requirements

  Things move fast. For example, FFmpeg 9.0 (August 2026) removed old NVENC options.
- **Commits:** small, meaningful commits. Push after each milestone and keep the build green.
- **Milestones.** Each one ends with all tests passing, PROGRESS.md updated, and a "test this on your hardware" list for me:
  - **M1, Core engine:** discovery, parsers, catalog, command builder, job runner, hardware probing with fixtures, and the CLI (`diagnose`, `probe-hw`, `convert`), plus `collect-hw-report.sh`.
  - **M2, GUI:** queue, input analysis, Simple presets, progress, notifications, preferences, hardware matrix and System Check.
  - **M3, Advanced:** advanced mode, the planner (full-GPU pipelines and the fallback chain), HDR handling, preview and target size.
  - **M4, Packaging:** Flatpak, deb/rpm, AUR, AppImage, tarball, Nix, Gentoo/Void, the signed repos, `install.sh`, and the CI release pipeline for both architectures.
  - **M5, Docs and polish:** README, the per-distro INSTALL.md, `test-docs.sh`, TESTING.md, MAINTAINER.md, final screenshots, and a full UI polish pass driven by screenshot review.
- **Definition of done:**
  - A new user on any listed distro can install the app from the docs, convert files with hardware acceleration wherever their machine supports it, update it, and uninstall it cleanly.
  - CI is green on x86-64 and arm64.
  - No TODOs remain in user-facing code paths.
