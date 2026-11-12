# Maintaining Retort

Everything a maintainer needs that is not day-to-day coding: releases,
package repositories, store submissions, dependency updates and renames.
Conventions for code are in `CLAUDE.md`; status and decisions in
`docs/PROGRESS.md`.

## One-time setup

1. **GitHub Pages** — Settings → Pages → Source: *GitHub Actions*. The
   release workflow publishes the APT/RPM repositories, `install.sh` and a
   small index page there (`https://oreo1298.github.io/FreeMediaConverter/`).
2. **Repository signing key** — create a dedicated key (not your personal one)
   and store the *armored secret key* as the repository secret
   `REPO_SIGNING_KEY` (Settings → Secrets and variables → Actions):

   ```bash
   gpg --batch --passphrase '' --quick-gen-key "Retort Packages <noreply@users.noreply.github.com>" ed25519 sign 3y
   ```

   ```bash
   gpg --armor --export-secret-keys "Retort Packages" > retort-signing-key.asc
   ```

   Paste the file's content into the secret, then delete the file. Publish
   the fingerprint (`gpg --fingerprint "Retort Packages"`) in
   `docs/INSTALL.md`. The key expires after 3 years: extend it with
   `gpg --quick-set-expire <fingerprint> 3y`, update the secret and re-release.
3. **arm64 runners** — the workflows use GitHub's `ubuntu-24.04-arm` runners
   (free for public repositories). Nothing to configure.

## Releasing

1. Update the version everywhere it is written down, then check:
   - `Cargo.toml` (`[workspace.package] version`), `meson.build` (`version:`)
   - `packaging/rpm/retort.spec` (`Version`, `%changelog`)
   - `packaging/arch/retort/PKGBUILD`, `packaging/arch/retort-bin/PKGBUILD` (`pkgver`, reset `pkgrel=1`)
   - `packaging/void/srcpkgs/retort/template` (`version`)
   - rename `packaging/gentoo/media-video/retort/retort-<version>.ebuild`
   - `data/app.metainfo.xml.in` (new `<release>` with date and notes)
   - `CHANGELOG.md` (new `## [x.y.z] - YYYY-MM-DD` section)

   ```bash
   scripts/check-versions.sh
   ```

2. If `Cargo.lock` changed, regenerate the Flatpak crate list:

   ```bash
   python3 flatpak-cargo-generator.py Cargo.lock -o packaging/flatpak/cargo-sources.json
   ```

   (`flatpak-cargo-generator.py` is in `flatpak/flatpak-builder-tools`, `cargo/`.)
3. Re-check distro package names used by System Check and INSTALL.md
   (needs Docker; see the script header for proxy settings):

   ```bash
   scripts/research/verify-packages.sh
   ```

4. Make sure CI is green on `main`, then tag and push:

   ```bash
   git tag -s v0.2.0 -m "Retort 0.2.0"
   ```

   ```bash
   git push origin v0.2.0
   ```

5. The **Release** workflow builds everything for x86-64 and arm64 —
   source + vendor tarballs, .deb (Ubuntu 24.04/26.04, Debian 13), .rpm
   (Fedora 45, EL 10, openSUSE Tumbleweed/Leap 16), Arch package (namcap),
   portable tarballs, AppImages (+ zsync) and Flatpak bundles — attaches them
   to a GitHub release with `SHA256SUMS`, and refreshes the signed
   repositories on Pages. Run it by hand (*Actions → Release → Run workflow*)
   for a dry run without publishing.
6. After the release: update the AUR, Flathub, GURU and Void as below.

Each package build is also runnable locally (Docker needed):

```bash
scripts/make-dist.sh
```

```bash
packaging/debian/build.sh ubuntu:24.04
```

```bash
packaging/rpm/build.sh fedora:45
```

```bash
packaging/arch/build.sh
```

```bash
packaging/tarball/build.sh
```

```bash
packaging/appimage/build.sh
```

Behind a TLS-intercepting proxy, pass `DOCKER_ARGS="--network host -v /path/ca.crt:/ca.crt:ro -e HTTPS_PROXY"`.

## AUR

Three packages: `retort` (source, split into `retort` + `retort-cli`),
`retort-git` and `retort-bin` (from the portable tarball). For each:

1. Copy `packaging/arch/<pkg>/PKGBUILD` into the AUR git clone.
2. Fill `sha256sums` from the release's `SHA256SUMS` (`updpkgsums` does it).
3. `makepkg --printsrcinfo > .SRCINFO`, `namcap PKGBUILD`, build once with
   `makepkg -si`, commit and push.

## Flathub

The manifest in `packaging/flatpak/` builds from the local checkout. For the
Flathub submission (`flathub/flathub`, branch `new-pr`), replace the `dir`
source of the `retort` module with

```json
{ "type": "archive", "url": "https://github.com/oreo1298/FreeMediaConverter/releases/download/v<version>/retort-<version>.tar.xz", "sha256": "<from SHA256SUMS>" }
```

keep `cargo-sources.json`, and run the linter before opening the PR:

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
