// SPDX-License-Identifier: GPL-3.0-or-later
//! File details: the ffprobe analysis in a clean layout, plus the plan,
//! the exact FFmpeg command and the log of the last run.

use adw::prelude::*;
use gtk::glib;

use crate::i18n::{gettext, tr_fmt};
use crate::util;
use crate::window::Ui;
use retort_core::media::{codec_display_name, format_duration, Hdr, StreamKind};

fn row(title: &str, value: &str) -> adw::ActionRow {
    let r = adw::ActionRow::builder()
        .title(glib::markup_escape_text(title))
        .subtitle(glib::markup_escape_text(value))
        .subtitle_selectable(true)
        .build();
    r.add_css_class("property");
    r
}

fn yes(b: bool) -> String {
    if b {
        gettext("Yes")
    } else {
        gettext("No")
    }
}

/// Show the details dialog for an item.
pub fn show(ui: &Ui, id: u64) {
    let st = ui.state.borrow();
    let Some(item) = st.items.iter().find(|i| i.id == id) else {
        return;
    };
    let page = adw::PreferencesPage::new();

    // Header: thumbnail + name.
    let head = gtk::Box::new(gtk::Orientation::Vertical, 8);
    head.set_halign(gtk::Align::Center);
    if let Some(t) = &item.thumbnail {
        let pic = gtk::Picture::for_paintable(t);
        pic.set_content_fit(gtk::ContentFit::Cover);
        // The thumbnail's own aspect ratio, 320 px wide, at most 240 tall.
        let (tw, th) = (
            t.intrinsic_width().max(1) as f64,
            t.intrinsic_height().max(1) as f64,
        );
        let h = (320.0 * th / tw).round().min(240.0) as i32;
        pic.set_size_request(320, h);
        pic.set_halign(gtk::Align::Center);
        pic.add_css_class("thumbnail");
        pic.set_overflow(gtk::Overflow::Hidden);
        head.append(&pic);
    }
    let name = gtk::Label::new(Some(&item.file_name()));
    name.add_css_class("title-3");
    name.set_wrap(true);
    name.set_justify(gtk::Justification::Center);
    head.append(&name);
    let path = gtk::Label::new(Some(&item.path.display().to_string()));
    path.add_css_class("dim-label");
    path.add_css_class("caption");
    path.set_wrap(true);
    path.set_selectable(true);
    path.set_justify(gtk::Justification::Center);
    head.append(&path);
    let hg = adw::PreferencesGroup::new();
    hg.add(&head);
    page.add(&hg);

    if let Some(m) = &item.media {
        let g = adw::PreferencesGroup::builder()
            .title(gettext("File"))
            .build();
        g.add(&row(
            &gettext("Container"),
            &m.format_long_name
                .clone()
                .unwrap_or_else(|| m.format_name.clone()),
        ));
        g.add(&row(
            &gettext("Duration"),
            &m.duration
                .map(format_duration)
                .unwrap_or_else(|| "—".into()),
        ));
        g.add(&row(
            &gettext("Size"),
            &m.size.map(util::human_size).unwrap_or_else(|| "—".into()),
        ));
        g.add(&row(
            &gettext("Overall Bitrate"),
            &m.bit_rate
                .map(|b| format!("{} kb/s", b / 1000))
                .unwrap_or_else(|| "—".into()),
        ));
        page.add(&g);

        for s in &m.streams {
            let (kind, icon) = match s.kind {
                StreamKind::Video if s.attached_pic => {
                    (gettext("Cover Art"), "image-x-generic-symbolic")
                }
                StreamKind::Video => (gettext("Video"), "video-x-generic-symbolic"),
                StreamKind::Audio => (gettext("Audio"), "audio-x-generic-symbolic"),
                StreamKind::Subtitle => (gettext("Subtitles"), "media-view-subtitles-symbolic"),
                StreamKind::Attachment => (gettext("Attachment"), "mail-attachment-symbolic"),
                _ => (gettext("Data"), "text-x-generic-symbolic"),
            };
            // "Audio 2" when there are several of a kind, else just "Audio".
            let same: Vec<_> = m
                .streams
                .iter()
                .filter(|o| o.kind == s.kind && o.attached_pic == s.attached_pic)
                .collect();
            let mut title = if same.len() > 1 {
                let n = same.iter().position(|o| o.index == s.index).unwrap_or(0) + 1;
                format!("{kind} {n}")
            } else {
                kind.clone()
            };
            if let Some(l) = &s.language {
                title.push_str(&format!(" · {l}"));
            }
            if let Some(t) = &s.title {
                title.push_str(&format!(" · {t}"));
            }
            let g = adw::PreferencesGroup::builder()
                .title(glib::markup_escape_text(&title))
                .build();
            let _ = icon;
            let mut codec = codec_display_name(&s.codec);
            if let Some(p) = &s.profile {
                codec.push_str(&format!(" ({p})"));
            }
            if let Some(l) = s
                .level
                .filter(|_| s.kind == StreamKind::Video)
                .and_then(|l| retort_core::media::level_display(&s.codec, l))
            {
                codec.push_str(&tr_fmt(", level {}", &[&l]));
            }
            g.add(&row(&gettext("Codec"), &codec));
            if let Some(v) = &s.video {
                let (dw, dh) = v.display_size();
                let mut res = format!("{}×{}", v.width, v.height);
                if v.rotation != 0 {
                    res.push_str(&tr_fmt(
                        " (rotated {}° → {}×{})",
                        &[&v.rotation.to_string(), &dw.to_string(), &dh.to_string()],
                    ));
                }
                if let Some(d) = v.dar.as_ref().filter(|d| *d != "0:1") {
                    res.push_str(&format!(" · {d}"));
                }
                g.add(&row(&gettext("Resolution"), &res));
                let fps = match (v.fps, v.vfr_likely) {
                    (Some(f), true) => {
                        tr_fmt("{} fps (variable frame rate)", &[&format!("{f:.3}")])
                    }
                    (Some(f), false) => format!("{f:.3} fps"),
                    (None, _) => "—".into(),
                };
                g.add(&row(&gettext("Frame Rate"), &fps));
                g.add(&row(
                    &gettext("Pixel Format"),
                    &format!(
                        "{} · {}-bit",
                        v.pix_fmt.clone().unwrap_or_default(),
                        v.bit_depth
                    ),
                ));
                let color = [
                    &v.color_space,
                    &v.color_primaries,
                    &v.color_transfer,
                    &v.color_range,
                ]
                .iter()
                .map(|c| (*c).clone().unwrap_or_else(|| "?".into()))
                .collect::<Vec<_>>()
                .join(" / ");
                g.add(&row(
                    &gettext("Color (matrix / primaries / transfer / range)"),
                    &color,
                ));
                let hdr = match &v.hdr {
                    Hdr::Sdr => gettext("SDR"),
                    h => h.label().unwrap_or_default(),
                };
                g.add(&row(&gettext("Dynamic Range"), &hdr));
                if let Some(md) = &v.mastering_display {
                    g.add(&row(&gettext("Mastering Display"), &md.x265_string()));
                }
                if let Some(cl) = &v.content_light {
                    g.add(&row(
                        &gettext("Content Light Level"),
                        &format!("MaxCLL {} · MaxFALL {}", cl.max_cll, cl.max_fall),
                    ));
                }
                if v.interlaced() {
                    g.add(&row(
                        &gettext("Scan"),
                        &tr_fmt(
                            "Interlaced ({})",
                            &[&v.field_order.clone().unwrap_or_default()],
                        ),
                    ));
                }
            }
            if let Some(a) = &s.audio {
                g.add(&row(
                    &gettext("Channels"),
                    &format!("{} ({})", a.layout_label(), a.channels),
                ));
                g.add(&row(
                    &gettext("Sample Rate"),
                    &a.sample_rate
                        .map(|r| format!("{r} Hz"))
                        .unwrap_or_else(|| "—".into()),
                ));
                if let Some(b) = a.bit_depth {
                    g.add(&row(&gettext("Bit Depth"), &b.to_string()));
                }
            }
            if s.kind == StreamKind::Subtitle {
                g.add(&row(
                    &gettext("Type"),
                    &if s.is_bitmap_subtitle() {
                        gettext("Bitmap (image-based)")
                    } else {
                        gettext("Text")
                    },
                ));
                g.add(&row(&gettext("Forced"), &yes(s.forced)));
            }
            if let Some((name, mime)) = &s.attachment {
                g.add(&row(&gettext("File"), &format!("{name} ({mime})")));
            }
            if matches!(
                s.kind,
                StreamKind::Audio | StreamKind::Subtitle | StreamKind::Video
            ) && !s.attached_pic
            {
                g.add(&row(&gettext("Default Track"), &yes(s.default)));
            }
            if let Some(b) = s.bit_rate {
                g.add(&row(&gettext("Bitrate"), &format!("{} kb/s", b / 1000)));
            }
            page.add(&g);
        }

        if !m.chapters.is_empty() {
            let g = adw::PreferencesGroup::builder()
                .title(gettext("Chapters"))
                .build();
            for c in &m.chapters {
                g.add(&row(
                    &format!("{} – {}", format_duration(c.start), format_duration(c.end)),
                    &c.title.clone().unwrap_or_default(),
                ));
            }
            page.add(&g);
        }
        if !m.tags.is_empty() {
            let g = adw::PreferencesGroup::builder()
                .title(gettext("Metadata"))
                .build();
            for (k, v) in &m.tags {
                g.add(&row(k, v));
            }
            page.add(&g);
        }
    }

    // Conversion.
    let g = adw::PreferencesGroup::builder()
        .title(gettext("Conversion"))
        .build();
    if let (Some(b), Some(s)) = (&item.badge, &item.summary) {
        g.add(&row(b, s));
    }
    for n in &item.notes {
        g.add(&row(&gettext("Note"), n));
    }
    if let Some(cmd) = &item.command {
        let label = gtk::Label::new(Some(cmd));
        label.add_css_class("command");
        label.set_selectable(true);
        label.set_wrap(true);
        label.set_wrap_mode(gtk::pango::WrapMode::Char);
        label.set_xalign(0.0);
        let copy = util::icon_button("edit-copy-symbolic", &gettext("Copy Command"));
        copy.add_css_class("flat");
        copy.set_valign(gtk::Align::Start);
        let c = cmd.clone();
        copy.connect_clicked(move |b| util::copy_text(b, &c));
        let b = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        b.append(&label);
        b.append(&copy);
        label.set_hexpand(true);
        b.set_margin_top(6);
        g.add(&b);
    }
    if !item.log.is_empty() {
        let exp = adw::ExpanderRow::builder()
            .title(gettext("FFmpeg Log"))
            .build();
        let buf = gtk::TextBuffer::new(None);
        buf.set_text(&item.log.join("\n"));
        let tv = gtk::TextView::with_buffer(&buf);
        tv.set_editable(false);
        tv.set_monospace(true);
        tv.add_css_class("log");
        tv.set_wrap_mode(gtk::WrapMode::WordChar);
        let sw = gtk::ScrolledWindow::builder()
            .child(&tv)
            .min_content_height(200)
            .max_content_height(360)
            .build();
        exp.add_row(&sw);
        g.add(&exp);
    }
    page.add(&g);

    let header = adw::HeaderBar::new();
    let copy = gtk::Button::with_label(&gettext("Copy Diagnostics"));
    let u = ui.clone();
    copy.connect_clicked(move |b| {
        util::copy_text(b, &diagnostics_text(&u, id));
        u.toast(&gettext(
            "Diagnostics copied — paste them into a bug report",
        ));
    });
    header.pack_start(&copy);
    let tv = adw::ToolbarView::new();
    tv.add_top_bar(&header);
    tv.set_content(Some(&page));
    let dialog = adw::Dialog::builder()
        .title(gettext("File Details"))
        .content_width(560)
        .content_height(720)
        .child(&tv)
        .build();
    drop(st);
    dialog.present(Some(&ui.window));
}

/// Plain-text diagnostics for a bug report.
pub fn diagnostics_text(ui: &Ui, id: u64) -> String {
    let st = ui.state.borrow();
    let mut s = String::new();
    s.push_str(&format!(
        "{} {}\n",
        retort_core::branding::APP_NAME,
        retort_core::branding::VERSION
    ));
    let (os, _) = retort_core::distro::detect(std::path::Path::new("/"));
    s.push_str(&format!(
        "OS: {}{}\n",
        os.display(),
        if retort_core::paths::in_flatpak() {
            " (Flatpak)"
        } else {
            ""
        }
    ));
    if let Some(e) = &st.engine {
        s.push_str(&format!(
            "FFmpeg: {} ({})\n",
            e.ff.version.raw,
            e.ff.bins.ffmpeg.display()
        ));
        for d in e.inventory.device_summaries() {
            s.push_str(&format!(
                "GPU: {} — encode {:?} decode {:?}\n",
                d.label, d.encode, d.decode
            ));
        }
    }
    if let Some(item) = st.items.iter().find(|i| i.id == id) {
        s.push_str(&format!("\nInput: {}\n", item.file_name()));
        if let Some(m) = &item.media {
            s.push_str(&format!(
                "  {} · {}\n",
                m.format_name,
                m.summary_pills().join(" · ")
            ));
            for st in &m.streams {
                s.push_str(&format!(
                    "  #{} {:?} {}{}\n",
                    st.index,
                    st.kind,
                    st.codec,
                    st.profile
                        .as_ref()
                        .map(|p| format!(" ({p})"))
                        .unwrap_or_default()
                ));
            }
        }
        s.push_str(&format!(
            "Preset: {}\nStatus: {:?}\n",
            item.preset, item.status
        ));
        if let Some(sum) = &item.summary {
            s.push_str(&format!("Plan: {sum}\n"));
        }
        for n in &item.notes {
            s.push_str(&format!("Note: {n}\n"));
        }
        if let Some(c) = &item.command {
            s.push_str(&format!("\nCommand:\n{c}\n"));
        }
        if !item.log.is_empty() {
            s.push_str("\nLog (last 80 lines):\n");
            let start = item.log.len().saturating_sub(80);
            for l in &item.log[start..] {
                s.push_str(l);
                s.push('\n');
            }
        }
    }
    s
}
