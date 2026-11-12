// SPDX-License-Identifier: GPL-3.0-or-later
use std::path::PathBuf;

/// Errors produced by the engine.
#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("ffmpeg was not found (looked in: {searched})")]
    FfmpegNotFound { searched: String },
    #[error("ffprobe was not found next to {ffmpeg} or in PATH")]
    FfprobeNotFound { ffmpeg: PathBuf },
    #[error("could not run {program}: {source}")]
    Spawn {
        program: String,
        #[source]
        source: std::io::Error,
    },
    #[error("{program} timed out after {seconds:.1} s")]
    Timeout { program: String, seconds: f64 },
    #[error("{program} failed (exit {code:?}): {stderr_tail}")]
    Failed {
        program: String,
        code: Option<i32>,
        stderr_tail: String,
    },
    #[error("could not parse {what}: {detail}")]
    Parse { what: &'static str, detail: String },
    #[error("I/O error on {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid settings: {0}")]
    InvalidSettings(String),
    #[error("no usable encoder for {0}")]
    NoEncoder(String),
    #[error("output already exists: {0}")]
    OutputExists(PathBuf),
    #[error("output would overwrite the input: {0}")]
    OutputIsInput(PathBuf),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl Error {
    pub fn io(path: impl Into<PathBuf>, source: std::io::Error) -> Self {
        Error::Io {
            path: path.into(),
            source,
        }
    }
    pub fn parse(what: &'static str, detail: impl Into<String>) -> Self {
        Error::Parse {
            what,
            detail: detail.into(),
        }
    }
}

/// Last `n` non-empty lines of a byte buffer, for error messages.
pub fn tail_lines(bytes: &[u8], n: usize) -> String {
    let text = String::from_utf8_lossy(bytes);
    let lines: Vec<&str> = text.lines().filter(|l| !l.trim().is_empty()).collect();
    let start = lines.len().saturating_sub(n);
    lines[start..].join("\n")
}
