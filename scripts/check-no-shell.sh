#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Fail if Rust code ever starts a shell. Retort runs programs with argv
# vectors only (docs/SPEC.md §3 "No shell, ever").
set -euo pipefail
cd "$(dirname "$0")/.."
pattern='Command::new\("(/bin/|/usr/bin/)?(sh|bash|zsh|dash|fish)"\)|"(sh|bash)",\s*"-c"|\.arg\("-c"\)'
if grep -RInE "$pattern" --include='*.rs' crates/; then
  echo "error: shell invocation found in Rust code (see above)" >&2
  exit 1
fi
echo "ok: no shell invocations in Rust code"
