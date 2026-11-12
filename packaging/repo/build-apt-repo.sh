#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Build a signed APT repository from dist/deb/<codename>/*.deb:
#   <out>/apt/dists/<codename>/{Release,InRelease,Release.gpg,main/binary-<arch>/Packages{,.gz}}
#   <out>/apt/pool/<codename>/main/*.deb
#   <out>/<bin>-archive-keyring.gpg   (public key, binary keyring)
# Needs: apt-utils (apt-ftparchive), gnupg. The signing key must already be in
# the GnuPG keyring; pass its fingerprint as $SIGNING_KEY.
# Usage: SIGNING_KEY=<fpr> packaging/repo/build-apt-repo.sh <dist/deb> <out>
set -euo pipefail
src=${1:?usage: build-apt-repo.sh <deb-dir> <out-dir>}
out=${2:?usage: build-apt-repo.sh <deb-dir> <out-dir>}
key=${SIGNING_KEY:?set SIGNING_KEY to the signing key fingerprint}
root=$(cd "$(dirname "$0")/../.." && pwd)
bin=$(sed -n 's/^APP_BIN=//p' "$root/branding.env")
name=$(sed -n 's/^APP_NAME=//p' "$root/branding.env")
mkdir -p "$out/apt"
gpg --export "$key" > "$out/$bin-archive-keyring.gpg"
gpg --armor --export "$key" > "$out/$bin-archive-keyring.asc"
cd "$out/apt"
for dir in "$src"/*/; do
  codename=$(basename "$dir")
  mkdir -p "pool/$codename/main"
  cp "$dir"/*.deb "pool/$codename/main/"
  archs=$(for d in "pool/$codename/main"/*.deb; do dpkg-deb -f "$d" Architecture; done | sort -u | tr '\n' ' ')
  for a in $archs; do
    bdir="dists/$codename/main/binary-$a"
    mkdir -p "$bdir"
    apt-ftparchive --arch "$a" packages "pool/$codename" > "$bdir/Packages"
    gzip -9kf "$bdir/Packages"
  done
  apt-ftparchive \
    -o "APT::FTPArchive::Release::Origin=$name" \
    -o "APT::FTPArchive::Release::Label=$name" \
    -o "APT::FTPArchive::Release::Suite=$codename" \
    -o "APT::FTPArchive::Release::Codename=$codename" \
    -o "APT::FTPArchive::Release::Components=main" \
    -o "APT::FTPArchive::Release::Architectures=$archs" \
    release "dists/$codename" > "dists/$codename/Release"
  gpg --batch --yes --local-user "$key" --clearsign -o "dists/$codename/InRelease" "dists/$codename/Release"
  gpg --batch --yes --local-user "$key" -abs -o "dists/$codename/Release.gpg" "dists/$codename/Release"
  echo "apt: $codename ($archs)"
done
