# SPDX-License-Identifier: GPL-3.0-or-later
# shellcheck shell=bash disable=SC2034
# Shared helpers for the packaging scripts (sourced, not executed).

# Repository root, app binary name and version.
pkg_root=$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)
pkg_bin=$(sed -n 's/^APP_BIN=//p' "$pkg_root/branding.env")
pkg_id=$(sed -n 's/^APP_ID=//p' "$pkg_root/branding.env")
pkg_version=$(sed -n 's/^version = "\(.*\)"/\1/p' "$pkg_root/Cargo.toml" | head -1)
export pkg_root pkg_bin pkg_id pkg_version

# Extra `docker run` arguments, e.g. DOCKER_ARGS="--network host -v /etc/ca.crt:/ca.crt:ro"
# behind a TLS-intercepting proxy (the container then trusts /ca.crt).
# shellcheck disable=SC2206
docker_args=(${DOCKER_ARGS:-})

# Shell prelude for containers: trust /ca.crt if mounted.
ca_prelude='
if [ -f /ca.crt ]; then
  # Debian/Ubuntu: the ca-certificates package picks this up when installed later.
  { [ -d /etc/apt ] && mkdir -p /usr/local/share/ca-certificates && cp /ca.crt /usr/local/share/ca-certificates/proxy.crt && { update-ca-certificates || true; }; } >/dev/null 2>&1 ||
  { cp /ca.crt /etc/pki/ca-trust/source/anchors/proxy.crt && update-ca-trust; } >/dev/null 2>&1 ||
  { mkdir -p /etc/pki/trust/anchors && cp /ca.crt /etc/pki/trust/anchors/proxy.crt && update-ca-certificates; } >/dev/null 2>&1 ||
  { cp /ca.crt /etc/ca-certificates/trust-source/anchors/proxy.crt && trust extract-compat; } >/dev/null 2>&1 || true
fi
'

# Make sure the release tarballs exist (scripts/make-dist.sh).
ensure_dist() {
  if [ ! -f "$pkg_root/dist/$pkg_bin-$pkg_version.tar.xz" ] || [ ! -f "$pkg_root/dist/$pkg_bin-$pkg_version-vendor.tar.xz" ]; then
    "$pkg_root/scripts/make-dist.sh" ${DIST_DIRTY:+--dirty} >/dev/null
  fi
}
