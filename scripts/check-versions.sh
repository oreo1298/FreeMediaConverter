#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Every place that states the release version must agree with Cargo.toml.
# Run before tagging (the release workflow runs it too).
set -euo pipefail
cd "$(dirname "$0")/.."
v=$(sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -1)
bin=$(sed -n 's/^APP_BIN=//p' branding.env)
fail=0
check() { # what found
  if [ "$2" != "$v" ]; then echo "MISMATCH: $1 says '$2', Cargo.toml says '$v'"; fail=1; else echo "ok: $1 ($2)"; fi
}
check meson.build "$(sed -n "s/^  version: '\(.*\)',/\1/p" meson.build)"
check "packaging/rpm/$bin.spec" "$(sed -n 's/^Version: *//p' "packaging/rpm/$bin.spec")"
check "packaging/arch/$bin/PKGBUILD" "$(sed -n 's/^pkgver=//p' "packaging/arch/$bin/PKGBUILD")"
check "packaging/arch/$bin-bin/PKGBUILD" "$(sed -n 's/^pkgver=//p' "packaging/arch/$bin-bin/PKGBUILD")"
check "packaging/void/srcpkgs/$bin/template" "$(sed -n 's/^version=//p' "packaging/void/srcpkgs/$bin/template")"
ebuild=$(find packaging/gentoo -name "$bin-*.ebuild" | head -1)
check "$ebuild" "$(basename "$ebuild" .ebuild | sed "s/^$bin-//")"
check data/app.metainfo.xml.in "$(grep -o '<release version="[^"]*"' data/app.metainfo.xml.in | head -1 | cut -d'"' -f2)"
check CHANGELOG.md "$(grep -m1 -oE '^## \[?[0-9][^] ]*' CHANGELOG.md | sed 's/^## \[\{0,1\}//')"
exit $fail
