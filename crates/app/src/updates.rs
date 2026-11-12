// SPDX-License-Identifier: GPL-3.0-or-later
//! Opt-in update check (only compiled into AppImage and tarball builds:
//! distro packages and Flatpak update through their own channels).

use adw::prelude::*;
use gtk::glib;

use crate::i18n::{gettext, tr_fmt};
use crate::util::spawn_worker;
use crate::window::Ui;
use retort_core::process::SystemRunner;
use retort_core::update;

const DAY: i64 = 24 * 60 * 60;

/// Check once a day if the user turned it on.
pub fn maybe_check(ui: &Ui) {
    if ui.demo || !ui.settings.boolean("check-updates") {
        return;
    }
    let now = glib::real_time() / 1_000_000;
    if now - ui.settings.int64("last-update-check") < DAY {
        return;
    }
    let _ = ui.settings.set_int64("last-update-check", now);
    let u = ui.clone();
    spawn_worker(
        || update::check(&SystemRunner).ok().flatten(),
        move |found| {
            let Some(release) = found else { return };
            let toast = adw::Toast::builder()
                .title(tr_fmt("Retort {} is available", &[&release.version]))
                .button_label(gettext("Download"))
                .timeout(0)
                .build();
            let w = u.window.clone();
            toast.connect_button_clicked(move |_| {
                gtk::UriLauncher::new(&release.url).launch(
                    Some(&w),
                    gtk::gio::Cancellable::NONE,
                    |_| {},
                );
            });
            u.toasts.add_toast(toast);
        },
    );
}
