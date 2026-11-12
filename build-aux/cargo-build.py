#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Build the Rust binaries with Cargo and copy them to Meson's output dir.

Called by meson.build; argv only, no shell. Environment (RETORT_LOCALEDIR,
CARGO_HOME, ...) comes from Meson.
"""
import argparse
import shutil
import subprocess
import sys
from pathlib import Path


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--manifest", required=True)
    ap.add_argument("--target-dir", required=True)
    ap.add_argument("--profile", choices=["release", "debug"], default="release")
    ap.add_argument("--offline", action="store_true")
    ap.add_argument("--outdir", required=True)
    ap.add_argument("--package", action="append", default=[])
    ap.add_argument("--features", default="", help="comma-separated, package-qualified")
    ap.add_argument("bins", nargs="+")
    a = ap.parse_args()

    cmd = ["cargo", "build", "--manifest-path", a.manifest, "--target-dir", a.target_dir, "--locked"]
    if a.profile == "release":
        cmd.append("--release")
    if a.offline:
        cmd.append("--offline")
    for p in a.package:
        cmd += ["-p", p]
    if a.features:
        cmd += ["--features", a.features]
    print("+", " ".join(cmd), flush=True)
    r = subprocess.run(cmd)
    if r.returncode != 0:
        return r.returncode
    out = Path(a.outdir)
    for b in a.bins:
        src = Path(a.target_dir) / a.profile / b
        shutil.copy2(src, out / b)
    return 0


if __name__ == "__main__":
    sys.exit(main())
