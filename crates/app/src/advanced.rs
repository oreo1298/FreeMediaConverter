// SPDX-License-Identifier: GPL-3.0-or-later
//! Advanced mode: every setting of the scope (selected files, or all files /
//! new files), grouped as Format, Video, Picture, Audio, Subtitles, Other.
//!
//! Rows write into the scope's `OutputSettings` through
//! `sidebar::apply_to_scope`; `sync` reflects the settings back (rebuilding
//! lists that depend on the container and codec). While `ui.syncing` is set,
//! change handlers do nothing.

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use adw::prelude::*;

use crate::i18n::{gettext, tr, tr_fmt};
use crate::sidebar::{self, FORMAT_PREFIX};
use crate::util;
use crate::window::Ui;
use retort_core::catalog::FormatKind;
use retort_core::ffmpeg::MediaKind;
use retort_core::media::{self, MediaInfo, StreamKind};
use retort_core::quality::RateControl;
use retort_core::settings::*;

/// A combo row whose items carry string ids.
#[derive(Clone)]
struct Combo {
    row: adw::ComboRow,
    ids: Rc<RefCell<Vec<String>>>,
}

impl Combo {
    fn new(title: &str) -> Combo {
        let row = adw::ComboRow::builder().title(title).build();
        row.set_use_subtitle(false);
        Combo {
            row,
            ids: Rc::default(),
        }
    }

    /// Replace the items (id, label) and select `selected` (else the first).
    fn set(&self, items: &[(String, String)], selected: &str) {
        let same = {
            let ids = self.ids.borrow();
            ids.len() == items.len() && ids.iter().zip(items).all(|(a, (b, _))| a == b)
        };
        if !same {
            let labels: Vec<&str> = items.iter().map(|(_, l)| l.as_str()).collect();
            self.row.set_model(Some(&gtk::StringList::new(&labels)));
            *self.ids.borrow_mut() = items.iter().map(|(i, _)| i.clone()).collect();
        }
        let idx = self
            .ids
            .borrow()
            .iter()
            .position(|i| i == selected)
            .unwrap_or(0);
        if self.row.selected() != idx as u32 {
            self.row.set_selected(idx as u32);
        }
    }

    fn id(&self) -> Option<String> {
        self.ids.borrow().get(self.row.selected() as usize).cloned()
    }

    fn on_change(&self, ui: &Ui, f: impl Fn(&Ui, String) + 'static) {
        let u = ui.clone();
        let this = self.clone();
        self.row.connect_selected_notify(move |_| {
            if u.syncing.get() {
                return;
            }
            if let Some(id) = this.id() {
                f(&u, id);
            }
        });
    }
}

fn opts(items: &[(&str, String)]) -> Vec<(String, String)> {
    items
        .iter()
        .map(|(i, l)| (i.to_string(), l.clone()))
        .collect()
}

fn spin(title: &str, min: f64, max: f64, step: f64) -> adw::SpinRow {
    let r = adw::SpinRow::with_range(min, max, step);
    r.set_title(title);
    r
}

fn entry(title: &str) -> adw::EntryRow {
    let r = adw::EntryRow::builder()
        .title(title)
        .show_apply_button(true)
        .build();
    r.set_input_hints(gtk::InputHints::NO_SPELLCHECK);
    r
}

fn switch(title: &str, subtitle: &str) -> adw::SwitchRow {
    let r = adw::SwitchRow::builder().title(title).build();
    if !subtitle.is_empty() {
        r.set_subtitle(subtitle);
    }
    r
}

pub struct Advanced {
    pub root: gtk::Widget,
    format_row: adw::ActionRow,
    // Video
    video_group: adw::PreferencesGroup,
    video_mode: Combo,
    video_codec: Combo,
    encoder: Combo,
    rate: Combo,
    quality: adw::SpinRow,
    bitrate: adw::SpinRow,
    size_mb: adw::SpinRow,
    two_pass: adw::SwitchRow,
    bit_depth: Combo,
    enc_preset: adw::EntryRow,
    tune: adw::EntryRow,
    profile: adw::EntryRow,
    level: adw::EntryRow,
    params: adw::EntryRow,
    gop: adw::SpinRow,
    bframes: adw::SpinRow,
    // Picture
    picture_group: adw::PreferencesGroup,
    resolution: Combo,
    fps: Combo,
    crop: Combo,
    crop_manual: adw::EntryRow,
    rotate: Combo,
    flip_h: adw::SwitchRow,
    flip_v: adw::SwitchRow,
    deinterlace: Combo,
    denoise: Combo,
    sharpen: Combo,
    hdr: Combo,
    // Trim
    trim_start: adw::EntryRow,
    trim_end: adw::EntryRow,
    // Audio
    audio_group: adw::PreferencesGroup,
    audio_mode: Combo,
    audio_codec: Combo,
    audio_bitrate: adw::SpinRow,
    channels: Combo,
    sample_rate: Combo,
    loudnorm: adw::SwitchRow,
    lufs: adw::SpinRow,
    audio_tracks: Combo,
    audio_langs: adw::EntryRow,
    default_audio: Combo,
    // Subtitles
    sub_group: adw::PreferencesGroup,
    sub_mode: Combo,
    sub_codec: Combo,
    burn_stream: Combo,
    sub_tracks: Combo,
    sub_langs: adw::EntryRow,
    default_sub: Combo,
    // Other
    copy_metadata: adw::SwitchRow,
    copy_chapters: adw::SwitchRow,
    keep_cover: adw::SwitchRow,
    extra_in: adw::EntryRow,
    extra_out: adw::EntryRow,
    extra_warning: gtk::Label,
    /// Codec lists per (format, kind), computed off the main thread because
    /// exotic containers need a test mux the first time.
    codec_cache: RefCell<HashMap<(String, u8), Vec<String>>>,
    codec_pending: RefCell<Option<String>>,
}

impl Advanced {
    pub fn build() -> Advanced {
        let page = gtk::Box::new(gtk::Orientation::Vertical, 18);
        page.set_margin_start(12);
        page.set_margin_end(12);
        page.set_margin_top(6);
        page.set_margin_bottom(18);

        // Format.
        let fg = adw::PreferencesGroup::new();
        let format_row = adw::ActionRow::builder()
            .title(gettext("Container"))
            .build();
        format_row.add_prefix(&gtk::Image::from_icon_name("folder-documents-symbolic"));
        let change = gtk::Button::with_label(&gettext("Change…"));
        change.set_valign(gtk::Align::Center);
        change.set_action_name(Some("win.advanced-format"));
        format_row.add_suffix(&change);
        format_row.set_activatable_widget(Some(&change));
        fg.add(&format_row);
        page.append(&fg);

        // Video.
        let video_group = adw::PreferencesGroup::builder()
            .title(gettext("Video"))
            .build();
        let video_mode = Combo::new(&gettext("Video"));
        let video_codec = Combo::new(&gettext("Codec"));
        let encoder = Combo::new(&gettext("Encoder"));
        let rate = Combo::new(&gettext("Rate Control"));
        let quality = spin(&gettext("Quality"), 0.0, 100.0, 1.0);
        let bitrate = spin(&gettext("Bitrate (kb/s)"), 100.0, 500_000.0, 100.0);
        let size_mb = spin(&gettext("Target Size (MB)"), 1.0, 100_000.0, 1.0);
        let two_pass = switch(
            &gettext("Two-Pass"),
            &gettext("Slower, more accurate bitrate (CPU encoders)"),
        );
        let bit_depth = Combo::new(&gettext("Bit Depth"));
        let enc_preset = entry(&gettext("Encoder Preset (empty = from Speed)"));
        let tune = entry(&gettext("Tune"));
        let profile = entry(&gettext("Profile"));
        let level = entry(&gettext("Level"));
        let params = entry(&gettext("Encoder Parameters (x265/SVT-AV1/x264)"));
        let gop = spin(
            &gettext("Keyframe Interval (frames, 0 = auto)"),
            0.0,
            10_000.0,
            1.0,
        );
        let bframes = spin(&gettext("B-Frames (-1 = auto)"), -1.0, 16.0, 1.0);
        video_group.add(&video_mode.row);
        video_group.add(&video_codec.row);
        video_group.add(&encoder.row);
        video_group.add(&rate.row);
        for w in [&quality, &bitrate, &size_mb] {
            video_group.add(w);
        }
        video_group.add(&two_pass);
        video_group.add(&bit_depth.row);
        let more = adw::ExpanderRow::builder()
            .title(gettext("Encoder Details"))
            .build();
        for w in [&enc_preset, &tune, &profile, &level, &params] {
            more.add_row(w);
        }
        more.add_row(&gop);
        more.add_row(&bframes);
        video_group.add(&more);
        page.append(&video_group);

        // Picture.
        let picture_group = adw::PreferencesGroup::builder()
            .title(gettext("Picture"))
            .build();
        let resolution = Combo::new(&gettext("Resolution"));
        let fps = Combo::new(&gettext("Frame Rate"));
        let crop = Combo::new(&gettext("Crop"));
        let crop_manual = entry(&gettext("Crop Rectangle (width:height:x:y)"));
        let rotate = Combo::new(&gettext("Rotate"));
        let flip_h = switch(&gettext("Flip Horizontally"), "");
        let flip_v = switch(&gettext("Flip Vertically"), "");
        let deinterlace = Combo::new(&gettext("Deinterlace"));
        let denoise = Combo::new(&gettext("Denoise"));
        let sharpen = Combo::new(&gettext("Sharpen"));
        let hdr = Combo::new(&gettext("HDR"));
        picture_group.add(&resolution.row);
        picture_group.add(&fps.row);
        picture_group.add(&hdr.row);
        picture_group.add(&crop.row);
        picture_group.add(&crop_manual);
        picture_group.add(&rotate.row);
        picture_group.add(&flip_h);
        picture_group.add(&flip_v);
        picture_group.add(&deinterlace.row);
        picture_group.add(&denoise.row);
        picture_group.add(&sharpen.row);
        page.append(&picture_group);

        // Trim.
        let trim_group = adw::PreferencesGroup::builder()
            .title(gettext("Trim"))
            .description(gettext(
                "Seconds, m:ss or h:mm:ss. Leave empty for the start / end of the file.",
            ))
            .build();
        let trim_start = entry(&gettext("Start"));
        let trim_end = entry(&gettext("End"));
        trim_group.add(&trim_start);
        trim_group.add(&trim_end);
        page.append(&trim_group);

        // Audio.
        let audio_group = adw::PreferencesGroup::builder()
            .title(gettext("Audio"))
            .build();
        let audio_mode = Combo::new(&gettext("Audio"));
        let audio_codec = Combo::new(&gettext("Codec"));
        let audio_bitrate = spin(&gettext("Bitrate (kb/s per stereo pair)"), 8.0, 1536.0, 8.0);
        let channels = Combo::new(&gettext("Channels"));
        let sample_rate = Combo::new(&gettext("Sample Rate"));
        let loudnorm = switch(
            &gettext("Normalize Loudness"),
            &gettext("EBU R128, two passes"),
        );
        let lufs = spin(&gettext("Target Loudness (LUFS)"), -40.0, -5.0, 1.0);
        let audio_tracks = Combo::new(&gettext("Tracks"));
        let audio_langs = entry(&gettext("Languages (e.g. eng, deu)"));
        let default_audio = Combo::new(&gettext("Default Track"));
        for c in [&audio_mode, &audio_codec] {
            audio_group.add(&c.row);
        }
        audio_group.add(&audio_bitrate);
        audio_group.add(&channels.row);
        audio_group.add(&sample_rate.row);
        audio_group.add(&loudnorm);
        audio_group.add(&lufs);
        audio_group.add(&audio_tracks.row);
        audio_group.add(&audio_langs);
        audio_group.add(&default_audio.row);
        page.append(&audio_group);

        // Subtitles.
        let sub_group = adw::PreferencesGroup::builder()
            .title(gettext("Subtitles"))
            .build();
        let sub_mode = Combo::new(&gettext("Subtitles"));
        let sub_codec = Combo::new(&gettext("Convert To"));
        let burn_stream = Combo::new(&gettext("Burn In"));
        let sub_tracks = Combo::new(&gettext("Tracks"));
        let sub_langs = entry(&gettext("Languages (e.g. eng, deu)"));
        let default_sub = Combo::new(&gettext("Default Track"));
        sub_group.add(&sub_mode.row);
        sub_group.add(&sub_codec.row);
        sub_group.add(&burn_stream.row);
        sub_group.add(&sub_tracks.row);
        sub_group.add(&sub_langs);
        sub_group.add(&default_sub.row);
        page.append(&sub_group);

        // Other.
        let other = adw::PreferencesGroup::builder()
            .title(gettext("Metadata and Extras"))
            .build();
        let copy_metadata = switch(
            &gettext("Copy Metadata"),
            &gettext("Title, artist, dates and other tags"),
        );
        let copy_chapters = switch(&gettext("Copy Chapters"), "");
        let keep_cover = switch(
            &gettext("Keep Cover Art"),
            &gettext("For audio files that support it"),
        );
        let extra_in = entry(&gettext("Extra Input Arguments"));
        let extra_out = entry(&gettext("Extra Output Arguments"));
        other.add(&copy_metadata);
        other.add(&copy_chapters);
        other.add(&keep_cover);
        other.add(&extra_in);
        other.add(&extra_out);
        let extra_warning = gtk::Label::new(None);
        extra_warning.add_css_class("caption");
        extra_warning.add_css_class("warning");
        extra_warning.set_wrap(true);
        extra_warning.set_xalign(0.0);
        extra_warning.set_margin_top(6);
        extra_warning.set_visible(false);
        page.append(&other);
        page.append(&extra_warning);

        let scroll = gtk::ScrolledWindow::builder()
            .child(&page)
            .hscrollbar_policy(gtk::PolicyType::Never)
            .vexpand(true)
            .build();
        Advanced {
            root: scroll.upcast(),
            format_row,
            video_group,
            video_mode,
            video_codec,
            encoder,
            rate,
            quality,
            bitrate,
            size_mb,
            two_pass,
            bit_depth,
            enc_preset,
            tune,
            profile,
            level,
            params,
            gop,
            bframes,
            picture_group,
            resolution,
            fps,
            crop,
            crop_manual,
            rotate,
            flip_h,
            flip_v,
            deinterlace,
            denoise,
            sharpen,
            hdr,
            trim_start,
            trim_end,
            audio_group,
            audio_mode,
            audio_codec,
            audio_bitrate,
            channels,
            sample_rate,
            loudnorm,
            lufs,
            audio_tracks,
            audio_langs,
            default_audio,
            sub_group,
            sub_mode,
            sub_codec,
            burn_stream,
            sub_tracks,
            sub_langs,
            default_sub,
            copy_metadata,
            copy_chapters,
            keep_cover,
            extra_in,
            extra_out,
            extra_warning,
            codec_cache: RefCell::default(),
            codec_pending: RefCell::default(),
        }
    }
}

// ---- Editing helpers

/// Apply `f` to the scope's settings; the scope becomes a custom setup of its
/// container (Simple mode shows it as such).
fn edit(ui: &Ui, f: impl Fn(&mut OutputSettings) + 'static) {
    if ui.syncing.get() {
        return;
    }
    sidebar::apply_to_scope(ui, move |s, p| {
        f(s);
        *p = format!("{FORMAT_PREFIX}{}", s.format);
    });
    sidebar::sync_from_state(ui);
}

fn edit_video(ui: &Ui, f: impl Fn(&mut VideoEncode) + 'static) {
    edit(ui, move |s| {
        if let Some(v) = s.video_encode_mut() {
            f(v);
        }
    });
}

fn edit_audio(ui: &Ui, f: impl Fn(&mut AudioEncode) + 'static) {
    edit(ui, move |s| {
        if let AudioSettings::Encode(a) = &mut s.audio {
            f(a);
        }
    });
}

fn none_if_empty(s: &str) -> Option<String> {
    Some(s.trim().to_string()).filter(|s| !s.is_empty())
}

fn langs(s: &str) -> Vec<String> {
    s.split([',', ' ', ';'])
        .map(|l| l.trim().to_lowercase())
        .filter(|l| !l.is_empty())
        .collect()
}

fn strength(id: &str) -> Option<Strength> {
    match id {
        "light" => Some(Strength::Light),
        "medium" => Some(Strength::Medium),
        "strong" => Some(Strength::Strong),
        _ => None,
    }
}

fn strength_id(s: Option<Strength>) -> &'static str {
    match s {
        Some(Strength::Light) => "light",
        Some(Strength::Medium) => "medium",
        Some(Strength::Strong) => "strong",
        None => "off",
    }
}

fn kind_key(k: MediaKind) -> u8 {
    match k {
        MediaKind::Video => 0,
        MediaKind::Audio => 1,
        _ => 2,
    }
}

/// The media of the scope when it is exactly one file (track lists need it),
/// and the number of files in the scope.
fn scope_media(ui: &Ui) -> (Option<MediaInfo>, usize) {
    let sel = ui.selected_ids();
    let st = ui.state.borrow();
    let items: Vec<_> = if sel.is_empty() {
        st.items.iter().collect()
    } else {
        st.items.iter().filter(|i| sel.contains(&i.id)).collect()
    };
    let one = if items.len() == 1 {
        items[0].media.clone()
    } else {
        None
    };
    (one, items.len())
}

/// "1:30", "1:02:03.5": like `format_duration` but keeps tenths.
fn fmt_time(t: f64) -> String {
    let whole = t.floor();
    let tenths = ((t - whole) * 10.0).round() as u32;
    let base = media::format_duration(whole);
    if tenths == 0 || tenths >= 10 {
        base
    } else {
        format!("{base}.{tenths}")
    }
}

fn track_label(s: &media::StreamInfo, n: usize) -> String {
    let mut l = tr_fmt("Track {}", &[&n.to_string()]);
    if let Some(lang) = &s.language {
        l.push_str(&format!(" · {lang}"));
    }
    l.push_str(&format!(" · {}", media::codec_display_name(&s.codec)));
    if let Some(t) = &s.title {
        l.push_str(&format!(" · {t}"));
    }
    l
}

// ---- Wiring

pub fn wire(ui: &Ui) {
    let a = &ui.sidebar.advanced;

    // Container.
    let act = gtk::gio::SimpleAction::new("advanced-format", None);
    let u = ui.clone();
    act.connect_activate(move |_, _| {
        let u2 = u.clone();
        crate::picker::show(&u, false, move |choice| {
            if let crate::picker::Choice::Format(id) = choice {
                change_format(&u2, id);
            }
        });
    });
    ui.window.add_action(&act);

    // Video.
    a.video_mode.on_change(ui, |u, id| {
        let format = sidebar::scope_settings(u).0.format;
        let engine = u.engine();
        edit(u, move |s| {
            s.video = match id.as_str() {
                "copy" => VideoSettings::Copy,
                "drop" => VideoSettings::Drop,
                _ => match &s.video {
                    VideoSettings::Encode(_) => return,
                    _ => {
                        let c = engine
                            .as_ref()
                            .and_then(|e| {
                                e.codec_options(&format, MediaKind::Video)
                                    .into_iter()
                                    .next()
                            })
                            .unwrap_or_else(|| "h264".into());
                        VideoSettings::Encode(Box::new(VideoEncode::new(&c)))
                    }
                },
            };
        });
    });
    a.video_codec.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            if v.codec != id {
                v.codec = id.clone();
                v.encoder = EncoderChoice::Auto;
                v.preset = None;
                v.tune = None;
                v.profile = None;
                v.level = None;
                v.encoder_params = None;
                v.tag = None;
            }
        })
    });
    a.encoder.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.encoder = if id == "auto" {
                EncoderChoice::Auto
            } else {
                let (enc, dev) = id
                    .split_once('@')
                    .map(|(e, d)| (e, Some(d.to_string())))
                    .unwrap_or((id.as_str(), None));
                EncoderChoice::Pinned {
                    encoder: enc.to_string(),
                    device: dev.filter(|d| !d.is_empty()),
                }
            };
        })
    });
    a.rate.on_change(ui, |u, id| {
        let mb = u.sidebar.advanced.size_mb.value();
        edit_video(u, move |v| {
            v.rate = match id.as_str() {
                "native" => RateControl::Native { value: 23.0 },
                "vbr" => RateControl::Vbr {
                    kbps: 5000,
                    max_kbps: None,
                },
                "cbr" => RateControl::Cbr { kbps: 5000 },
                "size" => {
                    v.two_pass = true;
                    RateControl::TargetSize {
                        bytes: (mb.max(1.0) * 1e6) as u64,
                    }
                }
                _ => RateControl::default(),
            };
        })
    });
    let u = ui.clone();
    a.quality.connect_value_notify(move |r| {
        let val = r.value();
        edit_video(&u, move |v| match &mut v.rate {
            RateControl::Native { value } => *value = val as f32,
            RateControl::Quality { value } => *value = val.clamp(0.0, 100.0) as u8,
            _ => {}
        })
    });
    let u = ui.clone();
    a.bitrate.connect_value_notify(move |r| {
        let k = r.value() as u32;
        edit_video(&u, move |v| match &mut v.rate {
            RateControl::Vbr { kbps, .. } | RateControl::Cbr { kbps } => *kbps = k,
            _ => {}
        })
    });
    let u = ui.clone();
    a.size_mb.connect_value_notify(move |r| {
        let b = (r.value().max(1.0) * 1e6) as u64;
        edit_video(&u, move |v| {
            if let RateControl::TargetSize { bytes } = &mut v.rate {
                *bytes = b;
            }
        })
    });
    let u = ui.clone();
    a.two_pass.connect_active_notify(move |r| {
        let on = r.is_active();
        edit_video(&u, move |v| v.two_pass = on)
    });
    a.bit_depth.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.bit_depth = match id.as_str() {
                "8" => BitDepth::Eight,
                "10" => BitDepth::Ten,
                _ => BitDepth::Auto,
            }
        })
    });
    type Setter = fn(&mut VideoEncode, Option<String>);
    let text_fields: [(&adw::EntryRow, Setter); 5] = [
        (&a.enc_preset, |v, t| v.preset = t),
        (&a.tune, |v, t| v.tune = t),
        (&a.profile, |v, t| v.profile = t),
        (&a.level, |v, t| v.level = t),
        (&a.params, |v, t| v.encoder_params = t),
    ];
    for (row, set) in text_fields {
        let u = ui.clone();
        row.connect_apply(move |r| {
            let t = none_if_empty(&r.text());
            edit_video(&u, move |v| set(v, t.clone()))
        });
    }
    let u = ui.clone();
    a.gop.connect_value_notify(move |r| {
        let g = r.value() as u32;
        edit_video(&u, move |v| v.gop = (g > 0).then_some(g))
    });
    let u = ui.clone();
    a.bframes.connect_value_notify(move |r| {
        let b = r.value() as i32;
        edit_video(&u, move |v| v.bframes = (b >= 0).then_some(b as u32))
    });

    // Picture.
    a.resolution.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.filters.scale = match id.as_str() {
                "orig" => None,
                other => {
                    let h: u32 = other.parse().unwrap_or(1080);
                    Some(Scale {
                        width: Some(h * 16 / 9 + (h * 16 / 9) % 2),
                        height: Some(h),
                        only_shrink: true,
                    })
                }
            }
        })
    });
    a.fps.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.filters.fps = (id != "orig").then(|| id.clone())
        })
    });
    a.crop.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.filters.crop = match id.as_str() {
                "auto" => Some(Crop::Auto),
                "manual" => Some(
                    v.filters
                        .crop
                        .filter(|c| matches!(c, Crop::Manual { .. }))
                        .unwrap_or(Crop::Manual {
                            w: 1920,
                            h: 800,
                            x: 0,
                            y: 140,
                        }),
                ),
                _ => None,
            }
        })
    });
    let u = ui.clone();
    a.crop_manual.connect_apply(move |r| {
        let parts: Vec<u32> = r
            .text()
            .split([':', 'x', ',', ' '])
            .filter_map(|p| p.trim().parse().ok())
            .collect();
        if parts.len() == 4 && parts[0] > 0 && parts[1] > 0 {
            edit_video(&u, move |v| {
                v.filters.crop = Some(Crop::Manual {
                    w: parts[0],
                    h: parts[1],
                    x: parts[2],
                    y: parts[3],
                })
            });
        } else {
            r.add_css_class("error");
            u.toast(&gettext("Use width:height:x:y, e.g. 1920:800:0:140"));
        }
    });
    a.rotate.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.filters.rotate = match id.as_str() {
                "cw90" => Rotate::Cw90,
                "ccw90" => Rotate::Ccw90,
                "180" => Rotate::R180,
                _ => Rotate::None,
            }
        })
    });
    let u = ui.clone();
    a.flip_h.connect_active_notify(move |r| {
        let on = r.is_active();
        edit_video(&u, move |v| v.filters.flip_h = on)
    });
    let u = ui.clone();
    a.flip_v.connect_active_notify(move |r| {
        let on = r.is_active();
        edit_video(&u, move |v| v.filters.flip_v = on)
    });
    a.deinterlace.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.filters.deinterlace = match id.as_str() {
                "on" => Deinterlace::On,
                "off" => Deinterlace::Off,
                _ => Deinterlace::Auto,
            }
        })
    });
    a.denoise.on_change(ui, |u, id| {
        edit_video(u, move |v| v.filters.denoise = strength(&id))
    });
    a.sharpen.on_change(ui, |u, id| {
        edit_video(u, move |v| v.filters.sharpen = strength(&id))
    });
    a.hdr.on_change(ui, |u, id| {
        edit_video(u, move |v| {
            v.hdr = match id.as_str() {
                "preserve" => HdrMode::Preserve,
                "tonemap" => HdrMode::Tonemap,
                _ => HdrMode::Auto,
            }
        })
    });

    // Trim.
    for (row, is_start) in [(&a.trim_start, true), (&a.trim_end, false)] {
        let u = ui.clone();
        row.connect_apply(move |r| {
            let text = r.text();
            let value = media::parse_time(&text);
            if value.is_none() && !text.trim().is_empty() {
                r.add_css_class("error");
                u.toast(&gettext("Times look like 90, 1:30 or 1:02:03.5"));
                return;
            }
            r.remove_css_class("error");
            edit(&u, move |s| {
                let mut t = s.trim.unwrap_or(Trim {
                    start: None,
                    end: None,
                });
                if is_start {
                    t.start = value;
                } else {
                    t.end = value;
                }
                s.trim = (t.start.is_some() || t.end.is_some()).then_some(t);
            });
        });
    }

    // Audio.
    a.audio_mode.on_change(ui, |u, id| {
        let format = sidebar::scope_settings(u).0.format;
        let engine = u.engine();
        edit(u, move |s| {
            s.audio = match id.as_str() {
                "copy" => AudioSettings::Copy,
                "drop" => AudioSettings::Drop,
                _ => match &s.audio {
                    AudioSettings::Encode(_) => return,
                    _ => {
                        let c = engine
                            .as_ref()
                            .and_then(|e| {
                                e.codec_options(&format, MediaKind::Audio)
                                    .into_iter()
                                    .next()
                            })
                            .unwrap_or_else(|| "aac".into());
                        AudioSettings::Encode(AudioEncode::new(
                            &c,
                            retort_core::preset::default_audio_kbps(&c),
                        ))
                    }
                },
            };
        });
    });
    a.audio_codec.on_change(ui, |u, id| {
        edit_audio(u, move |x| {
            if x.codec != id {
                x.codec = id.clone();
                x.encoder = None;
                x.vbr_quality = None;
                x.sample_fmt = None;
                x.bitrate_kbps = retort_core::preset::default_audio_kbps(&id);
            }
        })
    });
    let u = ui.clone();
    a.audio_bitrate.connect_value_notify(move |r| {
        let k = r.value() as u32;
        edit_audio(&u, move |x| {
            x.bitrate_kbps = Some(k);
            x.vbr_quality = None;
        })
    });
    a.channels.on_change(ui, |u, id| {
        edit_audio(u, move |x| x.channels = id.parse().ok())
    });
    a.sample_rate.on_change(ui, |u, id| {
        edit_audio(u, move |x| x.sample_rate = id.parse().ok())
    });
    let u = ui.clone();
    a.loudnorm.connect_active_notify(move |r| {
        let on = r.is_active();
        let lufs = u.sidebar.advanced.lufs.value() as f32;
        edit_audio(&u, move |x| {
            x.loudnorm = on.then(|| Loudnorm {
                target_lufs: lufs,
                ..Loudnorm::default()
            })
        })
    });
    let u = ui.clone();
    a.lufs.connect_value_notify(move |r| {
        let v = r.value() as f32;
        edit_audio(&u, move |x| {
            if let Some(l) = &mut x.loudnorm {
                l.target_lufs = v;
            }
        })
    });
    a.audio_tracks.on_change(ui, |u, id| {
        let l = langs(&u.sidebar.advanced.audio_langs.text());
        edit(u, move |s| {
            s.audio_tracks = match id.as_str() {
                "primary" => TrackSelection::Primary,
                "languages" => TrackSelection::Languages {
                    languages: l.clone(),
                },
                _ => TrackSelection::All,
            }
        })
    });
    let u = ui.clone();
    a.audio_langs.connect_apply(move |r| {
        let l = langs(&r.text());
        edit(&u, move |s| {
            s.audio_tracks = TrackSelection::Languages {
                languages: l.clone(),
            }
        })
    });
    a.default_audio.on_change(ui, |u, id| {
        edit(u, move |s| s.default_audio = id.parse().ok())
    });

    // Subtitles.
    a.sub_mode.on_change(ui, |u, id| {
        let format = sidebar::scope_settings(u).0.format;
        let engine = u.engine();
        edit(u, move |s| {
            s.subtitles = match id.as_str() {
                "convert" => {
                    let c = engine
                        .as_ref()
                        .and_then(|e| {
                            e.codec_options(&format, MediaKind::Subtitle)
                                .into_iter()
                                .next()
                        })
                        .unwrap_or_else(|| "srt".into());
                    SubtitleSettings::Convert { codec: c }
                }
                "burn" => SubtitleSettings::BurnIn { stream: None },
                "drop" => SubtitleSettings::Drop,
                _ => SubtitleSettings::Auto,
            }
        })
    });
    a.sub_codec.on_change(ui, |u, id| {
        edit(u, move |s| {
            if let SubtitleSettings::Convert { codec } = &mut s.subtitles {
                *codec = id.clone();
            }
        })
    });
    a.burn_stream.on_change(ui, |u, id| {
        edit(u, move |s| {
            if let SubtitleSettings::BurnIn { stream } = &mut s.subtitles {
                *stream = id.parse().ok();
            }
        })
    });
    a.sub_tracks.on_change(ui, |u, id| {
        let l = langs(&u.sidebar.advanced.sub_langs.text());
        edit(u, move |s| {
            s.subtitle_tracks = match id.as_str() {
                "primary" => TrackSelection::Primary,
                "languages" => TrackSelection::Languages {
                    languages: l.clone(),
                },
                _ => TrackSelection::All,
            }
        })
    });
    let u = ui.clone();
    a.sub_langs.connect_apply(move |r| {
        let l = langs(&r.text());
        edit(&u, move |s| {
            s.subtitle_tracks = TrackSelection::Languages {
                languages: l.clone(),
            }
        })
    });
    a.default_sub.on_change(ui, |u, id| {
        edit(u, move |s| {
            s.default_subtitle = match id.as_str() {
                "keep" => None,
                "none" => Some(NO_DEFAULT_TRACK),
                n => n.parse().ok(),
            }
        })
    });

    // Other.
    type MetaSetter = fn(&mut MetadataPolicy, bool);
    let meta_fields: [(&adw::SwitchRow, MetaSetter); 3] = [
        (&a.copy_metadata, |m, on| m.copy_metadata = on),
        (&a.copy_chapters, |m, on| m.copy_chapters = on),
        (&a.keep_cover, |m, on| m.keep_cover_art = on),
    ];
    for (row, set) in meta_fields {
        let u = ui.clone();
        row.connect_active_notify(move |r| {
            let on = r.is_active();
            edit(&u, move |s| set(&mut s.metadata, on))
        });
    }
    for (row, input) in [(&a.extra_in, true), (&a.extra_out, false)] {
        let u = ui.clone();
        row.connect_apply(move |r| match split_args(&r.text()) {
            Ok(args) => {
                r.remove_css_class("error");
                edit(&u, move |s| {
                    if input {
                        s.extra_input_args = args.clone();
                    } else {
                        s.extra_output_args = args.clone();
                    }
                })
            }
            Err(e) => {
                r.add_css_class("error");
                u.toast(&tr_fmt("Can't read these arguments: {}", &[&e]));
            }
        });
    }
}

/// Switch the scope to another container, keeping compatible choices.
fn change_format(ui: &Ui, id: String) {
    let Some(engine) = ui.engine() else { return };
    let current = sidebar::scope_settings(ui).0;
    let u = ui.clone();
    util::spawn_worker(
        move || engine.retarget(&current, &id),
        move |res| {
            if let Some(new) = res {
                edit(&u, move |s| *s = new.clone());
            }
        },
    );
}

// ---- Sync

fn codecs(ui: &Ui, format: &str, kind: MediaKind) -> Option<Vec<String>> {
    let a = &ui.sidebar.advanced;
    if let Some(v) = a
        .codec_cache
        .borrow()
        .get(&(format.to_string(), kind_key(kind)))
    {
        return Some(v.clone());
    }
    // Compute all kinds off the main thread, then sync again.
    if a.codec_pending.borrow().as_deref() == Some(format) {
        return None;
    }
    let engine = ui.engine()?;
    *a.codec_pending.borrow_mut() = Some(format.to_string());
    let f = format.to_string();
    let u = ui.clone();
    util::spawn_worker(
        move || {
            [MediaKind::Video, MediaKind::Audio, MediaKind::Subtitle]
                .into_iter()
                .map(|k| ((f.clone(), kind_key(k)), engine.codec_options(&f, k)))
                .collect::<Vec<_>>()
        },
        move |lists| {
            let a = &u.sidebar.advanced;
            for (k, v) in lists {
                a.codec_cache.borrow_mut().insert(k, v);
            }
            *a.codec_pending.borrow_mut() = None;
            sync(&u);
        },
    );
    None
}

fn codec_items(list: &[String], current: &str) -> Vec<(String, String)> {
    let mut v: Vec<(String, String)> = list
        .iter()
        .map(|c| (c.clone(), media::codec_display_name(c)))
        .collect();
    if !current.is_empty() && !list.iter().any(|c| c == current) {
        v.insert(0, (current.to_string(), media::codec_display_name(current)));
    }
    v
}

/// Reflect the scope's settings in every row.
pub fn sync(ui: &Ui) {
    let a = &ui.sidebar.advanced;
    let Some(engine) = ui.engine() else {
        a.root.set_sensitive(false);
        return;
    };
    a.root.set_sensitive(true);
    let was = ui.syncing.replace(true);
    let (s, _) = sidebar::scope_settings(ui);
    let (one, n_scope) = scope_media(ui);
    let fmt = engine.format(&s.format).cloned();
    let kind = fmt.as_ref().map(|f| f.kind).unwrap_or(FormatKind::Video);
    a.format_row.set_subtitle(&match &fmt {
        Some(f) => format!("{} (.{})", tr(&f.title), f.extension),
        None if s.format == "auto-audio" => gettext("Matches the audio codec"),
        None => s.format.clone(),
    });

    // ---- Video
    let has_video_out = matches!(
        kind,
        FormatKind::Video | FormatKind::Image | FormatKind::Animation | FormatKind::Other
    );
    a.video_group.set_visible(has_video_out);
    let vlist = codecs(ui, &s.format, MediaKind::Video);
    let can_copy = kind == FormatKind::Video || kind == FormatKind::Other;
    let mut modes = vec![("encode", gettext("Re-encode"))];
    if can_copy {
        modes.push(("copy", gettext("Copy (no re-encode)")));
        modes.push(("drop", gettext("Remove")));
    }
    a.video_mode.set(
        &opts(&modes),
        match s.video {
            VideoSettings::Encode(_) => "encode",
            VideoSettings::Copy => "copy",
            VideoSettings::Drop => "drop",
        },
    );
    a.video_mode.row.set_visible(can_copy);
    let venc = s.video_encode().cloned();
    a.picture_group.set_visible(has_video_out && venc.is_some());
    let encode_rows: [&gtk::Widget; 11] = [
        a.video_codec.row.upcast_ref(),
        a.encoder.row.upcast_ref(),
        a.rate.row.upcast_ref(),
        a.quality.upcast_ref(),
        a.bitrate.upcast_ref(),
        a.size_mb.upcast_ref(),
        a.two_pass.upcast_ref(),
        a.bit_depth.row.upcast_ref(),
        a.enc_preset.upcast_ref(),
        a.gop.upcast_ref(),
        a.bframes.upcast_ref(),
    ];
    for w in encode_rows {
        w.set_visible(venc.is_some());
    }
    if let Some(v) = &venc {
        let list = vlist.clone().unwrap_or_default();
        a.video_codec.set(&codec_items(&list, &v.codec), &v.codec);
        a.video_codec.row.set_sensitive(vlist.is_some());
        // Encoders: Automatic, then verified hardware per device, then software.
        let mut items = vec![("auto".to_string(), gettext("Automatic"))];
        for o in engine.encoder_options(&v.codec) {
            let id = format!("{}@{}", o.encoder, o.device.clone().unwrap_or_default());
            let label = if o.hardware {
                format!("⚡ {}", o.label)
            } else {
                o.label.clone()
            };
            items.push((id, label));
        }
        let current = match &v.encoder {
            EncoderChoice::Auto => "auto".to_string(),
            EncoderChoice::Pinned { encoder, device } => {
                let id = format!("{encoder}@{}", device.clone().unwrap_or_default());
                if !items.iter().any(|(i, _)| *i == id) {
                    // Pinned to something that no longer passes: still shown.
                    items.push((id.clone(), tr_fmt("{} (not verified here)", &[encoder])));
                }
                id
            }
        };
        a.encoder.set(&items, &current);
        a.encoder.row.set_subtitle(&if current == "auto" {
            gettext("Verified hardware, CPU fallback")
        } else {
            gettext("Pinned: no automatic fallback")
        });
        let rate_id = match v.rate {
            RateControl::Quality { .. } => "quality",
            RateControl::Native { .. } | RateControl::ConstQp { .. } => "native",
            RateControl::Vbr { .. } => "vbr",
            RateControl::Cbr { .. } => "cbr",
            RateControl::TargetSize { .. } => "size",
        };
        a.rate.set(
            &opts(&[
                ("quality", gettext("Constant Quality")),
                ("native", gettext("Encoder's Own Scale (CRF/CQ/QP)")),
                ("vbr", gettext("Average Bitrate")),
                ("cbr", gettext("Constant Bitrate")),
                ("size", gettext("Target File Size")),
            ]),
            rate_id,
        );
        a.quality
            .set_visible(matches!(rate_id, "quality" | "native"));
        match v.rate {
            RateControl::Quality { value } => {
                a.quality.set_title(&gettext("Quality (0–100)"));
                a.quality.set_range(0.0, 100.0);
                a.quality.set_digits(0);
                a.quality.set_value(value as f64);
            }
            RateControl::Native { value } => {
                a.quality.set_title(&gettext("CRF / CQ / QP"));
                a.quality.set_range(0.0, 63.0);
                a.quality.set_digits(1);
                a.quality.set_value(value as f64);
            }
            RateControl::ConstQp { qp } => {
                a.quality.set_title(&gettext("CRF / CQ / QP"));
                a.quality.set_range(0.0, 63.0);
                a.quality.set_value(qp as f64);
            }
            _ => {}
        }
        a.bitrate.set_visible(matches!(rate_id, "vbr" | "cbr"));
        if let RateControl::Vbr { kbps, .. } | RateControl::Cbr { kbps } = v.rate {
            a.bitrate.set_value(kbps as f64);
        }
        a.size_mb.set_visible(rate_id == "size");
        if let RateControl::TargetSize { bytes } = v.rate {
            a.size_mb.set_value((bytes as f64 / 1e6).round());
        }
        a.two_pass.set_visible(matches!(rate_id, "vbr" | "size"));
        a.two_pass.set_active(v.two_pass);
        a.bit_depth.set(
            &opts(&[
                ("auto", gettext("Same as Source (max 10-bit)")),
                ("8", gettext("8-bit")),
                ("10", gettext("10-bit")),
            ]),
            match v.bit_depth {
                BitDepth::Auto => "auto",
                BitDepth::Eight => "8",
                BitDepth::Ten => "10",
            },
        );
        a.enc_preset.set_text(v.preset.as_deref().unwrap_or(""));
        a.tune.set_text(v.tune.as_deref().unwrap_or(""));
        a.profile.set_text(v.profile.as_deref().unwrap_or(""));
        a.level.set_text(v.level.as_deref().unwrap_or(""));
        a.params.set_text(v.encoder_params.as_deref().unwrap_or(""));
        a.gop.set_value(v.gop.unwrap_or(0) as f64);
        a.bframes
            .set_value(v.bframes.map(|b| b as f64).unwrap_or(-1.0));

        // Picture.
        let f = &v.filters;
        let res_id = match f.scale.and_then(|s| s.height) {
            None => "orig".to_string(),
            Some(h) => h.to_string(),
        };
        let mut res = vec![("orig".to_string(), gettext("Same as Source"))];
        for (h, name) in [
            (2160, "4K"),
            (1440, "1440p"),
            (1080, "1080p"),
            (720, "720p"),
            (480, "480p"),
            (360, "360p"),
        ] {
            res.push((
                h.to_string(),
                tr_fmt(
                    "{} (fit in {}×{})",
                    &[
                        name,
                        &(h * 16 / 9 + (h * 16 / 9) % 2).to_string(),
                        &h.to_string(),
                    ],
                ),
            ));
        }
        a.resolution.set(&res, &res_id);
        let mut fps_items = vec![("orig".to_string(), gettext("Same as Source"))];
        for r in ["60", "50", "30", "30000/1001", "25", "24", "24000/1001"] {
            let label = match r {
                "30000/1001" => "29.97".to_string(),
                "24000/1001" => "23.976".to_string(),
                x => x.to_string(),
            };
            fps_items.push((r.to_string(), label));
        }
        if let Some(cur) = &f.fps {
            if !fps_items.iter().any(|(i, _)| i == cur) {
                fps_items.push((cur.clone(), cur.clone()));
            }
        }
        a.fps.set(&fps_items, f.fps.as_deref().unwrap_or("orig"));
        a.crop.set(
            &opts(&[
                ("off", gettext("Off")),
                ("auto", gettext("Remove Black Bars (auto-detect)")),
                ("manual", gettext("Manual")),
            ]),
            match f.crop {
                None => "off",
                Some(Crop::Auto) => "auto",
                Some(Crop::Manual { .. }) => "manual",
            },
        );
        a.crop_manual
            .set_visible(matches!(f.crop, Some(Crop::Manual { .. })));
        if let Some(Crop::Manual { w, h, x, y }) = f.crop {
            a.crop_manual.set_text(&format!("{w}:{h}:{x}:{y}"));
            a.crop_manual.remove_css_class("error");
        }
        a.rotate.set(
            &opts(&[
                ("none", gettext("None")),
                ("cw90", gettext("90° Clockwise")),
                ("ccw90", gettext("90° Counter-Clockwise")),
                ("180", gettext("180°")),
            ]),
            match f.rotate {
                Rotate::None => "none",
                Rotate::Cw90 => "cw90",
                Rotate::Ccw90 => "ccw90",
                Rotate::R180 => "180",
            },
        );
        a.flip_h.set_active(f.flip_h);
        a.flip_v.set_active(f.flip_v);
        a.deinterlace.set(
            &opts(&[
                ("auto", gettext("Automatic (detect interlacing)")),
                ("on", gettext("Always")),
                ("off", gettext("Never")),
            ]),
            match f.deinterlace {
                Deinterlace::Auto => "auto",
                Deinterlace::On => "on",
                Deinterlace::Off => "off",
            },
        );
        let strengths = opts(&[
            ("off", gettext("Off")),
            ("light", gettext("Light")),
            ("medium", gettext("Medium")),
            ("strong", gettext("Strong")),
        ]);
        a.denoise.set(&strengths, strength_id(f.denoise));
        a.sharpen.set(&strengths, strength_id(f.sharpen));
        a.hdr.set(
            &opts(&[
                ("auto", gettext("Keep When Possible, Else Convert to SDR")),
                ("preserve", gettext("Keep HDR (fail if impossible)")),
                ("tonemap", gettext("Convert to SDR")),
            ]),
            match v.hdr {
                HdrMode::Auto => "auto",
                HdrMode::Preserve => "preserve",
                HdrMode::Tonemap => "tonemap",
            },
        );
        a.hdr.row.set_visible(
            one.as_ref()
                .map(|m| {
                    m.primary_video()
                        .and_then(|v| v.video.as_ref())
                        .map(|v| v.hdr.is_hdr())
                        .unwrap_or(false)
                })
                .unwrap_or(true),
        );
    }

    // ---- Trim
    let t = s.trim.unwrap_or(Trim {
        start: None,
        end: None,
    });
    a.trim_start
        .set_text(&t.start.map(fmt_time).unwrap_or_default());
    a.trim_end
        .set_text(&t.end.map(fmt_time).unwrap_or_default());

    // ---- Audio
    let has_audio_out = matches!(
        kind,
        FormatKind::Video | FormatKind::Audio | FormatKind::Other
    );
    a.audio_group.set_visible(has_audio_out);
    let alist = codecs(ui, &s.format, MediaKind::Audio);
    let mut amodes = vec![
        ("encode", gettext("Re-encode")),
        ("copy", gettext("Copy (no re-encode)")),
    ];
    if kind != FormatKind::Audio {
        amodes.push(("drop", gettext("Remove")));
    }
    a.audio_mode.set(
        &opts(&amodes),
        match s.audio {
            AudioSettings::Encode(_) => "encode",
            AudioSettings::Copy => "copy",
            AudioSettings::Drop => "drop",
        },
    );
    let aenc = s.audio_encode().cloned();
    for w in [
        a.audio_codec.row.upcast_ref::<gtk::Widget>(),
        a.channels.row.upcast_ref(),
        a.sample_rate.row.upcast_ref(),
        a.loudnorm.upcast_ref(),
    ] {
        w.set_visible(aenc.is_some());
    }
    a.audio_bitrate.set_visible(false);
    a.lufs.set_visible(false);
    if let Some(x) = &aenc {
        let list = alist.unwrap_or_default();
        a.audio_codec.set(&codec_items(&list, &x.codec), &x.codec);
        let lossless = matches!(x.codec.as_str(), "flac" | "alac" | "wavpack" | "tta")
            || x.codec.starts_with("pcm_");
        a.audio_bitrate.set_visible(!lossless);
        a.audio_bitrate.set_value(
            x.bitrate_kbps
                .or_else(|| retort_core::preset::default_audio_kbps(&x.codec))
                .unwrap_or(192) as f64,
        );
        a.channels.set(
            &opts(&[
                ("orig", gettext("Same as Source")),
                ("1", gettext("Mono")),
                ("2", gettext("Stereo")),
                ("6", gettext("5.1 Surround")),
            ]),
            &x.channels
                .map(|c| c.to_string())
                .unwrap_or_else(|| "orig".into()),
        );
        a.sample_rate.set(
            &opts(&[
                ("orig", gettext("Same as Source")),
                ("44100", "44.1 kHz".into()),
                ("48000", "48 kHz".into()),
                ("96000", "96 kHz".into()),
            ]),
            &x.sample_rate
                .map(|c| c.to_string())
                .unwrap_or_else(|| "orig".into()),
        );
        a.loudnorm.set_active(x.loudnorm.is_some());
        a.lufs.set_visible(x.loudnorm.is_some());
        if let Some(l) = &x.loudnorm {
            a.lufs.set_value(l.target_lufs as f64);
        }
    }
    let audio_streams: Vec<media::StreamInfo> = one
        .as_ref()
        .map(|m| {
            m.streams
                .iter()
                .filter(|st| st.kind == StreamKind::Audio)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    a.audio_tracks
        .row
        .set_visible(!matches!(s.audio, AudioSettings::Drop));
    a.audio_tracks.set(
        &opts(&[
            ("all", gettext("All Tracks")),
            ("primary", gettext("Default Track Only")),
            ("languages", gettext("By Language")),
        ]),
        match &s.audio_tracks {
            TrackSelection::All | TrackSelection::Streams { .. } => "all",
            TrackSelection::Primary => "primary",
            TrackSelection::Languages { .. } => "languages",
        },
    );
    a.audio_langs
        .set_visible(matches!(s.audio_tracks, TrackSelection::Languages { .. }));
    if let TrackSelection::Languages { languages } = &s.audio_tracks {
        a.audio_langs.set_text(&languages.join(", "));
    }
    let mut def = vec![("keep".to_string(), gettext("As in the Source"))];
    for (i, st) in audio_streams.iter().enumerate() {
        def.push((st.index.to_string(), track_label(st, i + 1)));
    }
    a.default_audio.set(
        &def,
        &s.default_audio
            .map(|d| d.to_string())
            .unwrap_or_else(|| "keep".into()),
    );
    a.default_audio
        .row
        .set_visible(audio_streams.len() > 1 && !matches!(s.audio, AudioSettings::Drop));

    // ---- Subtitles
    let slist = codecs(ui, &s.format, MediaKind::Subtitle).unwrap_or_default();
    let can_burn = venc.is_some();
    let mut smodes = vec![("auto", gettext("Keep (convert if needed)"))];
    if !slist.is_empty() {
        smodes.push(("convert", gettext("Convert To…")));
    }
    if can_burn {
        smodes.push(("burn", gettext("Burn Into the Picture")));
    }
    smodes.push(("drop", gettext("Remove")));
    a.sub_group
        .set_visible(has_video_out && (can_burn || !slist.is_empty()));
    a.sub_mode.set(
        &opts(&smodes),
        match s.subtitles {
            SubtitleSettings::Auto => "auto",
            SubtitleSettings::Convert { .. } => "convert",
            SubtitleSettings::BurnIn { .. } => "burn",
            SubtitleSettings::Drop => "drop",
        },
    );
    a.sub_codec
        .row
        .set_visible(matches!(s.subtitles, SubtitleSettings::Convert { .. }));
    if let SubtitleSettings::Convert { codec } = &s.subtitles {
        a.sub_codec.set(&codec_items(&slist, codec), codec);
    }
    let sub_streams: Vec<media::StreamInfo> = one
        .as_ref()
        .map(|m| {
            m.streams
                .iter()
                .filter(|st| st.kind == StreamKind::Subtitle)
                .cloned()
                .collect()
        })
        .unwrap_or_default();
    a.burn_stream
        .row
        .set_visible(matches!(s.subtitles, SubtitleSettings::BurnIn { .. }));
    if let SubtitleSettings::BurnIn { stream } = &s.subtitles {
        let mut items = vec![("first".to_string(), gettext("First Subtitle Track"))];
        for (i, st) in sub_streams.iter().enumerate() {
            items.push((st.index.to_string(), track_label(st, i + 1)));
        }
        a.burn_stream.set(
            &items,
            &stream
                .map(|x| x.to_string())
                .unwrap_or_else(|| "first".into()),
        );
    }
    let keeps_subs = matches!(
        s.subtitles,
        SubtitleSettings::Auto | SubtitleSettings::Convert { .. }
    );
    a.sub_tracks.row.set_visible(keeps_subs);
    a.sub_tracks.set(
        &opts(&[
            ("all", gettext("All Tracks")),
            ("primary", gettext("Default Track Only")),
            ("languages", gettext("By Language")),
        ]),
        match &s.subtitle_tracks {
            TrackSelection::All | TrackSelection::Streams { .. } => "all",
            TrackSelection::Primary => "primary",
            TrackSelection::Languages { .. } => "languages",
        },
    );
    a.sub_langs
        .set_visible(keeps_subs && matches!(s.subtitle_tracks, TrackSelection::Languages { .. }));
    if let TrackSelection::Languages { languages } = &s.subtitle_tracks {
        a.sub_langs.set_text(&languages.join(", "));
    }
    let mut sdef = vec![
        ("keep".to_string(), gettext("As in the Source")),
        ("none".to_string(), gettext("None")),
    ];
    for (i, st) in sub_streams.iter().enumerate() {
        sdef.push((st.index.to_string(), track_label(st, i + 1)));
    }
    a.default_sub.set(
        &sdef,
        &match s.default_subtitle {
            None => "keep".to_string(),
            Some(NO_DEFAULT_TRACK) => "none".to_string(),
            Some(i) => i.to_string(),
        },
    );
    a.default_sub
        .row
        .set_visible(keeps_subs && (!sub_streams.is_empty() || n_scope > 1));

    // ---- Other
    a.copy_metadata.set_active(s.metadata.copy_metadata);
    a.copy_chapters.set_active(s.metadata.copy_chapters);
    a.keep_cover.set_active(s.metadata.keep_cover_art);
    a.keep_cover.set_visible(kind == FormatKind::Audio);
    a.extra_in.set_text(&s.extra_input_args.join(" "));
    a.extra_out.set_text(&s.extra_output_args.join(" "));
    let mut warnings = extra_args_warnings(&s.extra_input_args);
    warnings.extend(extra_args_warnings(&s.extra_output_args));
    let any_extra = !s.extra_input_args.is_empty() || !s.extra_output_args.is_empty();
    if any_extra {
        warnings.insert(
            0,
            gettext("Extra arguments are passed to FFmpeg as-is and can make a conversion fail."),
        );
    }
    a.extra_warning.set_text(&warnings.join("\n"));
    a.extra_warning.set_visible(any_extra);

    ui.syncing.set(was);
}

/// Headless e2e check (scripts/e2e-gui.sh): drive real widgets the way a user
/// would — rotate 90°, trim to 1.5 s, mono audio.
pub fn e2e_apply(ui: &Ui) {
    ui.sidebar.mode_advanced.set_active(true);
    sync(ui);
    let a = &ui.sidebar.advanced;
    let pick = |c: &Combo, id: &str| {
        if let Some(i) = c.ids.borrow().iter().position(|x| x == id) {
            c.row.set_selected(i as u32);
        } else {
            eprintln!("E2E: option {id} missing in {}", c.row.title());
        }
    };
    pick(&a.rotate, "cw90");
    a.trim_end.set_text("1.5");
    a.trim_end.emit_by_name::<()>("apply", &[]);
    pick(&a.channels, "1");
}
