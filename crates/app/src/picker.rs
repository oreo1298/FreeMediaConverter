// SPDX-License-Identifier: GPL-3.0-or-later
//! Format picker: a searchable grid of format cards (every muxer this FFmpeg
//! can write), grouped, with the Simple presets at the top.

use std::rc::Rc;

use adw::prelude::*;

use crate::i18n::gettext;
use crate::util;
use crate::window::Ui;
use retort_core::catalog::{formats, FormatGroup, FormatInfo, FormatKind};
use retort_core::preset;

/// What the user picked.
#[derive(Debug, Clone)]
pub enum Choice {
    Preset(&'static str),
    Format(String),
}

fn kind_icon(k: FormatKind) -> &'static str {
    match k {
        FormatKind::Video => "video-x-generic-symbolic",
        FormatKind::Audio => "audio-x-generic-symbolic",
        FormatKind::Image | FormatKind::Animation => "image-x-generic-symbolic",
        FormatKind::Subtitle => "media-view-subtitles-symbolic",
        FormatKind::Other => "text-x-generic-symbolic",
    }
}

fn card(icon: &str, title: &str, ext: &str, desc: &str) -> gtk::Button {
    let b = gtk::Box::new(gtk::Orientation::Vertical, 4);
    let top = gtk::Box::new(gtk::Orientation::Horizontal, 8);
    let img = gtk::Image::from_icon_name(icon);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("heading");
    t.set_xalign(0.0);
    t.set_hexpand(true);
    t.set_ellipsize(gtk::pango::EllipsizeMode::End);
    top.append(&img);
    top.append(&t);
    if !ext.is_empty() {
        let e = gtk::Label::new(Some(&format!(".{ext}")));
        e.add_css_class("format-ext");
        top.append(&e);
    }
    b.append(&top);
    let d = gtk::Label::new(Some(desc));
    d.add_css_class("dim-label");
    d.add_css_class("caption");
    d.set_xalign(0.0);
    d.set_wrap(true);
    d.set_lines(2);
    d.set_ellipsize(gtk::pango::EllipsizeMode::End);
    b.append(&d);
    let btn = gtk::Button::new();
    btn.set_child(Some(&b));
    btn.add_css_class("format-card");
    btn.add_css_class("card");
    btn.update_property(&[gtk::accessible::Property::Label(&format!(
        "{title}: {desc}"
    ))]);
    btn
}

fn flow() -> gtk::FlowBox {
    gtk::FlowBox::builder()
        .selection_mode(gtk::SelectionMode::None)
        .min_children_per_line(1)
        .max_children_per_line(4)
        .homogeneous(true)
        .row_spacing(8)
        .column_spacing(8)
        .build()
}

/// Show the picker; `on_pick` is called with the choice.
pub fn show(ui: &Ui, include_presets: bool, on_pick: impl Fn(Choice) + 'static) -> adw::Dialog {
    let on_pick = Rc::new(on_pick);
    let formats: Vec<FormatInfo> = ui
        .state
        .borrow()
        .engine
        .as_ref()
        .map(|e| e.formats.as_ref().clone())
        .unwrap_or_default();
    let search = gtk::SearchEntry::builder()
        .placeholder_text(gettext("Search formats"))
        .hexpand(true)
        .build();
    let show_all = gtk::ToggleButton::builder()
        .icon_name("view-more-horizontal-symbolic")
        .tooltip_text(gettext("Show All Formats"))
        .build();
    show_all.set_active(ui.settings.boolean("show-all-formats"));
    let header = adw::HeaderBar::new();
    let clamp = adw::Clamp::builder()
        .maximum_size(400)
        .child(&search)
        .build();
    header.set_title_widget(Some(&clamp));
    header.pack_end(&show_all);
    let body = gtk::Box::new(gtk::Orientation::Vertical, 12);
    body.set_margin_start(18);
    body.set_margin_end(18);
    body.set_margin_top(12);
    body.set_margin_bottom(18);
    let scroller = gtk::ScrolledWindow::builder()
        .child(&body)
        .hscrollbar_policy(gtk::PolicyType::Never)
        .vexpand(true)
        .build();
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&scroller));
    let dialog = adw::Dialog::builder()
        .title(gettext("Choose a Format"))
        .content_width(820)
        .content_height(680)
        .child(&tv)
        .build();

    let fill: Rc<dyn Fn()> = {
        let body = body.clone();
        let search = search.clone();
        let show_all = show_all.clone();
        let dialog = dialog.clone();
        let on_pick = on_pick.clone();
        Rc::new(move || {
            util::clear_box(&body);
            let q = search.text().to_string();
            let ql = q.to_lowercase();
            let heading = |t: &str| {
                let h = gtk::Label::new(Some(t));
                h.add_css_class("title-4");
                h.set_xalign(0.0);
                h.set_margin_top(6);
                h
            };
            if include_presets {
                let fb = flow();
                for p in preset::builtin() {
                    let hit = ql.is_empty()
                        || p.title.to_lowercase().contains(&ql)
                        || p.subtitle.to_lowercase().contains(&ql);
                    if !hit {
                        continue;
                    }
                    let c = card(p.icon, &gettext(p.title), "", &gettext(p.subtitle));
                    let id = p.id;
                    let d = dialog.clone();
                    let cb = on_pick.clone();
                    c.connect_clicked(move |_| {
                        cb(Choice::Preset(id));
                        d.close();
                    });
                    fb.append(&c);
                }
                if fb.first_child().is_some() {
                    body.append(&heading(&gettext("Presets")));
                    body.append(&fb);
                }
            }
            let shown = formats::filter(&formats, show_all.is_active(), &q);
            for g in FormatGroup::ALL {
                let items: Vec<_> = shown.iter().filter(|f| f.group == g).collect();
                if items.is_empty() {
                    continue;
                }
                let fb = flow();
                for f in items {
                    let c = card(kind_icon(f.kind), &f.title, &f.extension, &f.description);
                    let id = f.id.clone();
                    let d = dialog.clone();
                    let cb = on_pick.clone();
                    c.connect_clicked(move |_| {
                        cb(Choice::Format(id.clone()));
                        d.close();
                    });
                    fb.append(&c);
                }
                body.append(&heading(&gettext(g.title())));
                body.append(&fb);
            }
            if body.first_child().is_none() {
                let empty = adw::StatusPage::builder()
                    .icon_name("system-search-symbolic")
                    .title(gettext("No Matching Formats"))
                    .description(if show_all.is_active() {
                        gettext("Try another search")
                    } else {
                        gettext("Try “Show All Formats” for exotic ones")
                    })
                    .build();
                body.append(&empty);
            }
        })
    };
    fill();
    let f = fill.clone();
    search.connect_search_changed(move |_| f());
    let f = fill.clone();
    let s = ui.settings.clone();
    let demo = ui.demo;
    show_all.connect_toggled(move |b| {
        if !demo {
            let _ = s.set_boolean("show-all-formats", b.is_active());
        }
        f();
    });
    dialog.present(Some(&ui.window));
    dialog
}
