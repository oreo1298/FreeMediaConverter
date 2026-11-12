// SPDX-License-Identifier: GPL-3.0-or-later
//! Exposes the values of `branding.env` (repo root) as compile-time env vars
//! `RETORT_BRAND_<KEY>` so the app identity is defined in exactly one place.
use std::path::Path;

fn main() {
    let manifest = std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR");
    let path = Path::new(&manifest).join("../../branding.env");
    println!("cargo:rerun-if-changed={}", path.display());
    let text = std::fs::read_to_string(&path)
        .unwrap_or_else(|e| panic!("cannot read {}: {e}", path.display()));
    let mut seen = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let Some((key, value)) = line.split_once('=') else {
            panic!("malformed line in branding.env: {line}");
        };
        println!(
            "cargo:rustc-env=RETORT_BRAND_{}={}",
            key.trim(),
            value.trim()
        );
        seen.push(key.trim().to_string());
    }
    for required in [
        "APP_NAME",
        "APP_ID",
        "APP_BIN",
        "APP_SUMMARY",
        "APP_HOMEPAGE",
    ] {
        assert!(
            seen.iter().any(|k| k == required),
            "branding.env lacks {required}"
        );
    }
}
