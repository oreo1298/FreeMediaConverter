// SPDX-License-Identifier: GPL-3.0-or-later
//! Retort conversion engine.
//!
//! Everything that does not need GTK lives here: FFmpeg discovery and
//! capability parsing, media analysis, the format/codec catalog, hardware
//! discovery and probing, quality mapping, presets, the pipeline planner, the
//! command builder, the job runner and the system check. See
//! `docs/ARCHITECTURE.md`.

pub mod branding;
pub mod catalog;
pub mod command;
pub mod distro;
pub mod doctor;
pub mod engine;
pub mod error;
pub mod ffmpeg;
pub mod hw;
pub mod i18n;
pub mod job;
pub mod media;
pub mod naming;
pub mod paths;
pub mod plan;
pub mod preset;
pub mod preview;
pub mod process;
pub mod progress;
pub mod quality;
pub mod settings;
pub mod update;

pub use error::{Error, Result};
