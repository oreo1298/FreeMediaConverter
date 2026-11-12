# Retort — Architecture

Retort is a GTK 4 / libadwaita front-end for FFmpeg that converts any media
file FFmpeg can read into any format it can write, using hardware acceleration
only where a real test encode/decode proved it works. This document is the map
of the code; `docs/SPEC.md` is the requirements; `docs/PROGRESS.md` is the
status and decision log.

## 1. Repository layout

```
branding.env              Single source of truth for name / app ID / binary name
Cargo.toml                Cargo workspace (crates/*)
meson.build               Top-level build (wraps cargo, installs data files)
crates/
  core/   retort-core     Engine. No GTK. ~all logic and most tests live here.
  cli/    retort-cli      Headless binary: diagnose, probe-hw, list-formats, convert, analyze
  app/    retort          GTK 4 + libadwaita UI
data/                     .desktop, metainfo, gschema, icons, CSS (templated with @APP_ID@)
po/                       gettext translations
tests/fixtures/           Recorded ffmpeg outputs + simulated hardware profiles
packaging/                flatpak, debian, rpm, arch, appimage, tarball, gentoo, void
flake.nix / nix/          Nix package, overlay, NixOS / home-manager modules
scripts/                  install.sh, collect-hw-report.sh, test-docs.sh, release helpers
docs/                     SPEC, ARCHITECTURE, PROGRESS, INSTALL, TESTING, MAINTAINER, research/
```

## 2. The engine (`retort-core`)

```
                    ┌──────────────┐
  ffmpeg binary ───▶│ ffmpeg::Tool │  locate + version + buildconf (+ hash)
                    └──────┬───────┘
                           │ Runner trait (real process | recorded fixtures)
          ┌────────────────┼─────────────────────┐
          ▼                ▼                     ▼
  ffmpeg::caps        media::probe          hw::devices (sysfs, /dev, nvidia-smi)
  (-encoders,         (ffprobe JSON →            │
   -decoders,          MediaInfo)                ▼
   -hwaccels,              │              hw::probe (real test encodes/decodes,
   -muxers, -filters,      │               parallel, timeouts) ──▶ hw::cache
   -h encoder=X)           │                     │
          │                │                     ▼
          ▼                │              HwInventory: verified (backend, device,
  catalog (formats,        │               codec, encode/decode, bit depth, rc modes)
   codecs, compat) ────────┤                     │
                           ▼                     ▼
                 settings::OutputSettings ──▶ plan::Planner ──▶ Vec<Plan> (fallback chain)
                 (preset + overrides)                               │
                                                                    ▼
                                                     command::build(plan) → argv
                                                                    │
                                                                    ▼
                                         job::Runner (spawn, -progress parse, pause,
                                          cancel, log, automatic fallback retry)
```

### 2.1 Process invocation (`ffmpeg::runner`)

* Every external program is started with `std::process::Command` and an argv
  vector. **No shell anywhere.** A CI grep check (`scripts/check-no-shell.sh`)
  fails if `sh -c`/`bash -c` appear in Rust sources.
* All media paths are passed through `paths::ffmpeg_url()`, which makes them
  absolute and prefixes `file:` so names like `-x.mp4`, `a:b.mkv`, names with
  quotes/newlines/Unicode are never parsed as options or protocols. For the
  image2 muxer `%` is escaped as `%%`.
* `Runner` is a trait. `SystemRunner` runs processes (with timeout and kill);
  `FixtureRunner` answers from a recorded directory so discovery, probing and
  planning are unit-testable without a GPU.
* Every ffmpeg child gets `CUDA_DEVICE_ORDER=PCI_BUS_ID` so CUDA indices match
  `nvidia-smi`, and `LC_ALL=C` so output is parseable.

### 2.2 Discovery and capabilities (`ffmpeg::*`)

`Tool::locate()` resolution order: explicit path (Preferences / `--ffmpeg`) →
`$RETORT_FFMPEG` → bundled (`/app/bin`, AppImage `$APPDIR/usr/bin`) → `$PATH`.
`Caps` holds parsed `-encoders`, `-decoders`, `-hwaccels`, `-muxers`,
`-demuxers`, `-filters`, `-pix_fmts`, `-buildconf` and on-demand
`-h encoder=X` (supported pixel formats, hw device types, private options).
`Version` understands release (`9.0.2`), distro (`6.1.1-3ubuntu5`), git
(`N-12345-gabcdef`, `n9.1-dev-…`) and jellyfin (`7.1.1-Jellyfin`) strings and
exposes feature gates (`supports_vulkan_h264_encode()` etc.). Gates prefer
probing the actual option list from `-h encoder=X` over version numbers.

### 2.3 Media analysis (`media::*`)

`ffprobe -v error -print_format json -show_format -show_streams
-show_chapters` → `MediaInfo` (container, duration, size, bitrate, streams,
chapters, attachments, tags). Derived facts: bit depth from pix_fmt, HDR type
(PQ → HDR10 / HDR10+ if dynamic metadata, HLG, Dolby Vision from the DOVI
side data), VFR heuristic (`r_frame_rate` vs `avg_frame_rate`), interlacing
hint (`field_order`), audio layout. Thumbnails and waveform peaks are produced
by short ffmpeg runs writing to a pipe.

### 2.4 Catalog (`catalog::*`)

* `formats`: every muxer FFmpeg reports, grouped (Popular, Video, Audio,
  Images/Animation, Professional/Broadcast, Other) with human descriptions and
  default extensions; device/null/network-only muxers hidden unless "Show all".
* `codecs`: codec families, encoder metadata (hardware backend, software
  flag, bit depths), "best encoder for codec X" ordering.
* `compat`: curated container × codec table for popular containers; anything
  else is validated empirically with a ~0.1 s lavfi test mux to `-f <muxer>`
  and cached alongside the hardware cache. The UI only offers combinations
  that are known-good.

### 2.5 Hardware (`hw::*`)

* `devices`: scans an injectable root (`/` in production, fixture trees in
  tests): `/sys/class/drm/renderD*/device/{vendor,device,uevent,driver}`,
  `/dev/dri`, `/dev/nvidia*`, `/proc/driver/nvidia/gpus/*/information`,
  `nvidia-smi --query-gpu=…`, `/sys/class/video4linux/*` (M2M by capability
  name). PCI names come from `pci.ids` when present. Permission problems
  (render node not readable/writable, `/dev/nvidia*` absent) are recorded.
* `Backend` = NVENC/CUDA, VAAPI, QSV, Vulkan, AMF, V4L2M2M, RKMPP, Other.
  Each backend knows how to (a) build `-init_hw_device` for a given physical
  device, (b) upload/download frames, (c) name its scale/tonemap filters,
  (d) map rate control (via `quality`).
* `probe`: for each candidate (backend, device, encoder) a real encode of
  `testsrc2` 640×360, 8 frames, 8-bit and 10-bit, to `-f null`; for each
  hardware decoder a decode of embedded tiny clips (`data/probe-clips/`,
  generated by `scripts/gen-probe-clips.sh`) followed by `hwdownload` so a
  silent software fallback counts as failure. Rate-control modes are probed
  per encoder. Probes run in parallel (bounded) with per-probe timeouts.
* `cache`: JSON in `$XDG_CACHE_HOME/retort/hw-<key>.json`. The key hashes
  ffmpeg path + version + buildconf, kernel release, GPU driver versions and
  the device list. Any change → automatic re-probe.

### 2.6 Quality mapping (`quality`)

One table-driven module maps the unified 0–100 quality slider (and the
explicit modes CRF/CQ/QP/VBR/CBR/target size) to each encoder's native
controls, with ranges per FFmpeg version (see `docs/research/encoders.md`).
Exhaustively unit-tested.

### 2.7 Settings, presets, naming

* `settings::OutputSettings` is the full description of an output: container,
  per-stream-type `VideoSettings` / `AudioSettings` / `SubtitleSettings`
  (each `Copy`, `Drop` or `Encode{…}`), filters (scale, fps, crop, trim,
  rotate, deinterlace, tonemap, denoise, sharpen, loudnorm), metadata policy,
  extra args, speed preference, pinned encoder/device.
* `preset`: Simple-mode presets are just named `OutputSettings` builders.
* `naming`: `{name} {ext} {codec} {res} {date}` templates, output folder,
  conflict policy (rename/skip/ask/overwrite), timestamp preservation.

### 2.8 Planner (`plan`)

Given `MediaInfo`, `OutputSettings` and the verified `HwInventory`, produce an
ordered `Vec<Plan>` — the fallback chain. A `Plan` names decode / filter /
encode stages and the device for each:

1. Full GPU: hw decode → hw filters → hw encode on one device (zero-copy with
   `-hwaccel_output_format`), only if every requested filter has a hw
   equivalent there and the decoder probe passed for the input codec/depth.
2. Hybrid: hw decode → `hwdownload` → CPU filters → `hwupload` → hw encode,
   or software decode → hw encode.
3. Another verified backend for the same codec.
4. Software.

Constraints honoured: encoder bit depth / pixel formats (10-bit input to an
8-bit-only encoder → convert and say so), resolution limits, odd dimensions,
B-frames, HDR preservation (color tags + mastering/CLL where the encoder
supports it), "Fastest/Balanced/Smallest" ranking, user-pinned backend.
Each plan carries a human summary ("Decode: NVDEC · Filters: CUDA · Encode:
NVENC AV1 on RTX 4090").

### 2.9 Command builder (`command`)

Pure function `Plan + OutputSettings + MediaInfo + paths → CommandSet`
(one or more argv vectors: loudnorm measurement, pass 1, final encode).
Colour properties are tagged with `setparams` in the filter chain because
FFmpeg 8/9 take them from frames (D13).
Snapshot-tested with `insta` across inputs × presets × simulated hardware
profiles. Always emits `-hide_banner`, `-n` (never overwrite; conflicts are
resolved by Retort before the job starts, and an explicit "overwrite" policy
switches to `-y`), `-progress pipe:1 -nostats`, explicit `-map`s and
`-f <muxer>`. Stdin stays a pipe (not `-nostdin`) so a graceful cancel can
send `q`.

### 2.10 Jobs (`job`, `progress`, `queue`)

* Outputs go to a hidden temp file (`.<name>.retort-<pid>-<n>.part.<ext>`) in
  the destination folder and are renamed into place on success (decision D12).
* `job::start` spawns ffmpeg with stdin/stdout/stderr piped; a reader thread
  parses `-progress` key/value blocks (`progress::Parser`) into
  `Progress{percent, fps, speed, eta, size}`; stderr is captured to a per-job
  log file. Pause/resume = `SIGSTOP`/`SIGCONT`. Cancel = write `q` to stdin →
  wait 3 s → `SIGTERM` → wait 3 s → `SIGKILL`. Partial outputs are removed on
  cancel/failure (configurable).
* Automatic fallback: on a non-zero exit of a hardware plan (not cancelled),
  the job re-runs with the next plan of the chain and is marked
  "Completed (software fallback)" with the reason (last stderr lines). If the
  user pinned the encoder, the job fails with a "Retry with software" action.
* `queue::Scheduler` runs jobs with per-resource concurrency: one slot per
  hardware encode engine + one CPU slot by default; NVENC session-limit
  errors re-queue instead of failing.
* The queue persists to `$XDG_STATE_HOME/retort/queue.json`.

### 2.11 Engine facade (`engine`)

`Engine` bundles `Ffmpeg`, `Caps`, the format catalog, `Compat` and the
`Inventory` (all cheap to clone) and offers `analyze`, `plan`, `output_path`,
`request` and `probe_hardware`, plus the Advanced-mode helpers
`codec_options` (container ∩ this FFmpeg), `encoder_options` (verified
hardware per device, then software), `settings_for_format` and `retarget`
(switch container, keep what still fits). The CLI and the GUI only talk to this.

### 2.11a Preview (`preview`)

`sample_settings` trims the job to 10 s from the middle of the output range
(target size → the planned bitrate as VBR); `estimate` scales the sample's
size and encode time to the whole output, capped at a target-size limit.
Used by the GUI's Preview button and `retort-cli convert --preview`.

### 2.11b Translations (`i18n`)

Core has no gettext dependency: user-visible catalogue literals are wrapped
in the no-op `n_()` so xgettext extracts them; the app translates at display
time (`i18n::tr`).

### 2.12 Doctor (`doctor`, `distro`)

Checks produce `Finding{severity, title, explanation, fix_commands}`; fix
commands are chosen from a table keyed by distro family (`ID`/`ID_LIKE`) and
were verified in containers (see `docs/research/distro-packages.md`). Exposed
as the System Check page and `retort-cli diagnose [--json]`.

## 3. The CLI (`retort-cli`)

Thin layer over `retort-core` (clap). No GTK dependency, so it runs on headless
servers. Subcommands: `diagnose [--json]`, `probe-hw [--json] [--force]`,
`list-formats [--all] [--json]`, `analyze <file> [--json]`,
`convert <in>… --preset <id> [-o <out>] [--dry-run | --preview] [overrides]`,
`presets`.

## 4. The GUI (`retort`)

GTK 4.14+/libadwaita 1.5+ built in Rust code (no Blueprint, to keep distro
build dependencies minimal). Newer libadwaita widgets (e.g. `AdwSpinner` 1.6,
`AdwToggleGroup` 1.7, `AdwShortcutsDialog` 1.8) are used behind Cargo
features that Meson enables after checking the installed libadwaita version;
fallbacks use 1.5 widgets.

* `AdwApplicationWindow` → `AdwToastOverlay` → `AdwOverlaySplitView`
  (content: queue / empty state; sidebar: settings for the selection) with
  `AdwBreakpoint`s: the sidebar becomes an overlay below 760 sp, cards turn
  compact below 520 sp.
* Modules: `window` (shell, queue, analysis), `card` (queue card), `sidebar`
  (scope, Simple presets, shared footer), `advanced` (every setting),
  `picker` (format grid), `runner` (batch, limits, notifications), `preview`,
  `details`, `prefs` (incl. hardware matrix), `syscheck`, `wrapbox`
  (`WrapBox`/`FixedBin` layout widgets), `demo` (screenshot + e2e harness).
* Edits go through `sidebar::apply_to_scope` (selected files, else all files
  and the defaults for new files); `sync_from_state`/`advanced::sync`
  reflect the scope back into the widgets with `ui.syncing` set.
* Engine work (probing, analysis, jobs) runs on worker threads; results come
  back to the main loop via `async-channel` + `glib::spawn_future_local`.
* Preferences are GSettings (`@APP_ID@.gschema.xml`).
* Notifications via `GNotification`; suspend inhibition via
  `gtk::Application::inhibit` (portal-backed in Flatpak); files via
  `gtk::FileDialog` / `gtk::FileLauncher` (portals).
* A headless screenshot harness (`RETORT_SCREENSHOT_DIR`) drives the UI
  through every major state and renders the window with GSK to PNG
  (`scripts/screenshots.sh`, runs under Xvfb). `RETORT_E2E` instead converts
  the files given on the command line through the real UI (and with
  `RETORT_E2E_ADVANCED` sets Advanced widgets first); `scripts/e2e-gui.sh`
  checks the outputs with ffprobe.

## 5. Testing strategy

| Layer | How |
|---|---|
| Parsers | Recorded outputs from ffmpeg 6.1 (sandbox) and 9.0 (CI) in `tests/fixtures/ffmpeg/` |
| Devices | Fake sysfs/dev trees per hardware profile in `tests/fixtures/hw/<profile>/` |
| Probing | `FixtureRunner` with per-profile pass/fail answers |
| Planner + command lines | `insta` snapshots: inputs × presets × profiles |
| Quality mapping | Table-driven exhaustive tests |
| Integration | Real software ffmpeg on lavfi-generated media incl. nasty filenames; outputs verified with ffprobe |
| GUI | Headless launch under Xvfb + screenshots |
| Docs | `scripts/test-docs.sh`: every install/update/uninstall command run in distro containers |
