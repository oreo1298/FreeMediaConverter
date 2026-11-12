// SPDX-License-Identifier: GPL-3.0-or-later
//! gettext helpers.

pub use gettextrs::{gettext, ngettext};

/// Where translations live: the build-time path, or `../share/locale` next
/// to the executable for relocatable installs (portable tarball, AppImage).
fn locale_dir() -> std::path::PathBuf {
    let built = std::path::PathBuf::from(env!("RETORT_LOCALEDIR"));
    if built.is_dir() {
        return built;
    }
    std::env::current_exe()
        .ok()
        .and_then(|exe| {
            exe.parent()
                .and_then(|bin| bin.parent())
                .map(|p| p.join("share/locale"))
        })
        .filter(|p| p.is_dir())
        .unwrap_or(built)
}

/// Initialise translations for the app's text domain.
pub fn init() {
    let domain = retort_core::branding::APP_BIN;
    // SAFETY: called first thing in main(), before any other thread exists.
    unsafe {
        gettextrs::setlocale(gettextrs::LocaleCategory::LcAll, "");
    }
    let _ = gettextrs::bindtextdomain(domain, locale_dir());
    let _ = gettextrs::bind_textdomain_codeset(domain, "UTF-8");
    let _ = gettextrs::textdomain(domain);
}

/// `gettext` for runtime strings (core catalog entries marked with `n_`).
/// An empty msgid would return the catalog header, so it is passed through.
pub fn tr(s: &str) -> String {
    if s.is_empty() {
        String::new()
    } else {
        gettext(s)
    }
}

/// `gettext` with `{}` placeholders filled in order.
pub fn tr_fmt(msgid: &str, args: &[&str]) -> String {
    let mut s = gettext(msgid);
    for a in args {
        if let Some(i) = s.find("{}") {
            s.replace_range(i..i + 2, a);
        }
    }
    s
}

/// `ngettext` with `{}` placeholders filled in order.
pub fn ntr_fmt(single: &str, plural: &str, n: u32, args: &[&str]) -> String {
    let mut s = ngettext(single, plural, n);
    for a in args {
        if let Some(i) = s.find("{}") {
            s.replace_range(i..i + 2, a);
        }
    }
    s
}
