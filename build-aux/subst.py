#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Replace @KEY@ placeholders with values from branding.env (and --set K=V).

Used after translations are merged, so translators see the placeholders and
the msgids stay stable across renames.
"""
import argparse
import re
import sys


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("--branding", required=True)
    ap.add_argument("--set", action="append", default=[], metavar="KEY=VALUE")
    ap.add_argument("input")
    ap.add_argument("output")
    a = ap.parse_args()
    values = {}
    with open(a.branding, encoding="utf-8") as f:
        for line in f:
            line = line.strip()
            if line and not line.startswith("#") and "=" in line:
                k, v = line.split("=", 1)
                values[k.strip()] = v.strip()
    for kv in a.set:
        k, v = kv.split("=", 1)
        values[k] = v
    with open(a.input, encoding="utf-8") as f:
        text = f.read()
    missing = sorted({m for m in re.findall(r"@([A-Z_]+)@", text) if m not in values})
    if missing:
        print(f"{a.input}: unknown placeholders: {', '.join(missing)}", file=sys.stderr)
        return 1
    text = re.sub(r"@([A-Z_]+)@", lambda m: values[m.group(1)], text)
    with open(a.output, "w", encoding="utf-8") as f:
        f.write(text)
    return 0


if __name__ == "__main__":
    sys.exit(main())
