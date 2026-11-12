# Progress

Read this first in a new session. Requirements: `docs/SPEC.md`. Code map:
`docs/ARCHITECTURE.md`. Working conventions: `CLAUDE.md`.

## Status

| Milestone | State |
|---|---|
| M0 Bootstrap (spec, docs, name, scaffold) | done |
| M1 Core engine + CLI + collect-hw-report.sh | done |
| M2 GUI | done |
| M3 Advanced mode, planner, HDR, preview, target size | done |
| M4 Packaging + release CI | done (see checklist for what is verified locally vs. on CI only) |
| M5 Docs + polish | done |

## Milestone checklist

### M0 — Bootstrap
- [x] `docs/SPEC.md` saved verbatim
- [x] `CLAUDE.md`, `docs/PROGRESS.md`, `docs/ARCHITECTURE.md`
- [x] Environment check (below)
- [x] Name chosen and checked (below)
- [x] FFmpeg encoder options researched from source (`docs/research/encoders.md`)
- [x] Cargo workspace building (Meson build landed in M2)

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
- [x] Window shell, empty state, drag-and-drop, file chooser, open-with, CLI args
- [x] Queue cards (thumbnail/waveform, wrapping tags, progress, hw badge, retry/show actions), persistence
- [x] Input analysis view (File Details: streams, HDR, command, log, copy diagnostics)
- [x] Simple preset cards + searchable format picker ("All Formats…", container defaults)
- [x] Progress / pause / cancel / notifications / suspend inhibit
- [x] Preferences (incl. ffmpeg path) + hardware matrix + System Check
- [x] App icon (full color + symbolic)
- [x] Adaptive layout: sidebar collapses at 760sp, compact cards at 520sp (tested at 380 px)
- [x] Headless screenshot harness + review pass (`scripts/screenshots.sh`, `docs/screenshots/`)
- [x] GUI end-to-end conversion test under Xvfb (`scripts/e2e-gui.sh`, in CI)
- [x] Meson build wrapping Cargo; desktop/metainfo/schema validated by `meson test`
- [x] i18n: gettext domain, `po/POTFILES.in`, core strings marked with `n_()`

### M3 — Advanced
- [x] Advanced video/audio/subtitle/other settings UI (container switch keeps compatible choices; codec lists from the container + this FFmpeg; encoders = verified hardware per device + software; pinning)
- [x] Default-track flags (`-disposition`), track selection by language, burn-in stream choice
- [x] Extra input/output args: quote-aware splitting (no shell), conflict warnings
- [x] Planner: full-GPU pipelines, hybrid, fallback chain with runtime retry (from M1, now reachable from the UI)
- [x] HDR preservation + tonemapping (libplacebo / tonemap_vaapi / zscale) with Auto / Keep / Convert choice
- [x] Preview sample (10 s from the middle of the trimmed range) + size/time estimate — GUI and `retort-cli convert --preview`
- [x] Target size (2-pass software; estimate capped at the limit)
- [x] Concurrency per engine + NVENC session-limit requeue
- [x] GUI e2e drives Advanced widgets (rotate, trim, mono) and verifies the output with ffprobe

### M4 — Packaging
- [x] Flatpak (GNOME 51, bundled FFmpeg 9.0.2 with x264/x265/SVT-AV1/aom/vpx/libplacebo/NVENC/QSV/VA-API/Vulkan) — built, installed and converting in a container
- [x] .deb (Ubuntu 24.04, Ubuntu 26.04, Debian 13; GUI + GTK-free `retort-cli`) and .rpm (Fedora 45, EL 10, openSUSE Tumbleweed, Leap 16) — each built in its distro's container, `meson test` passing, installed with the distro FFmpeg and run
- [x] AUR source (split), -git and -bin PKGBUILDs — source package built, namcap-checked, installed and run in Arch
- [x] Portable tarball (+ install.sh / uninstall.sh, relocatable) — built; AppImage (+ zsync) — see checklist note below
- [x] Nix flake (evaluated for x86-64/arm64 locally; built by the Packaging workflow), Gentoo ebuild, Void template (not built here: no Gentoo/Void build hosts)
- [x] Signed APT/RPM repositories (throwaway-key end-to-end tests: apt `signed-by` + InRelease, dnf/zypper `gpgcheck` + `repo_gpgcheck`)
- [x] `scripts/install.sh` (tested on Ubuntu 24.04, Fedora 45, Tumbleweed against a local mirror)
- [x] `scripts/rename.sh` (tested: rename → Cargo + Meson build → renamed binaries, desktop file, schema, metainfo)
- [x] CI: lint/test x86-64 + arm64 (from M1), Packaging smoke workflow, Release workflow on tags (all formats × both arches, GitHub release, Pages repositories)

### M5 — Docs and polish
- [x] README, INSTALL.md (generated from `docs/install/install.toml`), `scripts/test-docs.sh`
- [x] TESTING.md, MAINTAINER.md, CONTRIBUTING, Code of Conduct, SECURITY, issue forms
- [x] Documentation test run locally against a signed local mirror: every INSTALL section on every listed distribution, README/CONTRIBUTING/TESTING/MAINTAINER blocks (results below); the Release workflow runs it before publishing
- [x] Final screenshots + polish pass

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
| D20 | Tags in cards use a custom `WrapBox`, thumbnails a `FixedBin` | AdwWrapBox needs libadwaita 1.7 (baseline 1.5); a horizontally scrolling tag row hid the hardware badge; GtkPicture's natural size made thumbnails grow on taller cards | AdwWrapBox behind a feature flag only (no wrap on 1.5–1.6), GtkFlowBox (aligns tags into columns) |
| D21 | Our small label class is `.tag`, not `.pill` | libadwaita's `button.pill` shares the class name; our label style turned the Convert button grey | Higher-specificity selectors |
| D22 | Simple sidebar pins Speed / Save To / plan below a scrolling preset list | With 19 presets these were below the fold | Single scroller (options invisible), collapsing preset categories |
| D23 | Translations are merged into desktop/metainfo templates first, branding substituted afterwards (`build-aux/subst.py`) | Msgids keep `@APP_NAME@` placeholders, so a rename doesn't invalidate translations, and translated strings still match after substitution | configure_file before merge (substituted text never matches the catalogue) |
| D24 | Core marks catalogue strings with a no-op `n_()`; the app translates at display time | Core stays free of gettext; xgettext (C parser, gettext < 0.24 has no Rust) extracts with `--keyword=n_` | gettext dependency in core |
| D25 | The copyable command shows the final output name; the log shows the argv that ran (temp name) | Copy-pasting the command must produce the file the user expects; the log stays exact | Showing the temp-file command (confusing when copied) |
| D26 | Advanced edits turn the scope's preset into `format:<container>` | Simple mode then shows "custom MP4" instead of pretending a preset is still selected | Keeping the preset id (misleading), a separate "modified" flag |
| D27 | Codec lists for a container are computed on a worker and cached per (container, kind) | Uncurated containers need a test mux the first time (seconds) | Computing in `sync` (froze the UI) |
| D28 | Preview samples run at the full job's planned bitrate (target size → VBR at the planned kb/s) and the size estimate is capped at the limit | A 10-s target-size sample would otherwise get the whole budget; short samples over-represent container overhead (2.10 MB estimate vs 1.97 MB real for a 2 MB limit) | Re-planning the sample as its own target-size job |
| D29 | One shared sidebar footer (speed, plan + Preview, remux suggestion in Simple only); destination is a one-line strip under the scope | Advanced needs the vertical space; both modes show the same plan and preview | Separate footers per mode |
| D30 | Distro packages build with a rustup toolchain and vendored crates | MSRV 1.92 is newer than Debian 13/Ubuntu 24.04/EL 10's rustc; vendoring makes builds offline and reproducible | Distro rustc (too old), packaging every crate (not feasible outside the distros) |
| D31 | Separate `retort-cli` package without GTK dependencies | Headless servers (Proxmox) should not pull GTK | One package with both binaries |
| D32 | RPMs require `/usr/bin/ffmpeg` (and ffprobe), not a package name | Works with Fedora `ffmpeg-free`, RPM Fusion, EPEL, openSUSE OSS and Packman alike | Requiring `ffmpeg` (missing on Fedora by default) |
| D33 | Flatpak builds x264, x265 (multilib 8/10/12-bit), zimg, libass, libvpl, libplacebo 7.360.1 and nv-codec-headers n12.2; no AMF | The GNOME 51 SDK lacks them (it has SVT-AV1, aom, vpx, dav1d, opus, lame, VA-API, Vulkan, shaderc); its ffnvcodec 13.1 needs newer drivers than D17 allows; AMD's AMF runtime is not available in the sandbox | org.freedesktop.Platform.codecs-extra (FFmpeg version not ours) |
| D34 | Flatpak permissions: `--device=all`, home + removable media | NVENC/CUDA need `/dev/nvidia*`; outputs go next to the inputs, which the document portal cannot grant | `--device=dri` only (no NVENC), portal-only access (no "next to the original") |
| D35 | AppImage built on Ubuntu 24.04 (glibc 2.39), bundling GTK/libadwaita/FFmpeg but not libva/libdrm/libvulkan/libvpl | Host GPU drivers must match the host's loader libraries; older systems use the Flatpak | Building on an old base (would require building GTK 4.14/libadwaita 1.5 from source) |
| D36 | Update check only in AppImage/tarball builds (Cargo feature), off by default, daily, via the system `curl` | Spec: opt-in, those two formats only; no TLS stack in the binary | A Rust HTTP client in every build |
| D37 | The app finds its schema, icons and translations next to the executable when the build-time paths don't exist | Portable tarball and AppImage are relocatable | Wrapper scripts setting environment variables |
| D38 | `docs/INSTALL.md` is generated from `docs/install/install.toml`; `scripts/test-docs.sh` runs every documented command in containers against a local signed mirror | One source for docs and tests; GPU driver installs are resolved (`apt-get -s`, `dnf --assumeno`, …), not installed; untestable commands are listed with reasons | Hand-written INSTALL.md with a separate test list |
| D39 | Stable repository names: APT suites by codename, RPM repos `fedora-45`, `el-10`, `opensuse-tumbleweed`, `opensuse-leap-16.0` | `install.sh` and docs can compute them; Tumbleweed's VERSION_ID is a date | `$ID-$VERSION_ID` everywhere |

## Findings worth knowing (from real FFmpeg / distro checks)

- FFmpeg 9.0 dropped legacy NVENC presets/rc modes (verified in source); we only emit `p1–p7`, `-tune`, `-rc vbr|cbr|constqp`.
- FFmpeg 9.0 needs libplacebo ≥ 7.351 and Vulkan headers ≥ 1.4.317 (for `av1_vulkan`); `scale_cuda` needs `--enable-cuda-llvm`.
- FFmpeg 9.0 ignores `-color_primaries/-color_trc` output options unless frames carry the same tags (D13).
- ffprobe 8+/9 reports animated WebP as codec/demuxer `webp_anim`.
- Fedora 45 folded VA-API drivers into `mesa-dri-drivers` (encode-crippled); the fix is now `dnf install mesa-va-drivers-freeworld` (not `swap`). Ubuntu 26.04 made `mesa-va-drivers` a virtual package (`mesa-libgallium`).
- RPM Fusion has no NVIDIA driver for EL10; NVIDIA's own `cuda-rhel10.repo` does.

- libadwaita: an `AdwStatusPage` inside a preferences group clips its icon and loses the title (it brings its own scroller); System Check uses a plain header instead.
- xgettext's C fallback parser warns about Rust lifetimes (`'static`) but still extracts every marked string (checked by script against all `gettext/tr_fmt/ntr_fmt/n_` literals).
- `GSETTINGS_BACKEND=memory` plus temporary XDG dirs fully isolate a GUI test run.

- Packaging (M4): `dh_clean` deletes `*.orig` files — vendored crates contain `Cargo.toml.orig` covered by checksums (`dh_clean -Xvendor/`); debhelper 13 runs with its own `HOME`, hiding rustup (`RUSTUP_HOME`); `cargo vendor` prints an absolute path; openSUSE's rpmbuild uses `/usr/src/packages` (`--define _topdir`); `meson install` re-runs the always-stale Cargo target unless `--no-rebuild`; Ubuntu's rpmsign needs `--define "__gpg …"`.
- Flatpak's `/tmp` is always private (even with `--filesystem=/tmp`); System Check reads granted devices from `/.flatpak-info` instead of inferring from `/dev/dri`.

## Documentation test (last local run, 2026-11-12)

`scripts/test-docs.sh` against a local signed mirror built from the packages above:

- PASS: quick install script on Ubuntu 24.04, Fedora 45, Tumbleweed; Flatpak bundle; APT on Ubuntu 24.04, 26.04, Debian 13 (incl. non-free + Intel/AMD driver installs); Fedora 45 (incl. RPM Fusion swap, Intel/AMD drivers); EL 10 (EPEL, CRB, RPM Fusion); Tumbleweed and Leap 16 (incl. Packman FFmpeg swap); Arch driver packages; AppImage (GUI started under Xvfb); portable tarball (install + uninstall); build from source; README CLI examples; CONTRIBUTING (fmt, clippy, tests, integration, screenshots, translation template); TESTING; MAINTAINER (key creation, version check, dist tarballs, rename).
- Resolved only (no GPU in containers): NVIDIA driver packages on Debian 13, Fedora 45, EL 10, Arch.
- Skipped with reasons in the report: `ubuntu-drivers install` (needs NVIDIA hardware); AUR helper/`makepkg` lines (the same PKGBUILD is built, namcap-checked, installed and run by `packaging/arch/build.sh`); `nix run` (the Packaging workflow builds the flake); `cargo insta review` (interactive); release tagging/pushing and Docker-based packaging commands in MAINTAINER.md (run by the workflows).
- Bugs it found and that are fixed: Leap 16 Packman `ffmpeg-8` conflicts with OSS `ffmpeg-7` (System Check's fix was wrong too); `sudo meson install` re-ran Cargo as root without rustup on PATH; `install.sh` printed the distro name instead of the app name.

## Open questions (for the maintainer)

- One-time setup before the first tag (details in MAINTAINER.md): enable GitHub Pages (source: GitHub Actions) and add the `REPO_SIGNING_KEY` secret; without it the Pages job of the Release workflow fails on purpose.
- Publishing to the AUR, Flathub, GURU and void-packages needs your accounts; the files are ready (`packaging/`), steps in MAINTAINER.md.
- Real hardware reports (below) will replace the synthetic profiles.

## Hardware test list

### After M4/M5 (packages)

Install Retort the way your users will, then repeat the GUI checks from M2/M3:

| Machine | Install with | Check |
|---|---|---|
| Desktop (RTX 4090) | Flatpak bundle from a release (`Retort-x86_64.flatpak`) | Flatpak pulls `org.freedesktop.Platform.GL.nvidia-<driver>` automatically; System Check shows NVENC verified; AV1 NVENC encode works inside the sandbox. Then the .deb/.rpm for your distro and compare. |
| Dell 5570 (Iris Xe + NVIDIA) | `install.sh` (APT or RPM repository) | Intel VA-API/QSV need the driver packages INSTALL.md lists; check that `org.freedesktop.Platform.VAAPI.Intel` is installed with the Flatpak. |
| Proxmox (Intel iGPU + GTX 1660 Ti) | `retort-cli` package only (no GTK pulled in) | `apt install retort-cli` / `dnf install retort-cli`; `retort-cli diagnose`, `probe-hw`, a batch `convert`. |
| M1 MacBook Air (Asahi, arm64) | AppImage `Retort-aarch64.AppImage` and the Fedora arm64 RPM | Both start; everything on CPU; arm64 Flatpak bundle too. |

### After M3 (Advanced mode, preview)

On each machine, with a real HDR10 file, a multi-audio MKV (e.g. a film with two languages + subtitles) and a phone video:

1. Advanced → Encoder: the list shows ⚡ entries only for encoders that passed the probe, one per GPU (Dell/Proxmox: Intel and NVIDIA separately). Pin one, convert, check the badge and `nvidia-smi`/`intel_gpu_top`. Pin an encoder that can't do 10-bit with a 10-bit source and check the error offers "Retry with Software".
2. HDR file: HDR → "Keep" with HEVC/AV1 (MKV/MP4) keeps HDR10 (`retort-cli analyze` the output); "Convert to SDR" looks right (no washed-out colours) — note which tonemapper the plan used (libplacebo / tonemap_vaapi / CPU).
3. Multi-audio MKV: Default Track → second language; play the result in mpv/VLC and check which track starts. Subtitles → Burn In a chosen track.
4. Preview on a long file: estimate vs the real size after converting; with "Fit under a size limit" the real file must be under the limit.
5. Container change (e.g. MKV → WebM): codec choices that fit are kept, others switch to defaults.
6. Desktop (RTX 4090): AV1 NVENC 10-bit pinned + Preview; HEVC NVENC with Rate Control → Constant Bitrate.

### After M2 (GUI)

Build and run from the tree: `meson setup _build && meson compile -C _build && meson devenv -C _build retort`
(or install: `meson setup _build --prefix=/usr/local && meson compile -C _build && sudo meson install -C _build --no-rebuild`).

On **each machine** (light and dark style, and once at narrow width):

1. First start: the header shows "Testing hardware acceleration…" briefly; then **Preferences → Hardware** must list exactly the GPUs you have, and the matrix must match `retort-cli probe-hw`.
2. Drop a folder of mixed files on the window: every card gets a thumbnail (or a waveform for audio), correct tags (resolution, codec, bit depth, HDR, channels, duration), and a ⚡ badge only where the probe passed.
3. Convert with **Fastest** and watch the badge/plan in the sidebar; pause/resume one card, cancel another, and check the notification when the batch ends (with the window unfocused) and that the machine doesn't suspend while converting.
4. Menu → **System Check**: every finding should be true for your machine; copy one fix command and check it is right for your distro.
5. File Details (double-click a card): HDR metadata and codec levels look right; "Copy FFmpeg Command" pastes a command that works in a terminal.
6. Close the window with files in the queue and reopen: the queue comes back.

| Machine | GUI-specific checks |
|---|---|
| Desktop (RTX 4090) | Start 4+ NVENC jobs at once (Preferences → max jobs) and confirm requeueing instead of failures when the session limit hits; HDR10 source → "Smaller MP4" keeps the HDR10 tag on the card of the result. |
| Dell 5570 (Iris Xe + NVIDIA) | "Use only this GPU" set to the Intel GPU really encodes on Intel (`intel_gpu_top`); the plan row in the sidebar names the right device; dGPU asleep vs awake. |
| Proxmox (Intel iGPU + GTX 1660 Ti) | (If you have a desktop session there.) AV1 must show CPU only; NVENC HEVC 10-bit works. Otherwise skip — the CLI checks from M1 cover it. |
| M1 MacBook Air (Asahi) | arm64 build; everything shows CPU; the narrow layout on the small screen; System Check explains there is no hardware encoder. |

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
