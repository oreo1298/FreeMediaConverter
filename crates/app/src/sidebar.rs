// SPDX-License-Identifier: GPL-3.0-or-later
//! The output settings sidebar: Simple mode (preset cards, speed, size limit,
//! destination, plan preview) and Advanced mode (crate::advanced).

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, ntr_fmt, tr_fmt};
use crate::model::Status;
use crate::util;
use crate::window::Ui;
use retort_core::preset::{self, PresetCategory};
use retort_core::quality::{RateControl, Speed};
use retort_core::settings::OutputSettings;

pub struct Sidebar {
    pub root: adw::ToolbarView,
    pub mode_simple: gtk::ToggleButton,
    pub mode_advanced: gtk::ToggleButton,
    pub stack: gtk::Stack,
    pub scope: gtk::Label,
    /// One preset grid per category; they behave as a single selection.
    pub flowboxes: Vec<gtk::FlowBox>,
    pub preset_ids: Vec<&'static str>,
    pub speed: [gtk::ToggleButton; 3],
    pub speed_hint: gtk::Label,
    pub size_row: adw::SpinRow,
    pub folder_row: adw::ActionRow,
    pub folder_reset: gtk::Button,
    pub plan_row: adw::ActionRow,
    pub plan_icon: gtk::Image,
    pub notes: gtk::Box,
    pub remux_row: adw::ActionRow,
    /// Shown when the scope uses a container picked from "All Formats".
    pub custom_card: gtk::Box,
    pub custom_title: gtk::Label,
    pub custom_subtitle: gtk::Label,
    #[allow(dead_code)] // filled in by Advanced mode (M3)
    pub advanced: crate::advanced::Advanced,
}

/// Preset id used for a container picked in the format picker.
pub const FORMAT_PREFIX: &str = "format:";

fn preset_card(icon: &str, title: &str, subtitle: &str) -> gtk::Box {
    preset_card_parts(icon, title, subtitle).0
}

fn preset_card_parts(
    icon: &str,
    title: &str,
    subtitle: &str,
) -> (gtk::Box, gtk::Label, gtk::Label) {
    let b = gtk::Box::new(gtk::Orientation::Horizontal, 10);
    b.add_css_class("preset-card");
    b.add_css_class("card");
    let img = gtk::Image::from_icon_name(icon);
    img.set_pixel_size(20);
    img.set_valign(gtk::Align::Center);
    let labels = gtk::Box::new(gtk::Orientation::Vertical, 1);
    labels.set_valign(gtk::Align::Center);
    labels.set_hexpand(true);
    let t = gtk::Label::new(Some(title));
    t.add_css_class("preset-title");
    t.set_xalign(0.0);
    t.set_wrap(true);
    t.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    let s = gtk::Label::new(Some(subtitle));
    s.add_css_class("preset-subtitle");
    s.set_xalign(0.0);
    s.set_wrap(true);
    s.set_wrap_mode(gtk::pango::WrapMode::WordChar);
    s.set_max_width_chars(28);
    s.set_lines(2);
    s.set_ellipsize(gtk::pango::EllipsizeMode::End);
    labels.append(&t);
    labels.append(&s);
    b.append(&img);
    b.append(&labels);
    b.update_property(&[gtk::accessible::Property::Label(&format!(
        "{title}: {subtitle}"
    ))]);
    (b, t, s)
}

impl Sidebar {
    pub fn build() -> Sidebar {
        // Header with Simple / Advanced.
        let header = adw::HeaderBar::new();
        let mode_simple = gtk::ToggleButton::with_label(&gettext("Simple"));
        let mode_advanced = gtk::ToggleButton::with_label(&gettext("Advanced"));
        mode_advanced.set_group(Some(&mode_simple));
        let modes = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        modes.add_css_class("linked");
        modes.append(&mode_simple);
        modes.append(&mode_advanced);
        header.set_title_widget(Some(&modes));

        // Scope line.
        let scope = gtk::Label::new(None);
        scope.add_css_class("dim-label");
        scope.add_css_class("caption");
        scope.set_xalign(0.0);
        scope.set_margin_start(18);
        scope.set_margin_end(18);
        scope.set_margin_top(6);

        // Presets.
        let mut preset_ids = Vec::new();
        let mut cat: Option<PresetCategory> = None;
        let simple = gtk::Box::new(gtk::Orientation::Vertical, 12);
        simple.set_margin_start(12);
        simple.set_margin_end(12);
        simple.set_margin_bottom(18);
        // Any other container, via the format picker.
        let all_formats = gtk::Button::builder()
            .child(
                &adw::ButtonContent::builder()
                    .icon_name("view-grid-symbolic")
                    .label(gettext("All Formats…"))
                    .build(),
            )
            .action_name("win.choose-format")
            .tooltip_text(gettext("Choose any container this FFmpeg can write"))
            .halign(gtk::Align::Start)
            .build();
        all_formats.add_css_class("flat");
        all_formats.set_margin_top(6);
        let (custom_card, custom_title, custom_subtitle) =
            preset_card_parts("view-grid-symbolic", "", "");
        custom_card.add_css_class("selected");
        custom_card.set_margin_start(3);
        custom_card.set_margin_end(3);
        custom_card.set_visible(false);
        simple.append(&all_formats);
        simple.append(&custom_card);
        // One flowbox per category keeps headings aligned with the grid.
        let mut flowboxes: Vec<gtk::FlowBox> = Vec::new();
        for p in preset::builtin() {
            if cat != Some(p.category) {
                cat = Some(p.category);
                let h = gtk::Label::new(Some(&gettext(p.category.title())));
                h.add_css_class("section-title");
                h.set_xalign(0.0);
                simple.append(&h);
                let fb = gtk::FlowBox::builder()
                    .selection_mode(gtk::SelectionMode::Single)
                    .min_children_per_line(1)
                    .max_children_per_line(2)
                    .homogeneous(true)
                    .row_spacing(2)
                    .column_spacing(2)
                    .activate_on_single_click(true)
                    .build();
                fb.add_css_class("presets");
                simple.append(&fb);
                flowboxes.push(fb);
            }
            let card = preset_card(p.icon, &gettext(p.title), &gettext(p.subtitle));
            flowboxes.last().expect("flowbox").append(&card);
            preset_ids.push(p.id);
        }

        // Speed.
        let speed_box = gtk::Box::new(gtk::Orientation::Horizontal, 0);
        speed_box.add_css_class("linked");
        speed_box.set_homogeneous(true);
        let mk = |icon: &str, label: &str| {
            let content = adw::ButtonContent::builder()
                .icon_name(icon)
                .label(label)
                .build();
            let b = gtk::ToggleButton::new();
            b.set_child(Some(&content));
            b
        };
        let fastest = mk("power-profile-performance-symbolic", &gettext("Fastest"));
        let balanced = mk("power-profile-balanced-symbolic", &gettext("Balanced"));
        let smallest = mk("power-profile-power-saver-symbolic", &gettext("Smallest"));
        balanced.set_group(Some(&fastest));
        smallest.set_group(Some(&fastest));
        speed_box.append(&fastest);
        speed_box.append(&balanced);
        speed_box.append(&smallest);
        let speed_hint = gtk::Label::new(None);
        speed_hint.add_css_class("dim-label");
        speed_hint.add_css_class("caption");
        speed_hint.set_xalign(0.0);
        speed_hint.set_wrap(true);
        let speed_title = gtk::Label::new(Some(&gettext("Speed")));
        speed_title.add_css_class("section-title");
        speed_title.set_xalign(0.0);
        let speed_col = gtk::Box::new(gtk::Orientation::Vertical, 6);
        speed_col.append(&speed_title);
        speed_col.append(&speed_box);
        speed_col.append(&speed_hint);

        // Size, destination.
        let size_row = adw::SpinRow::with_range(1.0, 100_000.0, 1.0);
        size_row.set_title(&gettext("Maximum Size"));
        size_row.set_subtitle(&gettext("Megabytes (Discord's free limit is 10 MB)"));
        size_row.set_digits(0);
        let folder_row = adw::ActionRow::builder().title(gettext("Save To")).build();
        folder_row.add_prefix(&gtk::Image::from_icon_name("folder-symbolic"));
        let folder_reset =
            util::icon_button("edit-undo-symbolic", &gettext("Save Next to the Original"));
        folder_reset.add_css_class("flat");
        folder_reset.set_valign(gtk::Align::Center);
        let folder_choose = gtk::Button::with_label(&gettext("Choose…"));
        folder_choose.set_valign(gtk::Align::Center);
        folder_choose.set_action_name(Some("win.choose-output-folder"));
        folder_row.add_suffix(&folder_reset);
        folder_row.add_suffix(&folder_choose);
        let out_group = adw::PreferencesGroup::new();
        out_group.add(&size_row);
        out_group.add(&folder_row);

        // Plan preview.
        let plan_group = adw::PreferencesGroup::new();
        let plan_row = adw::ActionRow::builder()
            .title(gettext("Add files to see the plan"))
            .subtitle_lines(3)
            .build();
        let plan_icon = gtk::Image::from_icon_name("system-run-symbolic");
        plan_row.add_prefix(&plan_icon);
        plan_group.add(&plan_row);
        let remux_row = adw::ActionRow::builder()
            .title(gettext("Instant, lossless"))
            .subtitle(gettext(
                "This file can change container without re-encoding",
            ))
            .visible(false)
            .build();
        remux_row.add_prefix(&gtk::Image::from_icon_name(
            "media-playlist-repeat-symbolic",
        ));
        let remux_btn = gtk::Button::with_label(&gettext("Remux"));
        remux_btn.set_valign(gtk::Align::Center);
        remux_btn.set_action_name(Some("win.use-remux"));
        remux_row.add_suffix(&remux_btn);
        plan_group.add(&remux_row);
        let notes = gtk::Box::new(gtk::Orientation::Vertical, 6);
        notes.set_margin_top(6);

        // Speed, destination and the plan stay visible below the preset list.
        let footer = gtk::Box::new(gtk::Orientation::Vertical, 12);
        footer.add_css_class("sidebar-footer");
        footer.append(&speed_col);
        footer.append(&out_group);
        footer.append(&plan_group);
        footer.append(&notes);

        let advanced = crate::advanced::Advanced::build();
        let stack = gtk::Stack::new();
        stack.set_transition_type(gtk::StackTransitionType::Crossfade);
        let simple_scroll = gtk::ScrolledWindow::builder()
            .child(&simple)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        let simple_page = gtk::Box::new(gtk::Orientation::Vertical, 0);
        simple_page.append(&simple_scroll);
        simple_page.append(&footer);
        stack.add_named(&simple_page, Some("simple"));
        stack.add_named(&advanced.root, Some("advanced"));

        let body = gtk::Box::new(gtk::Orientation::Vertical, 6);
        body.append(&scope);
        body.append(&stack);
        let root = adw::ToolbarView::new();
        root.add_top_bar(&header);
        root.set_content(Some(&body));
        root.add_css_class("sidebar-pane");
        Sidebar {
            root,
            mode_simple,
            mode_advanced,
            stack,
            scope,
            flowboxes,
            preset_ids,
            speed: [fastest, balanced, smallest],
            speed_hint,
            size_row,
            folder_row,
            folder_reset,
            plan_row,
            plan_icon,
            notes,
            remux_row,
            custom_card,
            custom_title,
            custom_subtitle,
            advanced,
        }
    }

    fn flowboxes(&self) -> Vec<gtk::FlowBox> {
        self.flowboxes.clone()
    }

    /// (flowbox, child index) of a preset id.
    fn locate(&self, id: &str) -> Option<(gtk::FlowBox, i32)> {
        let idx = self.preset_ids.iter().position(|p| *p == id)?;
        let mut n = idx as i32;
        for fb in self.flowboxes() {
            let mut count = 0;
            while fb.child_at_index(count).is_some() {
                count += 1;
            }
            if n < count {
                return Some((fb, n));
            }
            n -= count;
        }
        None
    }

    fn id_of(&self, fb: &gtk::FlowBox, child: &gtk::FlowBoxChild) -> Option<&'static str> {
        let mut offset = 0;
        for f in self.flowboxes() {
            if &f == fb {
                return self
                    .preset_ids
                    .get((offset + child.index()) as usize)
                    .copied();
            }
            let mut count = 0;
            while f.child_at_index(count).is_some() {
                count += 1;
            }
            offset += count;
        }
        None
    }
}

/// Apply the speed preference and (for the size-limit preset) the size.
pub fn apply_speed_and_size(
    mut s: OutputSettings,
    preset_id: &str,
    speed: &str,
    mb: f64,
) -> OutputSettings {
    s.speed = match speed {
        "fastest" => Speed::Fastest,
        "smallest" => Speed::Smallest,
        _ => Speed::Balanced,
    };
    if preset_id == "target-size" {
        s = preset::with_target_size(s, mb);
    }
    s
}

fn speed_key(s: Speed) -> &'static str {
    match s {
        Speed::Fastest => "fastest",
        Speed::Balanced => "balanced",
        Speed::Smallest => "smallest",
    }
}

/// Settings of the "focused" scope: first selected item, else the defaults.
fn scope_settings(ui: &Ui) -> (OutputSettings, String) {
    let sel = ui.selected_ids();
    let st = ui.state.borrow();
    if let Some(item) = sel
        .first()
        .and_then(|id| st.items.iter().find(|i| i.id == *id))
    {
        return (item.settings.clone(), item.preset.clone());
    }
    (st.defaults.clone(), st.preset.clone())
}

/// Apply a change to the selected items, or to the defaults and every
/// not-yet-finished item when nothing is selected.
pub fn apply_to_scope(ui: &Ui, f: impl Fn(&mut OutputSettings, &mut String)) {
    let sel = ui.selected_ids();
    let changed: Vec<u64> = {
        let mut st = ui.state.borrow_mut();
        let mut changed = Vec::new();
        if sel.is_empty() {
            let mut d = st.defaults.clone();
            let mut p = st.preset.clone();
            f(&mut d, &mut p);
            st.defaults = d;
            st.preset = p;
        }
        for item in st.items.iter_mut() {
            let target = if sel.is_empty() {
                matches!(
                    item.status,
                    Status::Ready
                        | Status::Analyzing
                        | Status::Failed { .. }
                        | Status::Cancelled
                        | Status::Skipped { .. }
                )
            } else {
                sel.contains(&item.id) && !item.status.is_active()
            };
            if target {
                f(&mut item.settings, &mut item.preset);
                if matches!(item.status, Status::Failed { .. } | Status::Cancelled) {
                    item.status = Status::Ready;
                }
                item.start_plan = None;
                changed.push(item.id);
            }
        }
        changed
    };
    if sel.is_empty() {
        let st = ui.state.borrow();
        if !ui.demo {
            let _ = ui.settings.set_string("preset", &st.preset);
            let _ = ui
                .settings
                .set_string("speed", speed_key(st.defaults.speed));
        }
    }
    for id in changed {
        ui.update_card(id);
        crate::window::replan(ui, id);
    }
    crate::window::refresh(ui);
    sync_from_state(ui);
    ui.save_queue();
}

pub fn wire(ui: &Ui) {
    let sb = &ui.sidebar;
    // Mode.
    let mode = ui.settings.string("mode");
    if mode == "advanced" {
        sb.mode_advanced.set_active(true);
        sb.stack.set_visible_child_name("advanced");
    } else {
        sb.mode_simple.set_active(true);
    }
    let u = ui.clone();
    sb.mode_simple.connect_toggled(move |b| {
        if b.is_active() {
            u.sidebar.stack.set_visible_child_name("simple");
            if !u.demo {
                let _ = u.settings.set_string("mode", "simple");
            }
        }
    });
    let u = ui.clone();
    sb.mode_advanced.connect_toggled(move |b| {
        if b.is_active() {
            u.sidebar.stack.set_visible_child_name("advanced");
            if !u.demo {
                let _ = u.settings.set_string("mode", "advanced");
            }
            crate::advanced::sync(&u);
        }
    });

    // Presets (several flowboxes behave as one selection).
    for fb in sb.flowboxes() {
        let u = ui.clone();
        fb.connect_child_activated(move |fb, child| {
            if u.syncing.get() {
                return;
            }
            let Some(id) = u.sidebar.id_of(fb, child) else {
                return;
            };
            choose_preset(&u, id);
        });
    }

    // Any container via the format picker.
    let a = gio::SimpleAction::new("choose-format", None);
    let u = ui.clone();
    a.connect_activate(move |_, _| {
        let u2 = u.clone();
        crate::picker::show(&u, true, move |choice| match choice {
            crate::picker::Choice::Preset(id) => choose_preset(&u2, id),
            crate::picker::Choice::Format(id) => choose_format(&u2, id),
        });
    });
    ui.window.add_action(&a);

    // Speed.
    for (i, b) in sb.speed.iter().enumerate() {
        let u = ui.clone();
        b.connect_toggled(move |b| {
            if !b.is_active() || u.syncing.get() {
                return;
            }
            let sp = [Speed::Fastest, Speed::Balanced, Speed::Smallest][i];
            apply_to_scope(&u, move |s, _| s.speed = sp);
        });
    }

    // Size.
    let u = ui.clone();
    sb.size_row.connect_value_notify(move |r| {
        if u.syncing.get() {
            return;
        }
        let mb = r.value();
        if !u.demo {
            let _ = u.settings.set_double("target-size-mb", mb);
        }
        apply_to_scope(&u, move |s, p| {
            if p == "target-size" {
                *s = preset::with_target_size(s.clone(), mb);
            }
        });
    });

    // Destination folder.
    let a = gio::SimpleAction::new("choose-output-folder", None);
    let u = ui.clone();
    a.connect_activate(move |_, _| {
        let dialog = gtk::FileDialog::builder()
            .title(gettext("Save Converted Files To"))
            .modal(true)
            .build();
        let u2 = u.clone();
        dialog.select_folder(Some(&u.window), gio::Cancellable::NONE, move |res| {
            if let Ok(f) = res {
                if let Some(p) = f.path() {
                    let _ = u2
                        .settings
                        .set_string("output-folder", &p.to_string_lossy());
                    sync_from_state(&u2);
                }
            }
        });
    });
    ui.window.add_action(&a);
    let u = ui.clone();
    sb.folder_reset.connect_clicked(move |_| {
        let _ = u.settings.set_string("output-folder", "");
        sync_from_state(&u);
    });

    // Remux suggestion.
    let a = gio::SimpleAction::new("use-remux", None);
    let u = ui.clone();
    a.connect_activate(move |_, _| {
        let (cur, _) = scope_settings(&u);
        apply_to_scope(&u, move |s, p| {
            if let Some(pr) = preset::find("remux") {
                let mut n = pr.settings;
                n.format = cur.format.clone();
                *s = n;
                *p = "remux".into();
            }
        });
    });
    ui.window.add_action(&a);
    crate::advanced::wire(ui);
}

/// Apply a built-in preset to the scope (keeps the speed choice).
pub fn choose_preset(ui: &Ui, id: &'static str) {
    let speed = speed_key(scope_settings(ui).0.speed).to_string();
    let mb = ui.sidebar.size_row.value();
    apply_to_scope(ui, move |s, p| {
        if let Some(pr) = preset::find(id) {
            *s = apply_speed_and_size(pr.settings, id, &speed, mb);
            *p = id.to_string();
        }
    });
    sync_from_state(ui);
}

/// Apply a container's default settings to the scope.
pub fn choose_format(ui: &Ui, id: String) {
    let Some(engine) = ui.engine() else { return };
    let speed = scope_settings(ui).0.speed;
    let u = ui.clone();
    crate::util::spawn_worker(
        move || engine.settings_for_format(&id).map(|s| (id, s)),
        move |res| {
            let Some((id, settings)) = res else { return };
            // A container that has a preset selects that preset instead.
            if let Some(p) = preset::builtin()
                .into_iter()
                .find(|p| p.settings == settings)
            {
                choose_preset(&u, p.id);
                return;
            }
            apply_to_scope(&u, move |s, p| {
                *s = settings.clone();
                s.speed = speed;
                *p = format!("{FORMAT_PREFIX}{id}");
            });
            sync_from_state(&u);
        },
    );
}

/// "FFV1 + FLAC" style summary of the codecs in `s`.
fn codec_summary(s: &OutputSettings) -> String {
    use retort_core::settings::{AudioSettings, VideoSettings};
    let mut parts = Vec::new();
    match &s.video {
        VideoSettings::Encode(v) => parts.push(crate::card::short_codec(&v.codec)),
        VideoSettings::Copy => parts.push(gettext("Video copied")),
        VideoSettings::Drop => {}
    }
    match &s.audio {
        AudioSettings::Encode(a) => parts.push(crate::card::short_codec(&a.codec)),
        AudioSettings::Copy => parts.push(gettext("Audio copied")),
        AudioSettings::Drop => {}
    }
    parts.join(" + ")
}

/// Reflect the scope's settings in the widgets.
pub fn sync_from_state(ui: &Ui) {
    let sb = &ui.sidebar;
    ui.syncing.set(true);
    let sel = ui.selected_ids();
    let n_items = ui.state.borrow().items.len();
    sb.scope.set_text(&if !sel.is_empty() {
        ntr_fmt(
            "Settings for {} selected file",
            "Settings for {} selected files",
            sel.len() as u32,
            &[&sel.len().to_string()],
        )
    } else if n_items > 0 {
        gettext("Settings for all files")
    } else {
        gettext("Settings for new files")
    });
    let (settings, preset_id) = scope_settings(ui);
    for fb in sb.flowboxes() {
        fb.unselect_all();
    }
    if let Some((fb, idx)) = sb.locate(&preset_id) {
        if let Some(child) = fb.child_at_index(idx) {
            fb.select_child(&child);
        }
    }
    match preset_id.strip_prefix(FORMAT_PREFIX) {
        Some(fid) => {
            let title = ui
                .state
                .borrow()
                .engine
                .as_ref()
                .and_then(|e| e.format(fid).map(|f| f.title.clone()))
                .unwrap_or_else(|| fid.to_uppercase());
            sb.custom_title.set_text(&title);
            sb.custom_subtitle.set_text(&codec_summary(&settings));
            sb.custom_card.set_visible(true);
        }
        None => sb.custom_card.set_visible(false),
    }
    let speed_idx = match settings.speed {
        Speed::Fastest => 0,
        Speed::Balanced => 1,
        Speed::Smallest => 2,
    };
    sb.speed[speed_idx].set_active(true);
    sb.speed_hint.set_text(&match settings.speed {
        Speed::Fastest => {
            gettext("Uses your GPU whenever a test proved it works; files are a bit larger.")
        }
        Speed::Balanced => gettext("Good quality at good speed; uses the GPU when available."),
        Speed::Smallest => {
            gettext("Slow CPU encoding with efficient settings for the smallest files.")
        }
    });
    let is_size = preset_id == "target-size";
    sb.size_row.set_visible(is_size);
    if let Some(v) = settings.video_encode() {
        if let RateControl::TargetSize { bytes } = v.rate {
            sb.size_row.set_value((bytes as f64 / 1e6).round());
        }
    }
    let folder = ui.settings.string("output-folder");
    sb.folder_row.set_subtitle(&if folder.is_empty() {
        gettext("Next to the original files")
    } else {
        glib::markup_escape_text(&folder).to_string()
    });
    sb.folder_reset.set_visible(!folder.is_empty());
    ui.syncing.set(false);
    update_plan_preview(ui);
    if sb.stack.visible_child_name().as_deref() == Some("advanced") {
        crate::advanced::sync(ui);
    }
}

/// Show the plan for the focused item (first selected, else first ready).
pub fn update_plan_preview(ui: &Ui) {
    let sb = &ui.sidebar;
    util::clear_box(&sb.notes);
    let sel = ui.selected_ids();
    let st = ui.state.borrow();
    let focused = sel
        .first()
        .and_then(|id| st.items.iter().find(|i| i.id == *id))
        .or_else(|| {
            st.items
                .iter()
                .find(|i| i.media.is_some() && !i.status.is_finished())
        })
        .or_else(|| st.items.iter().find(|i| i.media.is_some()));
    let Some(item) = focused else {
        sb.plan_row.set_title(&if st.engine_error.is_some() {
            gettext("FFmpeg is missing — open System Check")
        } else {
            gettext("Add files to see the plan")
        });
        sb.plan_row.set_subtitle("");
        sb.plan_icon.set_icon_name(Some("system-run-symbolic"));
        sb.remux_row.set_visible(false);
        return;
    };
    match (&item.badge, &item.summary) {
        (Some(b), Some(s)) => {
            sb.plan_row.set_title(&glib::markup_escape_text(b));
            sb.plan_row.set_subtitle(&glib::markup_escape_text(s));
            sb.plan_icon.set_icon_name(Some(if item.hardware {
                "thunderbolt-symbolic"
            } else {
                "computer-symbolic"
            }));
        }
        _ => {
            sb.plan_row
                .set_title(&glib::markup_escape_text(&item.file_name()));
            sb.plan_row.set_subtitle(&if item.notes.is_empty() {
                gettext("Planning…")
            } else {
                String::new()
            });
            sb.plan_icon.set_icon_name(Some("dialog-warning-symbolic"));
        }
    }
    for n in &item.notes {
        let row = gtk::Box::new(gtk::Orientation::Horizontal, 8);
        let img = gtk::Image::from_icon_name("dialog-information-symbolic");
        img.set_valign(gtk::Align::Start);
        img.add_css_class("dim-label");
        let l = gtk::Label::new(Some(n));
        l.set_wrap(true);
        l.set_xalign(0.0);
        l.add_css_class("caption");
        row.append(&img);
        row.append(&l);
        row.set_margin_start(6);
        sb.notes.append(&row);
    }
    let remux_possible = item.preset != "remux"
        && item.media.as_ref().is_some_and(|m| {
            st.engine
                .as_ref()
                .is_some_and(|e| e.can_remux(m, &item.settings.format))
        })
        && item.settings.format != "auto-audio";
    sb.remux_row.set_visible(remux_possible);
    if remux_possible {
        sb.remux_row.set_subtitle(&tr_fmt(
            "Every stream fits into {} as-is — convert in seconds without quality loss",
            &[&item.settings.format.to_uppercase()],
        ));
    }
}
