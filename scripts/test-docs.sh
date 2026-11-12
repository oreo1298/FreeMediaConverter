#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run every command in the documentation (INSTALL.md via docs/install/install.toml,
# and the bash blocks of README, CONTRIBUTING, TESTING and MAINTAINER) in fresh
# containers. Install sections run as a normal user on a copy of this checkout
# (instead of `git clone`); release downloads come from a local mirror of dist/.
#
# Prerequisites: Docker. The AppImage, tarball and Flatpak sections need those
# release files in dist/ (scripts/make-dist.sh, then packaging/*/build.sh; see
# docs/MAINTAINER.md). The Flatpak section runs in $FLATPAK_CONTAINER (a
# privileged container with flatpak and the GNOME runtime). Behind a TLS proxy,
# set DOCKER_ARGS (see packaging/common.sh).
#
# Usage: scripts/test-docs.sh [--list] [section|image|section@image|doc|context ...]
#   e.g. scripts/test-docs.sh arch debian@ubuntu:26.04 cli
# Logs: dist/doc-test/*.log
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/gen-install.py --check
exec python3 scripts/doc_test.py "$@"
