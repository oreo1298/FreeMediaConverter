#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Release source tarballs:
#   dist/<bin>-<version>.tar.xz         git archive of HEAD (or the working tree with --dirty)
#   dist/<bin>-<version>-vendor.tar.xz  cargo vendor (offline builds: deb, rpm, AUR, Gentoo, Void)
# The vendor tarball unpacks to vendor/ plus .cargo/config.toml next to the sources.
set -euo pipefail
cd "$(dirname "$0")/.."
APP_BIN=$(sed -n 's/^APP_BIN=//p' branding.env)
version=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
name="$APP_BIN-$version"
out=${OUT:-dist}
mkdir -p "$out"
tmp=$(mktemp -d)
trap 'rm -rf "${tmp:?}"' EXIT

if [ "${1:-}" = "--dirty" ]; then
  # Working tree including uncommitted changes (local testing only).
  git ls-files -co --exclude-standard -z | tar --null -T - --transform "s,^,$name/," -cf "$tmp/src.tar"
else
  git archive --format=tar --prefix="$name/" HEAD -o "$tmp/src.tar"
fi
xz -T0 -9 -c "$tmp/src.tar" > "$out/$name.tar.xz"

mkdir -p "$tmp/$name/.cargo"
cargo vendor --locked --versioned-dirs "$tmp/$name/vendor" >/dev/null 2>&1
cat > "$tmp/$name/.cargo/config.toml" <<'TOML'
[source.crates-io]
replace-with = "vendored-sources"

[source.vendored-sources]
directory = "vendor"
TOML
tar -C "$tmp" -cJf "$out/$name-vendor.tar.xz" "$name/vendor" "$name/.cargo"
(cd "$out" && sha256sum "$name.tar.xz" "$name-vendor.tar.xz" > "$name.sha256")
echo "$out/$name.tar.xz"
echo "$out/$name-vendor.tar.xz"
