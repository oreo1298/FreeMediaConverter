# Maintaining Retort

Everything a maintainer needs that is not day-to-day coding: releases,
store submissions, dependency updates and renames. Conventions for code are
in `CLAUDE.md`; status and decisions in `docs/PROGRESS.md`.

Users install Retort by cloning this repository and building a native package
on their own machine (`build-package.sh` for .deb/.rpm, `PKGBUILD` with
`makepkg -si` on Arch; see `docs/INSTALL.md`). There is no package
repository, signing key or AUR package to maintain: what is on the default
branch is what users build. Keep it releasable.

## One-time setup

**arm64 runners** — the workflows use GitHub's `ubuntu-24.04-arm` runners
(free for public repositories). Nothing to configure.

## Releasing

1. Update the version everywhere it is written down, then check:
   - `Cargo.toml` (`[workspace.package] version`), `meson.build` (`version:`)
   - `packaging/rpm/retort.spec` (`Version`, `%changelog`)
   - `PKGBUILD` (`pkgver`, reset `pkgrel=1`)
   - `packaging/void/srcpkgs/retort/template` (`version`)
   - rename `packaging/gentoo/media-video/retort/retort-<version>.ebuild`
   - `data/app.metainfo.xml.in` (new `<release>` with date and notes)
   - `CHANGELOG.md` (new `## [x.y.z] - YYYY-MM-DD` section)

   <!-- doc-test: context=dev -->
   ```bash
   scripts/check-versions.sh
   ```

2. If `Cargo.lock` changed, regenerate the Flatpak crate list:

   <!-- doc-test: skip needs a flatpak-builder-tools checkout and its Python modules; the Flatpak CI job builds from the committed file -->
   ```bash
   python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
   ```

   (`flatpak-cargo-generator.py` is in `flatpak/flatpak-builder-tools`, `cargo/`.)
3. Re-check distro package names used by System Check and INSTALL.md
   (needs Docker; see the script header for proxy settings):

   <!-- doc-test: skip starts its own Docker containers (maintainer machine) -->
   ```bash
   scripts/research/verify-packages.sh
   ```

4. Make sure CI is green on `main`, then tag and push:

   <!-- doc-test: skip creates a release tag -->
   ```bash
   git tag -s v0.2.0 -m "Retort 0.2.0"
   ```

   <!-- doc-test: skip publishes a release -->
   ```bash
   git push origin v0.2.0
   ```

5. The **Release** workflow follows `docs/INSTALL.md` on every supported
   distribution (Ubuntu 24.04/26.04, Debian 13, Fedora 45, EL 10, openSUSE
   Tumbleweed/Leap 16 on x86-64 and arm64, Arch on x86-64): build the package
   from the checkout, install, update, remove. It also builds the source +
   vendor tarballs, portable tarballs, AppImages (+ zsync) and Flatpak bundles,
   runs the remaining documented commands, and attaches the downloads to a
   GitHub release with `SHA256SUMS`. Run it by hand (*Actions → Release → Run
   workflow*) for a dry run without publishing.
6. After the release: update Flathub, GURU and Void as below.

The native install of one distribution is testable locally (Docker needed);
it runs the INSTALL.md section as a normal user on a copy of this checkout:

<!-- doc-test: skip runs Docker; the Packaging and Release workflows run it -->
```bash
scripts/test-docs.sh debian@ubuntu:24.04
```

The release downloads build locally too:

<!-- doc-test: context=dev -->
```bash
scripts/make-dist.sh
```

<!-- doc-test: skip runs Docker; the Packaging and Release workflows run this script -->
```bash
packaging/tarball/build.sh
```

<!-- doc-test: skip runs Docker; the Release workflow runs this script -->
```bash
packaging/appimage/build.sh
```

Behind a TLS-intercepting proxy, pass `DOCKER_ARGS="--network host -v /path/ca.crt:/ca.crt:ro -e HTTPS_PROXY"`.

## Flathub

The manifest in `packaging/flatpak/` builds from the local checkout. For the
Flathub submission (`flathub/flathub`, branch `new-pr`), replace the `dir`
source of the `retort` module with

```json
{ "type": "archive", "url": "https://github.com/oreo1298/FreeMediaConverter/releases/download/v<version>/retort-<version>.tar.xz", "sha256": "<from SHA256SUMS>" }
```

keep `cargo-sources.json`, and run the linter before opening the PR:

<!-- doc-test: skip needs the Flathub submission checkout; the Packaging workflow lints the manifest -->
```bash
flatpak run --command=flatpak-builder-lint org.flatpak.Builder manifest io.github.oreo1298.Retort.json
```

Permissions to justify in the PR: `--device=all` (NVENC/CUDA need
`/dev/nvidia*`, VA-API and Vulkan need `/dev/dri`) and filesystem access to
home and removable media (outputs are written next to the input files, which
the document portal does not allow).

## Gentoo and Void

- Gentoo: submit `packaging/gentoo/media-video/retort` to GURU; run
  `ebuild retort-<version>.ebuild manifest` first.
- Void: copy `packaging/void/srcpkgs/retort` into `void-packages`, replace the
  checksum placeholders with the values from `SHA256SUMS`, run
  `./xbps-src pkg retort` and open a PR.

## Updating FFmpeg

The bundled FFmpeg (Flatpak, AppImage, CI's latest-FFmpeg job) is pinned in
two places: `packaging/ffmpeg/build-ffmpeg.sh` (`FFMPEG_VERSION`, and the
nv-codec-headers / libplacebo / Vulkan-Headers versions) and the `ffmpeg`
module of the Flatpak manifest (`url` + `sha256`). When a new FFmpeg release
appears:

1. Re-run the option research and diff it (`scripts/research/ffmpeg-options.sh`),
   update `docs/research/encoders.md` and the quality/command code if options
   changed.
2. Bump both pins, push, and watch the `integration (FFmpeg 9.0.x)` CI job
   (it builds the new version and runs every integration test against it).

## Renaming the app

Edit `branding.env`, then:

<!-- doc-test: context=dev -->
```bash
scripts/rename.sh
```

It rewrites packaging files, Cargo binary names and the Meson project name,
and renames files named after the old ID or binary. Review `git diff`, build,
and regenerate `packaging/flatpak/cargo-sources.json`.

## Hardware reports

Users run `scripts/collect-hw-report.sh` and attach the tarball (or folder)
to an issue. Unpack it as `tests/fixtures/hw/<machine>/`: it is a complete
simulated profile, so `cargo test -p retort-core` immediately covers that
machine (probe outcomes, planner choices and commands are snapshotted).
