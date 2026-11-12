#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Install Retort from this portable tarball.
#   ./install.sh            for the current user (~/.local)
#   sudo ./install.sh --system   for everyone (/usr/local)
# Needs GTK >= 4.14, libadwaita >= 1.5 and FFmpeg from your distribution.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
bindir="$HOME/.local/bin"
datadir="${XDG_DATA_HOME:-$HOME/.local/share}"
if [ "${1:-}" = "--system" ]; then
  bindir=/usr/local/bin
  datadir=/usr/local/share
fi
app_id=$(sed -n 's/^APP_ID=//p' "$here/branding.env")
bin=$(sed -n 's/^APP_BIN=//p' "$here/branding.env")

mkdir -p "$bindir" "$datadir"
install -m 755 "$here/bin/$bin" "$here/bin/$bin-cli" "$bindir/"
cp -R "$here/share/." "$datadir/"
# Desktop entry: absolute Exec so it works even if $bindir is not on PATH.
sed -i "s|^Exec=$bin |Exec=$bindir/$bin |" "$datadir/applications/$app_id.desktop"
# Remember what was installed (uninstall.sh reads this).
manifest="$datadir/$bin/installed-files"
mkdir -p "$(dirname "$manifest")"
{
  echo "$bindir/$bin"
  echo "$bindir/$bin-cli"
  (cd "$here/share" && find . -type f | sed "s|^\.|$datadir|")
  echo "$manifest"
} > "$manifest"
cp "$here/uninstall.sh" "$datadir/$bin/uninstall.sh"
echo "$datadir/$bin/uninstall.sh" >> "$manifest"

command -v glib-compile-schemas >/dev/null && glib-compile-schemas "$datadir/glib-2.0/schemas" || true
command -v gtk4-update-icon-cache >/dev/null && gtk4-update-icon-cache -q -t -f "$datadir/icons/hicolor" || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$datadir/applications" || true

echo "Installed $bin to $bindir."
command -v ffmpeg >/dev/null || echo "Note: FFmpeg was not found. Install it from your distribution (package \"ffmpeg\")."
case ":$PATH:" in *":$bindir:"*) ;; *) echo "Note: $bindir is not on your PATH; the menu entry works regardless." ;; esac
echo "Uninstall with: $datadir/$bin/uninstall.sh"
