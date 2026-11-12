// SPDX-License-Identifier: GPL-3.0-or-later
//! Preview samples: encode 10 seconds from the middle of the (trimmed) input
//! with the job's settings, then extrapolate the final size and time.

use crate::catalog::FormatKind;
use crate::media::MediaInfo;
use crate::plan::JobPlan;
use crate::quality::RateControl;
use crate::settings::{OutputSettings, Trim};

/// Length of a preview sample in seconds.
pub const SAMPLE_SECONDS: f64 = 10.0;

/// Total output duration: the trimmed range, else the input duration.
pub fn output_duration(settings: &OutputSettings, media: &MediaInfo) -> Option<f64> {
    match settings.trim {
        Some(t) => t.duration(media.duration).map(|d| match media.duration {
            Some(total) => d.min((total - t.start.unwrap_or(0.0)).max(0.0)),
            None => d,
        }),
        None => media.duration,
    }
}

/// `(start, length)` of the sample, centred in the output range.
pub fn sample_range(settings: &OutputSettings, media: &MediaInfo) -> Option<(f64, f64)> {
    let len = output_duration(settings, media)?;
    if len <= 0.0 {
        return None;
    }
    let first = settings.trim.and_then(|t| t.start).unwrap_or(0.0);
    let l = SAMPLE_SECONDS.min(len);
    Some((first + (len - l) / 2.0, l))
}

/// Settings for the sample, or None when a preview makes no sense (still
/// images, unknown duration). A target size becomes the bitrate the full
/// job would use, so the sample shows the real quality.
pub fn sample_settings(
    settings: &OutputSettings,
    media: &MediaInfo,
    plan: &JobPlan,
) -> Option<OutputSettings> {
    if plan.format.kind == FormatKind::Image || media.is_still_image() {
        return None;
    }
    let (start, len) = sample_range(settings, media)?;
    let mut s = settings.clone();
    s.trim = Some(Trim {
        start: Some(start),
        end: Some(start + len),
    });
    if let (Some(v), Some(kbps)) = (s.video_encode_mut(), plan.target_kbps) {
        if matches!(v.rate, RateControl::TargetSize { .. }) {
            v.rate = RateControl::Vbr {
                kbps,
                max_kbps: None,
            };
        }
    }
    Some(s)
}

/// Extrapolated result of the full conversion.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Estimate {
    pub bytes: u64,
    /// Wall-clock encoding time.
    pub seconds: f64,
}

/// The size limit of a target-size job (its two-pass budget keeps the real
/// file under it, while a short sample over-represents container overhead).
pub fn size_cap(settings: &OutputSettings) -> Option<u64> {
    match settings.video_encode().map(|v| v.rate) {
        Some(RateControl::TargetSize { bytes }) => Some(bytes),
        _ => None,
    }
}

/// Scale a sample (`sample_bytes` for `sample_len` s of media, encoded in
/// `encode_secs`) up to `total_len` seconds, capped at `cap` bytes.
pub fn estimate(
    sample_bytes: u64,
    sample_len: f64,
    encode_secs: f64,
    total_len: f64,
    cap: Option<u64>,
) -> Option<Estimate> {
    if sample_len <= 0.0 || total_len <= 0.0 {
        return None;
    }
    let scale = total_len / sample_len;
    let bytes = (sample_bytes as f64 * scale).round() as u64;
    Some(Estimate {
        bytes: cap.map_or(bytes, |c| bytes.min(c)),
        seconds: encode_secs * scale,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::media::parse_ffprobe_json;
    use std::path::PathBuf;

    fn load(name: &str) -> MediaInfo {
        let dir =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/ffprobe");
        let json = std::fs::read_to_string(dir.join(format!("{name}.json"))).unwrap();
        parse_ffprobe_json(PathBuf::from(format!("/in/{name}")), &json).unwrap()
    }

    #[test]
    fn sample_is_centred_in_the_output_range() {
        let mut m = load("sdr-1080p.mp4");
        m.duration = Some(100.0);
        let mut s = crate::preset::find("mp4-compatible").unwrap().settings;
        assert_eq!(sample_range(&s, &m), Some((45.0, 10.0)));
        s.trim = Some(Trim {
            start: Some(20.0),
            end: Some(40.0),
        });
        assert_eq!(output_duration(&s, &m), Some(20.0));
        assert_eq!(sample_range(&s, &m), Some((25.0, 10.0)));
        // Shorter than a sample: the whole range.
        m.duration = Some(4.0);
        s.trim = None;
        assert_eq!(sample_range(&s, &m), Some((0.0, 4.0)));
        // Trim end beyond the file.
        m.duration = Some(30.0);
        s.trim = Some(Trim {
            start: Some(25.0),
            end: Some(90.0),
        });
        assert_eq!(output_duration(&s, &m), Some(5.0));
        m.duration = None;
        s.trim = None;
        assert_eq!(sample_range(&s, &m), None);
    }

    #[test]
    fn estimates_scale_linearly() {
        let e = estimate(2_000_000, 10.0, 4.0, 600.0, None).unwrap();
        assert_eq!(e.bytes, 120_000_000);
        assert!((e.seconds - 240.0).abs() < 1e-9);
        assert!(estimate(1, 0.0, 1.0, 10.0, None).is_none());
        // Target size: never above the limit.
        let s = crate::preset::with_target_size(
            crate::preset::find("target-size").unwrap().settings,
            10.0,
        );
        assert_eq!(size_cap(&s), Some(10_000_000));
        assert_eq!(
            estimate(3_000_000, 10.0, 1.0, 40.0, size_cap(&s))
                .unwrap()
                .bytes,
            10_000_000
        );
        assert_eq!(
            size_cap(&crate::preset::find("mp4-compatible").unwrap().settings),
            None
        );
    }
}
