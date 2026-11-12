# Encoder option research (verified against FFmpeg source)

All facts below were verified by reading the AVOption tables in the FFmpeg git
tags `n6.1.1`, `n7.1.5`, `n8.0.3` and `n9.0.2` (fetched 2026-11-12). Re-verify
when a new major FFmpeg is released: `scripts/research/ffmpeg-options.sh`
regenerates the tables.

Latest stable FFmpeg at the time of writing: **9.0.2 "Lei"**. Ubuntu 24.04
ships 6.1.1 (our floor).

## NVENC (`h264_nvenc`, `hevc_nvenc`, `av1_nvenc`)

| Option | 6.1 | 7.1 | 8.0 | 9.0 | Notes |
|---|---|---|---|---|---|
| `-preset p1..p7` | ✓ | ✓ | ✓ | ✓ | Only form we emit. |
| legacy presets `default hp hq bd ll llhq llhp lossless losslesshp slow medium fast` | ✓ | ✓ | ✓ | `slow/medium/fast` only | 9.0 removed `default hp bd ll* losslesshp` (as presets). Never emitted. |
| `-tune hq\|ll\|ull\|lossless` | ✓ | ✓ | ✓ | ✓ (+`uhq`) | `uhq` only on 9.0 (and needs SDK 12.2 + driver) — not used by default. |
| `-rc constqp\|vbr\|cbr` | ✓ | ✓ | ✓ | ✓ | |
| `-rc cbr_hq cbr_ld_hq vbr_hq vbr_2pass vbr_minqp ll_2pass_*` | ✓ | ✓ | ✓ | ✗ | Removed in 9.0. Never emitted. |
| `-2pass` | ✓ | ✓ | ✓ | ✗ | Use `-multipass qres\|fullres` instead (exists since 6.1). |
| `-spatial_aq` / `-temporal_aq` (underscore) | ✓ | ✓ | ✓ | ✗ | Use hyphenated `-spatial-aq` / `-temporal-aq` (all versions). |
| `-cq` (VBR target quality) | 0–51 | 0–51 (av1: 0–63) | same | same | AV1 range was 0–51 in 6.1, 0–63 from 7.1. |
| `-qp` (constqp) | 0–51 (av1 0–255) | | | | |
| `-highbitdepth` | hevc/av1 | hevc/av1 | + h264 | + h264 | h264 10-bit (High 10) only from 8.0 (`-profile high10`). |
| `-b_ref_mode`, `-lookahead_level` (7.1+), `-tf_level` (8.0+), `-split_encode_mode` (9.0) | | | | | Optional, gated. |

Device selection: `-gpu N` uses CUDA ordering; we set `CUDA_DEVICE_ORDER=PCI_BUS_ID`
on every ffmpeg child so indices match `nvidia-smi`.

## VAAPI (`*_vaapi`)

- `-rc_mode auto|CQP|CBR|VBR|ICQ|QVBR|AVBR` in all versions.
- Quality value comes from `-global_quality N` (used as QP in CQP, quality
  factor in ICQ/QVBR) in all versions; `-qp` exists for h264/hevc/vp9 but **not
  for av1_vaapi**, so we always use `-global_quality`.
- CQP: QP 1–51 (h264/hevc), 0–255 (av1 q_idx), 0–255 (vp9).
- ICQ: quality factor 1–51 (Intel iHD only; Mesa does not implement ICQ).
- `async_depth`, `b_depth`, `idr_interval` moved to `hw_base_encode.h` in 7.1
  (still the same CLI names).
- Input: `-vf format=nv12|p010,hwupload` with `-filter_hw_device`.

## QSV (`*_qsv`, oneVPL/libvpl)

Rate control is implicit (`qsvenc.c select_rc_mode`):
- `-global_quality N` with no bitrate and no qscale flag → **ICQ** (1–51).
- `-q:v N` (sets qscale flag) → **CQP** with QP N.
- `-look_ahead_depth` + `-global_quality` → LA_ICQ.
- `-preset veryfast..veryslow` (all versions).
Device: `-init_hw_device vaapi=va:/dev/dri/renderDN -init_hw_device qsv=qs@va`.
On Linux the QSV child device defaults to VAAPI with `vendor_id=0x8086,driver=iHD`.

## Vulkan Video (`h264_vulkan` 7.1+, `hevc_vulkan` 7.1+, `av1_vulkan` 8.0+)

- `-rc_mode auto|driver|cqp|cbr|vbr`, `-qp N` (constant QP), `-quality N`
  (speed/quality knob; its *description* flipped between 8.0 "higher is faster"
  and 9.0 "higher is slower" — we never set it), `-tune default|hq|ll|ull|lossless`.
- Device: derive from DRM node: `-init_hw_device drm=dr:/dev/dri/renderDN -init_hw_device vulkan=vk@dr`.
- Decode via `-hwaccel vulkan`: H.264/HEVC/AV1 since 6.1, VP9 since 8.0.

## AMF (`*_amf`)

- `-rc cqp|cbr|vbr_peak|vbr_latency|qvbr|hqvbr|hqcbr`, `-qp_i/-qp_p` (0–51; AV1 0–255),
  `-qvbr_quality_level` (0–51), `-quality speed|balanced|quality` (alias `-preset` from 7.1).
- Rare on Linux (needs AMD's proprietary AMF runtime). Only offered if a probe passes.

## Software encoders

| Encoder | Quality control | Speed control |
|---|---|---|
| libx264 | `-crf 0–51` | `-preset ultrafast..veryslow` |
| libx265 | `-crf 0–51` | `-preset ultrafast..veryslow`; HDR via `-x265-params` |
| libsvtav1 | `-crf 0–63` | `-preset 0–13`; extra via `-svtav1-params` (6.1 had `la_depth`, tile opts; 9.0 removed them — never emitted) |
| libaom-av1 | `-crf 0–63 -b:v 0` | `-cpu-used 0–8`, `-row-mt 1` |
| libvpx-vp9 | `-crf 0–63 -b:v 0` | `-deadline good -cpu-used 0–5`, `-row-mt 1` |
| libvpx (vp8) | `-crf 4–63 -b:v <cap>` | `-deadline good -cpu-used` |

## V4L2 M2M / RKMPP

- `*_v4l2m2m`: only `num_capture_buffers`, `qmin`, `qmax` + global `-b:v`. Bitrate-only RC.
- `*_rkmpp` encoders exist only in forks (e.g. nyanmisaka/ffmpeg-rockchip); treated generically.

## HDR static metadata (mastering display / content light) passthrough

Whether the encoder copies `AV_FRAME_DATA_MASTERING_DISPLAY_METADATA` /
`CONTENT_LIGHT_LEVEL` frame side data into the bitstream by itself
(verified by grepping each encoder source for `MASTERING_DISPLAY`):

| Encoder | 6.1 | 7.1 | 8.0 | 9.0 | What Retort does |
|---|---|---|---|---|---|
| hevc_vaapi | ✓ | ✓ | ✓ | ✓ | nothing extra |
| hevc_qsv | ✓ | ✓ | ✓ | ✓ | nothing extra |
| av1_vaapi, av1_qsv | ✗ | ✓ | ✓ | ✓ | warn on < 7.1 |
| hevc_vulkan, av1_vulkan | n/a | ✓ | ✓ | ✓ | nothing extra |
| libx265 | ✗ | ✓ | ✓ | ✓ | always pass `-x265-params hdr10=1:master-display=…:max-cll=…` |
| libsvtav1 | ✗ | ✓ | ✓ | ✓ | always pass `-svtav1-params mastering-display=…:content-light=…` |
| hevc_nvenc, av1_nvenc | ✗ | ✗ | ✓ | ✓ | warn on < 8.0 (colour tags still written) |
| libaom-av1 | ✗ | ✗ | ✗ | ✗ | warn (colour tags only) |

Colour signalling (`-color_primaries/-color_trc/-colorspace/-color_range`)
is always set explicitly when HDR is preserved.

## Muxing options

| Option | 6.1 | 9.0.2 | Notes |
|---|---|---|---|
| `-disposition:<a\|s>:<n> default\|0` | `set_dispositions()` in `fftools/ffmpeg_mux_init.c` evaluates the value with `av_opt_eval_flags` on the stream's `disposition` flags | per-stream `opt_match_per_stream_str(... &o->disposition ...)` then `av_opt_set(st, "disposition", ...)` | `0` clears all flags, `default` sets only "default". Used for the Advanced "default track" choice; checked by the `default_track_flags` integration test on 6.1 and 9.0.2. |
