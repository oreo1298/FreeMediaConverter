#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Propagate a rename. Edit branding.env first (APP_NAME, APP_ID, APP_BIN,
# APP_REPO_OWNER, APP_REPO_NAME, APP_HOMEPAGE), then run this script: it
# compares with the committed branding.env (or --from FILE) and rewrites the
# static files that cannot read branding.env (packaging, Cargo binary names,
# Meson project name, Nix, CI, docs) and renames files named after the old
# ID or binary. Rust code and data templates read branding.env at build time.
# Usage: scripts/rename.sh [--from OLD_BRANDING_ENV] [--dry-run]
set -euo pipefail
cd "$(dirname "$0")/.."
from="" dry=0
while [ $# -gt 0 ]; do
  case "$1" in
    --from) from=$2; shift 2 ;;
    --dry-run) dry=1; shift ;;
    *) echo "unknown argument: $1" >&2; exit 2 ;;
  esac
done
old_env=$(mktemp)
trap 'rm -f "$old_env"' EXIT
if [ -n "$from" ]; then cp "$from" "$old_env"; else git show HEAD:branding.env > "$old_env"; fi
exec python3 - "$old_env" branding.env "$dry" <<'PY'
import os, re, subprocess, sys
old_env, new_env, dry = sys.argv[1], sys.argv[2], sys.argv[3] == "1"

def load(p):
    d = {}
    for line in open(p, encoding="utf-8"):
        line = line.strip()
        if line and not line.startswith("#") and "=" in line:
            k, v = line.split("=", 1)
            d[k.strip()] = v.strip()
    return d

old, new = load(old_env), load(new_env)
changed = {k for k in new if old.get(k) != new[k]}
if not changed:
    print("branding.env is unchanged; nothing to do.")
    sys.exit(0)
print("changing:", ", ".join(f"{k}: {old.get(k)} -> {new[k]}" for k in sorted(changed)))

subs = []  # (regex, replacement), applied in order
def lit(k, extra_before="", extra_after=""):
    if k in changed:
        subs.append((re.compile(extra_before + re.escape(old[k]) + extra_after), new[k]))
lit("APP_HOMEPAGE")
lit("APP_ID")
if "APP_REPO_OWNER" in changed or "APP_REPO_NAME" in changed:
    subs.append((re.compile(re.escape(f"{old['APP_REPO_OWNER']}/{old['APP_REPO_NAME']}")),
                 f"{new['APP_REPO_OWNER']}/{new['APP_REPO_NAME']}"))
    subs.append((re.compile(re.escape(f"|{old['APP_REPO_OWNER']}|{old['APP_REPO_NAME']}|")),
                 f"|{new['APP_REPO_OWNER']}|{new['APP_REPO_NAME']}|"))
# The clone's folder name in the install instructions ("cd FreeMediaConverter").
lit("APP_REPO_NAME", r"\b", r"\b")
if "APP_BIN" in changed:
    ob, nb = old["APP_BIN"], new["APP_BIN"]
    subs.append((re.compile(rf"\b{re.escape(ob)}-cli\b"), f"{nb}-cli"))
    # The engine crate keeps its name (retort-core / retort_core).
    subs.append((re.compile(rf"\b{re.escape(ob)}\b(?![-_]core)"), nb))
lit("APP_NAME", r"\b", r"\b")

files = subprocess.run(["git", "ls-files", "packaging", "nix", "flake.nix", "meson.build", "meson_options.txt",
                        "Cargo.toml", "crates/app/Cargo.toml", "crates/cli/Cargo.toml", "crates/core/Cargo.toml",
                        ".github", "scripts", "PKGBUILD", "build-package.sh", "docs/install", "README.md", "docs/INSTALL.md", "docs/INSTALL.in.md", "docs/MAINTAINER.md",
                        "CHANGELOG.md"], capture_output=True, text=True, check=True).stdout.split()
skip = {"scripts/rename.sh", "packaging/flatpak/cargo-sources.json"}
edited = 0
for f in files:
    if f in skip or not os.path.isfile(f):
        continue
    try:
        text = open(f, encoding="utf-8").read()
    except UnicodeDecodeError:
        continue
    out = text
    for rx, rep in subs:
        out = rx.sub(rep, out)
    if out != text:
        edited += 1
        print("edit  ", f)
        if not dry:
            open(f, "w", encoding="utf-8").write(out)

# Files and directories named after the old ID / binary.
moves = []
for f in subprocess.run(["git", "ls-files", "packaging"], capture_output=True, text=True, check=True).stdout.split():
    parts = f.split("/")
    newparts = []
    for p in parts:
        q = p.replace(old["APP_ID"], new["APP_ID"]) if "APP_ID" in changed else p
        if "APP_BIN" in changed:
            q = re.sub(rf"^{re.escape(old['APP_BIN'])}(?=$|[-._])", new["APP_BIN"], q)
        newparts.append(q)
    nf = "/".join(newparts)
    if nf != f:
        moves.append((f, nf))
for src, dst in moves:
    print("rename", src, "->", dst)
    if not dry:
        os.makedirs(os.path.dirname(dst), exist_ok=True)
        subprocess.run(["git", "mv", "-k", src, dst], check=True)
if "APP_BIN" in changed and not dry:
    # Cargo.lock records the renamed package names.
    subprocess.run(["cargo", "update", "--workspace", "--offline", "--quiet"], check=False)
print(f"{edited} files edited, {len(moves)} renamed{' (dry run)' if dry else ''}.")
print("Next: regenerate packaging/flatpak/cargo-sources.json if Cargo.lock changed, then build and review `git diff`.")
PY
