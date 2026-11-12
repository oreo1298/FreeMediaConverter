#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Run every documented command (see scripts/test-docs.sh for usage).

Sources of commands:
  * docs/install/install.toml (→ docs/INSTALL.md): each section's steps run in
    a fresh container of each listed image, in order.
  * ```bash blocks in README.md, CONTRIBUTING.md, docs/TESTING.md and
    docs/MAINTAINER.md: each block is preceded by an HTML comment
    `<!-- doc-test: context=NAME -->` or `<!-- doc-test: skip REASON -->`; a
    block without one is an error, so new commands cannot slip in untested.

URLs of the real GitHub Pages site and release downloads are rewritten to a
local mirror (signed with a throwaway key) built from dist/ (run the package
builds first, see docs/MAINTAINER.md).
"""
import argparse
import http.server
import os
import re
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import tomllib
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
DIST = ROOT / "dist"
MD_DOCS = ["README.md", "CONTRIBUTING.md", "docs/TESTING.md", "docs/MAINTAINER.md"]

CA_PRELUDE = r"""
if [ -f /ca.crt ]; then
  { [ -d /etc/apt ] && mkdir -p /usr/local/share/ca-certificates && cp /ca.crt /usr/local/share/ca-certificates/proxy.crt && { update-ca-certificates || true; }; } >/dev/null 2>&1 ||
  { cp /ca.crt /etc/pki/ca-trust/source/anchors/proxy.crt && update-ca-trust; } >/dev/null 2>&1 ||
  { mkdir -p /etc/pki/trust/anchors && cp /ca.crt /etc/pki/trust/anchors/proxy.crt && update-ca-certificates; } >/dev/null 2>&1 ||
  { cp /ca.crt /etc/ca-certificates/trust-source/anchors/proxy.crt && trust extract-compat; } >/dev/null 2>&1 || true
fi
"""

# Minimal "a user's machine" per image family: sudo, curl, certificates.
BASE_TOOLS = {
    "apt": "export DEBIAN_FRONTEND=noninteractive; apt-get update -qq && apt-get install -y -qq --no-install-recommends sudo curl ca-certificates xz-utils >/dev/null "
           "&& echo 'APT::Get::Assume-Yes \"true\";' > /etc/apt/apt.conf.d/90assumeyes",
    "dnf": "dnf -y -q install sudo curl xz tar >/dev/null",
    "zypper": "zypper -n -q install sudo curl xz tar gzip >/dev/null",
    "pacman": "pacman -Syu --noconfirm --needed sudo curl >/dev/null && "
              "printf '#!/bin/sh\\nexec /usr/bin/pacman --noconfirm \"$@\"\\n' > /usr/local/bin/pacman && chmod +x /usr/local/bin/pacman",
}

FAMILY = {
    "ubuntu": "apt", "debian": "apt", "fedora": "dnf", "almalinux": "dnf",
    "opensuse/tumbleweed": "zypper", "opensuse/leap": "zypper", "archlinux": "pacman",
}

# Markdown contexts: image + extra setup run before the block.
CONTEXTS = {
    # Retort installed from the (local) APT repository, plus sample media.
    "cli": ("ubuntu:24.04", """
install -d -m 0755 /etc/apt/keyrings
curl -fsSL {base}/retort-archive-keyring.gpg > /etc/apt/keyrings/retort-archive-keyring.gpg
echo "deb [signed-by=/etc/apt/keyrings/retort-archive-keyring.gpg] {base}/apt noble main" > /etc/apt/sources.list.d/retort.list
apt-get update -qq && apt-get install -y -qq retort-cli >/dev/null
ffmpeg -v error -f lavfi -i testsrc2=s=640x360:d=3 -f lavfi -i sine=d=3 -c:v libx264 -c:a aac holiday.mkv
ffmpeg -v error -f lavfi -i testsrc2=s=640x360:d=20 -f lavfi -i sine=d=20 -c:v libx264 -c:a aac lecture.mp4
"""),
    # A development checkout (copy of the tree under test) with build tools.
    "dev": ("ubuntu:24.04", """
apt-get install -y -qq --no-install-recommends build-essential git meson ninja-build pkg-config gettext appstream \
  desktop-file-utils libgtk-4-dev libadwaita-1-dev ffmpeg python3 gnupg xvfb xauth dbus fonts-cantarell >/dev/null
curl -sSf https://sh.rustup.rs | sh -s -- -y -q --profile minimal --component rustfmt,clippy >/dev/null
. "$HOME/.cargo/env"
mkdir -p /work && tar -C /src --exclude=./target --exclude=./_build --exclude=./_install --exclude=./dist -cf - . | tar -C /work -xf -
cd /work
"""),
}


def family(image: str) -> str:
    for k, v in FAMILY.items():
        if image.startswith(k):
            return v
    raise SystemExit(f"unknown image family: {image}")


def resolve_only(cmd: str) -> str:
    """Turn a package install into a resolution check (no download)."""
    m = re.match(r"sudo apt install (.*)", cmd)
    if m:
        return f"apt-get install -s {m.group(1)} >/dev/null"
    m = re.match(r"sudo dnf install -y (.*)", cmd)
    if m:
        # --assumeno exits 1 by design; success = resolution reached the prompt.
        return (f"{{ dnf install --assumeno {m.group(1)} > /tmp/r.log 2>&1 || true; }}; "
                f"grep -q 'Operation aborted' /tmp/r.log || {{ tail -20 /tmp/r.log; false; }}")
    m = re.match(r"sudo zypper install -y (.*)", cmd)
    if m:
        return f"zypper -n install --dry-run {m.group(1)} >/dev/null"
    m = re.match(r"sudo pacman -S --needed (.*)", cmd)
    if m:
        return f"pacman -S --print {m.group(1)} >/dev/null"
    return cmd


class Mirror:
    """Local signed repositories + release files served over HTTP."""

    def __init__(self, work: Path):
        self.dir = work / "site"
        self.port = 0

    def build(self):
        site = self.dir
        rel = site / "releases"
        rel.mkdir(parents=True, exist_ok=True)
        # Throwaway signing key and repositories (inside a container: needs apt-utils/createrepo).
        script = f"""
set -e
{CA_PRELUDE}
export DEBIAN_FRONTEND=noninteractive
apt-get update -qq && apt-get install -y -qq --no-install-recommends apt-utils gnupg createrepo-c rpm >/dev/null
gpg --batch --passphrase '' --quick-gen-key 'Retort doc test <test@invalid>' ed25519 sign never 2>/dev/null
export SIGNING_KEY=$(gpg --list-secret-keys --with-colons | awk -F: '/^fpr/ {{print $10; exit}}')
bash /src/packaging/repo/build-apt-repo.sh /dist/deb /site >/dev/null
bash /src/packaging/repo/build-rpm-repo.sh /dist/rpm /site http://127.0.0.1:PORT >/dev/null
chmod -R a+rX /site
"""
        run_container("ubuntu:24.04", script, mounts=[(ROOT, "/src", "ro"), (DIST, "/dist", "ro"), (site, "/site", "rw")])
        shutil.copy(ROOT / "scripts/install.sh", site / "install.sh")
        for f in DIST.glob("*-linux.tar.xz"):
            arch = re.search(r"-(x86_64|aarch64)-linux", f.name).group(1)
            shutil.copy(f, rel / f"retort-linux-{arch}.tar.xz")
        for f in DIST.glob("*.AppImage"):
            arch = re.search(r"(x86_64|aarch64)", f.name).group(1)
            shutil.copy(f, rel / f"Retort-{arch}.AppImage")
        for f in DIST.glob("Retort-*.flatpak"):
            shutil.copy(f, rel / f.name)

    def serve(self):
        handler = lambda *a, **kw: http.server.SimpleHTTPRequestHandler(*a, directory=str(self.dir), **kw)
        httpd = socketserver.ThreadingTCPServer(("127.0.0.1", 0), handler)
        httpd.RequestHandlerClass.log_message = lambda *a: None
        self.port = httpd.server_address[1]
        threading.Thread(target=httpd.serve_forever, daemon=True).start()
        # The RPM .repo files were written with a placeholder port.
        for repo in (self.dir / "rpm").glob("*.repo"):
            repo.write_text(repo.read_text().replace("127.0.0.1:PORT", f"127.0.0.1:{self.port}"))
        return f"http://127.0.0.1:{self.port}"


DOCKER_ARGS = os.environ.get("DOCKER_ARGS", "").split()
# A running, privileged container with flatpak and the GNOME 51 runtime
# installed (`docker run -d --privileged --network host … sleep infinity`).
FLATPAK_CONTAINER = os.environ.get("FLATPAK_CONTAINER", "")


def run_container(image, script, mounts=(), privileged=False, log=None):
    if image == "flatpak":
        if not FLATPAK_CONTAINER:
            res = subprocess.CompletedProcess([], 1, "", "FLATPAK_CONTAINER is not set\n")
        else:
            env = []
            for k in ("HTTPS_PROXY", "https_proxy", "HTTP_PROXY", "http_proxy", "NO_PROXY"):
                if k in os.environ:
                    env += ["-e", f"{k}={os.environ[k]}"]
            res = subprocess.run(["docker", "exec", *env, FLATPAK_CONTAINER, "bash", "-c", script],
                                 stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
            res.stderr = ""
        if log:
            log.write_text(res.stdout + res.stderr)
        return res
    args = ["docker", "run", "--rm", "--network", "host", *DOCKER_ARGS]
    if privileged:
        args.append("--privileged")
    for src, dst, mode in mounts:
        args += ["-v", f"{src}:{dst}:{mode}"]
    args += [image, "bash", "-c", script]
    res = subprocess.run(args, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    res.stderr = ""
    if log:
        log.write_text(res.stdout)
    if res.returncode != 0 and not log:
        sys.stderr.write(res.stdout[-3000:] + res.stderr[-3000:])
        raise SystemExit(f"container {image} failed")
    return res


def script_for(commands, prelude, url, rel):
    # Contexts may `cd` (the dev context into its checkout): start in a scratch
    # home first so their working directory sticks.
    lines = ["set -eo pipefail", "mkdir -p /tmp/home && cd /tmp/home", CA_PRELUDE, prelude,
             f"export RETORT_INSTALL_BASE={url} RETORT_INSTALL_RELEASES={rel}"]
    for i, c in enumerate(commands):
        lines.append(f"echo '>>> [{i + 1}] {c.replace(chr(39), chr(39) + chr(92) + chr(39) + chr(39))}'")
        lines.append(c)
    lines.append("echo '>>> all commands passed'")
    return "\n".join(lines)


def install_jobs(data, only):
    jobs = []
    for s in data["section"]:
        for image in s.get("images", []):
            if only and not any(o in (s["id"], image) for o in only):
                continue
            cmds, report = [], []
            for step in s.get("step", []):
                if "images" in step and image not in step["images"]:
                    continue
                test = step.get("test", "run")
                todo = step.get("test_commands", step.get("commands", []))
                if test.startswith("skip"):
                    report.append(f"skipped ({test[5:].strip()}): " + "; ".join(step.get("commands", [])))
                    continue
                cmds += [resolve_only(c) if test == "resolve" else c for c in todo]
            if cmds:
                jobs.append((f"INSTALL.md#{s['id']} on {image}", image, cmds, report))
            else:
                for r in report:
                    jobs.append((f"INSTALL.md#{s['id']}", None, [], [r]))
    return jobs


def markdown_jobs(only):
    jobs = []
    rx = re.compile(r"(<!--\s*doc-test:\s*(.*?)\s*-->\s*)?```bash\n(.*?)```", re.S)
    for doc in MD_DOCS:
        text = (ROOT / doc).read_text(encoding="utf-8")
        for m in rx.finditer(text):
            directive, body = m.group(2), m.group(3)
            line = text[: m.start(3)].count("\n")
            where = f"{doc}:{line}"
            if not directive:
                raise SystemExit(f"{where}: bash block without a <!-- doc-test: … --> directive")
            cmds = [c.strip() for c in body.strip().splitlines() if c.strip()]
            if directive.startswith("skip"):
                jobs.append((where, None, [], [f"skipped ({directive[4:].strip()}): " + "; ".join(cmds)]))
                continue
            ctx = re.match(r"context=(\w+)", directive)
            if not ctx or ctx.group(1) not in CONTEXTS and ctx.group(1) != "install":
                raise SystemExit(f"{where}: unknown doc-test directive '{directive}'")
            if only and not any(o in (doc, ctx.group(1)) for o in only):
                continue
            # All blocks of one document and context share a container (in order).
            same = [i for i, j in enumerate(jobs) if j[1] == ctx.group(1) and j[0].split(":")[0] == doc]
            if same:
                prev = jobs[same[-1]]
                jobs[same[-1]] = (f"{prev[0]},{line}", prev[1], prev[2] + cmds, prev[3])
            else:
                jobs.append((where, ctx.group(1), cmds, []))
    return jobs


def main():
    ap = argparse.ArgumentParser(description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("only", nargs="*", help="section ids, images, docs or contexts to run (default: all)")
    ap.add_argument("--list", action="store_true", help="list what would run")
    a = ap.parse_args()
    data = tomllib.loads((ROOT / "docs/install/install.toml").read_text(encoding="utf-8"))
    jobs = install_jobs(data, a.only) + markdown_jobs(a.only)
    if a.list:
        for name, image, cmds, report in jobs:
            print(f"{name}: {image or '-'}: {len(cmds)} commands {' '.join(report)}")
        return 0
    work = Path(tempfile.mkdtemp(prefix="doc-test-"))
    logs = ROOT / "dist/doc-test"
    logs.mkdir(parents=True, exist_ok=True)
    mirror = Mirror(work)
    mirror.build()
    url = mirror.serve()
    rel = f"{url}/releases"
    results = []
    for name, image, cmds, report in jobs:
        if not cmds:
            results.append((name, "SKIP", report))
            continue
        fill = lambda c: c.replace(data["base"], url).replace("{base}", url).replace(data["releases"], rel).replace("{releases}", rel)
        cmds = [fill(c) for c in cmds]
        privileged = False
        if image == "flatpak":
            # Start clean: no system-wide copy of the app, no earlier user install.
            prelude = ("flatpak uninstall -y --noninteractive io.github.oreo1298.Retort >/dev/null 2>&1 || true; "
                       "rm -rf /tmp/home")
        elif image in CONTEXTS or image == "install":
            ctx_image, ctx_setup = CONTEXTS.get(image, ("ubuntu:24.04", ""))
            prelude = BASE_TOOLS["apt"] + "\n" + fill(ctx_setup)
            image = ctx_image
        else:
            prelude = BASE_TOOLS[family(image)]
        log = logs / (re.sub(r"[^A-Za-z0-9.]+", "_", name) + ".log")
        print(f"… {name}", flush=True)
        mounts = [(ROOT, "/src", "ro")]
        if "cargo/env" in prelude:
            # Development contexts share compiled crates between jobs.
            mounts += [("retort-doc-test-target", "/work/target", "rw"),
                       ("retort-doc-test-registry", "/root/.cargo/registry", "rw")]
        res = run_container(image, script_for(cmds, prelude, url, rel), mounts=mounts,
                            privileged=privileged, log=log)
        ok = res.returncode == 0
        results.append((name, "PASS" if ok else "FAIL", report + ([] if ok else [f"see {log.relative_to(ROOT)}"])))
        print(f"{'PASS' if ok else 'FAIL'} {name}", flush=True)
    print("\n== Documentation test report")
    failed = 0
    for name, status, notes in results:
        print(f"{status:4} {name}")
        for n in notes:
            print(f"       {n}")
        failed += status == "FAIL"
    shutil.rmtree(work, ignore_errors=True)
    return 1 if failed else 0


if __name__ == "__main__":
    sys.exit(main())
