// SPDX-License-Identifier: GPL-3.0-or-later
//! The full description of one output: container, per-stream-type choices,
//! filters, metadata policy and advanced extras. Presets build these; the
//! planner and command builder consume them. Serialised in the queue.

use serde::{Deserialize, Serialize};

use crate::quality::{RateControl, Speed};

/// Which encoder to use for a codec.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum EncoderChoice {
    /// Let the planner pick (hardware when verified, with fallback).
    #[default]
    Auto,
    /// Use exactly this encoder (no automatic fallback; offer "Retry with software").
    Pinned {
        encoder: String,
        /// `GpuDevice::id` to run on, if any.
        device: Option<String>,
    },
}

/// Output bit depth.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum BitDepth {
    /// Keep the source depth when the codec supports it (max 10-bit).
    #[default]
    Auto,
    Eight,
    Ten,
}

/// What to do with HDR sources.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum HdrMode {
    /// Preserve HDR when codec, bit depth and container allow it; otherwise tonemap.
    #[default]
    Auto,
    /// Preserve (fail planning if impossible).
    Preserve,
    /// Always convert to SDR.
    Tonemap,
}

/// Scaling request. Dimensions are maximums; aspect ratio is kept and
/// dimensions are rounded to even numbers.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Scale {
    pub width: Option<u32>,
    pub height: Option<u32>,
    /// Never upscale.
    pub only_shrink: bool,
}

/// Crop rectangle (in source pixels) or auto-detect.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Crop {
    /// Run cropdetect before encoding (black bars).
    Auto,
    Manual {
        w: u32,
        h: u32,
        x: u32,
        y: u32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Rotate {
    #[default]
    None,
    Cw90,
    Ccw90,
    R180,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum Deinterlace {
    /// Deinterlace when the source is flagged interlaced (or `idet` says so).
    #[default]
    Auto,
    Off,
    On,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Strength {
    Light,
    Medium,
    Strong,
}

/// Video filters (applied in a fixed, correct order by the command builder).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct VideoFilters {
    pub deinterlace: Deinterlace,
    pub crop: Option<Crop>,
    pub rotate: Rotate,
    pub flip_h: bool,
    pub flip_v: bool,
    pub scale: Option<Scale>,
    /// Output frame rate as "30" or "30000/1001".
    pub fps: Option<String>,
    pub denoise: Option<Strength>,
    pub sharpen: Option<Strength>,
}

impl VideoFilters {
    /// Filters that have no GPU equivalent in our pipelines (force hybrid).
    pub fn needs_cpu(&self) -> bool {
        self.crop.is_some()
            || self.rotate != Rotate::None
            || self.flip_h
            || self.flip_v
            || self.denoise.is_some()
            || self.sharpen.is_some()
            || self.deinterlace == Deinterlace::On
    }
}

/// Re-encode settings for video.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct VideoEncode {
    /// Codec id (`h264`, `hevc`, `av1`, `prores`…).
    pub codec: String,
    #[serde(default)]
    pub encoder: EncoderChoice,
    #[serde(default)]
    pub rate: RateControl,
    /// Native preset override (`slow`, `p6`, `8`…).
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default)]
    pub tune: Option<String>,
    #[serde(default)]
    pub profile: Option<String>,
    #[serde(default)]
    pub level: Option<String>,
    #[serde(default)]
    pub bit_depth: BitDepth,
    /// Explicit software pixel format (advanced).
    #[serde(default)]
    pub pix_fmt: Option<String>,
    /// Keyframe interval in frames.
    #[serde(default)]
    pub gop: Option<u32>,
    #[serde(default)]
    pub bframes: Option<u32>,
    #[serde(default)]
    pub filters: VideoFilters,
    #[serde(default)]
    pub hdr: HdrMode,
    /// Container tag, e.g. `hvc1` for Apple players.
    #[serde(default)]
    pub tag: Option<String>,
    /// Raw encoder parameter string (`-x265-params`, `-svtav1-params`, `-x264-params`).
    #[serde(default)]
    pub encoder_params: Option<String>,
    /// Two-pass (software encoders with `-pass` support).
    #[serde(default)]
    pub two_pass: bool,
}

impl VideoEncode {
    pub fn new(codec: &str) -> Self {
        VideoEncode {
            codec: codec.to_string(),
            encoder: EncoderChoice::Auto,
            rate: RateControl::default(),
            preset: None,
            tune: None,
            profile: None,
            level: None,
            bit_depth: BitDepth::Auto,
            pix_fmt: None,
            gop: None,
            bframes: None,
            filters: VideoFilters::default(),
            hdr: HdrMode::Auto,
            tag: None,
            encoder_params: None,
            two_pass: false,
        }
    }
}

/// Video stream choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum VideoSettings {
    Copy,
    Drop,
    Encode(Box<VideoEncode>),
}

/// EBU R128 loudness normalisation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Loudnorm {
    /// Integrated loudness target (LUFS), e.g. -16 (streaming) or -23 (broadcast).
    pub target_lufs: f32,
    /// True peak ceiling (dBTP).
    pub true_peak: f32,
    /// Loudness range target (LU).
    pub lra: f32,
    /// Measure first, then apply (accurate); otherwise single-pass dynamic.
    pub two_pass: bool,
}

impl Default for Loudnorm {
    fn default() -> Self {
        Loudnorm {
            target_lufs: -16.0,
            true_peak: -1.5,
            lra: 11.0,
            two_pass: true,
        }
    }
}

/// Re-encode settings for audio (applied to every selected audio track).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AudioEncode {
    pub codec: String,
    #[serde(default)]
    pub encoder: Option<String>,
    /// Bitrate per stereo pair in kb/s (scaled up for more channels); None = encoder default/VBR.
    #[serde(default)]
    pub bitrate_kbps: Option<u32>,
    /// Native VBR quality (`-q:a`) when the encoder has one (LAME V-level, Vorbis q).
    #[serde(default)]
    pub vbr_quality: Option<f32>,
    /// Downmix to this many channels (1 = mono, 2 = stereo).
    #[serde(default)]
    pub channels: Option<u32>,
    #[serde(default)]
    pub sample_rate: Option<u32>,
    /// Output sample format for lossless/PCM (e.g. `s16`, `s32`).
    #[serde(default)]
    pub sample_fmt: Option<String>,
    #[serde(default)]
    pub loudnorm: Option<Loudnorm>,
}

impl AudioEncode {
    pub fn new(codec: &str, kbps: Option<u32>) -> Self {
        AudioEncode {
            codec: codec.into(),
            encoder: None,
            bitrate_kbps: kbps,
            vbr_quality: None,
            channels: None,
            sample_rate: None,
            sample_fmt: None,
            loudnorm: None,
        }
    }
}

/// Audio stream choice.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum AudioSettings {
    Copy,
    Drop,
    Encode(AudioEncode),
}

/// Which input tracks of a kind to keep.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum TrackSelection {
    #[default]
    All,
    /// Only the default/first track.
    Primary,
    /// Specific input stream indices (absolute `StreamInfo::index`).
    Streams { indices: Vec<u32> },
    /// Tracks whose language matches (ISO 639-2, e.g. "eng"); untagged tracks kept.
    Languages { languages: Vec<String> },
}

/// Subtitle handling.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(tag = "mode", rename_all = "snake_case")]
pub enum SubtitleSettings {
    /// Copy when the container allows, else convert text subtitles to the
    /// container's format (e.g. SRT → mov_text); drop what cannot be carried.
    #[default]
    Auto,
    /// Convert all text subtitles to this codec.
    Convert {
        codec: String,
    },
    /// Burn one subtitle stream into the video (absolute stream index; None = first).
    BurnIn {
        stream: Option<u32>,
    },
    Drop,
}

/// Metadata / chapters / attachments.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MetadataPolicy {
    pub copy_metadata: bool,
    pub copy_chapters: bool,
    /// Keep cover art in audio outputs that support it.
    pub keep_cover_art: bool,
}

impl Default for MetadataPolicy {
    fn default() -> Self {
        MetadataPolicy {
            copy_metadata: true,
            copy_chapters: true,
            keep_cover_art: true,
        }
    }
}

/// Trim range in seconds.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Trim {
    pub start: Option<f64>,
    pub end: Option<f64>,
}

impl Trim {
    pub fn duration(&self, total: Option<f64>) -> Option<f64> {
        let start = self.start.unwrap_or(0.0);
        match (self.end, total) {
            (Some(e), _) => Some((e - start).max(0.0)),
            (None, Some(t)) => Some((t - start).max(0.0)),
            _ => None,
        }
    }
}

/// Animated image options (GIF / animated WebP / APNG).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct AnimationOptions {
    pub fps: u32,
    pub max_width: u32,
    /// 0 = loop forever.
    pub loop_count: u32,
}

impl Default for AnimationOptions {
    fn default() -> Self {
        AnimationOptions {
            fps: 15,
            max_width: 480,
            loop_count: 0,
        }
    }
}

/// Everything about one output.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct OutputSettings {
    /// Format id from the catalog (`mp4`, `mkv`, `png`, …). The special id
    /// `auto-audio` picks a container matching the copied audio codec.
    pub format: String,
    pub video: VideoSettings,
    pub audio: AudioSettings,
    #[serde(default)]
    pub audio_tracks: TrackSelection,
    #[serde(default)]
    pub subtitles: SubtitleSettings,
    #[serde(default)]
    pub subtitle_tracks: TrackSelection,
    #[serde(default)]
    pub metadata: MetadataPolicy,
    #[serde(default)]
    pub speed: Speed,
    #[serde(default)]
    pub trim: Option<Trim>,
    #[serde(default)]
    pub animation: Option<AnimationOptions>,
    /// Extra raw args before `-i` (advanced; shown with a warning).
    #[serde(default)]
    pub extra_input_args: Vec<String>,
    /// Extra raw args before the output (advanced; shown with a warning).
    #[serde(default)]
    pub extra_output_args: Vec<String>,
}

impl OutputSettings {
    pub fn video_encode(&self) -> Option<&VideoEncode> {
        match &self.video {
            VideoSettings::Encode(v) => Some(v),
            _ => None,
        }
    }
    pub fn video_encode_mut(&mut self) -> Option<&mut VideoEncode> {
        match &mut self.video {
            VideoSettings::Encode(v) => Some(v),
            _ => None,
        }
    }
    pub fn audio_encode(&self) -> Option<&AudioEncode> {
        match &self.audio {
            AudioSettings::Encode(a) => Some(a),
            _ => None,
        }
    }
    /// True if nothing is re-encoded.
    pub fn is_remux(&self) -> bool {
        !matches!(self.video, VideoSettings::Encode(_))
            && !matches!(self.audio, AudioSettings::Encode(_))
            && !matches!(self.subtitles, SubtitleSettings::BurnIn { .. })
    }
}
