// SPDX-License-Identifier: GPL-3.0-or-later
//! The pipeline planner: for a job, choose the best plan and its fallback
//! chain (chosen hardware → another verified hardware backend → software),
//! and plan every audio/subtitle stream.

use serde::{Deserialize, Serialize};

use crate::catalog::{codecs, compat, FormatInfo, FormatKind};
use crate::error::{Error, Result};
use crate::ffmpeg::{Caps, Ffmpeg, MediaKind};
use crate::hw::{Backend, DeviceRef, EncoderCap, Inventory};
use crate::media::{Hdr, MediaInfo, StreamInfo};
use crate::preset::container_for_audio_codec;
use crate::quality::{RateControl, RcMode, Speed};
use crate::settings::*;

/// User preferences that steer hardware selection.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct HwPrefs {
    /// Master switch (Preferences → "Use hardware acceleration").
    pub disable_hardware: bool,
    /// Try this backend first when several work.
    pub preferred_backend: Option<Backend>,
    /// Only use this GPU (`GpuDevice::id`); None = automatic.
    pub device: Option<String>,
}

/// Where decoding happens.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "on", rename_all = "snake_case")]
pub enum DecodeOn {
    Cpu,
    /// Hardware decode. `keep_frames`: frames stay on the GPU (zero-copy).
    Gpu {
        backend: Backend,
        device: DeviceRef,
        keep_frames: bool,
    },
}

/// How HDR → SDR conversion is done, if needed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "via", rename_all = "snake_case")]
pub enum Tonemap {
    /// zscale + tonemap on the CPU.
    Zscale,
    /// libplacebo on a Vulkan device (GPU), frames to/from system memory.
    Libplacebo { device: DeviceRef },
    /// `tonemap_vaapi` inside a full VAAPI pipeline.
    Vaapi,
}

/// Kind of video pipeline.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKind {
    /// Decode, filter and encode on one GPU.
    FullGpu,
    /// GPU decode → CPU filters → GPU encode.
    Hybrid,
    /// CPU decode/filters → GPU encode.
    GpuEncode,
    /// All CPU.
    Software,
}

/// The video part of a plan.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoPlan {
    pub input_stream: u32,
    pub codec: String,
    pub encoder: String,
    pub backend: Option<Backend>,
    pub device: Option<DeviceRef>,
    pub decode: DecodeOn,
    pub kind: PipelineKind,
    pub ten_bit: bool,
    /// Keep HDR signalling (colour tags + static metadata).
    pub keep_hdr: bool,
    pub tonemap: Option<Tonemap>,
    pub deinterlace: bool,
    /// Probed RC modes for hardware encoders.
    pub rc_modes: Vec<RcMode>,
    /// B-frames supported (None = unknown).
    pub bframes: Option<bool>,
    /// Software pixel format fed to a software encoder (or uploaded).
    pub pix_fmt: Option<String>,
}

/// One complete way to run the job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Plan {
    /// None = video copied, dropped or absent.
    pub video: Option<VideoPlan>,
    /// "Decode: NVDEC · Filters: CUDA · Encode: NVENC AV1 on RTX 4090"
    pub summary: String,
    /// "⚡ NVENC · RTX 4090" or "CPU".
    pub badge: String,
    pub hardware: bool,
}

/// What happens to one input audio stream.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum AudioAction {
    Copy,
    Encode {
        codec: String,
        encoder: String,
        /// Bitrate for this stream (already scaled for channels).
        kbps: Option<u32>,
        vbr_quality: Option<f32>,
        channels: Option<u32>,
        sample_rate: Option<u32>,
        sample_fmt: Option<String>,
        loudnorm: Option<Loudnorm>,
        /// Force a standard channel layout (libopus rejects e.g. "5.1(side)").
        normalize_layout: bool,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioStreamPlan {
    pub input_stream: u32,
    pub action: AudioAction,
    pub language: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "action", rename_all = "snake_case")]
pub enum SubtitleAction {
    Copy,
    Convert { codec: String, encoder: String },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct SubtitleStreamPlan {
    pub input_stream: u32,
    pub action: SubtitleAction,
    pub language: Option<String>,
}

/// Burn-in request resolved to a stream.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BurnIn {
    pub input_stream: u32,
    /// Index among the input's subtitle streams (for the `subtitles` filter's `si`).
    pub subtitle_ordinal: u32,
    pub bitmap: bool,
}

/// The planner's full answer for a job.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct JobPlan {
    pub format: FormatInfo,
    /// Fallback chain; at least one entry. Copy-only jobs have a single plan.
    pub chain: Vec<Plan>,
    /// The encoder was pinned by the user: no automatic fallback.
    pub pinned: bool,
    pub video_copy: Option<u32>,
    pub audio: Vec<AudioStreamPlan>,
    pub subtitles: Vec<SubtitleStreamPlan>,
    pub burn_in: Option<BurnIn>,
    /// Cover art stream to carry into an audio output.
    pub cover_art: Option<u32>,
    pub copy_attachments: bool,
    /// Bitrate for target-size encodes (kb/s).
    pub target_kbps: Option<u32>,
    /// Use two passes (software encoders with pass support).
    pub two_pass: bool,
    /// Human notes ("Converting HDR to SDR because…").
    pub notes: Vec<String>,
}

/// Inputs to planning.
pub struct PlanContext<'a> {
    pub ff: &'a Ffmpeg,
    pub caps: &'a Caps,
    pub inventory: &'a Inventory,
    pub formats: &'a [FormatInfo],
    /// Container compatibility (curated or empirical).
    pub can_hold: &'a dyn Fn(&FormatInfo, MediaKind, &str) -> bool,
    pub prefs: &'a HwPrefs,
}

/// Codecs that can carry HDR (10-bit, BT.2020 PQ/HLG) in our pipelines.
pub fn codec_keeps_hdr(codec: &str) -> bool {
    matches!(codec, "hevc" | "av1" | "vp9" | "prores" | "ffv1")
}

/// Containers whose muxers write colour signalling and HDR metadata.
pub fn container_keeps_hdr(format_id: &str) -> bool {
    matches!(
        format_id,
        "mp4" | "mkv" | "mov" | "webm" | "ts" | "m2ts" | "m4v" | "nut"
    )
}

/// Codecs whose encoders can produce 10-bit output.
fn codec_supports_10bit(codec: &str) -> bool {
    matches!(
        codec,
        "hevc" | "av1" | "vp9" | "h264" | "prores" | "ffv1" | "dnxhd"
    )
}

/// Other hardware encoders tried after the chosen one fails.
const MAX_EXTRA_HW_BACKENDS: usize = 2;

/// Backend preference order when several verified backends can do the job.
fn backend_rank(b: Backend) -> u8 {
    match b {
        Backend::Nvenc => 0,
        Backend::Qsv => 1,
        Backend::Vaapi => 2,
        Backend::Vulkan => 3,
        Backend::Amf => 4,
        Backend::V4l2m2m => 5,
        Backend::Rkmpp => 6,
        Backend::Other => 7,
    }
}

/// Short device name for badges: "NVIDIA GeForce RTX 4090" → "RTX 4090".
pub fn short_device_name(label: &str) -> String {
    let mut s = label.to_string();
    for prefix in [
        "NVIDIA GeForce ",
        "NVIDIA ",
        "Intel ",
        "AMD Radeon ",
        "AMD ",
    ] {
        if let Some(rest) = s.strip_prefix(prefix) {
            s = rest.to_string();
            break;
        }
    }
    s.trim_end_matches(" Graphics").to_string()
}

/// Friendly encoder name: libx265 → "x265", hevc_nvenc → "NVENC HEVC".
pub fn encoder_display(encoder: &str, codec: &str) -> String {
    if let Some(b) = Backend::from_codec_name(encoder) {
        return format!("{} {}", b.label(), crate::media::codec_display_name(codec));
    }
    match encoder {
        "libx264" => "x264".into(),
        "libx265" => "x265".into(),
        "libsvtav1" => "SVT-AV1".into(),
        "libaom-av1" => "libaom AV1".into(),
        "libvpx-vp9" => "libvpx VP9".into(),
        "libvpx" => "libvpx VP8".into(),
        "prores_ks" => "ProRes (prores_ks)".into(),
        other => other.into(),
    }
}

/// Resolve the output format (handles `auto-audio`).
pub fn resolve_format(
    settings: &OutputSettings,
    media: &MediaInfo,
    formats: &[FormatInfo],
) -> Result<FormatInfo> {
    let id = if settings.format == "auto-audio" {
        let a = media
            .primary_audio()
            .ok_or_else(|| Error::InvalidSettings("the file has no audio".into()))?;
        container_for_audio_codec(&a.codec).to_string()
    } else {
        settings.format.clone()
    };
    formats
        .iter()
        .find(|f| f.id == id)
        .cloned()
        .ok_or_else(|| Error::InvalidSettings(format!("this FFmpeg cannot write the {id} format")))
}

fn select_tracks<'a>(
    streams: Vec<&'a StreamInfo>,
    sel: &TrackSelection,
    primary: Option<&'a StreamInfo>,
) -> Vec<&'a StreamInfo> {
    match sel {
        TrackSelection::All => streams,
        TrackSelection::Primary => primary.into_iter().collect(),
        TrackSelection::Streams { indices } => streams
            .into_iter()
            .filter(|s| indices.contains(&s.index))
            .collect(),
        TrackSelection::Languages { languages } => streams
            .into_iter()
            .filter(|s| match &s.language {
                Some(l) => languages.iter().any(|x| x.eq_ignore_ascii_case(l)),
                None => true,
            })
            .collect(),
    }
}

/// Audio bitrate for `channels` given a per-stereo-pair setting.
pub fn scale_audio_kbps(kbps: u32, channels: u32) -> u32 {
    match channels {
        0..=1 => (kbps / 2).max(48),
        2 => kbps,
        _ => kbps * 2,
    }
}

fn plan_audio(
    ctx: &PlanContext,
    media: &MediaInfo,
    s: &OutputSettings,
    fmt: &FormatInfo,
    notes: &mut Vec<String>,
) -> Result<Vec<AudioStreamPlan>> {
    if matches!(s.audio, AudioSettings::Drop)
        || matches!(
            fmt.kind,
            FormatKind::Image | FormatKind::Animation | FormatKind::Subtitle
        )
    {
        return Ok(Vec::new());
    }
    let rules = compat::curated(&fmt.id);
    if rules.map(|r| r.audio.is_empty()).unwrap_or(false) {
        return Ok(Vec::new());
    }
    let tracks = select_tracks(
        media.audio_streams().collect(),
        &s.audio_tracks,
        media.primary_audio(),
    );
    let max_streams = rules.and_then(|r| r.max_streams).unwrap_or(u32::MAX);
    let mut out = Vec::new();
    for st in tracks {
        let channels = st.audio.as_ref().map(|a| a.channels).unwrap_or(2);
        let encode_with = |codec: &str, a: Option<&AudioEncode>| -> Result<AudioAction> {
            let encoder = a
                .and_then(|a| a.encoder.clone())
                .or_else(|| codecs::best_software_encoder(ctx.caps, codec))
                .ok_or_else(|| Error::NoEncoder(codecs::title(codec)))?;
            let out_ch = a
                .and_then(|a| a.channels)
                .unwrap_or(channels)
                .min(channels.max(1));
            let lossless = codecs::meta(codec).map(|m| m.lossless).unwrap_or(false);
            let kbps = if lossless {
                None
            } else {
                a.and_then(|a| a.bitrate_kbps)
                    .or(if a.and_then(|a| a.vbr_quality).is_some() {
                        None
                    } else {
                        Some(160)
                    })
                    .map(|k| scale_audio_kbps(k, out_ch))
            };
            Ok(AudioAction::Encode {
                codec: codec.to_string(),
                kbps,
                vbr_quality: a.and_then(|a| a.vbr_quality),
                channels: a.and_then(|a| a.channels).filter(|c| *c < channels),
                sample_rate: a.and_then(|a| a.sample_rate),
                sample_fmt: a.and_then(|a| a.sample_fmt.clone()),
                loudnorm: a.and_then(|a| a.loudnorm),
                normalize_layout: encoder == "libopus" && out_ch > 2,
                encoder,
            })
        };
        let action = match &s.audio {
            AudioSettings::Copy => {
                if (ctx.can_hold)(fmt, MediaKind::Audio, &st.codec) {
                    AudioAction::Copy
                } else {
                    let target = rules
                        .and_then(|r| r.default_audio)
                        .map(String::from)
                        .or_else(|| ctx.ff.muxer_help(&fmt.muxer).and_then(|h| h.default_audio))
                        .ok_or_else(|| {
                            Error::InvalidSettings(format!(
                                "{} audio can't be stored in {}",
                                codecs::title(&st.codec),
                                fmt.title
                            ))
                        })?;
                    notes.push(format!(
                        "{} audio can't be copied into {}; converting to {}",
                        crate::media::codec_display_name(&st.codec),
                        fmt.title,
                        codecs::title(&target)
                    ));
                    encode_with(&target, None)?
                }
            }
            AudioSettings::Encode(a) => {
                if !(ctx.can_hold)(fmt, MediaKind::Audio, &a.codec) {
                    return Err(Error::InvalidSettings(format!(
                        "{} can't hold {} audio",
                        fmt.title,
                        codecs::title(&a.codec)
                    )));
                }
                encode_with(&a.codec, Some(a))?
            }
            AudioSettings::Drop => unreachable!(),
        };
        out.push(AudioStreamPlan {
            input_stream: st.index,
            action,
            language: st.language.clone(),
        });
        if out.len() as u32 >= max_streams {
            break;
        }
    }
    Ok(out)
}

fn plan_subtitles(
    ctx: &PlanContext,
    media: &MediaInfo,
    s: &OutputSettings,
    fmt: &FormatInfo,
    notes: &mut Vec<String>,
) -> (Vec<SubtitleStreamPlan>, Option<BurnIn>) {
    let all: Vec<&StreamInfo> = media.subtitle_streams().collect();
    if all.is_empty() {
        return (Vec::new(), None);
    }
    let rules = compat::curated(&fmt.id);
    match &s.subtitles {
        SubtitleSettings::Drop => (Vec::new(), None),
        SubtitleSettings::BurnIn { stream } => {
            let pick = stream
                .and_then(|i| all.iter().position(|s| s.index == i))
                .or_else(|| all.iter().position(|s| s.forced))
                .or_else(|| all.iter().position(|s| s.default))
                .unwrap_or(0);
            let st = all[pick];
            (
                Vec::new(),
                Some(BurnIn {
                    input_stream: st.index,
                    subtitle_ordinal: pick as u32,
                    bitmap: st.is_bitmap_subtitle(),
                }),
            )
        }
        SubtitleSettings::Auto | SubtitleSettings::Convert { .. } => {
            if matches!(
                fmt.kind,
                FormatKind::Image | FormatKind::Animation | FormatKind::Audio
            ) {
                return (Vec::new(), None);
            }
            let tracks = select_tracks(all, &s.subtitle_tracks, media.subtitle_streams().next());
            let forced_target = match &s.subtitles {
                SubtitleSettings::Convert { codec } => Some(codec.clone()),
                _ => None,
            };
            let default_target = rules
                .and_then(|r| r.default_subtitle)
                .map(String::from)
                .or_else(|| {
                    ctx.ff
                        .muxer_help(&fmt.muxer)
                        .and_then(|h| h.default_subtitle)
                });
            let mut out = Vec::new();
            let mut dropped = 0;
            for st in tracks {
                let target = forced_target.clone();
                let action = match target {
                    None if (ctx.can_hold)(fmt, MediaKind::Subtitle, &st.codec) => {
                        Some(SubtitleAction::Copy)
                    }
                    t => {
                        let codec = t.or_else(|| default_target.clone());
                        match codec {
                            Some(c)
                                if !st.is_bitmap_subtitle()
                                    && (ctx.can_hold)(fmt, MediaKind::Subtitle, &c) =>
                            {
                                codecs::best_software_encoder(ctx.caps, &c)
                                    .map(|encoder| SubtitleAction::Convert { codec: c, encoder })
                            }
                            _ => None,
                        }
                    }
                };
                match action {
                    Some(a) => out.push(SubtitleStreamPlan {
                        input_stream: st.index,
                        action: a,
                        language: st.language.clone(),
                    }),
                    None => dropped += 1,
                }
            }
            if dropped > 0 {
                notes.push(format!(
                    "{dropped} subtitle track(s) can't be stored in {} and will be left out (use burn-in or MKV to keep them)",
                    fmt.title
                ));
            }
            (out, None)
        }
    }
}

/// Plan a job.
pub fn plan(ctx: &PlanContext, media: &MediaInfo, settings: &OutputSettings) -> Result<JobPlan> {
    let format = resolve_format(settings, media, ctx.formats)?;
    let mut notes = Vec::new();
    let audio = plan_audio(ctx, media, settings, &format, &mut notes)?;
    let (subtitles, burn_in) = plan_subtitles(ctx, media, settings, &format, &mut notes);

    let rules = compat::curated(&format.id);
    let container_takes_video = rules.map(|r| !r.video.is_empty()).unwrap_or(true)
        && !matches!(format.kind, FormatKind::Audio | FormatKind::Subtitle);
    let source = media.primary_video();

    // Cover art into audio outputs.
    let cover_art = if settings.metadata.keep_cover_art
        && matches!(format.id.as_str(), "mp3" | "m4a" | "flac")
    {
        media
            .streams
            .iter()
            .find(|s| s.attached_pic && matches!(s.codec.as_str(), "mjpeg" | "png"))
            .map(|s| s.index)
    } else {
        None
    };
    let copy_attachments = format.id == "mkv"
        && media.attachments().next().is_some()
        && subtitles.iter().any(|s| s.action == SubtitleAction::Copy);

    let mut job = JobPlan {
        format: format.clone(),
        chain: Vec::new(),
        pinned: false,
        video_copy: None,
        audio,
        subtitles,
        burn_in,
        cover_art,
        copy_attachments,
        target_kbps: None,
        two_pass: false,
        notes,
    };

    let copy_plan = |summary: &str| Plan {
        video: None,
        summary: summary.to_string(),
        badge: "CPU".into(),
        hardware: false,
    };

    let video_wanted = container_takes_video && source.is_some();
    match (&settings.video, video_wanted) {
        (VideoSettings::Drop, _) | (_, false) => {
            if job.audio.is_empty() && job.subtitles.is_empty() && cover_art.is_none() {
                return Err(Error::InvalidSettings(
                    "nothing to convert: no streams would be written".into(),
                ));
            }
            let all_copy = job.audio.iter().all(|a| a.action == AudioAction::Copy);
            job.chain.push(copy_plan(if all_copy {
                "Copy streams (no re-encode)"
            } else {
                "Audio: CPU"
            }));
        }
        (VideoSettings::Copy, true) => {
            let v = source.expect("video");
            if !(ctx.can_hold)(&format, MediaKind::Video, &v.codec) {
                return Err(Error::InvalidSettings(format!(
                    "{} video can't be copied into {}; choose re-encode",
                    crate::media::codec_display_name(&v.codec),
                    format.title
                )));
            }
            if job.burn_in.is_some() {
                return Err(Error::InvalidSettings(
                    "burning in subtitles requires re-encoding the video".into(),
                ));
            }
            job.video_copy = Some(v.index);
            if let Some(Hdr::DolbyVision { .. }) = v.video.as_ref().map(|x| &x.hdr) {
                if !matches!(format.id.as_str(), "mkv" | "mp4" | "mov") {
                    job.notes
                        .push("Dolby Vision configuration may be lost in this container".into());
                }
            }
            job.chain.push(copy_plan("Video: copy (instant, lossless)"));
        }
        (VideoSettings::Encode(v), true) => {
            let src = source.expect("video");
            plan_video(ctx, media, src, settings, v, &format, &mut job)?;
        }
    }
    Ok(job)
}

fn audio_total_kbps(job: &JobPlan, media: &MediaInfo) -> u32 {
    job.audio
        .iter()
        .map(|a| match &a.action {
            AudioAction::Encode { kbps, .. } => kbps.unwrap_or(192),
            AudioAction::Copy => media
                .streams
                .iter()
                .find(|s| s.index == a.input_stream)
                .and_then(|s| s.bit_rate)
                .map(|b| (b / 1000) as u32)
                .unwrap_or(192),
        })
        .sum()
}

fn plan_video(
    ctx: &PlanContext,
    media: &MediaInfo,
    src: &StreamInfo,
    settings: &OutputSettings,
    v: &VideoEncode,
    format: &FormatInfo,
    job: &mut JobPlan,
) -> Result<()> {
    let vi = src.video.clone().unwrap_or_default();
    let codec = format
        .implied_video_codec
        .as_ref()
        .map(|c| {
            // image2 entries imply the codec id via their encoder name.
            match c.as_str() {
                "libwebp" => "webp".to_string(),
                "libjxl" => "jpegxl".to_string(),
                other => other.to_string(),
            }
        })
        .unwrap_or_else(|| v.codec.clone());
    if format.implied_video_codec.is_none() && !(ctx.can_hold)(format, MediaKind::Video, &codec) {
        return Err(Error::InvalidSettings(format!(
            "{} can't hold {} video",
            format.title,
            codecs::title(&codec)
        )));
    }

    // HDR and bit depth.
    let src_hdr = vi.hdr.base_layer();
    let hdr_possible = codec_keeps_hdr(&codec)
        && container_keeps_hdr(&format.id)
        && v.bit_depth != BitDepth::Eight;
    let keep_hdr = src_hdr.is_hdr() && v.hdr != HdrMode::Tonemap && hdr_possible;
    let needs_tonemap = src_hdr.is_hdr() && !keep_hdr;
    if src_hdr.is_hdr() && v.hdr == HdrMode::Preserve && !keep_hdr {
        return Err(Error::InvalidSettings(format!(
            "HDR can't be preserved with {} in {} at 8-bit; choose HEVC/AV1/VP9 in MP4/MKV",
            codecs::title(&codec),
            format.title
        )));
    }
    if needs_tonemap {
        job.notes.push(if v.hdr == HdrMode::Tonemap {
            "Converting HDR to SDR (tone mapping)".into()
        } else {
            format!(
                "{} in {} can't keep HDR; tone mapping to SDR",
                codecs::title(&codec),
                format.title
            )
        });
    }
    if let Hdr::DolbyVision { .. } = vi.hdr {
        job.notes.push("Dolby Vision dynamic metadata is dropped when re-encoding; the HDR10/HLG base layer is kept".into());
    }
    let ten_bit = match v.bit_depth {
        BitDepth::Eight => false,
        BitDepth::Ten => codec_supports_10bit(&codec),
        BitDepth::Auto => {
            keep_hdr || (vi.bit_depth > 8 && codec_supports_10bit(&codec) && codec != "h264")
        }
    };
    let deinterlace = match v.filters.deinterlace {
        Deinterlace::On => true,
        Deinterlace::Off => false,
        Deinterlace::Auto => vi.interlaced(),
    };
    let animated = matches!(format.kind, FormatKind::Animation | FormatKind::Image);

    // Rate control / passes.
    let mut two_pass = v.two_pass;
    if let RateControl::TargetSize { bytes } = v.rate {
        let dur = settings
            .trim
            .and_then(|t| t.duration(media.duration))
            .or(media.duration)
            .filter(|d| *d > 0.0)
            .ok_or_else(|| Error::InvalidSettings("target size needs a known duration".into()))?;
        let total = (bytes as f64 * 8.0 / dur / 1000.0) * 0.97;
        let video_kbps = total - audio_total_kbps(job, media) as f64;
        if video_kbps < 30.0 {
            return Err(Error::InvalidSettings(format!(
                "{:.1} MB is too small for {} — even the audio needs more",
                bytes as f64 / 1e6,
                crate::media::format_duration(dur)
            )));
        }
        if video_kbps < 300.0 {
            job.notes
                .push("The size limit is very tight for this length; expect low quality".into());
        }
        // Never spend more than a very-high-quality bitrate: "under N MB" is
        // a ceiling, not a goal, and short clips would otherwise get absurd rates.
        let (w, h) = match v.filters.scale {
            Some(sc) => crate::command::fit_size(vi.display_size().0, vi.display_size().1, &sc),
            None => vi.display_size(),
        };
        let ceiling = crate::quality::bitrate_for_quality(95, w, h, vi.fps.unwrap_or(30.0)) as f64;
        job.target_kbps = Some(video_kbps.min(ceiling) as u32);
        two_pass = true;
    }

    // Candidates.
    let sw = match &v.encoder {
        EncoderChoice::Pinned { encoder, .. } if Backend::from_codec_name(encoder).is_none() => {
            Some(encoder.clone())
        }
        _ => codecs::best_software_encoder(ctx.caps, &codec),
    };
    let pinned = matches!(v.encoder, EncoderChoice::Pinned { .. });
    let mut hw: Vec<&EncoderCap> = Vec::new();
    let hw_allowed =
        !ctx.prefs.disable_hardware && !animated && media.has_video() && !media.is_still_image();
    if hw_allowed {
        hw = ctx
            .inventory
            .working_encoders(&codec, ten_bit)
            .filter(|e| match &ctx.prefs.device {
                Some(id) => e.device.gpu_id.as_deref() == Some(id.as_str()),
                None => true,
            })
            .collect();
        if hw.is_empty()
            && ten_bit
            && ctx
                .inventory
                .working_encoders(&codec, false)
                .next()
                .is_some()
        {
            job.notes.push(format!(
                "No GPU encoder here supports 10-bit {}; encoding on the CPU",
                crate::media::codec_display_name(&codec)
            ));
        }
    }
    if let EncoderChoice::Pinned { encoder, device } = &v.encoder {
        if Backend::from_codec_name(encoder).is_some() {
            let caps_for: Vec<&EncoderCap> = ctx
                .inventory
                .probes
                .encoders
                .iter()
                .filter(|e| &e.encoder == encoder)
                .filter(|e| {
                    device
                        .as_ref()
                        .map(|d| e.device.gpu_id.as_deref() == Some(d.as_str()))
                        .unwrap_or(true)
                })
                .collect();
            hw = caps_for
                .iter()
                .copied()
                .filter(|e| e.works(ten_bit))
                .collect();
            if hw.is_empty() {
                job.notes.push(format!("{encoder} did not pass the hardware test on this machine; trying anyway because it was chosen"));
                hw = caps_for.into_iter().take(1).collect();
                if hw.is_empty() {
                    return Err(Error::NoEncoder(format!("{encoder} is not available")));
                }
            }
        }
    }
    // Rank: user-preferred backend, then devices that can run the whole
    // pipeline on the GPU, then our backend order.
    let mut ranked: Vec<(&EncoderCap, Vec<Plan>)> = hw
        .iter()
        .map(|cap| {
            (
                *cap,
                hardware_plans(
                    ctx,
                    media,
                    src,
                    &codec,
                    cap,
                    ten_bit,
                    keep_hdr,
                    needs_tonemap,
                    deinterlace,
                    v,
                    settings,
                ),
            )
        })
        .collect();
    ranked.sort_by_key(|(e, plans)| {
        let preferred = ctx.prefs.preferred_backend == Some(e.backend);
        let full = plans
            .first()
            .and_then(|p| p.video.as_ref())
            .map(|v| v.kind == PipelineKind::FullGpu)
            .unwrap_or(false);
        (!preferred, !full, backend_rank(e.backend), e.device.key())
    });

    // Two-pass and target size are software-only.
    let hw_for_rate = !(two_pass && sw.is_some());
    let mut chain: Vec<Plan> = Vec::new();
    let mk_sw = |chain: &mut Vec<Plan>| -> bool {
        let Some(enc) = sw.clone() else { return false };
        if pinned && Backend::from_codec_name(&enc).is_some() {
            return false;
        }
        chain.push(software_plan(
            ctx,
            src,
            &codec,
            &enc,
            ten_bit,
            keep_hdr,
            needs_tonemap,
            deinterlace,
            v,
        ));
        true
    };

    let pinned_sw = pinned
        && matches!(&v.encoder, EncoderChoice::Pinned { encoder, .. } if Backend::from_codec_name(encoder).is_none());
    if pinned_sw {
        mk_sw(&mut chain);
    } else {
        let sw_first = settings.speed == Speed::Smallest || !hw_for_rate;
        if sw_first {
            mk_sw(&mut chain);
        }
        if hw_for_rate || sw.is_none() {
            // Chosen hardware (best pipeline + a CPU-decode variant), then up
            // to two other verified hardware encoders (robust variant only).
            for (i, (_, plans)) in ranked.iter().enumerate() {
                let take: Vec<&Plan> = if i == 0 {
                    plans.iter().collect()
                } else {
                    plans.last().into_iter().collect()
                };
                for p in take {
                    if !chain.contains(p) {
                        chain.push(p.clone());
                    }
                }
                if i >= MAX_EXTRA_HW_BACKENDS {
                    break;
                }
            }
        }
        if !sw_first && !pinned {
            mk_sw(&mut chain);
        }
    }
    if pinned {
        chain.truncate(1);
        job.pinned = true;
    }
    if chain.is_empty() {
        return Err(Error::NoEncoder(format!(
            "{}{}",
            codecs::title(&codec),
            if ten_bit { " 10-bit" } else { "" }
        )));
    }
    if two_pass {
        let first = chain[0]
            .video
            .as_ref()
            .map(|p| p.encoder.as_str())
            .unwrap_or("");
        if supports_two_pass(first) {
            job.two_pass = true;
        } else if job.target_kbps.is_some() {
            job.notes.push(format!(
                "{} has no two-pass mode; using single-pass VBR (size may overshoot slightly)",
                encoder_display(first, &codec)
            ));
        }
    }
    job.chain = chain;
    Ok(())
}

/// Encoders that support `-pass 1/2`.
pub fn supports_two_pass(encoder: &str) -> bool {
    matches!(
        encoder,
        "libx264" | "libvpx-vp9" | "libvpx" | "libx265" | "mpeg4" | "mpeg2video" | "libaom-av1"
    )
}

/// Software pixel format for a codec/encoder at a bit depth.
fn sw_pix_fmt(
    ctx: &PlanContext,
    encoder: &str,
    codec: &str,
    ten_bit: bool,
    v: &VideoEncode,
    src: &StreamInfo,
) -> Option<String> {
    if let Some(p) = &v.pix_fmt {
        return Some(p.clone());
    }
    let src_fmt = src
        .video
        .as_ref()
        .and_then(|x| x.pix_fmt.clone())
        .unwrap_or_default();
    let wanted: Vec<&str> = match (codec, ten_bit) {
        ("prores", _) => match v.profile.as_deref() {
            Some("4" | "5" | "4444" | "4444xq") => vec!["yuv444p10le", "yuva444p10le"],
            _ => vec!["yuv422p10le"],
        },
        ("dnxhd", _) => match v.profile.as_deref() {
            Some("dnxhr_hqx") => vec!["yuv422p10le"],
            Some("dnxhr_444") => vec!["yuv444p10le"],
            _ => vec!["yuv422p"],
        },
        ("mjpeg", _) => vec!["yuvj420p", "yuv420p"],
        ("gif", _) | ("apng", _) | ("png", _) => return None,
        ("webp", _) => vec!["yuv420p", "yuva420p"],
        ("ffv1", _) => {
            return if src_fmt.is_empty() {
                None
            } else {
                Some(src_fmt)
            }
        }
        (_, true) => vec!["yuv420p10le"],
        (_, false) => vec!["yuv420p"],
    };
    let supported = ctx
        .ff
        .encoder_help(encoder)
        .map(|h| h.pix_fmts)
        .unwrap_or_default();
    wanted
        .iter()
        .find(|w| supported.is_empty() || supported.iter().any(|s| s == *w))
        .map(|s| s.to_string())
}

fn cpu_tonemap(ctx: &PlanContext) -> Option<Tonemap> {
    if ctx.caps.has_filter("zscale") && ctx.caps.has_filter("tonemap") {
        return Some(Tonemap::Zscale);
    }
    ctx.inventory
        .probes
        .filters
        .iter()
        .find(|f| f.filter == "libplacebo" && f.outcome.passed())
        .map(|f| Tonemap::Libplacebo {
            device: f.device.clone(),
        })
}

#[allow(clippy::too_many_arguments)]
fn software_plan(
    ctx: &PlanContext,
    src: &StreamInfo,
    codec: &str,
    encoder: &str,
    ten_bit: bool,
    keep_hdr: bool,
    needs_tonemap: bool,
    deinterlace: bool,
    v: &VideoEncode,
) -> Plan {
    let tonemap = if needs_tonemap {
        cpu_tonemap(ctx)
    } else {
        None
    };
    let summary = format!(
        "Decode: CPU · Filters: {} · Encode: {} (CPU)",
        match &tonemap {
            Some(Tonemap::Libplacebo { .. }) => "Vulkan (libplacebo)",
            _ => "CPU",
        },
        encoder_display(encoder, codec)
    );
    Plan {
        video: Some(VideoPlan {
            input_stream: src.index,
            codec: codec.to_string(),
            encoder: encoder.to_string(),
            backend: None,
            device: None,
            decode: DecodeOn::Cpu,
            kind: PipelineKind::Software,
            ten_bit,
            keep_hdr,
            tonemap,
            deinterlace,
            rc_modes: Vec::new(),
            bframes: None,
            pix_fmt: sw_pix_fmt(ctx, encoder, codec, ten_bit, v, src),
        }),
        summary,
        badge: "CPU".into(),
        hardware: false,
    }
}

/// Plans for one verified hardware encoder: the best pipeline first, then a
/// CPU-decode variant if the best one relies on GPU decoding.
#[allow(clippy::too_many_arguments)]
fn hardware_plans(
    ctx: &PlanContext,
    media: &MediaInfo,
    src: &StreamInfo,
    codec: &str,
    cap: &EncoderCap,
    ten_bit: bool,
    keep_hdr: bool,
    needs_tonemap: bool,
    deinterlace: bool,
    v: &VideoEncode,
    settings: &OutputSettings,
) -> Vec<Plan> {
    let vi = src.video.clone().unwrap_or_default();
    let backend = cap.backend;
    let dev = cap.device.clone();
    let can_decode = backend.hwaccel().is_some()
        && ctx
            .inventory
            .can_decode(backend, &dev, &src.codec, vi.bit_depth.max(8));
    let depth_change = (vi.bit_depth > 8) != ten_bit;
    let scaling = v.filters.scale.is_some();
    let scale_ok = backend
        .scale_filter()
        .map(|f| ctx.inventory.filter_ok(backend, &dev, f))
        .unwrap_or(false);
    let gpu_tonemap = match (needs_tonemap, backend) {
        (false, _) => true,
        (true, Backend::Vaapi) => ctx.inventory.filter_ok(backend, &dev, "tonemap_vaapi"),
        _ => false,
    };
    let full_gpu = can_decode
        && !v.filters.needs_cpu()
        && !deinterlace
        && vi.rotation == 0
        && vi.width % 2 == 0
        && vi.height % 2 == 0
        && gpu_tonemap
        && (!(scaling || depth_change) || scale_ok)
        && !matches!(settings.subtitles, SubtitleSettings::BurnIn { .. })
        && !media.is_still_image();
    let tonemap_cpu = if needs_tonemap {
        cpu_tonemap(ctx)
    } else {
        None
    };
    let name = short_device_name(&dev.label);
    let enc_name = encoder_display(&cap.encoder, codec);
    let badge = if dev.label.is_empty() {
        format!("⚡ {}", backend.label())
    } else {
        format!("⚡ {} · {}", backend.label(), name)
    };
    let on = if dev.label.is_empty() {
        String::new()
    } else {
        format!(" on {}", dev.label)
    };
    let base = VideoPlan {
        input_stream: src.index,
        codec: codec.to_string(),
        encoder: cap.encoder.clone(),
        backend: Some(backend),
        device: Some(dev.clone()),
        decode: DecodeOn::Cpu,
        kind: PipelineKind::GpuEncode,
        ten_bit,
        keep_hdr,
        tonemap: tonemap_cpu.clone(),
        deinterlace,
        rc_modes: cap.rc_modes.clone(),
        bframes: cap.bframes,
        pix_fmt: None,
    };
    let filters_label = |gpu: bool, t: &Option<Tonemap>| -> String {
        if gpu {
            return match backend {
                Backend::Nvenc => "CUDA".into(),
                b => b.label().into(),
            };
        }
        match t {
            Some(Tonemap::Libplacebo { .. }) => "Vulkan (libplacebo)".into(),
            _ => "CPU".into(),
        }
    };
    let mut plans = Vec::new();
    if full_gpu {
        let p = VideoPlan {
            decode: DecodeOn::Gpu {
                backend,
                device: dev.clone(),
                keep_frames: true,
            },
            kind: PipelineKind::FullGpu,
            tonemap: if needs_tonemap {
                Some(Tonemap::Vaapi)
            } else {
                None
            },
            ..base.clone()
        };
        plans.push(Plan {
            video: Some(p),
            summary: format!(
                "Decode: {} · Filters: {} · Encode: {enc_name}{on}",
                backend.decode_label(),
                filters_label(true, &None)
            ),
            badge: badge.clone(),
            hardware: true,
        });
    } else if can_decode && !media.is_still_image() {
        let p = VideoPlan {
            decode: DecodeOn::Gpu {
                backend,
                device: dev.clone(),
                keep_frames: false,
            },
            kind: PipelineKind::Hybrid,
            ..base.clone()
        };
        plans.push(Plan {
            video: Some(p),
            summary: format!(
                "Decode: {} · Filters: {} · Encode: {enc_name}{on}",
                backend.decode_label(),
                filters_label(false, &tonemap_cpu)
            ),
            badge: badge.clone(),
            hardware: true,
        });
    }
    // CPU decode → GPU encode (most robust hardware path).
    plans.push(Plan {
        video: Some(base),
        summary: format!(
            "Decode: CPU · Filters: {} · Encode: {enc_name}{on}",
            filters_label(false, &tonemap_cpu)
        ),
        badge,
        hardware: true,
    });
    plans
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names() {
        assert_eq!(short_device_name("NVIDIA GeForce RTX 4090"), "RTX 4090");
        assert_eq!(short_device_name("Intel Iris Xe Graphics"), "Iris Xe");
        assert_eq!(
            short_device_name("AMD Radeon RX 7900 XT/7900 XTX/7900 GRE/7900M"),
            "RX 7900 XT/7900 XTX/7900 GRE/7900M"
        );
        assert_eq!(encoder_display("av1_nvenc", "av1"), "NVENC AV1");
        assert_eq!(encoder_display("libx265", "hevc"), "x265");
        assert_eq!(scale_audio_kbps(192, 6), 384);
        assert_eq!(scale_audio_kbps(128, 1), 64);
    }
}
