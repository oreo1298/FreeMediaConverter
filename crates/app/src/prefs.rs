// SPDX-License-Identifier: GPL-3.0-or-later
//! Preferences: output, queue, hardware (with the verified capability matrix)
//! and the FFmpeg binary.

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, tr_fmt};
use crate::util;
use crate::window::{HwStatus, Ui};
use retort_core::hw::{Backend, Inventory};

const CODECS: &[(&str, &str)] = &[
    ("h264", "H.264"),
    ("hevc", "HEVC"),
    ("av1", "AV1"),
    ("vp9", "VP9"),
    ("vp8", "VP8"),
    ("mpeg2video", "MPEG-2"),
    ("mjpeg", "JPEG"),
];

fn string_combo(title: &str, labels: &[String]) -> adw::ComboRow {
    let refs: Vec<&str> = labels.iter().map(String::as_str).collect();
    adw::ComboRow::builder()
        .title(title)
        .model(&gtk::StringList::new(&refs))
        .build()
}

/// Bind a string setting to a combo row via parallel `keys`.
fn bind_combo(row: &adw::ComboRow, settings: &gio::Settings, key: &'static str, keys: Vec<String>) {
    let cur = settings.string(key).to_string();
    row.set_selected(keys.iter().position(|k| *k == cur).unwrap_or(0) as u32);
    let s = settings.clone();
    row.connect_selected_notify(move |r| {
        if let Some(k) = keys.get(r.selected() as usize) {
            let _ = s.set_string(key, k);
        }
    });
}

pub fn show(ui: &Ui) -> adw::PreferencesDialog {
    let dialog = adw::PreferencesDialog::new();
    dialog.set_search_enabled(true);
    let s = &ui.settings;

    // ---- General.
    let general = adw::PreferencesPage::builder()
        .title(gettext("General"))
        .icon_name("preferences-other-symbolic")
        .build();
    let out = adw::PreferencesGroup::builder()
        .title(gettext("Output"))
        .build();
    let template = adw::EntryRow::builder()
        .title(gettext("File Name Template"))
        .text(s.string("filename-template"))
        .show_apply_button(true)
        .build();
    let help = gtk::MenuButton::builder()
        .icon_name("help-about-symbolic")
        .valign(gtk::Align::Center)
        .tooltip_text(gettext("Placeholders"))
        .build();
    help.add_css_class("flat");
    let pop_label = gtk::Label::new(Some(&gettext(
        "{name} — original file name\n{ext} — new extension\n{codec} — video (or audio) codec\n{res} — resolution, e.g. 1080p\n{date} — today, e.g. 2026-11-12\n{preset} — preset id",
    )));
    pop_label.set_margin_top(8);
    pop_label.set_margin_bottom(8);
    pop_label.set_margin_start(8);
    pop_label.set_margin_end(8);
    help.set_popover(Some(&gtk::Popover::builder().child(&pop_label).build()));
    template.add_suffix(&help);
    let st = s.clone();
    template.connect_apply(move |e| {
        let t = e.text();
        let t = if t.trim().is_empty() {
            "{name}.{ext}".into()
        } else {
            t.to_string()
        };
        let _ = st.set_string("filename-template", &t);
    });
    out.add(&template);
    let conflict = string_combo(
        &gettext("If the File Exists"),
        &[
            gettext("Keep both (add a number)"),
            gettext("Skip"),
            gettext("Ask"),
            gettext("Replace"),
        ],
    );
    bind_combo(
        &conflict,
        s,
        "conflict-policy",
        vec![
            "rename".into(),
            "skip".into(),
            "ask".into(),
            "overwrite".into(),
        ],
    );
    out.add(&conflict);
    let ts = adw::SwitchRow::builder()
        .title(gettext("Keep Original Dates"))
        .subtitle(gettext(
            "Copy the modification time of each source to its output",
        ))
        .build();
    s.bind("preserve-timestamps", &ts, "active").build();
    out.add(&ts);
    let partial = adw::SwitchRow::builder()
        .title(gettext("Keep Partial Files"))
        .subtitle(gettext(
            "Keep what was written when a conversion is cancelled or fails",
        ))
        .build();
    s.bind("keep-partial", &partial, "active").build();
    out.add(&partial);
    general.add(&out);

    let queue = adw::PreferencesGroup::builder()
        .title(gettext("Queue"))
        .build();
    let jobs = adw::SpinRow::with_range(0.0, 16.0, 1.0);
    jobs.set_title(&gettext("Parallel Conversions"));
    jobs.set_subtitle(&gettext("0 = automatic: one per GPU plus one on the CPU"));
    s.bind("max-jobs", &jobs, "value").build();
    queue.add(&jobs);
    let notify = adw::SwitchRow::builder()
        .title(gettext("Notify When Finished"))
        .build();
    s.bind("notify-on-complete", &notify, "active").build();
    queue.add(&notify);
    let exts = adw::EntryRow::builder()
        .title(gettext("File Types Added From Folders"))
        .text(s.string("import-extensions"))
        .show_apply_button(true)
        .build();
    let st = s.clone();
    exts.connect_apply(move |e| {
        let _ = st.set_string("import-extensions", &e.text());
    });
    queue.add(&exts);
    general.add(&queue);
    dialog.add(&general);

    // ---- Hardware.
    let hw = adw::PreferencesPage::builder()
        .title(gettext("Hardware"))
        .icon_name("thunderbolt-symbolic")
        .build();
    let hg = adw::PreferencesGroup::builder()
        .title(gettext("Hardware Acceleration"))
        .description(gettext("Only encoders and decoders that passed a real test on this machine are used. Anything else falls back to the CPU automatically."))
        .build();
    let use_hw = adw::SwitchRow::builder()
        .title(gettext("Use Hardware Acceleration"))
        .build();
    s.bind("use-hardware", &use_hw, "active").build();
    hg.add(&use_hw);
    let backends = [
        Backend::Nvenc,
        Backend::Qsv,
        Backend::Vaapi,
        Backend::Vulkan,
        Backend::Amf,
        Backend::V4l2m2m,
    ];
    let mut labels = vec![gettext("Automatic")];
    labels.extend(backends.iter().map(|b| b.label().to_string()));
    let pref = string_combo(&gettext("Preferred Backend"), &labels);
    let mut keys = vec![String::new()];
    keys.extend(backends.iter().map(|b| b.key().to_string()));
    bind_combo(&pref, s, "preferred-backend", keys);
    hg.add(&pref);
    let (dev_labels, dev_keys) = {
        let st = ui.state.borrow();
        let mut l = vec![gettext("Automatic")];
        let mut k = vec![String::new()];
        if let Some(e) = &st.engine {
            for g in &e.inventory.scan.gpus {
                l.push(g.name.clone());
                k.push(g.id.clone());
            }
        }
        (l, k)
    };
    let dev = string_combo(&gettext("Use Only This GPU"), &dev_labels);
    bind_combo(&dev, s, "preferred-device", dev_keys);
    hg.add(&dev);
    // (AdwButtonRow needs libadwaita 1.6; an activatable row works on 1.5.)
    let retest = adw::ActionRow::builder()
        .title(gettext("Re-test Hardware"))
        .subtitle(gettext(
            "Run all tests again, e.g. after installing a driver",
        ))
        .activatable(true)
        .build();
    retest.add_suffix(&gtk::Image::from_icon_name("view-refresh-symbolic"));
    let u = ui.clone();
    retest.connect_activated(move |_| crate::window::probe_hardware(&u, true));
    hg.add(&retest);
    hw.add(&hg);
    let matrix_box = gtk::Box::new(gtk::Orientation::Vertical, 18);
    let mg = adw::PreferencesGroup::new();
    mg.add(&matrix_box);
    hw.add(&mg);
    *ui.prefs_hw.borrow_mut() = Some(matrix_box.clone());
    fill_matrix(ui, &matrix_box);
    dialog.add(&hw);

    // ---- FFmpeg.
    let ffp = adw::PreferencesPage::builder()
        .title(gettext("FFmpeg"))
        .icon_name("applications-multimedia-symbolic")
        .build();
    let fg = adw::PreferencesGroup::builder()
        .title(gettext("FFmpeg"))
        .description(gettext("Leave empty to use the bundled or system FFmpeg. You can point to jellyfin-ffmpeg, a static build or your own build; ffprobe must be next to it."))
        .build();
    let path = adw::EntryRow::builder()
        .title(gettext("ffmpeg Binary"))
        .text(s.string("ffmpeg-path"))
        .show_apply_button(true)
        .build();
    let browse = util::icon_button("document-open-symbolic", &gettext("Choose ffmpeg…"));
    browse.add_css_class("flat");
    browse.set_valign(gtk::Align::Center);
    path.add_suffix(&browse);
    let current = adw::ActionRow::builder()
        .title(gettext("In Use"))
        .subtitle_selectable(true)
        .build();
    {
        let st = ui.state.borrow();
        current.set_subtitle(&match (&st.engine, &st.engine_error) {
            (Some(e), _) => glib::markup_escape_text(&format!(
                "{} — {}",
                e.ff.version.short(),
                e.ff.bins.ffmpeg.display()
            ))
            .to_string(),
            (None, Some(err)) => glib::markup_escape_text(err).to_string(),
            _ => gettext("Starting…"),
        });
    }
    let u = ui.clone();
    path.connect_apply(move |e| {
        let _ = u.settings.set_string("ffmpeg-path", e.text().trim());
        u.state.borrow_mut().engine = None;
        crate::window::start_engine(&u);
        u.toast(&gettext("Switching FFmpeg…"));
    });
    let p2 = path.clone();
    let u = ui.clone();
    browse.connect_clicked(move |_| {
        let d = gtk::FileDialog::builder()
            .title(gettext("Choose ffmpeg"))
            .modal(true)
            .build();
        let p3 = p2.clone();
        d.open(Some(&u.window), gio::Cancellable::NONE, move |r| {
            if let Ok(f) = r {
                if let Some(pp) = f.path() {
                    p3.set_text(&pp.to_string_lossy());
                    p3.emit_by_name::<()>("apply", &[]);
                }
            }
        });
    });
    fg.add(&path);
    fg.add(&current);
    ffp.add(&fg);
    if cfg!(feature = "update-check") {
        let ug = adw::PreferencesGroup::builder()
            .title(gettext("Updates"))
            .build();
        let upd = adw::SwitchRow::builder()
            .title(gettext("Check for Updates"))
            .subtitle(gettext(
                "Ask GitHub once a day whether a new version is available. Nothing else is sent.",
            ))
            .build();
        s.bind("check-updates", &upd, "active").build();
        ug.add(&upd);
        ffp.add(&ug);
    }
    dialog.add(&ffp);
    let u = ui.clone();
    dialog.connect_closed(move |_| {
        *u.prefs_hw.borrow_mut() = None;
        crate::window::replan_all(&u);
    });
    hw.set_name(Some("hardware"));
    dialog.present(Some(&ui.window));
    dialog
}

/// Rebuild the matrix if the preferences dialog is open.
pub fn refresh_hardware(ui: &Ui) {
    let b = ui.prefs_hw.borrow().clone();
    if let Some(b) = b {
        fill_matrix(ui, &b);
    }
}

fn cell(text: &str, class: &str, tooltip: &str) -> gtk::Label {
    let l = gtk::Label::new(Some(text));
    l.add_css_class(class);
    l.add_css_class("caption");
    l.set_xalign(0.5);
    l.set_hexpand(true);
    l.set_wrap(true);
    if !tooltip.is_empty() {
        l.set_tooltip_text(Some(tooltip));
    }
    l
}

fn fill_matrix(ui: &Ui, container: &gtk::Box) {
    util::clear_box(container);
    let st = ui.state.borrow();
    if let HwStatus::Probing { done, total } = st.hw {
        let row = adw::ActionRow::builder()
            .title(gettext("Testing hardware…"))
            .subtitle(tr_fmt(
                "{} of {} tests done",
                &[&done.to_string(), &total.to_string()],
            ))
            .build();
        row.add_suffix(&util::spinner());
        let lb = gtk::ListBox::new();
        lb.add_css_class("boxed-list");
        lb.append(&row);
        container.append(&lb);
        return;
    }
    let Some(engine) = &st.engine else { return };
    let inv: &Inventory = &engine.inventory;
    let gpus = &inv.scan.gpus;
    if gpus.is_empty() && inv.probes.encoders.is_empty() {
        let page = adw::StatusPage::builder()
            .icon_name("computer-symbolic")
            .title(gettext("No GPU Found"))
            .description(gettext("Everything is converted on the CPU."))
            .build();
        page.add_css_class("compact");
        container.append(&page);
        return;
    }
    // Devices: GPUs plus non-GPU encoders (V4L2…).
    let mut devices: Vec<(Option<String>, String, String)> = gpus
        .iter()
        .map(|g| {
            let sub = format!(
                "{}{}",
                g.driver.clone().unwrap_or_default(),
                g.render_node
                    .as_ref()
                    .map(|n| format!(" · {}", n.display()))
                    .unwrap_or_default()
            );
            (Some(g.id.clone()), g.name.clone(), sub)
        })
        .collect();
    for e in &inv.probes.encoders {
        if e.device.gpu_id.is_none()
            && !devices
                .iter()
                .any(|(id, l, _)| id.is_none() && *l == e.device.label)
        {
            devices.push((None, e.device.label.clone(), e.backend.label().to_string()));
        }
    }
    for (gpu_id, label, sub) in devices {
        let on_dev = |d: &retort_core::hw::DeviceRef| match &gpu_id {
            Some(g) => d.gpu_id.as_deref() == Some(g.as_str()),
            None => d.gpu_id.is_none() && d.label == label,
        };
        let title = gtk::Label::new(Some(&label));
        title.add_css_class("heading");
        title.set_xalign(0.0);
        let subtitle = gtk::Label::new(Some(&sub));
        subtitle.add_css_class("dim-label");
        subtitle.add_css_class("caption");
        subtitle.set_xalign(0.0);
        let grid = gtk::Grid::builder()
            .column_spacing(12)
            .row_spacing(8)
            .column_homogeneous(false)
            .build();
        grid.add_css_class("matrix");
        let heads = [
            gettext("Codec"),
            gettext("Encode 8-bit"),
            gettext("Encode 10-bit"),
            gettext("Decode 8-bit"),
            gettext("Decode 10-bit"),
        ];
        for (c, h) in heads.iter().enumerate() {
            let l = gtk::Label::new(Some(h));
            l.add_css_class("matrix-head");
            l.set_xalign(if c == 0 { 0.0 } else { 0.5 });
            l.set_hexpand(c > 0);
            l.set_wrap(true);
            grid.attach(&l, c as i32, 0, 1, 1);
        }
        let mut any_row = false;
        for (r, (codec, name)) in CODECS.iter().enumerate() {
            let enc: Vec<_> = inv
                .probes
                .encoders
                .iter()
                .filter(|e| e.codec == *codec && on_dev(&e.device))
                .collect();
            let dec: Vec<_> = inv
                .probes
                .decoders
                .iter()
                .filter(|d| d.codec == *codec && on_dev(&d.device))
                .collect();
            if enc.is_empty() && dec.is_empty() {
                continue;
            }
            any_row = true;
            let row = (r + 1) as i32;
            let n = gtk::Label::new(Some(name));
            n.set_xalign(0.0);
            n.add_css_class("heading");
            grid.attach(&n, 0, row, 1, 1);
            for (col, ten) in [(1, false), (2, true)] {
                let ok: Vec<&str> = enc
                    .iter()
                    .filter(|e| e.works(ten))
                    .map(|e| e.backend.label())
                    .collect();
                let fail: Vec<String> = enc
                    .iter()
                    .filter(|e| !e.works(ten))
                    .filter_map(|e| {
                        let o = if ten { &e.ten_bit } else { &e.eight_bit };
                        o.reason().map(|r| format!("{}: {r}", e.backend.label()))
                    })
                    .collect();
                let w = if !ok.is_empty() {
                    cell(&dedup(ok).join(" · "), "matrix-ok", &fail.join("\n"))
                } else if !enc.is_empty() {
                    cell("✕", "matrix-fail", &fail.join("\n"))
                } else {
                    cell("—", "matrix-no", "")
                };
                grid.attach(&w, col, row, 1, 1);
            }
            for (col, depth) in [(3, 8u8), (4, 10u8)] {
                let ds: Vec<_> = dec.iter().filter(|d| d.bit_depth == depth).collect();
                let ok: Vec<&str> = ds
                    .iter()
                    .filter(|d| d.outcome.passed())
                    .map(|d| d.backend.decode_label())
                    .collect();
                let w = if !ok.is_empty() {
                    cell(&dedup(ok).join(" · "), "matrix-ok", "")
                } else if !ds.is_empty() {
                    cell(
                        "✕",
                        "matrix-fail",
                        &ds.iter()
                            .filter_map(|d| {
                                d.outcome
                                    .reason()
                                    .map(|r| format!("{}: {r}", d.backend.decode_label()))
                            })
                            .collect::<Vec<_>>()
                            .join("\n"),
                    )
                } else {
                    cell("—", "matrix-no", "")
                };
                grid.attach(&w, col, row, 1, 1);
            }
        }
        if !any_row {
            let l = gtk::Label::new(Some(&gettext(
                "No hardware encoders or decoders were found for this device",
            )));
            l.add_css_class("dim-label");
            l.set_xalign(0.0);
            grid.attach(&l, 0, 1, 5, 1);
        }
        let frame = gtk::Box::new(gtk::Orientation::Vertical, 4);
        frame.add_css_class("card");
        let inner = gtk::Box::new(gtk::Orientation::Vertical, 2);
        inner.set_margin_top(12);
        inner.set_margin_start(12);
        inner.set_margin_end(12);
        inner.append(&title);
        inner.append(&subtitle);
        frame.append(&inner);
        let sc = gtk::ScrolledWindow::builder()
            .child(&grid)
            .vscrollbar_policy(gtk::PolicyType::Never)
            .propagate_natural_height(true)
            .build();
        frame.append(&sc);
        container.append(&frame);
    }
    let when = gtk::Label::new(Some(&gettext("Hover over a ✕ to see why a test failed. Results are cached and re-tested automatically when FFmpeg, the kernel or drivers change.")));
    when.add_css_class("dim-label");
    when.add_css_class("caption");
    when.set_wrap(true);
    when.set_xalign(0.0);
    container.append(&when);
}

fn dedup(v: Vec<&str>) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for x in v {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}
