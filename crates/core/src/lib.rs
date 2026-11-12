// SPDX-License-Identifier: GPL-3.0-or-later
//! Retort conversion engine.
//!
//! Everything that does not need GTK lives here: FFmpeg discovery and
//! capability parsing, media analysis, the format/codec catalog, hardware
//! discovery and probing, quality mapping, presets, the pipeline planner, the
//! command builder, the job runner and the system check. See
//! `docs/ARCHITECTURE.md`.

pub mod branding;
pub mod error;
pub mod ffmpeg;
pub mod media;
pub mod paths;
pub mod process;

pub use error::{Error, Result};
