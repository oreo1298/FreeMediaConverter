// SPDX-License-Identifier: GPL-3.0-or-later
//! Application identity, generated from `branding.env` by `build.rs`.

/// Human-readable application name, e.g. "Retort".
pub const APP_NAME: &str = env!("RETORT_BRAND_APP_NAME");
/// Reverse-DNS application ID, e.g. "io.github.owner.Retort".
pub const APP_ID: &str = env!("RETORT_BRAND_APP_ID");
/// Name of the GUI binary; the CLI is `<APP_BIN>-cli`. Also used for XDG dirs.
pub const APP_BIN: &str = env!("RETORT_BRAND_APP_BIN");
/// One-line summary.
pub const APP_SUMMARY: &str = env!("RETORT_BRAND_APP_SUMMARY");
/// Project homepage.
pub const APP_HOMEPAGE: &str = env!("RETORT_BRAND_APP_HOMEPAGE");
/// Version from Cargo.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Environment variable that overrides the ffmpeg binary, e.g. `RETORT_FFMPEG`.
pub fn ffmpeg_env_var() -> String {
    format!("{}_FFMPEG", APP_BIN.to_ascii_uppercase().replace('-', "_"))
}

/// Environment variable that overrides the ffprobe binary, e.g. `RETORT_FFPROBE`.
pub fn ffprobe_env_var() -> String {
    format!("{}_FFPROBE", APP_BIN.to_ascii_uppercase().replace('-', "_"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn branding_is_consistent() {
        assert!(
            APP_ID.ends_with(APP_NAME),
            "{APP_ID} should end with {APP_NAME}"
        );
        assert_eq!(APP_BIN, APP_NAME.to_lowercase());
        assert!(ffmpeg_env_var().ends_with("_FFMPEG"));
    }
}
