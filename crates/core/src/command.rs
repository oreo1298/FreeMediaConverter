// SPDX-License-Identifier: GPL-3.0-or-later
//! The command builder: a pure function from (plan, settings, media, paths)
//! to ffmpeg argv vectors. Never produces a shell string for execution; the
//! `display()` string is only for showing/copying.

use std::ffi::OsString;
use std::path::{Path, PathBuf};

use crate::catalog::FormatKind;
use crate::error::{Error, Result};
use crate::ffmpeg::{Caps, Ffmpeg};
use crate::hw::{args as hwargs, Backend};
use crate::media::{analyze, Hdr, MediaInfo, StreamInfo};
use crate::paths;
use crate::plan::{
    AudioAction, DecodeOn, JobPlan, PipelineKind, Plan, SubtitleAction, Tonemap, VideoPlan,
};
use crate::quality::{self, EncoderContext, RateControl};
use crate::settings::*;

/// Paths for a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JobPaths {
    pub input: PathBuf,
    pub output: PathBuf,
    /// Overwrite an existing output (user chose "overwrite").
    pub overwrite: bool,
    /// Scratch directory for pass logs.
    pub work_dir: PathBuf,
}

/// Kind of ffmpeg run within a job.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum StepKind {
    /// Loudness measurement for output audio stream `index`; its JSON fills
    /// the placeholder [`loudnorm_token`] in the final step.
    LoudnormMeasure { index: usize },
    /// First pass of a two-pass encode.
    Pass1,
    /// The run that writes the output.
    Encode,
}

/// One ffmpeg invocation (arguments after the program name).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    pub kind: StepKind,
    pub args: Vec<OsString>,
    /// Share of the job's total progress.
    pub weight: f32,
}

/// All steps of a job.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct CommandSet {
    pub steps: Vec<Step>,
}

/// Placeholder in the final step replaced by measured loudnorm values.
pub fn loudnorm_token(index: usize) -> String {
    format!("@@RETORT_LOUDNORM_{index}@@")
}

/// Quote one argument for display in a POSIX shell (display only).
pub fn shell_quote(arg: &str) -> String {
    if !arg.is_empty()
        && arg
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || "_./:=,+@%^-".contains(c))
    {
        arg.to_string()
    } else {
        format!("'{}'", arg.replace('\'', r"'\''"))
    }
}

impl CommandSet {
    /// Human-readable, copyable command line(s) — one per step, joined with `&&`.
    pub fn display(&self, ffmpeg: &Path) -> String {
        self.steps
            .iter()
            .map(|s| {
                let mut parts = vec![shell_quote(&ffmpeg.to_string_lossy())];
                parts.extend(s.args.iter().map(|a| shell_quote(&a.to_string_lossy())));
                parts.join(" ")
            })
            .collect::<Vec<_>>()
            .join(" && \\\n  ")
    }
}

/// Escape a value for an ffmpeg filter option (level 1) and then for the
/// filtergraph description (level 2), per ffmpeg-filters "Notes on filtergraph escaping".
pub fn escape_filter_value(v: &str) -> String {
    let mut l1 = String::new();
    for c in v.chars() {
        if matches!(c, '\\' | '\'' | ':') {
            l1.push('\\');
        }
        l1.push(c);
    }
    let mut l2 = String::new();
    for c in l1.chars() {
        if matches!(c, '\\' | '\'' | '[' | ']' | ',' | ';') {
            l2.push('\\');
        }
        l2.push(c);
    }
    l2
}

/// Even output size fitting `scale` for a `w`×`h` frame, aspect kept.
pub fn fit_size(w: u32, h: u32, scale: &Scale) -> (u32, u32) {
    let (w, h) = (w.max(2) as f64, h.max(2) as f64);
    let mut f = 1.0f64;
    if let Some(mw) = scale.width {
        f = f.min(mw as f64 / w);
    }
    if let Some(mh) = scale.height {
        f = f.min(mh as f64 / h);
    }
    if scale.width.is_none() && scale.height.is_none() {
        f = 1.0;
    }
    if scale.only_shrink {
        f = f.min(1.0);
    } else {
        match (scale.width, scale.height) {
            (Some(mw), None) => f = mw as f64 / w,
            (None, Some(mh)) => f = mh as f64 / h,
            _ => {}
        }
    }
    let even = |x: f64| ((x / 2.0).round() as u32 * 2).max(2);
    (even(w * f), even(h * f))
}

struct Ctx<'a> {
    ff: &'a Ffmpeg,
    media: &'a MediaInfo,
    settings: &'a OutputSettings,
    job: &'a JobPlan,
    plan: &'a Plan,
    paths: &'a JobPaths,
}

fn s<T: Into<String>>(v: T) -> OsString {
    OsString::from(v.into())
}

fn push<I: IntoIterator<Item = S>, S: Into<String>>(args: &mut Vec<OsString>, items: I) {
    args.extend(items.into_iter().map(|x| OsString::from(x.into())));
}

impl Ctx<'_> {
    fn src(&self, idx: u32) -> Option<&StreamInfo> {
        self.media.streams.iter().find(|s| s.index == idx)
    }

    /// Input seek in seconds: the trim start, or ~10% in for still-image
    /// outputs (the first frame is often black).
    fn input_seek(&self) -> f64 {
        if let Some(start) = self
            .settings
            .trim
            .and_then(|t| t.start)
            .filter(|x| *x > 0.0)
        {
            return start;
        }
        if self.job.format.kind == FormatKind::Image && !self.media.is_still_image() {
            if let Some(d) = self.media.duration.filter(|d| *d > 1.0) {
                return (d * 0.1).min(d - 0.5).max(0.0);
            }
        }
        0.0
    }

    fn video_encode(&self) -> Option<&VideoEncode> {
        self.settings.video_encode()
    }

    /// Size of frames entering the CPU filter chain (after autorotate and crop).
    fn frame_size(&self, vp: &VideoPlan) -> (u32, u32) {
        let vi = self
            .src(vp.input_stream)
            .and_then(|s| s.video.clone())
            .unwrap_or_default();
        let (mut w, mut h) = vi.display_size();
        if let Some(Crop::Manual { w: cw, h: ch, .. }) =
            self.video_encode().and_then(|v| v.filters.crop)
        {
            w = cw;
            h = ch;
        }
        (w, h)
    }

    fn target_size(&self, vp: &VideoPlan) -> Option<(u32, u32)> {
        let v = self.video_encode()?;
        let scale = v.filters.scale?;
        let (w, h) = self.frame_size(vp);
        Some(fit_size(w, h, &scale))
    }

    /// CPU filters, in order. Ends with pixel-format conversion/upload.
    fn cpu_filters(&self, vp: &VideoPlan) -> Vec<String> {
        let mut f = Vec::new();
        let Some(v) = self.video_encode() else {
            return f;
        };
        let vf = &v.filters;
        if vp.deinterlace {
            let deint = if vf.deinterlace == Deinterlace::On {
                "all"
            } else {
                "interlaced"
            };
            f.push(format!("bwdif=mode=send_frame:parity=auto:deint={deint}"));
        }
        match vf.crop {
            Some(Crop::Manual { w, h, x, y }) => f.push(format!("crop={w}:{h}:{x}:{y}")),
            Some(Crop::Auto) | None => {}
        }
        let (fw, fh) = self.frame_size(vp);
        if let Some((w, h)) = self.target_size(vp) {
            if (w, h) != (fw, fh) {
                f.push(format!("scale={w}:{h}:flags=lanczos"));
            }
        } else if (fw % 2 == 1 || fh % 2 == 1)
            && !matches!(
                vp.codec.as_str(),
                "gif" | "png" | "apng" | "ffv1" | "webp" | "tiff" | "bmp"
            )
        {
            // 4:2:0 encoders need even dimensions: drop the odd row/column.
            f.push("crop=trunc(iw/2)*2:trunc(ih/2)*2".into());
        }
        match &vp.tonemap {
            Some(Tonemap::Zscale) => {
                f.push(analyze::ZSCALE_TONEMAP.to_string());
            }
            Some(Tonemap::Libplacebo { .. }) => f.push(
                "libplacebo=tonemapping=auto:colorspace=bt709:color_primaries=bt709:color_trc=bt709:range=tv:format=yuv420p".into(),
            ),
            _ => {}
        }
        match vf.rotate {
            Rotate::Cw90 => f.push("transpose=clock".into()),
            Rotate::Ccw90 => f.push("transpose=cclock".into()),
            Rotate::R180 => f.push("hflip,vflip".into()),
            Rotate::None => {}
        }
        if vf.flip_h {
            f.push("hflip".into());
        }
        if vf.flip_v {
            f.push("vflip".into());
        }
        if let Some(fps) = &vf.fps {
            f.push(format!("fps={fps}"));
        }
        match vf.denoise {
            Some(Strength::Light) => f.push("hqdn3d=2:1.5:3:2.5".into()),
            Some(Strength::Medium) => f.push("hqdn3d=4:3:6:4.5".into()),
            Some(Strength::Strong) => f.push("hqdn3d=7:5:10:7.5".into()),
            None => {}
        }
        match vf.sharpen {
            Some(Strength::Light) => f.push("unsharp=5:5:0.4".into()),
            Some(Strength::Medium) => f.push("unsharp=5:5:0.8".into()),
            Some(Strength::Strong) => f.push("unsharp=5:5:1.3".into()),
            None => {}
        }
        if let Some(b) = &self.job.burn_in {
            if !b.bitmap {
                let url = paths::ffmpeg_url(&self.paths.input);
                // Input seeking resets timestamps to 0; shift them back so the
                // subtitles filter (which reads the file from the start) lines up.
                let start = self.input_seek();
                if start > 0.0 {
                    f.push(format!("setpts=PTS+{start:.3}/TB"));
                }
                f.push(format!(
                    "subtitles=filename={}:si={}",
                    escape_filter_value(&url.to_string_lossy()),
                    b.subtitle_ordinal
                ));
                if start > 0.0 {
                    f.push("setpts=PTS-STARTPTS".into());
                }
            }
        }
        // Tag colour properties on the frames: since FFmpeg 8/9 the encoder
        // takes them from frames, and `-color_*` output options alone no longer
        // tag the stream. (The output options are still emitted for 6.1/7.x.)
        if let Some(sp) = self.color_setparams(vp) {
            f.push(sp);
        }
        match (vp.backend, &vp.pix_fmt) {
            (Some(b), _) => f.push(hwargs::upload_chain(b, vp.ten_bit)),
            (None, Some(p)) => f.push(format!("format={p}")),
            (None, None) => {}
        }
        f
    }

    /// `setparams=…` tagging the output colour properties, if they must be set.
    fn color_setparams(&self, vp: &VideoPlan) -> Option<String> {
        if vp.keep_hdr {
            let vi = self
                .src(vp.input_stream)
                .and_then(|s| s.video.clone())
                .unwrap_or_default();
            return Some(format!(
                "setparams=color_primaries={}:color_trc={}:colorspace={}",
                vi.color_primaries.as_deref().unwrap_or("bt2020"),
                vi.color_transfer.as_deref().unwrap_or("smpte2084"),
                vi.color_space.as_deref().unwrap_or("bt2020nc")
            ));
        }
        vp.tonemap
            .as_ref()
            .map(|_| "setparams=color_primaries=bt709:color_trc=bt709:colorspace=bt709".to_string())
    }

    /// GPU filters for a full-GPU plan.
    fn gpu_filters(&self, vp: &VideoPlan, backend: Backend) -> Vec<String> {
        let mut f = Vec::new();
        let vi = self
            .src(vp.input_stream)
            .and_then(|s| s.video.clone())
            .unwrap_or_default();
        let depth_change = (vi.bit_depth > 8) != vp.ten_bit;
        if matches!(vp.tonemap, Some(Tonemap::Vaapi)) {
            f.push("tonemap_vaapi=format=nv12:t=bt709:m=bt709:p=bt709".into());
        }
        let size = self.target_size(vp);
        let fmt = match (backend, vp.ten_bit) {
            (Backend::Vaapi | Backend::Vulkan, true) => "p010",
            (_, true) => "p010le",
            _ => "nv12",
        };
        let fmt_arg = if depth_change && !matches!(vp.tonemap, Some(Tonemap::Vaapi)) {
            Some(fmt)
        } else {
            None
        };
        if size.is_some() || fmt_arg.is_some() {
            if let Some(name) = backend.scale_filter() {
                let mut opts = Vec::new();
                if let Some((w, h)) = size {
                    opts.push(format!("w={w}:h={h}"));
                }
                if let Some(fmt) = fmt_arg {
                    opts.push(format!("format={fmt}"));
                }
                f.push(format!("{name}={}", opts.join(":")));
            }
        }
        if let Some(fps) = self.video_encode().and_then(|v| v.filters.fps.clone()) {
            f.push(format!("fps={fps}"));
        }
        f
    }

    fn input_args(&self, args: &mut Vec<OsString>, vp: Option<&VideoPlan>) {
        if let Some(vp) = vp {
            let mut need_filter_dev = false;
            let mut init: Vec<String> = Vec::new();
            if let (Some(b), Some(dev)) = (vp.backend, &vp.device) {
                match vp.kind {
                    PipelineKind::FullGpu => {
                        if hwargs::decode_needs_init(b) {
                            init.extend(hwargs::init_device_args(b, dev));
                        }
                    }
                    PipelineKind::Hybrid | PipelineKind::GpuEncode => {
                        if b.encoder_needs_upload()
                            || hwargs::decode_needs_init(b)
                            || b == Backend::Qsv
                        {
                            init.extend(hwargs::init_device_args(b, dev));
                            need_filter_dev = b.encoder_needs_upload() || b == Backend::Qsv;
                        }
                    }
                    PipelineKind::Software => {}
                }
            }
            if let Some(Tonemap::Libplacebo { device }) = &vp.tonemap {
                init.extend(hwargs::init_device_args(Backend::Vulkan, device));
                if !need_filter_dev {
                    init.extend(hwargs::filter_device_args(Backend::Vulkan));
                }
            }
            push(args, init);
            if need_filter_dev {
                if let Some(b) = vp.backend {
                    push(args, hwargs::filter_device_args(b));
                }
            }
            if let DecodeOn::Gpu {
                backend,
                device,
                keep_frames,
            } = &vp.decode
            {
                push(args, hwargs::decode_args(*backend, device, *keep_frames));
                if *backend == Backend::Qsv {
                    let codec = self
                        .src(vp.input_stream)
                        .map(|s| s.codec.clone())
                        .unwrap_or_default();
                    if let Some(d) = hwargs::qsv_decoder(&codec) {
                        push(args, ["-c:v".to_string(), d]);
                    }
                }
            }
        }
        let seek = self.input_seek();
        if seek > 0.0 {
            push(args, ["-ss".to_string(), format!("{seek:.3}")]);
        }
        push(args, self.settings.extra_input_args.iter().cloned());
        args.extend(analyze::input_format_args(&self.paths.input));
        args.push(s("-i"));
        args.push(paths::ffmpeg_url(&self.paths.input));
    }

    fn video_codec_args(
        &self,
        args: &mut Vec<OsString>,
        vp: &VideoPlan,
        pass: Option<u8>,
    ) -> Result<()> {
        let v = self
            .video_encode()
            .ok_or_else(|| Error::InvalidSettings("no video settings".into()))?;
        let enc = &vp.encoder;
        push(args, ["-c:v", enc.as_str()]);
        if let (Some(b), Some(dev)) = (vp.backend, &vp.device) {
            if vp.kind != PipelineKind::FullGpu {
                push(args, hwargs::encoder_device_args(b, dev));
            }
        }
        let help = self.ff.encoder_help(enc);
        let has_opt = |o: &str| help.as_ref().map(|h| h.has_option(o)).unwrap_or(true);
        // A value is accepted if the option exists and either lists no named
        // values (free-form string/int) or lists this one.
        let accepts = |o: &str, val: &str| {
            help.as_ref()
                .map(|h| {
                    h.has_option(o)
                        && (h.option_values(o).is_empty()
                            || h.option_values(o).iter().any(|x| x == val))
                })
                .unwrap_or(true)
        };
        let opt_values = |o: &str| -> Vec<String> {
            help.as_ref()
                .map(|h| h.option_values(o).to_vec())
                .unwrap_or_default()
        };
        // Rate control.
        let (w, h) = self.target_size(vp).unwrap_or_else(|| self.frame_size(vp));
        let fps = self
            .src(vp.input_stream)
            .and_then(|s| s.video.as_ref())
            .and_then(|v| v.fps)
            .unwrap_or(30.0);
        let ectx = EncoderContext {
            rc_modes: vp.rc_modes.clone(),
            width: w,
            height: h,
            fps,
        };
        let rc = match (self.job.target_kbps, &v.rate) {
            (Some(k), _) if self.job.two_pass => {
                push(args, ["-b:v".to_string(), format!("{k}k")]);
                None
            }
            (Some(k), _) => Some(RateControl::Vbr {
                kbps: k * 95 / 100,
                max_kbps: Some(k),
            }),
            (None, r) => Some(*r),
        };
        if let Some(rc) = rc {
            let m = quality::map(enc, &vp.codec, &self.ff.version, &rc, &ectx);
            push(args, m.args);
        }
        // Speed preset.
        match v.preset.as_ref().filter(|p| accepts("preset", p)) {
            Some(p) => push(args, ["-preset", p.as_str()]),
            None => {
                if let Some((flag, val)) = quality::speed_preset(enc, self.settings.speed) {
                    push(args, [flag.to_string(), val]);
                }
            }
        }
        if matches!(enc.as_str(), "libaom-av1" | "libvpx-vp9" | "libvpx") {
            push(args, ["-row-mt", "1"]);
            if enc != "libaom-av1" {
                push(args, ["-deadline", "good"]);
            }
        }
        if let Some(t) = v.tune.as_ref().filter(|t| accepts("tune", t)) {
            push(args, ["-tune", t.as_str()]);
        }
        // Profile.
        let mut profile = v.profile.clone();
        if vp.backend.is_some() && vp.ten_bit {
            match vp.codec.as_str() {
                "hevc" => profile = Some("main10".into()),
                "h264" => profile = Some("high10".into()),
                _ => {}
            }
        }
        if vp.backend.is_none()
            && vp.ten_bit
            && vp.codec == "h264"
            && profile.as_deref() == Some("high")
        {
            profile = Some("high10".into());
        }
        if let Some(p) = profile {
            let vals = opt_values("profile");
            if has_opt("profile")
                && (vals.is_empty() || vals.contains(&p) || p.parse::<i32>().is_ok())
            {
                push(args, ["-profile:v", p.as_str()]);
            }
        }
        if let Some(l) = &v.level {
            if has_opt("level") {
                push(args, ["-level", l.as_str()]);
            }
        }
        if !crate::catalog::codecs::is_intra_only(&vp.codec) {
            if let Some(g) = v.gop {
                push(args, ["-g".to_string(), g.to_string()]);
            }
            if let Some(bf) = v.bframes {
                if vp.bframes != Some(false) {
                    push(args, ["-bf".to_string(), bf.to_string()]);
                }
            }
        }
        if let Some(tag) = &v.tag {
            if vp.codec == "hevc" && matches!(self.job.format.id.as_str(), "mp4" | "mov" | "m4v") {
                push(args, ["-tag:v", tag.as_str()]);
            }
        }
        // HDR signalling.
        let vi = self
            .src(vp.input_stream)
            .and_then(|s| s.video.clone())
            .unwrap_or_default();
        let mut x265: Vec<String> = Vec::new();
        let mut svt: Vec<String> = Vec::new();
        if vp.keep_hdr {
            let prim = vi
                .color_primaries
                .clone()
                .unwrap_or_else(|| "bt2020".into());
            let trc = vi
                .color_transfer
                .clone()
                .unwrap_or_else(|| "smpte2084".into());
            let space = vi.color_space.clone().unwrap_or_else(|| "bt2020nc".into());
            push(
                args,
                [
                    "-color_primaries",
                    prim.as_str(),
                    "-color_trc",
                    trc.as_str(),
                    "-colorspace",
                    space.as_str(),
                ],
            );
            push(
                args,
                ["-color_range", vi.color_range.as_deref().unwrap_or("tv")],
            );
            let base = vi.hdr.base_layer();
            if enc == "libx265" {
                x265.push(format!(
                    "colorprim={prim}:transfer={trc}:colormatrix={space}"
                ));
                if base == Hdr::Hdr10 || base == Hdr::Hdr10Plus {
                    x265.push("hdr10=1:repeat-headers=1".into());
                    if let Some(md) = &vi.mastering_display {
                        x265.push(format!("master-display={}", md.x265_string()));
                    }
                    if let Some(cl) = &vi.content_light {
                        x265.push(format!("max-cll={},{}", cl.max_cll, cl.max_fall));
                    }
                }
            }
            if enc == "libsvtav1" {
                if let Some(md) = &vi.mastering_display {
                    svt.push(format!("mastering-display={}", md.svt_string()));
                }
                if let Some(cl) = &vi.content_light {
                    svt.push(format!("content-light={},{}", cl.max_cll, cl.max_fall));
                }
            }
        } else if vp.tonemap.is_some() {
            push(
                args,
                [
                    "-color_primaries",
                    "bt709",
                    "-color_trc",
                    "bt709",
                    "-colorspace",
                    "bt709",
                ],
            );
        }
        // Two-pass.
        if let Some(n) = pass {
            let log = self.paths.work_dir.join("passlog");
            if enc == "libx265" {
                x265.push(format!(
                    "pass={n}:stats={}",
                    log.with_extension("x265").display()
                ));
            } else {
                push(args, ["-pass".to_string(), n.to_string()]);
                args.push(s("-passlogfile"));
                args.push(log.into_os_string());
            }
        }
        // Encoder params (user string merged with ours).
        if let Some(p) = v.encoder_params.as_ref().filter(|p| !p.trim().is_empty()) {
            match enc.as_str() {
                "libx265" => x265.push(p.trim().to_string()),
                "libsvtav1" => svt.push(p.trim().to_string()),
                "libx264" => push(args, ["-x264-params", p.trim()]),
                "libaom-av1" => push(args, ["-aom-params", p.trim()]),
                _ => {}
            }
        }
        if enc == "libx265" {
            x265.insert(0, "log-level=error".into());
            push(args, ["-x265-params".to_string(), x265.join(":")]);
        }
        if !svt.is_empty() {
            push(args, ["-svtav1-params".to_string(), svt.join(":")]);
        }
        Ok(())
    }

    fn audio_args(&self, args: &mut Vec<OsString>) {
        for (j, a) in self.job.audio.iter().enumerate() {
            match &a.action {
                AudioAction::Copy => push(args, [format!("-c:a:{j}"), "copy".into()]),
                AudioAction::Encode {
                    encoder,
                    kbps,
                    vbr_quality,
                    channels,
                    sample_rate,
                    sample_fmt,
                    loudnorm,
                    normalize_layout,
                    ..
                } => {
                    push(args, [format!("-c:a:{j}"), encoder.clone()]);
                    if let Some(q) = vbr_quality {
                        push(args, [format!("-q:a:{j}"), format!("{q}")]);
                    } else if let Some(k) = kbps {
                        push(args, [format!("-b:a:{j}"), format!("{k}k")]);
                    }
                    if let Some(c) = channels {
                        push(args, [format!("-ac:a:{j}"), c.to_string()]);
                    }
                    if let Some(r) = sample_rate {
                        push(args, [format!("-ar:a:{j}"), r.to_string()]);
                    }
                    if let Some(f) = sample_fmt {
                        push(args, [format!("-sample_fmt:a:{j}"), f.clone()]);
                    }
                    let mut af: Vec<String> = Vec::new();
                    if let Some(ln) = loudnorm {
                        let measured = if ln.two_pass {
                            loudnorm_token(j)
                        } else {
                            String::new()
                        };
                        af.push(format!(
                            "loudnorm=I={}:TP={}:LRA={}{measured}",
                            ln.target_lufs, ln.true_peak, ln.lra
                        ));
                        let rate = sample_rate
                            .or_else(|| {
                                self.src(a.input_stream)
                                    .and_then(|s| s.audio.as_ref())
                                    .and_then(|x| x.sample_rate)
                            })
                            .unwrap_or(48000);
                        af.push(format!("aresample={rate}"));
                    }
                    if *normalize_layout {
                        af.push("aformat=channel_layouts=7.1|5.1|5.0|quad|3.0|stereo|mono".into());
                    }
                    if !af.is_empty() {
                        push(args, [format!("-filter:a:{j}"), af.join(",")]);
                    }
                }
            }
        }
    }

    fn subtitle_args(&self, args: &mut Vec<OsString>) {
        for (k, sp) in self.job.subtitles.iter().enumerate() {
            match &sp.action {
                SubtitleAction::Copy => push(args, [format!("-c:s:{k}"), "copy".into()]),
                SubtitleAction::Convert { encoder, .. } => {
                    push(args, [format!("-c:s:{k}"), encoder.clone()])
                }
            }
        }
    }

    fn metadata_args(&self, args: &mut Vec<OsString>) {
        let m = &self.settings.metadata;
        push(
            args,
            ["-map_metadata", if m.copy_metadata { "0" } else { "-1" }],
        );
        push(
            args,
            ["-map_chapters", if m.copy_chapters { "0" } else { "-1" }],
        );
        if !m.copy_metadata {
            // Keep track languages even when stripping everything else.
            for (j, a) in self.job.audio.iter().enumerate() {
                if let Some(l) = &a.language {
                    push(
                        args,
                        [format!("-metadata:s:a:{j}"), format!("language={l}")],
                    );
                }
            }
            for (k, sp) in self.job.subtitles.iter().enumerate() {
                if let Some(l) = &sp.language {
                    push(
                        args,
                        [format!("-metadata:s:s:{k}"), format!("language={l}")],
                    );
                }
            }
        }
    }

    fn output_args(&self, args: &mut Vec<OsString>) {
        let id = self.job.format.id.as_str();
        if matches!(id, "mp4" | "mov" | "m4a" | "m4v" | "3gp") {
            push(args, ["-movflags", "+faststart"]);
        }
        if let Some(anim) = &self.settings.animation {
            match id {
                "gif" | "webp-anim" => {
                    push(args, ["-loop".to_string(), anim.loop_count.to_string()])
                }
                "apng" => push(args, ["-plays".to_string(), anim.loop_count.to_string()]),
                _ => {}
            }
        }
        if id == "mp3" {
            push(args, ["-id3v2_version", "3"]);
        }
        if let Some(t) = &self.settings.trim {
            if let Some(d) = t.duration(self.media.duration).filter(|_| t.end.is_some()) {
                push(args, ["-t".to_string(), format!("{d:.3}")]);
            }
        }
        push(args, self.settings.extra_output_args.iter().cloned());
    }

    fn output_url(&self) -> OsString {
        // Single-image outputs use `-update 1`, with which the image2 muxer
        // takes the name literally: no `%` escaping (unlike sequence patterns).
        paths::ffmpeg_url(&self.paths.output)
    }
}

/// Build all steps for running `plan` (one entry of `job.chain`).
pub fn build(
    ff: &Ffmpeg,
    caps: &Caps,
    media: &MediaInfo,
    settings: &OutputSettings,
    job: &JobPlan,
    plan: &Plan,
    paths: &JobPaths,
) -> Result<CommandSet> {
    let _ = caps;
    let c = Ctx {
        ff,
        media,
        settings,
        job,
        plan,
        paths,
    };
    let mut set = CommandSet::default();
    let vp = plan.video.as_ref();

    // Loudness measurement passes.
    for (j, a) in job.audio.iter().enumerate() {
        if let AudioAction::Encode {
            loudnorm: Some(ln), ..
        } = &a.action
        {
            if ln.two_pass {
                let mut args: Vec<OsString> = Vec::new();
                push(&mut args, ["-hide_banner", "-nostdin", "-nostats"]);
                if let Some(t) = &settings.trim {
                    if let Some(start) = t.start.filter(|x| *x > 0.0) {
                        push(&mut args, ["-ss".to_string(), format!("{start:.3}")]);
                    }
                }
                args.extend(analyze::input_format_args(&paths.input));
                args.push(s("-i"));
                args.push(paths::ffmpeg_url(&paths.input));
                push(
                    &mut args,
                    ["-map".to_string(), format!("0:{}", a.input_stream)],
                );
                push(
                    &mut args,
                    [
                        "-af".to_string(),
                        format!(
                            "loudnorm=I={}:TP={}:LRA={}:print_format=json",
                            ln.target_lufs, ln.true_peak, ln.lra
                        ),
                    ],
                );
                if let Some(t) = &settings.trim {
                    if let Some(d) = t.duration(media.duration).filter(|_| t.end.is_some()) {
                        push(&mut args, ["-t".to_string(), format!("{d:.3}")]);
                    }
                }
                push(&mut args, ["-progress", "pipe:1", "-f", "null", "-"]);
                set.steps.push(Step {
                    kind: StepKind::LoudnormMeasure { index: j },
                    args,
                    weight: 0.15,
                });
            }
        }
    }

    let passes: Vec<Option<u8>> = if job.two_pass && vp.is_some() {
        vec![Some(1), Some(2)]
    } else {
        vec![None]
    };
    for pass in passes {
        let mut args: Vec<OsString> = Vec::new();
        push(&mut args, ["-hide_banner"]);
        push(
            &mut args,
            [if paths.overwrite || pass == Some(1) {
                "-y"
            } else {
                "-n"
            }],
        );
        push(
            &mut args,
            ["-progress", "pipe:1", "-stats_period", "0.5", "-nostats"],
        );
        c.input_args(&mut args, vp);

        let animated = matches!(job.format.kind, FormatKind::Animation);
        let mut mapped_video_label: Option<String> = None;
        if let Some(vp) = vp {
            let gpu_full = vp.kind == PipelineKind::FullGpu;
            let mut chain = if gpu_full {
                c.gpu_filters(vp, vp.backend.unwrap_or(Backend::Other))
            } else {
                c.cpu_filters(vp)
            };
            if animated && job.format.id == "gif" {
                let anim = settings.animation.unwrap_or_default();
                chain.retain(|f| !f.starts_with("format="));
                chain.push(format!("fps={}", anim.fps));
                chain.push(format!(
                    "scale='min({},iw)':-1:flags=lanczos",
                    anim.max_width
                ));
                let pre = chain.join(",");
                let graph = format!(
                    "[0:{}]{pre},split[s0][s1];[s0]palettegen=stats_mode=diff[p];[s1][p]paletteuse=dither=bayer:bayer_scale=5:diff_mode=rectangle[v]",
                    vp.input_stream
                );
                push(&mut args, ["-filter_complex".to_string(), graph]);
                mapped_video_label = Some("[v]".into());
            } else if let Some(b) = job.burn_in.as_ref().filter(|b| b.bitmap) {
                let fmt_tail: Vec<String> = chain
                    .iter()
                    .filter(|f| f.starts_with("format=") || f.contains("hwupload"))
                    .cloned()
                    .collect();
                chain.retain(|f| !(f.starts_with("format=") || f.contains("hwupload")));
                let pre = if chain.is_empty() {
                    "null".to_string()
                } else {
                    chain.join(",")
                };
                let post = if fmt_tail.is_empty() {
                    "null".to_string()
                } else {
                    fmt_tail.join(",")
                };
                let graph = format!(
                    "[0:{}]{pre}[base];[base][0:{}]overlay=eof_action=pass,{post}[v]",
                    vp.input_stream, b.input_stream
                );
                push(&mut args, ["-filter_complex".to_string(), graph]);
                mapped_video_label = Some("[v]".into());
            } else {
                if animated {
                    let anim = settings.animation.unwrap_or_default();
                    chain.insert(
                        0,
                        format!("scale='min({},iw)':-2:flags=lanczos", anim.max_width),
                    );
                    chain.insert(0, format!("fps={}", anim.fps));
                }
                if !chain.is_empty() {
                    push(&mut args, ["-vf".to_string(), chain.join(",")]);
                }
            }
        }

        // Stream mapping.
        if let Some(label) = &mapped_video_label {
            push(&mut args, ["-map", label.as_str()]);
        } else if let Some(vp) = vp {
            push(
                &mut args,
                ["-map".to_string(), format!("0:{}", vp.input_stream)],
            );
        } else if let Some(i) = job.video_copy {
            push(&mut args, ["-map".to_string(), format!("0:{i}")]);
        }
        if pass != Some(1) {
            if let Some(i) = job.cover_art {
                push(&mut args, ["-map".to_string(), format!("0:{i}")]);
            }
            for a in &job.audio {
                push(
                    &mut args,
                    ["-map".to_string(), format!("0:{}", a.input_stream)],
                );
            }
            for sp in &job.subtitles {
                push(
                    &mut args,
                    ["-map".to_string(), format!("0:{}", sp.input_stream)],
                );
            }
            if job.copy_attachments {
                push(&mut args, ["-map", "0:t?"]);
            }
        }

        // Codecs.
        if let Some(vp) = vp {
            c.video_codec_args(&mut args, vp, pass)?;
            if pass.is_some() {
                // Both passes must see identical frames: the null muxer would
                // otherwise pass timestamps through while MP4 forces CFR.
                push(&mut args, ["-fps_mode", "vfr"]);
            }
            if matches!(job.format.kind, FormatKind::Image) {
                push(&mut args, ["-frames:v", "1", "-update", "1"]);
            }
        } else if job.video_copy.is_some() {
            push(&mut args, ["-c:v", "copy"]);
        }
        if pass == Some(1) {
            push(&mut args, ["-an", "-sn", "-dn", "-f", "null", "-"]);
            set.steps.push(Step {
                kind: StepKind::Pass1,
                args,
                weight: 0.45,
            });
            continue;
        }
        if job.cover_art.is_some() {
            push(
                &mut args,
                ["-c:v:0", "copy", "-disposition:v:0", "attached_pic"],
            );
        }
        c.audio_args(&mut args);
        c.subtitle_args(&mut args);
        if job.copy_attachments {
            push(&mut args, ["-c:t", "copy"]);
        }
        c.metadata_args(&mut args);
        c.output_args(&mut args);
        push(&mut args, ["-f", job.format.muxer.as_str()]);
        args.push(c.output_url());
        let w = if job.two_pass { 0.55 } else { 1.0 };
        set.steps.push(Step {
            kind: StepKind::Encode,
            args,
            weight: w,
        });
    }
    let _ = c.plan;
    // Normalise weights.
    let total: f32 = set.steps.iter().map(|s| s.weight).sum();
    if total > 0.0 {
        for st in &mut set.steps {
            st.weight /= total;
        }
    }
    Ok(set)
}

/// Replace loudnorm placeholders with measured values (JSON from the measure step's stderr).
pub fn apply_loudnorm(args: &mut [OsString], index: usize, stderr: &str) -> Result<()> {
    let start = stderr
        .rfind('{')
        .ok_or_else(|| Error::parse("loudnorm", "no JSON in output"))?;
    let end = stderr[start..]
        .find('}')
        .map(|e| start + e + 1)
        .ok_or_else(|| Error::parse("loudnorm", "unterminated JSON"))?;
    let v: serde_json::Value = serde_json::from_str(&stderr[start..end])
        .map_err(|e| Error::parse("loudnorm", e.to_string()))?;
    let g = |k: &str| v.get(k).and_then(|x| x.as_str()).unwrap_or("0").to_string();
    let measured = format!(
        ":measured_I={}:measured_TP={}:measured_LRA={}:measured_thresh={}:offset={}:linear=true",
        g("input_i"),
        g("input_tp"),
        g("input_lra"),
        g("input_thresh"),
        g("target_offset")
    );
    let token = loudnorm_token(index);
    for a in args.iter_mut() {
        let st = a.to_string_lossy();
        if st.contains(&token) {
            *a = OsString::from(st.replace(&token, &measured));
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting() {
        assert_eq!(shell_quote("-c:v"), "-c:v");
        assert_eq!(shell_quote("file:/a b/c.mp4"), "'file:/a b/c.mp4'");
        assert_eq!(shell_quote("it's"), r"'it'\''s'");
        assert_eq!(shell_quote(""), "''");
    }

    #[test]
    fn filter_escaping() {
        assert_eq!(escape_filter_value("file:/m/a.mkv"), r"file\\:/m/a.mkv");
        assert_eq!(escape_filter_value("x'y"), r"x\\\'y");
        assert_eq!(escape_filter_value("a[1],b;c"), r"a\[1\]\,b\;c");
    }

    #[test]
    fn sizes() {
        let s = |w, h, o| Scale {
            width: w,
            height: h,
            only_shrink: o,
        };
        assert_eq!(
            fit_size(3840, 2160, &s(Some(1920), None, true)),
            (1920, 1080)
        );
        assert_eq!(fit_size(1280, 720, &s(Some(1920), None, true)), (1280, 720));
        assert_eq!(
            fit_size(1280, 720, &s(Some(1920), None, false)),
            (1920, 1080)
        );
        assert_eq!(fit_size(1920, 1080, &s(None, Some(720), true)), (1280, 720));
        assert_eq!(
            fit_size(1920, 800, &s(Some(1280), Some(720), true)),
            (1280, 534)
        );
        assert_eq!(fit_size(1001, 563, &s(Some(640), None, true)), (640, 360));
    }

    #[test]
    fn loudnorm_substitution() {
        let mut args = vec![
            OsString::from("-filter:a:0"),
            OsString::from(format!(
                "loudnorm=I=-16{},aresample=48000",
                loudnorm_token(0)
            )),
        ];
        let stderr = "[Parsed_loudnorm_0 @ 0x1]\n{\n\t\"input_i\" : \"-27.61\",\n\t\"input_tp\" : \"-4.47\",\n\t\"input_lra\" : \"18.06\",\n\t\"input_thresh\" : \"-39.20\",\n\t\"output_i\" : \"-16.58\",\n\t\"target_offset\" : \"0.58\"\n}\n";
        apply_loudnorm(&mut args, 0, stderr).unwrap();
        assert_eq!(
            args[1],
            "loudnorm=I=-16:measured_I=-27.61:measured_TP=-4.47:measured_LRA=18.06:measured_thresh=-39.20:offset=0.58:linear=true,aresample=48000"
        );
    }
}
