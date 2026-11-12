# Progress

Read this first in a new session. Requirements: `docs/SPEC.md`. Code map:
`docs/ARCHITECTURE.md`. Working conventions: `CLAUDE.md`.

## Status

| Milestone | State |
|---|---|
| M0 Bootstrap (spec, docs, name, scaffold) | in progress |
| M1 Core engine + CLI + collect-hw-report.sh | not started |
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
- [ ] Cargo workspace + Meson skeleton building

### M1 — Core engine
- [ ] ffmpeg locate / version / buildconf / caps parsers (+ fixtures 6.1, 9.0)
- [ ] ffprobe → MediaInfo (HDR, VFR, bit depth, chapters, attachments)
- [ ] Format catalog + grouping + compat table + empirical mux validation
- [ ] Device discovery (sysfs/dev/nvidia-smi/v4l2) with fake trees
- [ ] Hardware probing (encode 8/10-bit, decode clips, rc modes) + cache
- [ ] Quality mapping table + tests
- [ ] Presets + OutputSettings + naming templates + conflict policy
- [ ] Planner (basic) + command builder + insta snapshots
- [ ] Job runner (progress, pause/resume, cancel, logs, fallback)
- [ ] Doctor + distro fix table
- [ ] CLI: diagnose, probe-hw, list-formats, analyze, convert, presets
- [ ] `scripts/collect-hw-report.sh`
- [ ] Integration tests with real ffmpeg 6.1 (nasty filenames etc.)

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

## Open questions (for the maintainer)

- None blocking yet.

## Hardware test list

(Filled in at the end of each milestone.)
