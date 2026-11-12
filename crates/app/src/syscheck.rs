// SPDX-License-Identifier: GPL-3.0-or-later
//! System Check: the doctor report with copyable, distro-specific fixes.

use std::path::Path;

use adw::prelude::*;
use gtk::glib;

use crate::i18n::gettext;
use crate::util::{self, spawn_worker};
use crate::window::Ui;
use retort_core::doctor::{self, Report, Severity};
use retort_core::hw::{self, Inventory};
use retort_core::process::SystemRunner;

pub fn show(ui: &Ui) {
    let content = gtk::Box::new(gtk::Orientation::Vertical, 0);
    let spinner = gtk::Spinner::new();
    spinner.set_spinning(true);
    spinner.set_size_request(32, 32);
    let loading = adw::StatusPage::builder()
        .title(gettext("Checking Your System…"))
        .child(&spinner)
        .vexpand(true)
        .build();
    content.append(&loading);
    let header = adw::HeaderBar::new();
    let copy = gtk::Button::with_label(&gettext("Copy Report"));
    copy.set_sensitive(false);
    header.pack_start(&copy);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&content));
    let dialog = adw::Dialog::builder()
        .title(gettext("System Check"))
        .content_width(640)
        .content_height(720)
        .child(&tv)
        .build();
    dialog.present(Some(&ui.window));

    let (engine, err) = {
        let st = ui.state.borrow();
        (st.engine.clone(), st.engine_error.clone())
    };
    let u = ui.clone();
    spawn_worker(
        move || -> Report {
            match engine {
                Some(e) => doctor::check(
                    Some(&e.ff),
                    None,
                    Some(&e.caps),
                    &e.inventory,
                    Path::new("/"),
                ),
                None => {
                    let runner = SystemRunner;
                    let scan = hw::Scanner::new("/", &runner).scan();
                    let inv = Inventory {
                        schema: hw::SCHEMA,
                        key: String::new(),
                        created_unix: 0,
                        ffmpeg_path: Default::default(),
                        ffmpeg_version: String::new(),
                        scan,
                        probes: Default::default(),
                    };
                    doctor::check(None, err, None, &inv, Path::new("/"))
                }
            }
        },
        move |report| {
            util::clear_box(&content);
            content.append(&render(&report));
            let text = doctor::render_text(&report);
            copy.set_sensitive(true);
            let u2 = u.clone();
            copy.connect_clicked(move |b| {
                util::copy_text(b, &text);
                u2.toast(&gettext("Report copied"));
            });
        },
    );
}

/// Show a ready report (screenshot harness).
pub fn show_report(ui: &Ui, report: &Report) -> adw::Dialog {
    let header = adw::HeaderBar::new();
    let copy = gtk::Button::with_label(&gettext("Copy Report"));
    header.pack_start(&copy);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&render(report)));
    let dialog = adw::Dialog::builder()
        .title(gettext("System Check"))
        .content_width(640)
        .content_height(720)
        .child(&tv)
        .build();
    dialog.present(Some(&ui.window));
    dialog
}

/// Widgets for a report (also used by the screenshot harness).
pub fn render(report: &Report) -> gtk::Widget {
    let page = adw::PreferencesPage::new();
    let worst = report.worst();
    let (icon, title, desc) = match worst {
        Severity::Error => (
            "dialog-error-symbolic",
            gettext("Problems Found"),
            gettext("Follow the steps below; each command can be copied."),
        ),
        Severity::Warning => (
            "dialog-warning-symbolic",
            gettext("Mostly Fine"),
            gettext("Some features are limited. The steps below can fix that."),
        ),
        _ => (
            "emblem-ok-symbolic",
            gettext("Everything Works"),
            gettext("No problems were found."),
        ),
    };
    // A compact header (AdwStatusPage brings its own scroller and clips here).
    let head = gtk::Box::new(gtk::Orientation::Horizontal, 16);
    head.add_css_class("status-header");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(48);
    img.add_css_class(match worst {
        Severity::Error => "error",
        Severity::Warning => "warning",
        _ => "success",
    });
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 4);
    labels.set_valign(gtk::Align::Center);
    let t = gtk::Label::new(Some(&title));
    t.add_css_class("title");
    t.set_xalign(0.0);
    t.set_wrap(true);
    let d = gtk::Label::new(Some(&desc));
    d.add_css_class("dim-label");
    d.set_xalign(0.0);
    d.set_wrap(true);
    labels.append(&t);
    labels.append(&d);
    head.append(&img);
    head.append(&labels);
    let hg = adw::PreferencesGroup::new();
    hg.add(&head);
    page.add(&hg);

    let info = adw::PreferencesGroup::builder()
        .title(gettext("This Computer"))
        .build();
    let r = |t: &str, v: &str| {
        let row = adw::ActionRow::builder()
            .title(t)
            .subtitle(glib::markup_escape_text(v))
            .subtitle_selectable(true)
            .build();
        row.add_css_class("property");
        row
    };
    info.add(&r(
        &gettext("System"),
        &format!(
            "{}{}",
            report.os.display(),
            if report.in_flatpak { " · Flatpak" } else { "" }
        ),
    ));
    info.add(&r(&gettext("Kernel"), &report.kernel));
    info.add(&r(
        &gettext("FFmpeg"),
        &match (&report.ffmpeg_version, &report.ffmpeg_path) {
            (Some(v), Some(p)) => format!("{v} — {p}"),
            _ => gettext("Not found"),
        },
    ));
    page.add(&info);

    let groups = [
        (Severity::Error, gettext("Needs Attention")),
        (Severity::Warning, gettext("Recommended")),
        (Severity::Info, gettext("Good to Know")),
        (Severity::Ok, gettext("Working")),
    ];
    for (sev, title) in groups {
        let items: Vec<_> = report
            .findings
            .iter()
            .filter(|f| f.severity == sev)
            .collect();
        if items.is_empty() {
            continue;
        }
        let g = adw::PreferencesGroup::builder().title(title).build();
        for f in items {
            let icon = match f.severity {
                Severity::Error => "dialog-error-symbolic",
                Severity::Warning => "dialog-warning-symbolic",
                Severity::Info => "dialog-information-symbolic",
                Severity::Ok => "emblem-ok-symbolic",
            };
            let img = gtk::Image::from_icon_name(icon);
            match f.severity {
                Severity::Error => img.add_css_class("error"),
                Severity::Warning => img.add_css_class("warning"),
                Severity::Ok => img.add_css_class("success"),
                Severity::Info => {}
            }
            if f.fixes.is_empty() {
                let row = adw::ActionRow::builder()
                    .title(glib::markup_escape_text(&f.title))
                    .subtitle(glib::markup_escape_text(&f.detail))
                    .subtitle_lines(0)
                    .build();
                row.add_prefix(&img);
                g.add(&row);
                continue;
            }
            let exp = adw::ExpanderRow::builder()
                .title(glib::markup_escape_text(&f.title))
                .subtitle(glib::markup_escape_text(&f.detail))
                .subtitle_lines(0)
                .expanded(f.severity >= Severity::Warning)
                .build();
            exp.add_prefix(&img);
            for step in &f.fixes {
                let b = gtk::Box::new(gtk::Orientation::Vertical, 6);
                b.set_margin_top(10);
                b.set_margin_bottom(10);
                b.set_margin_start(12);
                b.set_margin_end(12);
                let d = gtk::Label::new(Some(&step.description));
                d.set_wrap(true);
                d.set_xalign(0.0);
                b.append(&d);
                for cmd in &step.commands {
                    let line = gtk::Box::new(gtk::Orientation::Horizontal, 6);
                    let l = gtk::Label::new(Some(cmd));
                    l.add_css_class("command");
                    l.set_selectable(true);
                    l.set_wrap(true);
                    l.set_wrap_mode(gtk::pango::WrapMode::Char);
                    l.set_xalign(0.0);
                    l.set_hexpand(true);
                    let copy = util::icon_button("edit-copy-symbolic", &gettext("Copy Command"));
                    copy.add_css_class("flat");
                    copy.set_valign(gtk::Align::Center);
                    let c = cmd.clone();
                    copy.connect_clicked(move |b| util::copy_text(b, &c));
                    line.append(&l);
                    line.append(&copy);
                    b.append(&line);
                }
                exp.add_row(&b);
            }
            g.add(&exp);
        }
        page.add(&g);
    }
    page.upcast()
}
