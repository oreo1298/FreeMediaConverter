// SPDX-License-Identifier: GPL-3.0-or-later
//! Retort: convert video and audio files with FFmpeg, using verified
//! hardware acceleration.

mod advanced;
mod card;
mod demo;
mod details;
mod i18n;
mod model;
mod picker;
mod prefs;
mod preview;
mod runner;
mod sidebar;
mod syscheck;
mod util;
mod window;
mod wrapbox;

use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gdk, gio, glib};

use i18n::gettext;
use retort_core::branding::{self, APP_ID};

fn load_css() {
    let provider = gtk::CssProvider::new();
    provider.load_from_string(include_str!("../../../data/style.css"));
    if let Some(display) = gdk::Display::default() {
        gtk::style_context_add_provider_for_display(
            &display,
            &provider,
            gtk::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
        // Uninstalled runs: find the app icon in the build tree.
        let theme = gtk::IconTheme::for_display(&display);
        if !theme.has_icon(APP_ID) {
            theme.add_search_path(env!("RETORT_DEV_ICON_DIR"));
        }
    }
}

fn add_action(app: &adw::Application, name: &str, f: impl Fn(&adw::Application) + 'static) {
    let a = gio::SimpleAction::new(name, None);
    let app2 = app.clone();
    a.connect_activate(move |_, _| f(&app2));
    app.add_action(&a);
}

fn add_path_action(
    app: &adw::Application,
    name: &str,
    f: impl Fn(&adw::Application, PathBuf) + 'static,
) {
    let a = gio::SimpleAction::new(name, Some(glib::VariantTy::STRING));
    let app2 = app.clone();
    a.connect_activate(move |_, v| {
        if let Some(p) = v.and_then(|v| v.get::<String>()) {
            f(&app2, PathBuf::from(p));
        }
    });
    app.add_action(&a);
}

fn about(app: &adw::Application) {
    let Some(ui) = window::app_ui(app) else {
        return;
    };
    let about = adw::AboutDialog::builder()
        .application_name(branding::APP_NAME)
        .application_icon(APP_ID)
        .version(branding::VERSION)
        .developer_name(format!("{} contributors", branding::APP_NAME))
        .license_type(gtk::License::Gpl30)
        .website(branding::APP_HOMEPAGE)
        .issue_url(format!("{}/issues", branding::APP_HOMEPAGE))
        .comments(gettext("Convert video and audio files with FFmpeg. Hardware acceleration is used only where a real test proved it works on your computer."))
        .build();
    let ffmpeg = ui
        .state
        .borrow()
        .engine
        .as_ref()
        .map(|e| {
            format!(
                "FFmpeg {} — {}",
                e.ff.version.short(),
                e.ff.bins.ffmpeg.display()
            )
        })
        .unwrap_or_default();
    if !ffmpeg.is_empty() {
        about.set_debug_info(&ffmpeg);
    }
    about.add_credit_section(
        Some(&gettext("Built on")),
        &[
            "FFmpeg https://ffmpeg.org",
            "GTK https://gtk.org",
            "libadwaita https://gnome.pages.gitlab.gnome.org/libadwaita/",
        ],
    );
    about.present(Some(&ui.window));
}

/// Keyboard shortcuts as (group, [(accelerator, title)]).
fn shortcut_groups() -> Vec<(String, Vec<(&'static str, String)>)> {
    vec![
        (
            gettext("Queue"),
            vec![
                ("<Ctrl>o", gettext("Add files")),
                ("<Ctrl>Return", gettext("Convert")),
                ("Delete", gettext("Remove selected files")),
                ("<Ctrl>a", gettext("Select all files")),
            ],
        ),
        (
            gettext("General"),
            vec![
                ("<Ctrl>comma", gettext("Preferences")),
                ("<Ctrl>question", gettext("Keyboard shortcuts")),
                ("F9", gettext("Show or hide output settings")),
                ("<Ctrl>q", gettext("Quit")),
            ],
        ),
    ]
}

/// AdwShortcutsDialog on libadwaita 1.8+ (GtkShortcutsWindow is deprecated
/// since GTK 4.18), GtkShortcutsWindow otherwise.
#[cfg(feature = "adw_1_8")]
fn shortcuts(app: &adw::Application) {
    let Some(ui) = window::app_ui(app) else {
        return;
    };
    let dialog = adw::ShortcutsDialog::new();
    for (title, items) in shortcut_groups() {
        let section = adw::ShortcutsSection::new(Some(&title));
        for (accel, t) in items {
            section.add(adw::ShortcutsItem::new(&t, accel));
        }
        dialog.add(section);
    }
    dialog.present(Some(&ui.window));
}

#[cfg(not(feature = "adw_1_8"))]
fn shortcuts(app: &adw::Application) {
    let Some(ui) = window::app_ui(app) else {
        return;
    };
    let section = gtk::ShortcutsSection::builder()
        .section_name("main")
        .build();
    for (title, items) in shortcut_groups() {
        let g = gtk::ShortcutsGroup::builder().title(title.as_str()).build();
        for (accel, t) in items {
            let s = gtk::ShortcutsShortcut::builder()
                .accelerator(accel)
                .title(t.as_str())
                .build();
            g.add_shortcut(&s);
        }
        section.add_group(&g);
    }
    let w = gtk::ShortcutsWindow::builder()
        .modal(true)
        .transient_for(&ui.window)
        .build();
    w.add_section(&section);
    w.present();
}

fn main() -> glib::ExitCode {
    i18n::init();
    let shot_dir = std::env::var_os("RETORT_SCREENSHOT_DIR").map(PathBuf::from);
    let demo = shot_dir.is_some();
    let mut flags = gio::ApplicationFlags::HANDLES_OPEN;
    if demo {
        flags |= gio::ApplicationFlags::NON_UNIQUE;
    }
    let app = adw::Application::builder()
        .application_id(APP_ID)
        .flags(flags)
        .build();
    glib::set_application_name(branding::APP_NAME);

    app.connect_startup(move |app| {
        load_css();
        gtk::Window::set_default_icon_name(APP_ID);
        add_action(app, "quit", |app| {
            if let Some(ui) = window::app_ui(app) {
                ui.window.close();
            }
            app.quit();
        });
        add_action(app, "preferences", |app| {
            if let Some(ui) = window::app_ui(app) {
                prefs::show(&ui);
            }
        });
        add_action(app, "system-check", |app| {
            if let Some(ui) = window::app_ui(app) {
                syscheck::show(&ui);
            }
        });
        add_action(app, "about", about);
        add_action(app, "shortcuts", shortcuts);
        add_action(app, "toggle-sidebar", |app| {
            if let Some(ui) = window::app_ui(app) {
                ui.split.set_show_sidebar(!ui.split.shows_sidebar());
            }
        });
        add_path_action(app, "open-folder", |app, p| {
            let w = window::app_ui(app).map(|u| u.window.clone().upcast::<gtk::Window>());
            util::open_folder(w.as_ref(), &p);
        });
        add_path_action(app, "show-file", |app, p| {
            let w = window::app_ui(app).map(|u| u.window.clone().upcast::<gtk::Window>());
            util::show_in_files(w.as_ref(), &p);
        });
        app.set_accels_for_action("app.quit", &["<Ctrl>q"]);
        app.set_accels_for_action("app.preferences", &["<Ctrl>comma"]);
        app.set_accels_for_action("app.shortcuts", &["<Ctrl>question"]);
        app.set_accels_for_action("app.toggle-sidebar", &["F9"]);
        app.set_accels_for_action("win.add-files", &["<Ctrl>o"]);
        app.set_accels_for_action("win.convert", &["<Ctrl>Return"]);
    });
    let shots = shot_dir.clone();
    app.connect_activate(move |app| {
        let ui = window::present(app, Vec::new(), demo);
        if let Some(dir) = &shots {
            demo::run(ui, dir.clone());
        }
    });
    let e2e = std::env::var_os("RETORT_E2E").is_some();
    app.connect_open(move |app, files, _| {
        let paths: Vec<PathBuf> = files.iter().filter_map(|f| f.path()).collect();
        let ui = window::present(app, paths, demo);
        if e2e {
            demo::e2e(ui);
        }
    });
    let code = app.run();
    if e2e {
        return glib::ExitCode::from(
            demo::E2E_STATUS.load(std::sync::atomic::Ordering::SeqCst) as u8
        );
    }
    code
}
