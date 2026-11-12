# CLAUDE.md — working on Retort

Retort is a GTK 4 + libadwaita FFmpeg converter for Linux (x86-64 + arm64),
GPL-3.0-or-later. Requirements: `docs/SPEC.md` (source of truth). Status,
decisions and next steps: `docs/PROGRESS.md` (read it first, update it at the
end of every milestone). Code map: `docs/ARCHITECTURE.md`.

## Layout

- `crates/core` (`retort-core`): engine, no GTK. Put logic and tests here.
- `crates/cli` (`retort-cli`): headless CLI over the engine.
- `crates/app` (`retort`): GTK UI.
- `branding.env`: the only place the app name / ID / binary name are defined.
  `scripts/rename.sh` propagates a rename to static packaging files.

## Commands

```bash
# Build / test / lint (what CI runs)
cargo build --workspace
cargo test --workspace
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo deny check            # if cargo-deny is installed
scripts/check-no-shell.sh   # forbids shell invocation in Rust code

# Engine-only (fast loop, no GTK needed)
cargo test -p retort-core
cargo run -p retort-cli -- diagnose
cargo run -p retort-cli -- convert in.mkv --preset mp4-compatible -o out.mp4 --dry-run

# Snapshot tests (insta)
cargo insta test -p retort-core       # or: INSTA_UPDATE=always cargo test -p retort-core
cargo insta review

# Integration tests that run the real ffmpeg (needs ffmpeg in PATH)
cargo test -p retort-core --test integration -- --include-ignored

# Full Meson build + install to a prefix
meson setup _build --prefix=$PWD/_install && meson compile -C _build && meson install -C _build

# Run the GUI from the build tree
meson devenv -C _build retort       # or: cargo run -p retort (build.rs compiles a dev schema)

# Headless screenshots (Xvfb) and GUI end-to-end conversion test
scripts/screenshots.sh              # writes docs/screenshots/*.png (pngquant if installed)
scripts/e2e-gui.sh                  # real conversion through the GUI, isolated settings

# Translations template
meson compile -C _build retort-pot  # writes po/retort.pot (not committed)

# Packages (Docker; each builds in its distro's container, installs, smoke-tests)
scripts/make-dist.sh                 # dist/retort-<v>.tar.xz + -vendor.tar.xz (--dirty: working tree)
packaging/debian/build.sh ubuntu:24.04   # also ubuntu:26.04, debian:trixie
packaging/rpm/build.sh fedora:45         # also almalinux:10, opensuse/tumbleweed, opensuse/leap:16.0
packaging/arch/build.sh | packaging/tarball/build.sh | packaging/appimage/build.sh
scripts/check-versions.sh            # every stated version agrees with Cargo.toml

# Docs: INSTALL.md is generated; every documented command is tested
scripts/gen-install.py               # docs/install/install.toml -> docs/INSTALL.md
scripts/test-docs.sh [section|image|doc|context]   # needs dist/ packages; logs in dist/doc-test/
```

New ```bash blocks in README/CONTRIBUTING/TESTING/MAINTAINER need a
`<!-- doc-test: context=… -->` or `<!-- doc-test: skip reason -->` line
before them (CI fails otherwise). Behind the sandbox's TLS proxy, Docker-based
scripts need `DOCKER_ARGS="--network host -v /root/.ccr/ca-bundle.crt:/ca.crt:ro -e HTTPS_PROXY -e https_proxy -e HTTP_PROXY -e http_proxy"`.

Meson enables the `adw_1_6/1_7/1_8` Cargo features from the installed
libadwaita version; code behind them must also have a 1.5 fallback. Check
the newest path with `cargo check -p retort --features adw_1_8` on a distro
with libadwaita ≥ 1.8 (CI job `latest-gnome`).

## Conventions

- Rust stable, edition 2021, MSRV 1.92 (gtk4-rs 0.11 requirement).
- **Never invoke a shell.** argv arrays only; media paths go through
  `retort_core::paths::ffmpeg_url()` (absolute + `file:` prefix).
- Never modify inputs; never overwrite outputs silently (`-n` unless the user
  chose "overwrite").
- Hardware support is data-driven: candidates from ffmpeg, confirmed by a real
  probe. Never assume support from the vendor.
- New FFmpeg option usage must be verified against FFmpeg source for 6.1 →
  latest and recorded in `docs/research/encoders.md`.
- Errors: `thiserror` in core, `anyhow` in binaries. No `unwrap()` on
  user-data paths.
- User-facing strings in the app go through gettext (`gettext("…")`,
  `tr_fmt`, `ntr_fmt`); user-visible literals in core are wrapped in
  `retort_core::i18n::n_()` and translated by the app with `i18n::tr()`.
  New source files with strings go into `po/POTFILES.in`.
- Commit messages: imperative, scoped (`core: …`, `app: …`, `cli: …`,
  `packaging: …`, `docs: …`, `ci: …`). Small commits.
- Keep `docs/PROGRESS.md` current: milestone checklist, decisions log
  (decision / why / alternatives rejected), open questions, hardware test list.

## Environment notes (cloud sandbox)

- Ubuntu 24.04, no GPU, no display. Use Xvfb for GUI runs.
- `dockerd` is not started automatically: `(nohup dockerd >/tmp/dockerd.log 2>&1 &)`.
- `apt-get update` is needed before installing packages in a fresh session.
- Required apt packages for building: `libgtk-4-dev libadwaita-1-dev meson
  gettext appstream desktop-file-utils ffmpeg xvfb`.
