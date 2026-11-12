# Contributing to Retort

Thanks for helping! Bug reports, hardware reports, translations and code are
all welcome.

## Reporting problems

- **Something went wrong in a conversion**: open the file's details (double-
  click the card), press *Copy Diagnostics* and paste it into a bug report.
  It contains the exact FFmpeg command and the end of the log.
- **Hardware acceleration missing or failing**: run *System Check* (main menu)
  and `retort-cli diagnose`, and attach a hardware report:

  ```bash
  scripts/collect-hw-report.sh
  ```

## Development

The code map is in [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md), the status
and decisions in [docs/PROGRESS.md](docs/PROGRESS.md). Most logic lives in
`crates/core` (no GTK), which has fast tests:

```bash
cargo test -p retort-core
```

Before sending a pull request, run what CI runs:

```bash
cargo fmt --all --check
```

```bash
cargo clippy --workspace --all-targets -- -D warnings
```

```bash
cargo test --workspace
```

```bash
scripts/check-no-shell.sh
```

Integration tests run the real `ffmpeg` (6.1 or newer):

```bash
cargo test -p retort-core --test integration -- --include-ignored
```

GUI changes: render the screenshots and look at them (light, dark, narrow):

```bash
scripts/screenshots.sh
```

Rules that keep Retort trustworthy:

- Never invoke a shell; run programs with argument arrays.
- Never assume hardware support: candidates come from FFmpeg, a real probe
  decides.
- New FFmpeg options must be checked against FFmpeg's source for every
  supported version (6.1 → latest) and recorded in `docs/research/encoders.md`.
- User-visible strings go through gettext.

Commit messages are imperative and scoped: `core: …`, `app: …`, `cli: …`,
`packaging: …`, `docs: …`, `ci: …`.

## Translations

Translations use gettext. Generate the template, then create or update your
language's file in `po/` and add its code to `po/LINGUAS`:

```bash
meson setup _build
```

```bash
meson compile -C _build retort-pot
```

## Code of conduct

Everyone taking part follows the [Code of Conduct](CODE_OF_CONDUCT.md).
