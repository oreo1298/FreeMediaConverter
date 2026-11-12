# Progress

Read this first in a new session. Requirements: `docs/SPEC.md`. Code map:
`docs/ARCHITECTURE.md`. Working conventions: `CLAUDE.md`.

## Status

| Milestone | State |
|---|---|
| M0 Bootstrap (spec, docs, name, scaffold) | done (Meson skeleton lands with M2) |
| M1 Core engine + CLI + collect-hw-report.sh | done |
| M2 GUI | not started |
| M3 Advanced mode, planner, HDR, preview, target size | not started |
| M4 Packaging + release CI | not started |
| M5 Docs + polish | not started |

## Milestone checklist

### M0 — Bootstrap
- [x] `docs/SPEC.md` saved verbatim
- [x] `CLAUDE.md`, `docs/PROGRESS.md`, `docs/ARCHITECTURE.md`
- [x] Environment check (below)
- [x] Name chosen and checked (below)
- [x] FFmpeg encoder options researched from source (`docs/research/encoders.md`)
- [x] Cargo workspace building (Meson skeleton lands with the app in M2)

### M1 — Core engine
- [x] ffmpeg locate / version / buildconf / caps parsers (+ fixtures 6.1, 9.0)
- [x] ffprobe → MediaInfo (HDR10/HLG/HDR10+/Dolby Vision, VFR, rotation, bit depth, chapters, attachments)
- [x] Format catalog + grouping + compat table + empirical mux validation (cached)
- [x] Device discovery (sysfs/dev/nvidia-smi or /proc/driver/nvidia/v4l2) with fake trees
- [x] Hardware probing (encode 8/10-bit, B-frames, rc modes; decode clips + hwdownload; GPU filters) + cache
- [x] Quality mapping table + tests
- [x] Presets + OutputSettings + naming templates + conflict policy
- [x] Planner (fallback chain, full-GPU/hybrid/GPU-encode/software, HDR, target size) + command builder + insta snapshots
- [x] Job runner (progress, pause/resume, cancel, logs, automatic fallback, atomic temp output)
- [x] Doctor + distro fix table (package names verified in containers)
- [x] CLI: diagnose, probe-hw (+ --simulate, --profile-spec), list-formats, analyze, convert, presets
- [x] `scripts/collect-hw-report.sh` (output is a drop-in simulated profile)
- [x] Integration tests with real ffmpeg 6.1 **and** 9.0.2 (nasty filenames etc.)
- [x] CI workflow: fmt, clippy -D warnings, cargo-deny, shellcheck, tests on x86-64 + arm64, integration vs 6.1 and 9.0.x

### M2 — GUI
- [ ] Window shell, empty state, drag-and-drop, file chooser, open-with, CLI args
- [ ] Queue cards (thumbnail/waveform, pills, progress, hw badge), persistence
- [ ] Input analysis view
- [ ] Simple preset picker + format picker
- [ ] Progress / pause / cancel / notifications / suspend inhibit
- [ ] Preferences (incl. ffmpeg path) + hardware matrix + System Check
- [ ] App icon (full color + symbolic)
- [ ] Headless screenshot harness + review pass

### M3 — Advanced
- [ ] Advanced video/audio/subtitle/other settings UI
- [ ] Planner: full-GPU pipelines, hybrid, fallback chain with runtime retry
- [ ] HDR preservation + tonemapping (libplacebo / tonemap_vaapi / zscale)
- [ ] Preview sample (10 s) + size/time estimate
- [ ] Target size (2-pass software)
- [ ] Concurrency per engine + NVENC session-limit requeue

### M4 — Packaging
- [ ] Flatpak (GNOME 51, bundled FFmpeg 9.0.x) + lint
- [ ] .deb (Debian 13, Ubuntu 24.04+) and .rpm (Fedora, openSUSE, EL) built in target containers
- [ ] AUR -bin / -git / source PKGBUILDs (namcap)
- [ ] AppImage (+zsync), tarball (+install/uninstall)
- [ ] Nix flake, Gentoo ebuild, Void template
- [ ] Signed apt/rpm repos on GitHub Pages
- [ ] `scripts/install.sh`
- [ ] CI: lint/test/build x86-64 + arm64, release pipeline on tags

### M5 — Docs and polish
- [ ] README, INSTALL.md (generated from one source), test-docs.sh
- [ ] TESTING.md, MAINTAINER.md, CONTRIBUTING, CoC, SECURITY, issue templates
- [ ] Final screenshots + polish pass

## Environment (sandbox, checked 2026-11-12)

- Ubuntu 24.04.5 LTS x86-64, 4 vCPU, 15 GiB RAM, ~31 GB free disk, no GPU, no display.
- Rust 1.97.0, GCC 13.3, Clang 18, Docker 29.9 (daemon must be started by hand), Nix 2.34.
- System ffmpeg 6.1.1-3ubuntu5 (has x264/x265/svtav1/aom/vpx/opus/lame/vpl/vulkan/libplacebo/zimg).
- GTK 4.14.5, libadwaita 1.5.0, meson 1.3.2 from apt (installed this session).
- Network: crates.io, GitHub, Docker Hub, Flathub, AUR, archlinux.org,
  sources.debian.org, packages.ubuntu.com, src.fedoraproject.org,
  download.opensuse.org, ffmpeg.org, download.gnome.org reachable.
  repology.org times out.

## Upstream versions verified (2026-11-12)

- FFmpeg latest stable: **9.0.2** (ffmpeg.org/releases). Flatpak/AppImage bundle this.
- GNOME runtime latest stable on Flathub: **org.gnome.Platform 51**.
- Latest GTK series 4.24 (4.25 = dev), libadwaita 1.10.
- gtk4-rs 0.11.5 (MSRV 1.92), libadwaita-rs 0.9.2 (features up to v1_10).

## Name

Top three candidates (checked on Flathub search API, AUR RPC, Debian sources,
Fedora src, GitHub repository search on 2026-11-12):

1. **Retort** — chosen. No Flathub app, no AUR/Debian/Fedora package. GitHub:
   largest is an unrelated "Platform Evolution Engine" (212★); nothing in the
   media space. A glass retort (alchemy vessel that transforms substances) is
   a strong, original icon motif.
2. **Tincture** — clean everywhere (max 33★ on GitHub), weaker meaning.
3. **Ladle** — clean on packages; one 3k★ GitHub project.

Rejected: Transmute (1.4k★ self-hosted file converter), Frameshift (FFmpeg
media utility on GitHub), Smelter (Software Mansion video toolkit), Kiln
(packaged on AUR/Fedora), Refract, Morpho, Imago, Metamorph (taken).

App ID: `io.github.oreo1298.Retort` (owner from `git remote`). Defined in `branding.env`.

## Decisions log

| # | Decision | Why | Alternatives rejected |
|---|---|---|---|
| D1 | UI built in Rust code, no Blueprint/.ui | Fewer build deps on every distro (blueprint-compiler is not packaged everywhere at a usable version); type-checked | Blueprint (extra build dep), GtkBuilder XML (verbose, untyped) |
| D2 | Separate `retort-cli` binary without GTK linkage | Runs on headless servers (Proxmox) without GTK installed | GUI binary with subcommands (drags GTK into headless installs) |
| D3 | Threads + channels in core, no async runtime | Process I/O is the only concurrency; easy to test; GUI bridges with async-channel | tokio (heavy, no benefit) |
| D4 | GUI prefs in GSettings; CLI uses flags/env; hw probe cache is a shared JSON file | GNOME convention + Flathub friendly; CLI stays dependency-free; both reuse probe results | Shared TOML config (non-standard for GNOME apps) |
| D5 | QSV device derived from an explicit VAAPI node (`qsv=qs@va`) | Correct device on hybrid Intel+NVIDIA laptops | `-qsv_device` / implicit default (may pick wrong GPU) |
| D6 | Vulkan device derived from DRM node (`vulkan=vk@dr`) | Maps Vulkan to the same physical GPU as VAAPI without guessing indices | `vulkan:N` indices (order differs from DRM order) |
| D7 | `CUDA_DEVICE_ORDER=PCI_BUS_ID` for every ffmpeg child | CUDA indices then match nvidia-smi / PCI order | Default FASTEST_FIRST ordering |
| D8 | Decode probes end with `hwdownload` | A silent software fallback in ffmpeg must count as a failure | Parsing stderr warnings only |
| D9 | Only modern NVENC options (`p1–p7`, `-tune`, `-rc vbr/cbr/constqp`, `-multipass`, `spatial-aq`) | They exist from 6.1 through 9.0; legacy ones were removed in 9.0 | Version-switched legacy options |
| D10 | VAAPI quality via `-rc_mode CQP|ICQ -global_quality N` | Works for every VAAPI encoder incl. av1_vaapi (which has no `-qp`) in 6.1–9.0 | `-qp` (missing on av1_vaapi) |
| D11 | Name/ID in `branding.env`, read by build.rs and meson | Rename = edit one file + run rename script | Hard-coded strings |
| D12 | Outputs are written to a hidden temp file in the destination folder and renamed on success | Failed/cancelled jobs never leave partial files; an existing file is untouched unless "overwrite" was chosen; atomic | Writing in place + deleting on failure (destroys the old file with -y, leaves junk on crash) |
| D13 | Colour properties are tagged with `setparams` in the filter chain (plus `-color_*` output options) | FFmpeg 8/9 take colour properties from frames; output options alone no longer tag the stream (found by the 9.0.2 integration run) | Output options only (silently untagged on 9.0) |
| D14 | Fallback chain = best plan of the chosen GPU + its CPU-decode variant, then ≤ 2 other verified backends (robust variant), then software | Spec order; bounded retries; full-GPU-capable devices ranked first | Every plan of every backend (11-plan chains in review) |
| D15 | Two-pass and target size are software-only (libx264/x265/libvpx/libaom); hardware encoders get single-pass VBR if no software encoder exists | Hardware encoders can't do true 2-pass in FFmpeg; spec asks for 2-pass in software | NVENC "multipass" (it is not a size-accurate 2-pass) |
| D16 | Target-size bitrate capped at a very-high-quality ceiling (0.2 bpp) | "Under N MB" is a limit, not a goal; short clips got 38 Mbps otherwise | Using the full budget |
| D17 | Bundled FFmpeg builds use nv-codec-headers n12.2 (driver ≥ 550) | Widest current driver compatibility while keeping AV1 NVENC and 9.0 features | n13.x headers (driver ≥ 570) |
| D18 | Hardware reports from `collect-hw-report.sh` are self-contained simulated profiles (`profile.json` from `retort-cli probe-hw --profile-spec`) | A real machine's results become a regression fixture by copying one folder | Hand-converting raw logs |
| D19 | Copy-only codecs (dts, truehd, bitmap subtitles) are accepted for stream copy but never offered for encoding | Their FFmpeg encoders are experimental or need bitmap input | Offering them with `-strict -2` |

## Findings worth knowing (from real FFmpeg / distro checks)

- FFmpeg 9.0 dropped legacy NVENC presets/rc modes (verified in source); we only emit `p1–p7`, `-tune`, `-rc vbr|cbr|constqp`.
- FFmpeg 9.0 needs libplacebo ≥ 7.351 and Vulkan headers ≥ 1.4.317 (for `av1_vulkan`); `scale_cuda` needs `--enable-cuda-llvm`.
- FFmpeg 9.0 ignores `-color_primaries/-color_trc` output options unless frames carry the same tags (D13).
- ffprobe 8+/9 reports animated WebP as codec/demuxer `webp_anim`.
- Fedora 45 folded VA-API drivers into `mesa-dri-drivers` (encode-crippled); the fix is now `dnf install mesa-va-drivers-freeworld` (not `swap`). Ubuntu 26.04 made `mesa-va-drivers` a virtual package (`mesa-libgallium`).
- RPM Fusion has no NVIDIA driver for EL10; NVIDIA's own `cuda-rhel10.repo` does.

## Open questions (for the maintainer)

- None blocking. Real hardware reports (below) will replace the synthetic profiles.

## Hardware test list

### After M1 (CLI only — no GUI yet)

Build: `cargo build --release -p retort-cli` (Rust ≥ 1.92). Then on **each machine**:

1. `scripts/collect-hw-report.sh` → commit the folder as `tests/fixtures/hw/<machine>/` (or attach the tarball to an issue). This is the most valuable thing you can send.
2. `target/release/retort-cli diagnose` — check every finding is true and the fix commands make sense for your distro.
3. `target/release/retort-cli probe-hw` — compare against `vainfo`, `nvidia-smi`, `vulkaninfo --summary`.
4. Convert a real file with each Simple preset you care about, e.g.
   `retort-cli convert some.mkv --preset mp4-small --output-dir /tmp/out` and
   `--dry-run` to see the exact ffmpeg commands.

Machine-specific checks:

| Machine | What to check |
|---|---|
| Desktop (RTX 4090) | `probe-hw` shows h264/hevc/av1_nvenc with 10-bit for hevc/av1; `--preset av1-smallest --speed fastest` uses NVENC AV1 ("Decode: NVDEC · Filters: CUDA"); with the Ryzen iGPU enabled, a second device appears with VAAPI. HDR10 file → `--preset mp4-small` keeps HDR (check with `retort-cli analyze` on the output). |
| Dell 5570 (Iris Xe + NVIDIA) | Devices listed separately as "Intel Iris Xe Graphics — QSV/VAAPI" and "NVIDIA … — NVENC"; `--device pci-0000:00:02.0` forces the Intel GPU; `--backend vaapi` prefers VAAPI. Note: run once with the dGPU asleep and once awake. |
| Proxmox (Intel iGPU + GTX 1660 Ti) | Headless: `diagnose` and `probe-hw` over SSH; NVENC Turing (no AV1 encode — must show as failed with a clear reason); QSV on the iGPU; a 4-way parallel `convert` to see NVENC session behaviour. |
| M1 MacBook Air (Asahi) | Everything software; `diagnose` shows "No hardware video encoder on Apple Silicon"; all presets complete. Build/test on arm64 with `cargo test --workspace`. |
