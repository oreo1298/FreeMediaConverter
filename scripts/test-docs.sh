#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
# Run every command in the documentation (INSTALL.md via docs/install/install.toml,
# and the bash blocks of README, CONTRIBUTING, TESTING and MAINTAINER) in fresh
# containers, against a local signed mirror of the package repositories and
# release files built from dist/.
#
# Prerequisites: Docker, and the release artifacts in dist/ (scripts/make-dist.sh,
# then the packaging/*/build.sh scripts; see docs/MAINTAINER.md). The Flatpak
# section runs in $FLATPAK_CONTAINER (a privileged container with flatpak and
# the GNOME runtime). Behind a TLS proxy, set DOCKER_ARGS (see packaging/common.sh).
#
# Usage: scripts/test-docs.sh [--list] [section|image|doc|context ...]
# Logs: dist/doc-test/*.log
set -euo pipefail
cd "$(dirname "$0")/.."
python3 scripts/gen-install.py --check
exec python3 scripts/doc_test.py "$@"
