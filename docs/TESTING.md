# How Retort is tested

Retort's promise — "hardware acceleration only where a real test proved it
works, and a working fallback everywhere else" — is only as good as its
tests. Most of them run without any GPU, by simulating hardware from real
machines' recordings.

## Layers

| Layer | What | Where | Runs in CI |
|---|---|---|---|
| Unit tests | parsers (FFmpeg version/buildconf/caps, ffprobe JSON incl. HDR/Dolby Vision/VFR/rotation), quality mapping per encoder, naming and conflicts, presets, preview maths, update-check parsing, argument splitting | `crates/core/src/**` | yes |
| Snapshot tests (insta) | full FFmpeg command lines for every Simple preset and many Advanced combinations, on every simulated machine, including the whole fallback chain | `crates/core/tests/commands.rs`, `snapshots/` | yes |
| Simulated hardware | device discovery from fake `/sys`/`/dev`/`nvidia-smi` trees and probe outcomes per machine (NVIDIA-only, Intel-only, AMD, hybrid laptop, Raspberry Pi, broken drivers, …) | `tests/fixtures/hw/<machine>/`, `crates/core/tests/profiles.rs` | yes |
| System Check | the doctor's findings and distro-specific fix commands for each simulated machine | `crates/core/tests/doctor.rs` | yes |
| Integration | real conversions with the real `ffmpeg`: every Simple preset, HDR keep/tonemap, awkward inputs (VFR, odd dimensions, interlaced, rotated, cover art, still images), nasty file names, never-overwrite, timestamps, runtime fallback from a broken hardware plan, pause/cancel, default-track flags, preview samples | `crates/core/tests/integration.rs` | yes — against the distro FFmpeg (6.1) on x86-64 and arm64, and against the latest FFmpeg (9.0.x) built from source |
| GUI end-to-end | the real app under Xvfb converts files (Simple), then drives Advanced widgets (rotate, trim, mono) and the outputs are checked with ffprobe | `scripts/e2e-gui.sh` | yes |
| Screenshots | every major state in light and dark, plus phone width, rendered headlessly for review | `scripts/screenshots.sh` → `docs/screenshots/` | yes (artifact) |
| Packages | each package is built in a container of its distribution, `meson test` validates the desktop file, AppStream metadata and schema, then the package is installed with the distro's FFmpeg and run | `packaging/*/build.sh`, Packaging and Release workflows | yes |
| Documentation | every command in the docs runs in a fresh container of each listed distribution, as a normal user, on a copy of the checkout (install, update, remove) | `scripts/test-docs.sh` | before releases; native sections also in the Packaging workflow |

## Running the tests

Fast engine tests:

<!-- doc-test: context=dev -->
```bash
cargo test -p retort-core
```

Everything CI runs for code (needs GTK 4 and libadwaita development files):

<!-- doc-test: context=dev -->
```bash
cargo test --workspace
```

Integration tests with the `ffmpeg` on your PATH:

<!-- doc-test: context=dev -->
```bash
cargo test -p retort-core --test integration -- --include-ignored
```

Review snapshot changes after changing the planner or command builder
(`cargo install cargo-insta` once):

<!-- doc-test: skip interactive review; CI fails on any unreviewed snapshot change -->
```bash
cargo insta review
```

GUI end-to-end test and screenshots (Xvfb):

<!-- doc-test: context=dev -->
```bash
scripts/e2e-gui.sh
```

The documentation test needs Docker. Install sections run as a normal user
with sudo on a copy of this checkout (in place of `git clone`), so they test
the code you have, including the second build of the update step. The
AppImage, tarball and Flatpak sections also need those release files in
`dist/` (see [MAINTAINER.md](MAINTAINER.md)). Arguments pick sections,
images, `section@image` pairs, documents or contexts:

<!-- doc-test: skip runs Docker itself; this is the documentation test -->
```bash
scripts/test-docs.sh
```

## Testing on hardware we don't have

Each folder in `tests/fixtures/hw/` is a complete simulated machine: a fake
root file system (`/sys/class/drm`, `/dev/dri`, `/proc/driver/nvidia`,
`/etc/os-release`, …), the recorded output of that machine's `ffmpeg`
(`-encoders`, `-h encoder=…`, …) and `profile.json`, which says which probe
commands succeed there. The prober builds the same argument lists it would
run for real; the simulator answers from the profile. Planner, command
builder and System Check then run unchanged.

Real machines become fixtures with one script, run on that machine:

<!-- doc-test: context=dev -->
```bash
scripts/collect-hw-report.sh
```

Its output folder can be copied to `tests/fixtures/hw/<machine>/` as is; the
next `cargo test` covers that machine, and the snapshots show exactly which
commands Retort would run there.

To see what Retort would do on a simulated machine:

<!-- doc-test: context=dev -->
```bash
cargo run -q -p retort-cli -- probe-hw --simulate tests/fixtures/hw/hybrid-laptop
```

## What still needs real hardware

Simulation proves the logic; only real GPUs prove the drivers. The list of
checks per machine is in [PROGRESS.md](PROGRESS.md#hardware-test-list).
