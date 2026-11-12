// SPDX-License-Identifier: GPL-3.0-or-later
//! A queue card: thumbnail or waveform, file name, spec pills, source →
//! target, hardware badge, progress and actions.

use std::cell::RefCell;
use std::rc::Rc;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, tr_fmt};
use crate::model::{Item, Status};
use crate::util::{self, icon_button, pill};
use crate::wrapbox::{FixedBin, WrapBox};
use retort_core::catalog::codecs;

/// Widgets of one card.
pub struct Card {
    pub row: gtk::ListBoxRow,
    thumb_stack: gtk::Stack,
    thumb_bin: FixedBin,
    picture: gtk::Picture,
    placeholder: gtk::Image,
    wave: gtk::DrawingArea,
    peaks: Rc<RefCell<Vec<f32>>>,
    name: gtk::Label,
    pills: WrapBox,
    arrow: gtk::Image,
    target: gtk::Label,
    badge: gtk::Label,
    progress: gtk::ProgressBar,
    status: gtk::Label,
    status_icon: gtk::Image,
    spinner: gtk::Spinner,
    pause: gtk::Button,
    cancel: gtk::Button,
    remove: gtk::Button,
    show_file: gtk::Button,
    retry: gtk::Button,
    compact: std::cell::Cell<bool>,
    pub actions: gio::SimpleActionGroup,
}

impl Card {
    pub fn new(item: &Item) -> Card {
        let row = gtk::ListBoxRow::new();
        row.set_activatable(true);
        let outer = gtk::Box::new(gtk::Orientation::Horizontal, 14);
        outer.add_css_class("queue-card");

        // Thumbnail / waveform / placeholder.
        let thumb_stack = gtk::Stack::new();
        thumb_stack.set_overflow(gtk::Overflow::Hidden);
        thumb_stack.add_css_class("thumbnail");
        let picture = gtk::Picture::new();
        picture.set_content_fit(gtk::ContentFit::Cover);
        picture.set_can_shrink(true);
        picture.set_overflow(gtk::Overflow::Hidden);
        picture.add_css_class("thumbnail");
        let placeholder = gtk::Image::from_icon_name("video-x-generic-symbolic");
        placeholder.set_pixel_size(32);
        placeholder.add_css_class("thumbnail-placeholder");
        let wave = gtk::DrawingArea::new();
        let peaks: Rc<RefCell<Vec<f32>>> = Rc::default();
        {
            let peaks = peaks.clone();
            wave.set_draw_func(move |area, cr, w, h| {
                let p = peaks.borrow();
                let accent = area.color();
                let (wf, hf) = (w as f64, h as f64);
                cr.set_source_rgba(
                    accent.red() as f64,
                    accent.green() as f64,
                    accent.blue() as f64,
                    0.08,
                );
                cr.rectangle(0.0, 0.0, wf, hf);
                let _ = cr.fill();
                if p.is_empty() {
                    return;
                }
                cr.set_source_rgba(0.21, 0.52, 0.89, 0.9);
                let n = p.len() as f64;
                // Fewer, wider bars on small thumbnails.
                let step = if wf < 100.0 { 2 } else { 1 };
                let n = (n / step as f64).ceil();
                let bar = (wf - 16.0) / n;
                for (i, v) in p.iter().step_by(step).enumerate() {
                    let amp = (*v as f64).max(0.04) * (hf * 0.38);
                    let x = 8.0 + i as f64 * bar;
                    cr.rectangle(x, hf / 2.0 - amp, (bar * 0.6).max(1.0), amp * 2.0);
                }
                let _ = cr.fill();
            });
        }
        thumb_stack.add_named(&placeholder, Some("placeholder"));
        thumb_stack.add_named(&picture, Some("picture"));
        thumb_stack.add_named(&wave, Some("wave"));
        let thumb_bin = FixedBin::new(&thumb_stack, 128, 72);
        thumb_bin.set_valign(gtk::Align::Center);
        outer.append(&thumb_bin);

        // Middle column.
        let mid = gtk::Box::new(gtk::Orientation::Vertical, 6);
        mid.set_hexpand(true);
        mid.set_valign(gtk::Align::Center);
        let name = gtk::Label::new(Some(&item.file_name()));
        name.add_css_class("filename");
        name.set_xalign(0.0);
        name.set_wrap(true);
        name.set_wrap_mode(gtk::pango::WrapMode::WordChar);
        name.set_lines(2);
        name.set_ellipsize(gtk::pango::EllipsizeMode::Middle);
        name.set_tooltip_text(Some(&item.path.display().to_string()));
        mid.append(&name);

        // Source tags → target tag and hardware badge, wrapping on narrow cards.
        let pills = WrapBox::new(4, 4);
        let arrow = gtk::Image::from_icon_name("go-next-symbolic");
        arrow.add_css_class("arrow");
        arrow.set_margin_start(2);
        arrow.set_margin_end(2);
        let target = pill("", Some("target"));
        let badge = pill("", Some("cpu"));
        badge.set_visible(false);
        mid.append(&pills);

        let progress = gtk::ProgressBar::new();
        progress.add_css_class("card-progress");
        progress.set_visible(false);
        mid.append(&progress);
        let status_box = gtk::Box::new(gtk::Orientation::Horizontal, 6);
        let status_icon = gtk::Image::new();
        status_icon.set_visible(false);
        let spinner = gtk::Spinner::new();
        spinner.set_visible(false);
        let status = gtk::Label::new(None);
        status.set_xalign(0.0);
        status.add_css_class("status-line");
        status.add_css_class("dim-label");
        status.set_ellipsize(gtk::pango::EllipsizeMode::End);
        status.set_hexpand(true);
        status_box.append(&spinner);
        status_box.append(&status_icon);
        status_box.append(&status);
        mid.append(&status_box);
        let retry = gtk::Button::with_label(&gettext("Retry with Software"));
        retry.add_css_class("small-pill");
        retry.set_halign(gtk::Align::Start);
        retry.set_action_name(Some("card.retry-software"));
        retry.set_visible(false);
        mid.append(&retry);
        outer.append(&mid);

        // Actions.
        let buttons = gtk::Box::new(gtk::Orientation::Horizontal, 2);
        buttons.set_valign(gtk::Align::Center);
        let pause = icon_button("media-playback-pause-symbolic", &gettext("Pause"));
        let cancel = icon_button("process-stop-symbolic", &gettext("Cancel"));
        let remove = icon_button("window-close-symbolic", &gettext("Remove from Queue"));
        let show_file = icon_button("folder-open-symbolic", &gettext("Show in Files"));
        show_file.set_action_name(Some("card.show"));
        for b in [&pause, &cancel, &remove, &show_file] {
            b.add_css_class("flat");
            b.add_css_class("circular");
        }
        let menu = gio::Menu::new();
        let section = gio::Menu::new();
        section.append(Some(&gettext("File Details")), Some("card.details"));
        section.append(Some(&gettext("Show in Files")), Some("card.show"));
        menu.append_section(None, &section);
        let section = gio::Menu::new();
        section.append(
            Some(&gettext("Copy FFmpeg Command")),
            Some("card.copy-command"),
        );
        section.append(
            Some(&gettext("Copy Diagnostics")),
            Some("card.copy-diagnostics"),
        );
        section.append(
            Some(&gettext("Retry with Software")),
            Some("card.retry-software"),
        );
        menu.append_section(None, &section);
        let section = gio::Menu::new();
        section.append(Some(&gettext("Remove from Queue")), Some("card.remove"));
        menu.append_section(None, &section);
        let menu_btn = gtk::MenuButton::builder()
            .icon_name("view-more-symbolic")
            .menu_model(&menu)
            .tooltip_text(gettext("More"))
            .build();
        menu_btn.add_css_class("flat");
        menu_btn.add_css_class("circular");
        menu_btn.update_property(&[gtk::accessible::Property::Label(&gettext("More"))]);
        buttons.append(&show_file);
        buttons.append(&pause);
        buttons.append(&cancel);
        buttons.append(&remove);
        buttons.append(&menu_btn);
        outer.append(&buttons);

        row.set_child(Some(&outer));
        let actions = gio::SimpleActionGroup::new();
        row.insert_action_group("card", Some(&actions));
        let card = Card {
            row,
            thumb_stack,
            thumb_bin,
            picture,
            placeholder,
            wave,
            peaks,
            name,
            pills,
            arrow,
            target,
            badge,
            progress,
            status,
            status_icon,
            spinner,
            pause,
            cancel,
            remove,
            show_file,
            retry,
            compact: Default::default(),
            actions,
        };
        card.update(item);
        card
    }

    pub fn connect_pause(&self, f: impl Fn() + 'static) {
        self.pause.connect_clicked(move |_| f());
    }
    pub fn connect_cancel(&self, f: impl Fn() + 'static) {
        self.cancel.connect_clicked(move |_| f());
    }
    pub fn connect_remove(&self, f: impl Fn() + 'static) {
        let f = std::rc::Rc::new(f);
        let g = f.clone();
        self.remove.connect_clicked(move |_| g());
        self.add_action("remove", move || f());
    }

    /// Narrow layout: smaller square thumbnail, removal only via the menu.
    pub fn set_compact(&self, compact: bool) {
        self.compact.set(compact);
        if compact {
            self.thumb_bin.set_size(56, 56);
        } else {
            self.thumb_bin.set_size(128, 72);
        }
        self.wave.queue_draw();
    }

    /// Add a `card.<name>` action.
    pub fn add_action(&self, name: &str, f: impl Fn() + 'static) {
        let a = gio::SimpleAction::new(name, None);
        a.connect_activate(move |_, _| f());
        self.actions.add_action(&a);
    }

    fn set_action_enabled(&self, name: &str, enabled: bool) {
        if let Some(a) = self
            .actions
            .lookup_action(name)
            .and_then(|a| a.downcast::<gio::SimpleAction>().ok())
        {
            a.set_enabled(enabled);
        }
    }

    fn set_status(&self, text: &str, icon: Option<&str>, class: Option<&str>) {
        self.status.set_text(text);
        self.status.set_tooltip_text(Some(text));
        for c in [
            "status-done",
            "status-failed",
            "status-warning",
            "dim-label",
        ] {
            self.status.remove_css_class(c);
            self.status_icon.remove_css_class(c);
        }
        match class {
            Some(c) => {
                self.status.add_css_class(c);
                self.status_icon.add_css_class(c);
            }
            None => self.status.add_css_class("dim-label"),
        }
        match icon {
            Some(i) => {
                self.status_icon.set_icon_name(Some(i));
                self.status_icon.set_visible(true);
            }
            None => self.status_icon.set_visible(false),
        }
    }

    /// Refresh everything from the item.
    pub fn update(&self, item: &Item) {
        self.name.set_text(&item.file_name());
        // Thumbnail.
        if let Some(t) = &item.thumbnail {
            self.picture.set_paintable(Some(t));
            self.thumb_stack.set_visible_child_name("picture");
        } else if let Some(p) = &item.peaks {
            *self.peaks.borrow_mut() = p.clone();
            self.wave.queue_draw();
            self.thumb_stack.set_visible_child_name("wave");
        } else {
            let audio = item.media.as_ref().map(|m| !m.has_video()).unwrap_or(false);
            self.placeholder.set_icon_name(Some(if audio {
                "audio-x-generic-symbolic"
            } else {
                "video-x-generic-symbolic"
            }));
            self.thumb_stack.set_visible_child_name("placeholder");
        }
        // Source tags, then → target and the hardware badge.
        self.pills.remove_all();
        if let Some(m) = &item.media {
            for p in m.summary_pills() {
                let hdr = p.starts_with("HDR") || p.starts_with("HLG") || p.starts_with("Dolby");
                self.pills
                    .append(&pill(&p, if hdr { Some("hdr") } else { None }));
            }
        }
        self.pills.append(&self.arrow);
        self.pills.append(&self.target);
        self.pills.append(&self.badge);
        // Target pill.
        let fmt_title = retort_core::preset::find(&item.preset)
            .filter(|p| p.settings == item.settings)
            .map(|p| p.title.to_string());
        let target = target_label(item);
        self.target.set_text(&target);
        self.target.set_tooltip_text(fmt_title.as_deref());
        // Hardware badge.
        match &item.badge {
            Some(b) => {
                self.badge.set_text(b);
                self.badge
                    .remove_css_class(if item.hardware { "cpu" } else { "hw" });
                self.badge
                    .add_css_class(if item.hardware { "hw" } else { "cpu" });
                self.badge.set_tooltip_text(item.summary.as_deref());
                self.badge.set_visible(true);
            }
            None => self.badge.set_visible(false),
        }
        // Status.
        let running = matches!(item.status, Status::Running | Status::Paused);
        self.progress.set_visible(running);
        self.spinner
            .set_visible(matches!(item.status, Status::Analyzing));
        self.spinner
            .set_spinning(matches!(item.status, Status::Analyzing));
        self.pause.set_visible(running);
        self.cancel.set_visible(item.status.is_active());
        let done = matches!(item.status, Status::Done { .. });
        self.remove
            .set_visible(!item.status.is_active() && !self.compact.get());
        self.show_file.set_visible(done);
        self.retry.set_visible(matches!(
            item.status,
            Status::Failed {
                retry_software: true,
                ..
            }
        ));
        self.pause.set_icon_name(if item.status == Status::Paused {
            "media-playback-start-symbolic"
        } else {
            "media-playback-pause-symbolic"
        });
        let pause_label = if item.status == Status::Paused {
            gettext("Resume")
        } else {
            gettext("Pause")
        };
        self.pause.set_tooltip_text(Some(&pause_label));
        self.pause
            .update_property(&[gtk::accessible::Property::Label(&pause_label)]);
        match &item.status {
            Status::Analyzing => self.set_status(&gettext("Reading file…"), None, None),
            Status::Ready => {
                let note = item.notes.first().cloned();
                match note {
                    Some(n) => self.set_status(&n, Some("dialog-information-symbolic"), None),
                    None => self.set_status(&gettext("Ready"), None, None),
                }
            }
            Status::Queued => {
                self.set_status(&gettext("Waiting…"), Some("content-loading-symbolic"), None)
            }
            Status::Running | Status::Paused => {
                let l = item.live;
                if let Some(f) = l.fraction {
                    self.progress.set_fraction(f);
                } else {
                    self.progress.pulse();
                }
                let mut parts = Vec::new();
                if item.status == Status::Paused {
                    parts.push(gettext("Paused"));
                }
                if let Some(f) = l.fraction {
                    parts.push(format!("{:.0}%", f * 100.0));
                }
                if let Some(s) = l.progress.speed.filter(|s| *s > 0.0) {
                    parts.push(format!("{s:.1}×"));
                }
                if let Some(fps) = l.progress.fps.filter(|f| *f > 0.0) {
                    parts.push(format!("{fps:.0} fps"));
                }
                if let Some(e) = l.eta.filter(|_| item.status == Status::Running) {
                    parts.push(tr_fmt("{} left", &[&util::eta(e)]));
                }
                if let Some(sz) = l.progress.size.filter(|s| *s > 0) {
                    parts.push(util::human_size(sz));
                }
                self.set_status(&parts.join(" · "), None, None);
            }
            Status::Done {
                output,
                size,
                fallback,
            } => {
                let name = output
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                match fallback {
                    Some(reason) => {
                        self.set_status(
                            &tr_fmt(
                                "Completed (software fallback) · {} · {}",
                                &[&util::human_size(*size), &name],
                            ),
                            Some("dialog-warning-symbolic"),
                            Some("status-warning"),
                        );
                        self.status.set_tooltip_text(Some(reason));
                    }
                    None => self.set_status(
                        &tr_fmt("Done · {} · {}", &[&util::human_size(*size), &name]),
                        Some("emblem-ok-symbolic"),
                        Some("status-done"),
                    ),
                }
            }
            Status::Failed { reason, .. } => {
                self.set_status(reason, Some("dialog-error-symbolic"), Some("status-failed"))
            }
            Status::Cancelled => {
                self.set_status(&gettext("Cancelled"), Some("process-stop-symbolic"), None)
            }
            Status::Skipped { reason } => {
                self.set_status(reason, Some("action-unavailable-symbolic"), None)
            }
            Status::Unreadable { reason } => {
                self.set_status(reason, Some("dialog-error-symbolic"), Some("status-failed"))
            }
        }
        self.set_action_enabled("show", matches!(item.status, Status::Done { .. }));
        self.set_action_enabled("copy-command", item.command.is_some());
        self.set_action_enabled("details", item.media.is_some());
        self.set_action_enabled(
            "retry-software",
            matches!(
                item.status,
                Status::Failed {
                    retry_software: true,
                    ..
                }
            ),
        );
        self.row
            .update_property(&[gtk::accessible::Property::Label(&format!(
                "{} — {}",
                item.file_name(),
                self.status.text()
            ))]);
        let _ = glib::Propagation::Proceed;
    }
}

/// "MP4 · H.264" for the card's target pill.
pub fn target_label(item: &Item) -> String {
    let s = &item.settings;
    let fmt = if s.format == "auto-audio" {
        gettext("Audio")
    } else {
        s.format.to_ascii_uppercase().replace("-ANIM", "")
    };
    use retort_core::settings::{AudioSettings, VideoSettings};
    let detail = match (&s.video, &s.audio) {
        (VideoSettings::Encode(v), _)
            if item.media.as_ref().map(|m| m.has_video()).unwrap_or(true) =>
        {
            short_codec(&v.codec)
        }
        (VideoSettings::Copy, AudioSettings::Copy) => gettext("Copy"),
        (_, AudioSettings::Encode(a)) => short_codec(&a.codec),
        (_, AudioSettings::Copy) => gettext("Copy"),
        _ => String::new(),
    };
    if detail.is_empty() {
        fmt
    } else {
        format!("{fmt} · {detail}")
    }
}

pub fn short_codec(c: &str) -> String {
    match c {
        "h264" => "H.264".into(),
        "hevc" => "HEVC".into(),
        "pcm_s16le" | "pcm_s24le" => "PCM".into(),
        "dnxhd" => "DNxHR".into(),
        "webp" => "WebP".into(),
        other => codecs::meta(other)
            .map(|m| {
                m.title
                    .split(" (")
                    .next()
                    .unwrap_or(m.title)
                    .split(" / ")
                    .next()
                    .unwrap_or(m.title)
                    .to_string()
            })
            .unwrap_or_else(|| other.to_uppercase()),
    }
}
