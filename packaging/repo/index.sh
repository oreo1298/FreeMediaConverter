#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Landing page for the GitHub Pages site (APT/RPM repositories + install.sh).
# Usage: packaging/repo/index.sh <site-dir> <base-url> > index.html
set -euo pipefail
site=${1:?usage: index.sh <site-dir> <base-url>}
base=${2:?usage: index.sh <site-dir> <base-url>}
root=$(cd "$(dirname "$0")/../.." && pwd)
name=$(sed -n 's/^APP_NAME=//p' "$root/branding.env")
bin=$(sed -n 's/^APP_BIN=//p' "$root/branding.env")
home=$(sed -n 's/^APP_HOMEPAGE=//p' "$root/branding.env")
cat <<HTML
<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1">
<title>$name packages</title>
<style>body{font:16px/1.5 system-ui,sans-serif;max-width:46rem;margin:2rem auto;padding:0 1rem}pre{background:#f4f4f4;padding:.75rem;overflow-x:auto}</style>
</head><body>
<h1>$name package repositories</h1>
<p>Signed APT and RPM repositories for <a href="$home">$name</a>. The easiest way in:</p>
<pre>curl -fsSL $base/install.sh | sh</pre>
<p>Manual setup and every other format: <a href="$home/blob/main/docs/INSTALL.md">INSTALL.md</a>.</p>
<h2>Contents</h2><ul>
$(cd "$site" && find apt/dists -maxdepth 1 -mindepth 1 -type d 2>/dev/null | sed 's|apt/dists/||' | sort | sed 's|.*|<li>APT: <code>&</code></li>|')
$(cd "$site" && find rpm -maxdepth 1 -name "$bin-*.repo" 2>/dev/null | sort | sed 's|.*|<li>RPM: <a href="&">&</a></li>|')
<li>Signing key: <a href="$bin-archive-keyring.asc">$bin-archive-keyring.asc</a></li>
</ul></body></html>
HTML
