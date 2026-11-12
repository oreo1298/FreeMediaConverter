// SPDX-License-Identifier: GPL-3.0-or-later
//! Running ffprobe/ffmpeg to analyse inputs, extract thumbnails and audio peaks.

use std::ffi::OsString;
use std::path::Path;
use std::time::Duration;

use super::{merge_frame_side_data, parse_ffprobe_json, Hdr, MediaInfo};
use crate::error::{Error, Result};
use crate::ffmpeg::Ffmpeg;
use crate::paths;

/// Still-image extensions handled by FFmpeg's image2 demuxer, which treats
/// `%` in names as a sequence pattern.
const IMAGE2_EXTS: &[&str] = &[
    "png", "jpg", "jpeg", "bmp", "tif", "tiff", "dpx", "exr", "tga", "jxl", "pgm", "ppm", "pbm",
    "pcx", "sgi", "xbm", "xwd", "qoi", "j2k", "jp2", "pam", "hdr", "psd",
];

/// Demuxer arguments needed *before* `-i` for this input path so that its
/// name is never interpreted as a pattern.
pub fn input_format_args(path: &Path) -> Vec<OsString> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy())
        .unwrap_or_default();
    let ext = path
        .extension()
        .map(|e| e.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    if name.contains('%') && IMAGE2_EXTS.contains(&ext.as_str()) {
        ["-f", "image2", "-pattern_type", "none"]
            .iter()
            .map(OsString::from)
            .collect()
    } else {
        Vec::new()
    }
}

/// Run ffprobe on `path` and parse the result. HDR inputs get a second, very
/// short probe of the first frame for mastering-display / CLL / HDR10+ data.
pub fn analyze(ff: &Ffmpeg, path: &Path) -> Result<MediaInfo> {
    let abs = paths::absolutize(path);
    if !abs.exists() {
        return Err(Error::io(
            &abs,
            std::io::Error::new(std::io::ErrorKind::NotFound, "file not found"),
        ));
    }
    let mut inv = ff
        .probe_cmd()
        .args([
            "-v",
            "error",
            "-print_format",
            "json",
            "-show_format",
            "-show_streams",
            "-show_chapters",
        ])
        .timeout(Some(Duration::from_secs(60)));
    inv.args.extend(input_format_args(&abs));
    inv.args.push(paths::ffmpeg_url(&abs));
    let out = ff.runner().run(&inv)?.into_result("ffprobe")?;
    let mut info = parse_ffprobe_json(abs.clone(), &out.stdout_str())?;

    let wants_frame_data = info
        .primary_video()
        .and_then(|v| v.video.as_ref())
        .map(|v| v.hdr != Hdr::Sdr)
        .unwrap_or(false);
    if wants_frame_data {
        let mut inv = ff
            .probe_cmd()
            .args([
                "-v",
                "error",
                "-select_streams",
                "v:0",
                "-read_intervals",
                "%+#1",
                "-show_frames",
                "-show_entries",
                "frame=side_data_list",
                "-print_format",
                "json",
            ])
            .timeout(Some(Duration::from_secs(20)));
        inv.args.extend(input_format_args(&abs));
        inv.args.push(paths::ffmpeg_url(&abs));
        if let Ok(o) = ff.runner().run(&inv) {
            if o.success() {
                merge_frame_side_data(&mut info, &o.stdout_str());
            }
        }
    }
    Ok(info)
}

/// Software tonemap chain to make HDR thumbnails look right (needs zscale).
pub const ZSCALE_TONEMAP: &str = "zscale=t=linear:npl=100,format=gbrpf32le,zscale=p=bt709,\
tonemap=tonemap=hable:desat=0,zscale=t=bt709:m=bt709:r=tv";

/// Extract a PNG thumbnail (at ~10% of the duration), at most `max_width` wide.
/// `can_tonemap` = the zscale filter is available.
pub fn thumbnail_png(
    ff: &Ffmpeg,
    info: &MediaInfo,
    max_width: u32,
    can_tonemap: bool,
) -> Result<Vec<u8>> {
    let video = info
        .streams
        .iter()
        .find(|s| s.kind == super::StreamKind::Video && !s.attached_pic)
        .or_else(|| info.streams.iter().find(|s| s.attached_pic))
        .ok_or_else(|| Error::InvalidSettings("no video stream".into()))?;
    let mut args: Vec<OsString> = vec!["-v".into(), "error".into()];
    let is_cover = video.attached_pic;
    if !is_cover && !info.is_still_image() {
        if let Some(d) = info.duration {
            let t = (d * 0.1).min((d - 0.5).max(0.0));
            args.extend(["-ss".into(), format!("{t:.3}").into()]);
        }
    }
    args.extend(input_format_args(&info.path));
    args.push("-i".into());
    args.push(paths::ffmpeg_url(&info.path));
    args.extend(["-map".into(), format!("0:{}", video.index).into()]);
    let hdr = video
        .video
        .as_ref()
        .map(|v| v.hdr.is_hdr())
        .unwrap_or(false);
    let mut vf = String::new();
    if hdr && can_tonemap {
        vf.push_str(ZSCALE_TONEMAP);
        vf.push(',');
    }
    vf.push_str(&format!(
        "scale='min({max_width},iw)':-2:flags=bicubic,format=rgb24"
    ));
    args.extend([
        "-vf".into(),
        vf.into(),
        "-frames:v".into(),
        "1".into(),
        "-c:v".into(),
        "png".into(),
        "-f".into(),
        "image2pipe".into(),
        "pipe:1".into(),
    ]);
    let out = ff
        .run_args(args, Duration::from_secs(30))?
        .into_result("ffmpeg")?;
    if out.stdout.starts_with(b"\x89PNG") {
        Ok(out.stdout)
    } else {
        Err(Error::parse("thumbnail", "ffmpeg produced no PNG"))
    }
}

/// Peak envelope of the first audio stream, `buckets` values in 0.0..=1.0.
pub fn waveform_peaks(ff: &Ffmpeg, info: &MediaInfo, buckets: usize) -> Result<Vec<f32>> {
    let audio = info
        .primary_audio()
        .ok_or_else(|| Error::InvalidSettings("no audio stream".into()))?;
    let mut args: Vec<OsString> = vec!["-v".into(), "error".into()];
    args.extend(input_format_args(&info.path));
    args.push("-i".into());
    args.push(paths::ffmpeg_url(&info.path));
    args.extend(
        [
            "-map".to_string(),
            format!("0:{}", audio.index),
            "-ac".into(),
            "1".into(),
            "-ar".into(),
            "2000".into(),
            "-c:a".into(),
            "pcm_s16le".into(),
            "-f".into(),
            "s16le".into(),
            "pipe:1".into(),
        ]
        .map(OsString::from),
    );
    let out = ff
        .run_args(args, Duration::from_secs(120))?
        .into_result("ffmpeg")?;
    Ok(peaks_from_s16le(&out.stdout, buckets))
}

/// Reduce mono s16le samples to `buckets` normalised peak values.
pub fn peaks_from_s16le(data: &[u8], buckets: usize) -> Vec<f32> {
    let samples: Vec<i16> = data
        .chunks_exact(2)
        .map(|c| i16::from_le_bytes([c[0], c[1]]))
        .collect();
    if samples.is_empty() || buckets == 0 {
        return vec![0.0; buckets];
    }
    let per = (samples.len() as f64 / buckets as f64).max(1.0);
    let mut peaks = Vec::with_capacity(buckets);
    for b in 0..buckets {
        let start = (b as f64 * per) as usize;
        let end = (((b + 1) as f64 * per) as usize).min(samples.len());
        let peak = samples
            .get(start..end.max(start))
            .unwrap_or(&[])
            .iter()
            .map(|s| s.unsigned_abs())
            .max()
            .unwrap_or(0);
        peaks.push(peak as f32 / i16::MAX as f32);
    }
    let max = peaks.iter().cloned().fold(0.0f32, f32::max);
    if max > 0.0 {
        for p in &mut peaks {
            *p = (*p / max).min(1.0);
        }
    }
    peaks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn image2_guard_only_for_percent_images() {
        assert!(input_format_args(Path::new("/x/a.png")).is_empty());
        assert!(input_format_args(Path::new("/x/100%.mp4")).is_empty());
        let a = input_format_args(Path::new("/x/100%.PNG"));
        assert_eq!(
            a,
            ["-f", "image2", "-pattern_type", "none"].map(OsString::from)
        );
    }

    #[test]
    fn peaks_normalised() {
        let mut data = Vec::new();
        for i in 0..1000i16 {
            let v: i16 = if i < 500 { 1000 } else { 4000 };
            data.extend_from_slice(&v.to_le_bytes());
        }
        let p = peaks_from_s16le(&data, 4);
        assert_eq!(p.len(), 4);
        assert!((p[0] - 0.25).abs() < 1e-6);
        assert!((p[3] - 1.0).abs() < 1e-6);
        assert_eq!(peaks_from_s16le(&[], 3), vec![0.0; 3]);
    }
}
