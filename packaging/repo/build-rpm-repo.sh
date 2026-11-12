#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build signed RPM repositories from dist/rpm/<distro>-<version>/*.rpm:
#   <out>/rpm/<distro>-<version>/{*.rpm,repodata/repomd.xml{,.asc}}
#   <out>/rpm/<bin>.repo files per distro, <out>/<bin>-archive-keyring.asc
# Packages are signed with rpmsign; repodata is signed for repo_gpgcheck.
# Needs: createrepo_c, rpm-sign (rpmsign), gnupg; key fingerprint in $SIGNING_KEY.
# Usage: SIGNING_KEY=<fpr> packaging/repo/build-rpm-repo.sh <dist/rpm> <out> <base-url>
set -euo pipefail
src=${1:?usage: build-rpm-repo.sh <rpm-dir> <out-dir> <base-url>}
out=${2:?usage: build-rpm-repo.sh <rpm-dir> <out-dir> <base-url>}
url=${3:?usage: build-rpm-repo.sh <rpm-dir> <out-dir> <base-url>}
key=${SIGNING_KEY:?set SIGNING_KEY to the signing key fingerprint}
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=$(sed -n 's/^APP_BIN=//p' "$root/branding.env")
name=$(sed -n 's/^APP_NAME=//p' "$root/branding.env")
mkdir -p "$out/rpm"
gpg --armor --export "$key" > "$out/$bin-archive-keyring.asc"
for dir in "$src"/*/; do
  d=$(basename "$dir")
  mkdir -p "$out/rpm/$d"
  find "$dir" -name '*.rpm' ! -name '*.src.rpm' -exec cp {} "$out/rpm/$d/" \;
  rpmsign --define "_gpg_name $key" --define "__gpg $(command -v gpg)" --addsign "$out/rpm/$d"/*.rpm >/dev/null
  createrepo_c --quiet "$out/rpm/$d"
  gpg --batch --yes --local-user "$key" --detach-sign --armor "$out/rpm/$d/repodata/repomd.xml"
  cat > "$out/rpm/$bin-$d.repo" <<REPO
[$bin]
name=$name ($d)
baseurl=$url/rpm/$d
enabled=1
gpgcheck=1
repo_gpgcheck=1
gpgkey=$url/$bin-archive-keyring.asc
REPO
  echo "rpm: $d"
done
