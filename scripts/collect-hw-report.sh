#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Collect everything Retort's parsers and hardware prober need from this
# machine into one folder, laid out as a simulated hardware profile:
#
#   <out>/profile.json       which probes pass/fail here (from retort-cli)
#   <out>/ffmpeg/            recorded ffmpeg -encoders/-decoders/-h encoder=… output
#   <out>/root/              snapshot of the relevant /sys, /proc, /dev, /etc bits
#   <out>/raw/               vainfo, nvidia-smi -q, vulkaninfo, lspci, diagnose --json…
#
# Commit the folder to tests/fixtures/hw/<machine>/ (see TESTING.md).
# Nothing is uploaded anywhere. Read-only: it never changes your system.
#
# Usage:  scripts/collect-hw-report.sh [output-dir]
# Env:    RETORT_CLI=/path/to/retort-cli   FFMPEG=/path/to/ffmpeg
set -uo pipefail

host=$(hostname 2>/dev/null || cat /proc/sys/kernel/hostname)
out=${1:-"hw-report-${host}-$(date +%Y%m%d)"}
mkdir -p "$out/ffmpeg/help_encoder" "$out/ffmpeg/help_muxer" "$out/root" "$out/raw"
export LC_ALL=C

say() { printf '\033[1m==>\033[0m %s\n' "$*"; }
have() { command -v "$1" >/dev/null 2>&1; }
# Run a command, saving stdout+stderr; never fail the script.
cap() { local file=$1; shift; { "$@"; } >"$file" 2>&1 || true; }

# --- locate tools -----------------------------------------------------------
ffmpeg_bin=${FFMPEG:-$(command -v ffmpeg || true)}
cli=${RETORT_CLI:-}
if [ -z "$cli" ]; then
  for c in retort-cli ./target/release/retort-cli ./target/debug/retort-cli; do
    if have "$c" || [ -x "$c" ]; then cli=$c; break; fi
  done
fi
cli_cmd=()
if [ -n "$cli" ]; then
  cli_cmd=("$cli")
elif have flatpak && flatpak info io.github.oreo1298.Retort >/dev/null 2>&1; then
  cli_cmd=(flatpak run --command=retort-cli io.github.oreo1298.Retort)
  ffmpeg_bin=""  # the Flatpak uses its bundled ffmpeg; recorded via flatpak below
fi

# --- ffmpeg outputs (same layout as tests/fixtures/ffmpeg/<name>/) ----------
ff() {
  if [ -n "$ffmpeg_bin" ]; then "$ffmpeg_bin" -hide_banner "$@";
  elif [ ${#cli_cmd[@]} -gt 0 ] && [ "${cli_cmd[0]}" = flatpak ]; then
    flatpak run --command=ffmpeg io.github.oreo1298.Retort -hide_banner "$@"
  else return 1; fi
}
if ff -version >/dev/null 2>&1; then
  say "Recording ffmpeg capabilities"
  ff -version >"$out/ffmpeg/version.txt" 2>&1
  ff -buildconf >"$out/ffmpeg/buildconf.txt" 2>&1
  for l in encoders decoders hwaccels muxers demuxers filters pix_fmts; do
    ff "-$l" >"$out/ffmpeg/$l.txt" 2>/dev/null
  done
  # -h encoder= for every hardware encoder and the software ones we use.
  sed -n 's/^ [VAS][F.][S.][X.][B.][D.] \([^ ]*\).*/\1/p' "$out/ffmpeg/encoders.txt" |
    grep -E '_(nvenc|vaapi|qsv|vulkan|amf|v4l2m2m|rkmpp|mediacodec|omx)$|^lib(x264|x265|svtav1|aom-av1|vpx|vpx-vp9|opus|mp3lame|vorbis|webp|webp_anim)$|^(aac|flac|alac|prores_ks|dnxhd|gif|png|mjpeg|mpeg2video|mpeg4|mov_text|srt)$' |
    while read -r e; do ff -h "encoder=$e" >"$out/ffmpeg/help_encoder/$e.txt" 2>&1; done
  for m in mp4 matroska webm mov ipod opus ogg mp3 flac wav gif webp apng mpegts mxf avi; do
    ff -h "muxer=$m" >"$out/ffmpeg/help_muxer/$m.txt" 2>&1
  done
else
  say "ffmpeg not found — skipping ffmpeg capabilities"
fi

# --- system snapshot for the device scanner ---------------------------------
say "Snapshotting /sys, /proc and /dev (device facts only)"
r="$out/root"
mkdir -p "$r/etc" "$r/proc/sys/kernel" "$r/dev/dri" "$r/usr/share/hwdata"
cp /etc/os-release "$r/etc/os-release" 2>/dev/null || cp /usr/lib/os-release "$r/etc/os-release" 2>/dev/null
cat /proc/sys/kernel/osrelease >"$r/proc/sys/kernel/osrelease"
ids=""
for node in /sys/class/drm/renderD*; do
  [ -e "$node" ] || continue
  n=$(basename "$node")
  d="$r/sys/class/drm/$n/device"
  mkdir -p "$d"
  for f in vendor device subsystem_vendor subsystem_device boot_vga uevent; do
    [ -r "$node/device/$f" ] && cat "$node/device/$f" >"$d/$f" 2>/dev/null
  done
  # Record whether *this user* can open the node ("denied" marks no access).
  if [ -r "/dev/dri/$n" ] && [ -w "/dev/dri/$n" ]; then : >"$r/dev/dri/$n"; else echo denied >"$r/dev/dri/$n"; fi
  v=$(sed 's/0x//' "$node/device/vendor" 2>/dev/null); p=$(sed 's/0x//' "$node/device/device" 2>/dev/null)
  [ -n "$v" ] && ids="$ids $v:$p"
done
if [ -d /proc/driver/nvidia ]; then
  mkdir -p "$r/proc/driver/nvidia/gpus"
  cat /proc/driver/nvidia/version >"$r/proc/driver/nvidia/version" 2>/dev/null
  for g in /proc/driver/nvidia/gpus/*; do
    [ -e "$g/information" ] || continue
    mkdir -p "$r/proc/driver/nvidia/gpus/$(basename "$g")"
    cat "$g/information" >"$r/proc/driver/nvidia/gpus/$(basename "$g")/information"
  done
fi
for n in /dev/nvidiactl /dev/nvidia[0-9]*; do
  [ -e "$n" ] || continue
  if [ -r "$n" ] && [ -w "$n" ]; then : >"$r$n"; else echo denied >"$r$n"; fi
done
for v in /sys/class/video4linux/video*; do
  [ -e "$v/name" ] || continue
  mkdir -p "$r/sys/class/video4linux/$(basename "$v")"
  cat "$v/name" >"$r/sys/class/video4linux/$(basename "$v")/name"
  : >"$r/dev/$(basename "$v")"
done
# Only the pci.ids lines for this machine's GPUs (keeps the report small).
pciids=""
for f in /usr/share/hwdata/pci.ids /usr/share/misc/pci.ids /usr/share/pci.ids; do [ -r "$f" ] && pciids=$f && break; done
if [ -n "$pciids" ]; then
  for id in $ids; do
    vend=${id%%:*}; dev=${id##*:}
    awk -v V="$vend" -v D="$dev" '
      /^[0-9a-f][0-9a-f][0-9a-f][0-9a-f]  / { inv = ($1 == V); if (inv) vline = $0; next }
      inv && $0 ~ "^\t" D "  " { print vline; print; exit }' "$pciids"
  done >"$r/usr/share/hwdata/pci.ids"
fi
have nvidia-smi && nvidia-smi --query-gpu=index,name,pci.bus_id,driver_version,uuid --format=csv,noheader >"$out/nvidia-smi.csv" 2>/dev/null

# --- raw diagnostics for humans ---------------------------------------------
say "Recording vainfo / nvidia-smi / vulkaninfo / lspci"
raw="$out/raw"
cap "$raw/uname.txt" uname -a
cap "$raw/groups.txt" id
cap "$raw/dev-dri.txt" ls -l /dev/dri /dev/dri/by-path
cap "$raw/dev-nvidia.txt" ls -l /dev/nvidia*
have lspci && cap "$raw/lspci.txt" lspci -nnk
for node in /dev/dri/renderD*; do
  [ -e "$node" ] || continue
  have vainfo && cap "$raw/vainfo-$(basename "$node").txt" vainfo --display drm --device "$node"
done
have nvidia-smi && cap "$raw/nvidia-smi-q.txt" nvidia-smi -q
have vulkaninfo && cap "$raw/vulkaninfo-summary.txt" vulkaninfo --summary
[ -f /.flatpak-info ] && cp /.flatpak-info "$raw/flatpak-info.txt"

# --- Retort's own probe ------------------------------------------------------
if [ ${#cli_cmd[@]} -gt 0 ]; then
  say "Running ${cli_cmd[*]} (hardware probe, ~10-30 s)"
  "${cli_cmd[@]}" probe-hw --force --profile-spec >"$out/profile.json" 2>"$raw/probe-hw.stderr" || true
  "${cli_cmd[@]}" probe-hw --json >"$raw/probe-hw.json" 2>/dev/null || true
  "${cli_cmd[@]}" diagnose --json >"$raw/diagnose.json" 2>/dev/null || true
  "${cli_cmd[@]}" diagnose >"$raw/diagnose.txt" 2>&1 || true
else
  say "retort-cli not found: build it (cargo build --release -p retort-cli) or set RETORT_CLI, then re-run"
fi

tar czf "$out.tar.gz" "$out"
say "Done: $out (and $out.tar.gz)"
echo "Please look through the files (nothing secret should be in them), then attach"
echo "  $out.tar.gz"
echo "to a GitHub issue (\"Hardware report\"), or commit the folder as tests/fixtures/hw/<machine-name>/."
