// SPDX-License-Identifier: GPL-3.0-or-later
//! Small helpers shared by the UI modules.

use adw::prelude::*;
use gtk::{gio, glib};

use retort_core::branding::APP_ID;

/// Run `work` on a background thread and `done` with its result on the GTK
/// main loop. Engine calls (ffprobe, probing, planning) never block the UI.
pub fn spawn_worker<T: Send + 'static>(
    work: impl FnOnce() -> T + Send + 'static,
    done: impl FnOnce(T) + 'static,
) {
    let (tx, rx) = async_channel::bounded::<T>(1);
    std::thread::spawn(move || {
        let _ = tx.send_blocking(work());
    });
    glib::spawn_future_local(async move {
        if let Ok(v) = rx.recv().await {
            done(v);
        }
    });
}

/// The app's GSettings. Uses the installed schema, else the one build.rs
/// compiled (uninstalled runs). `memory` keeps changes in memory (screenshots).
/// `<prefix>/share` for the running executable (`<prefix>/bin/<exe>`).
pub fn exe_share_dir() -> Option<std::path::PathBuf> {
    let exe = std::env::current_exe().ok()?;
    let share = exe.parent()?.parent()?.join("share");
    share.is_dir().then_some(share)
}

pub fn settings(memory: bool) -> gio::Settings {
    let default = gio::SettingsSchemaSource::default();
    let schema = default
        .as_ref()
        .and_then(|s| s.lookup(APP_ID, true))
        .or_else(|| {
            // Relocatable installs (portable tarball, AppImage) ship a compiled
            // schema next to the binary; uninstalled builds use the one
            // build.rs compiled.
            let mut dirs = Vec::new();
            if let Some(share) = exe_share_dir() {
                dirs.push(share.join("glib-2.0/schemas"));
            }
            // Empty in Meson (installable) builds.
            let dev = env!("RETORT_DEV_SCHEMA_DIR");
            if !dev.is_empty() {
                dirs.push(std::path::PathBuf::from(dev));
            }
            dirs.into_iter()
                .filter(|d| d.join("gschemas.compiled").is_file())
                .find_map(|d| {
                    gio::SettingsSchemaSource::from_directory(&d, default.as_ref(), false)
                        .ok()
                        .and_then(|s| s.lookup(APP_ID, false))
                })
        })
        .expect("GSettings schema not found (install the app or build with cargo)");
    if memory {
        let backend = gio::memory_settings_backend_new();
        gio::Settings::new_full(&schema, Some(&backend), None)
    } else {
        gio::Settings::new_full(&schema, None::<&gio::SettingsBackend>, None)
    }
}

pub fn human_size(b: u64) -> String {
    let b = b as f64;
    if b >= 1e9 {
        format!("{:.2} GB", b / 1e9)
    } else if b >= 1e6 {
        format!("{:.1} MB", b / 1e6)
    } else if b >= 1e3 {
        format!("{:.0} kB", b / 1e3)
    } else {
        format!("{b:.0} B")
    }
}

pub fn eta(secs: f64) -> String {
    retort_core::media::format_duration(secs)
}

/// A spinning indicator (AdwSpinner on libadwaita 1.6+). It animates only
/// while mapped, so callers just toggle visibility.
pub fn spinner() -> gtk::Widget {
    #[cfg(feature = "adw_1_6")]
    {
        adw::Spinner::new().upcast()
    }
    #[cfg(not(feature = "adw_1_6"))]
    {
        let s = gtk::Spinner::new();
        s.set_spinning(true);
        s.upcast()
    }
}

/// Icon-only button with tooltip and accessible label.
pub fn icon_button(icon: &str, tooltip: &str) -> gtk::Button {
    let b = gtk::Button::from_icon_name(icon);
    b.set_tooltip_text(Some(tooltip));
    b.update_property(&[gtk::accessible::Property::Label(tooltip)]);
    b
}

/// A small rounded label.
pub fn pill(text: &str, extra_class: Option<&str>) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class("tag");
    if let Some(c) = extra_class {
        l.add_css_class(c);
    }
    l.set_valign(gtk::Align::Center);
    l
}

/// Copy text to the clipboard of `widget`'s display.
pub fn copy_text(widget: &impl IsA<gtk::Widget>, text: &str) {
    widget.as_ref().clipboard().set_text(text);
}

/// Open a folder in the file manager (portal-backed).
pub fn open_folder(parent: Option<&gtk::Window>, dir: &std::path::Path) {
    let file = gio::File::for_path(dir);
    let launcher = gtk::FileLauncher::new(Some(&file));
    launcher.launch(parent, gio::Cancellable::NONE, |_| {});
}

/// Show a file selected in the file manager (portal-backed).
pub fn show_in_files(parent: Option<&gtk::Window>, path: &std::path::Path) {
    let file = gio::File::for_path(path);
    let launcher = gtk::FileLauncher::new(Some(&file));
    launcher.open_containing_folder(parent, gio::Cancellable::NONE, |_| {});
}

/// Remove all children of a box.
pub fn clear_box(b: &gtk::Box) {
    while let Some(c) = b.first_child() {
        b.remove(&c);
    }
}
