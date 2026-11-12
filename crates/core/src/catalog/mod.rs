// SPDX-License-Identifier: GPL-3.0-or-later
//! The format/codec catalog: what can be written, and what fits in what.

pub mod codecs;
pub mod compat;
pub mod formats;

pub use codecs::{best_software_encoder, EncoderInfo};
pub use compat::{Compat, ContainerRules};
pub use formats::{FormatGroup, FormatInfo, FormatKind, Visibility};
