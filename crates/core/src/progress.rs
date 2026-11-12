// SPDX-License-Identifier: GPL-3.0-or-later
//! Parser for ffmpeg's `-progress pipe:1` key=value blocks.

use serde::{Deserialize, Serialize};

/// A progress snapshot for one step.
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
pub struct Progress {
    /// Output timestamp reached, seconds.
    pub out_time: f64,
    pub frame: Option<u64>,
    pub fps: Option<f64>,
    /// Realtime factor (e.g. 3.2 = 3.2× realtime).
    pub speed: Option<f64>,
    /// Bytes written so far.
    pub size: Option<u64>,
    /// Current output bitrate in kb/s.
    pub bitrate_kbps: Option<f64>,
    /// 0.0–1.0 for this step (if the duration is known).
    pub fraction: Option<f64>,
    /// Seconds remaining for this step (if computable).
    pub eta: Option<f64>,
    /// ffmpeg reported `progress=end`.
    pub finished: bool,
}

/// Incremental parser: feed lines, get a [`Progress`] at each block end.
#[derive(Debug, Default)]
pub struct Parser {
    /// Expected output duration (seconds) for fractions/ETA.
    pub duration: Option<f64>,
    cur: Progress,
}

fn num<T: std::str::FromStr>(v: &str) -> Option<T> {
    let v = v.trim();
    if v == "N/A" || v.is_empty() {
        None
    } else {
        v.parse().ok()
    }
}

/// "00:01:02.500000" → 62.5
fn parse_clock(v: &str) -> Option<f64> {
    let v = v.trim().trim_start_matches('-');
    let parts: Vec<&str> = v.split(':').collect();
    if parts.len() != 3 {
        return None;
    }
    Some(
        parts[0].parse::<f64>().ok()? * 3600.0
            + parts[1].parse::<f64>().ok()? * 60.0
            + parts[2].parse::<f64>().ok()?,
    )
}

impl Parser {
    pub fn new(duration: Option<f64>) -> Self {
        Parser {
            duration: duration.filter(|d| *d > 0.0),
            cur: Progress::default(),
        }
    }

    /// Feed one line. Returns a snapshot when a block ends (`progress=…`).
    pub fn feed(&mut self, line: &str) -> Option<Progress> {
        let (k, v) = line.trim().split_once('=')?;
        match k {
            "frame" => self.cur.frame = num(v),
            "fps" => self.cur.fps = num(v),
            "speed" => self.cur.speed = num(v.trim().trim_end_matches('x')),
            "total_size" => self.cur.size = num(v),
            "bitrate" => self.cur.bitrate_kbps = num(v.trim().trim_end_matches("kbits/s")),
            // out_time_us is authoritative; out_time_ms was in µs before 6.1 too (historic bug).
            "out_time_us" => {
                if let Some(us) = num::<i64>(v) {
                    self.cur.out_time = (us.max(0) as f64) / 1e6;
                }
            }
            "out_time" => {
                if self.cur.out_time == 0.0 {
                    if let Some(t) = parse_clock(v) {
                        self.cur.out_time = t;
                    }
                }
            }
            "progress" => {
                let mut p = self.cur;
                p.finished = v.trim() == "end";
                if let Some(d) = self.duration {
                    let frac = (p.out_time / d).clamp(0.0, 1.0);
                    p.fraction = Some(if p.finished { 1.0 } else { frac });
                    if let Some(sp) = p.speed.filter(|s| *s > 0.0) {
                        p.eta = Some(((d - p.out_time).max(0.0)) / sp);
                    }
                }
                self.cur = Progress::default();
                return Some(p);
            }
            _ => {}
        }
        None
    }
}

/// Format seconds as "1:02:03" / "2:03" for ETA displays.
pub fn format_eta(secs: f64) -> String {
    crate::media::format_duration(secs)
}

#[cfg(test)]
mod tests {
    use super::*;

    const BLOCK_61: &str = "frame=120\nfps=59.71\nstream_0_0_q=28.0\nbitrate=1234.5kbits/s\ntotal_size=786432\nout_time_us=5000000\nout_time_ms=5000000\nout_time=00:00:05.000000\ndup_frames=0\ndrop_frames=0\nspeed=2.49x\nprogress=continue\n";

    #[test]
    fn parses_blocks() {
        let mut p = Parser::new(Some(20.0));
        let mut out = None;
        for line in BLOCK_61.lines() {
            if let Some(x) = p.feed(line) {
                out = Some(x);
            }
        }
        let x = out.unwrap();
        assert_eq!(x.frame, Some(120));
        assert_eq!(x.fps, Some(59.71));
        assert_eq!(x.speed, Some(2.49));
        assert_eq!(x.size, Some(786432));
        assert_eq!(x.bitrate_kbps, Some(1234.5));
        assert_eq!(x.out_time, 5.0);
        assert_eq!(x.fraction, Some(0.25));
        assert!((x.eta.unwrap() - 15.0 / 2.49).abs() < 1e-9);
        assert!(!x.finished);
    }

    #[test]
    fn handles_na_and_end() {
        let mut p = Parser::new(None);
        for l in [
            "frame=0",
            "fps=0.00",
            "bitrate=N/A",
            "total_size=N/A",
            "out_time_us=N/A",
            "out_time=N/A",
            "speed=N/A",
        ] {
            assert!(p.feed(l).is_none());
        }
        let x = p.feed("progress=end").unwrap();
        assert!(x.finished);
        assert_eq!(x.size, None);
        assert_eq!(x.fraction, None);
        assert!(p.feed("garbage").is_none());
    }

    #[test]
    fn negative_out_time_clamped() {
        let mut p = Parser::new(Some(10.0));
        p.feed("out_time_us=-23220");
        let x = p.feed("progress=continue").unwrap();
        assert_eq!(x.out_time, 0.0);
    }
}
