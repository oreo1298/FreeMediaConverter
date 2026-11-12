// SPDX-License-Identifier: GPL-3.0-or-later
//! Translation markers. The core crate has no gettext dependency: it marks
//! user-visible literals with [`n_`] so `xgettext --keyword=n_` extracts
//! them, and the app translates them at display time with `gettext()`.

/// Mark a string for extraction; returns it unchanged.
pub const fn n_(s: &'static str) -> &'static str {
    s
}
