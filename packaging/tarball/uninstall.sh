#!/bin/sh
# SPDX-License-Identifier: GPL-3.0-or-later
# Remove what install.sh installed (listed in installed-files next to this script).
set -eu
here=$(cd "$(dirname "$0")" && pwd)
manifest="$here/installed-files"
[ -f "$manifest" ] || { echo "No installation found ($manifest missing)." >&2; exit 1; }
datadir=$(dirname "$here")
while IFS= read -r f; do
  [ -n "$f" ] && rm -f "$f"
done < "$manifest"
rmdir "$here" 2>/dev/null || true
command -v glib-compile-schemas >/dev/null && glib-compile-schemas "$datadir/glib-2.0/schemas" 2>/dev/null || true
command -v update-desktop-database >/dev/null && update-desktop-database -q "$datadir/applications" 2>/dev/null || true
echo "Removed. Settings and caches are kept in ~/.config, ~/.cache and ~/.local/state (delete them by hand if you like)."
