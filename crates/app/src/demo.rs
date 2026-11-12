// SPDX-License-Identifier: GPL-3.0-or-later
//! Headless screenshot harness. With RETORT_SCREENSHOT_DIR set, the app
//! fills the queue with generated media, uses a simulated hardware profile
//! (RETORT_DEMO_PROFILE, default tests/fixtures/hw/nvidia-only), walks through
//! every major state in light and dark, renders the window with GSK to PNG,
//! and quits. Run via scripts/screenshots.sh (Xvfb).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::rc::Rc;
use std::time::Duration;

use adw::prelude::*;
use gtk::{gdk, glib, graphene};

use crate::model::{Item, Live, Status};
use crate::window::{self, HwStatus, Ui};
use retort_core::engine::Engine;
use retort_core::hw::simulate::Profile;
use retort_core::media::analyze;
use retort_core::process::SystemRunner;
use retort_core::progress::Progress;

/// Render `widget` (normally the window) to a PNG at 2× scale. Returns false
/// when nothing could be rendered yet (layout pending), so the caller retries.
pub fn capture(widget: &impl IsA<gtk::Widget>, path: &Path) -> bool {
    let w = widget.as_ref();
    let (width, height) = (w.width() as f32, w.height() as f32);
    let paintable = gtk::WidgetPaintable::new(Some(w));
    let snapshot = gtk::Snapshot::new();
    snapshot.scale(2.0, 2.0);
    paintable.snapshot(&snapshot, width as f64, height as f64);
    let Some(node) = snapshot.to_node() else {
        return false;
    };
    let Some(renderer) = w.native().and_then(|n| n.renderer()) else {
        return false;
    };
    let tex = renderer.render_texture(
        &node,
        Some(&graphene::Rect::new(0.0, 0.0, width * 2.0, height * 2.0)),
    );
    match tex.save_to_png(path) {
        Ok(()) => eprintln!("wrote {}", path.display()),
        Err(e) => eprintln!("screenshot {}: {e}", path.display()),
    }
    true
}

/// Generate attractive demo media with lavfi (argv only).
fn make_media(dir: &Path) -> Vec<PathBuf> {
    let gen = |name: &str, args: &[&str]| -> PathBuf {
        let p = dir.join(name);
        if !p.exists() {
            let mut url = std::ffi::OsString::from("file:");
            url.push(p.as_os_str());
            let _ = Command::new("ffmpeg")
                .args(["-v", "error", "-nostdin", "-y"])
                .args(args)
                .arg(url)
                .status();
        }
        p
    };
    vec![
        gen("Lisbon at Dusk (HDR).mkv", &[
            "-f", "lavfi", "-i", "gradients=s=1920x1080:c0=0xff7e5f:c1=0x2b1055:c2=0xfeb47b:x0=0:y0=0:x1=1920:y1=1080:speed=0.02:d=3",
            "-f", "lavfi", "-i", "sine=f=220:d=3,pan=5.1|FL=c0|FR=c0|FC=c0|LFE=c0|BL=c0|BR=c0",
            "-vf", "setparams=color_primaries=bt2020:color_trc=smpte2084:colorspace=bt2020nc,format=yuv420p10le",
            "-c:v", "libx265", "-preset", "ultrafast", "-x265-params", "log-level=error", "-c:a", "aac",
        ]),
        gen("Interview — final cut.mov", &[
            "-f", "lavfi", "-i", "gradients=s=1920x1080:c0=0x0f2027:c1=0x2c5364:c2=0x56ab2f:x0=0:y0=1080:x1=1920:y1=0:d=3",
            "-f", "lavfi", "-i", "sine=f=330:d=3", "-c:v", "libx264", "-preset", "ultrafast", "-c:a", "aac",
        ]),
        gen("Drone flight 4K.mp4", &[
            "-f", "lavfi", "-i", "mandelbrot=s=3840x2160:end_scale=0.05:rate=25", "-t", "1",
            "-c:v", "libx264", "-preset", "ultrafast",
        ]),
        gen("Podcast episode 42.flac", &[
            "-f", "lavfi", "-i", "aevalsrc=0.6*sin(2*PI*220*t)*(0.5+0.5*sin(2*PI*0.7*t))*(0.3+0.7*abs(sin(2*PI*0.13*t))):s=44100:d=8",
            "-c:a", "flac",
        ]),
        gen("Old family video.ts", &[
            "-f", "lavfi", "-i", "gradients=s=720x576:c0=0xc79081:c1=0xdfa579:c2=0x4b6cb7:x0=0:y0=0:x1=720:y1=576:d=3",
            "-f", "lavfi", "-i", "sine=f=440:d=3", "-vf", "tinterlace=interleave_top,fieldorder=tff",
            "-flags", "+ildct+ilme", "-c:v", "mpeg2video", "-b:v", "4M", "-c:a", "mp2",
        ]),
    ]
}

type Step = Box<dyn Fn(&Demo)>;

struct Demo {
    ui: Ui,
    dir: PathBuf,
    engine: Engine,
    dialog: RefCell<Option<adw::Dialog>>,
    profile: Profile,
    /// Set by a shot that rendered nothing; the step is retried.
    retry: std::cell::Cell<bool>,
}

impl Demo {
    fn shot(&self, name: &str) {
        let dark = adw::StyleManager::default().is_dark();
        let ok = capture(
            &self.ui.window,
            &self.dir.join(format!(
                "{name}-{}.png",
                if dark { "dark" } else { "light" }
            )),
        );
        self.retry.set(!ok);
    }
    fn close_dialog(&self) {
        if let Some(d) = self.dialog.borrow_mut().take() {
            d.force_close();
        }
    }
}

fn set_live(item: &mut Item, fraction: f64, speed: f64, fps: f64, eta: f64, size: u64) {
    item.status = Status::Running;
    item.live = Live {
        fraction: Some(fraction),
        eta: Some(eta),
        progress: Progress {
            speed: Some(speed),
            fps: Some(fps),
            size: Some(size),
            ..Default::default()
        },
    };
}

fn load_items(d: &Demo) {
    let files = make_media(&d.dir.join("media"));
    let presets = [
        "mp4-small",
        "mp4-compatible",
        "av1-smallest",
        "opus",
        "mp4-compatible",
    ];
    for (i, path) in files.iter().enumerate() {
        let Ok(media) = d.engine.analyze(path) else {
            continue;
        };
        let pr = retort_core::preset::find(presets[i]).expect("preset");
        let mut item = Item::new(
            i as u64 + 1,
            path.clone(),
            pr.settings.clone(),
            pr.id.to_string(),
        );
        item.thumbnail = if media.has_video() {
            analyze::thumbnail_png(&d.engine.ff, &media, 256, true)
                .ok()
                .and_then(|b| gdk::Texture::from_bytes(&glib::Bytes::from(&b)).ok())
        } else {
            None
        };
        item.peaks = if media.has_video() {
            None
        } else {
            analyze::waveform_peaks(&d.engine.ff, &media, 48).ok()
        };
        if let Ok(plan) = d.engine.plan(&media, &item.settings) {
            item.summary = Some(plan.chain[0].summary.clone());
            item.badge = Some(plan.chain[0].badge.clone());
            item.hardware = plan.chain[0].hardware;
            item.notes = plan.notes.clone();
            if let Ok(set) = retort_core::job::commands_for(
                &d.engine.request(
                    media.clone(),
                    item.settings.clone(),
                    plan,
                    path.with_extension("out.mp4"),
                    false,
                ),
                0,
                &path.with_extension("out.mp4"),
                Path::new("/tmp"),
            ) {
                item.command = Some(set.display(&d.engine.ff.bins.ffmpeg));
            }
        }
        item.media = Some(media);
        item.status = Status::Ready;
        window::insert_item(&d.ui, item);
    }
    window::refresh(&d.ui);
    crate::sidebar::sync_from_state(&d.ui);
}

fn with_items(d: &Demo, f: impl Fn(&mut Vec<Item>)) {
    f(&mut d.ui.state.borrow_mut().items);
    let ids: Vec<u64> = d.ui.state.borrow().items.iter().map(|i| i.id).collect();
    for id in ids {
        d.ui.update_card(id);
    }
    window::refresh(&d.ui);
    crate::sidebar::sync_from_state(&d.ui);
}

pub fn run(ui: Ui, dir: PathBuf) {
    let _ = std::fs::create_dir_all(dir.join("media"));
    if let Some(s) = gtk::Settings::default() {
        s.set_gtk_font_name(Some("Cantarell 11"));
    }
    ui.window.set_default_size(1240, 800);
    let profile_dir = std::env::var_os("RETORT_DEMO_PROFILE")
        .map(PathBuf::from)
        .unwrap_or_else(|| retort_core::hw::simulate::profiles_dir().join("nvidia-only"));
    let profile = Profile::load(&profile_dir).expect("demo profile");
    let inv = profile.inventory().expect("demo inventory");
    // Real ffmpeg for analysis/thumbnails; the simulated profile for hardware.
    let engine = Engine::discover(None, SystemRunner::shared())
        .expect("ffmpeg")
        .with_inventory(inv);
    {
        let mut st = ui.state.borrow_mut();
        st.engine = Some(engine.clone());
        st.hw = HwStatus::Ready;
    }
    window::refresh(&ui);
    let demo = Rc::new(Demo {
        ui: ui.clone(),
        dir,
        engine,
        dialog: RefCell::default(),
        profile,
        retry: Default::default(),
    });

    let mut steps: Vec<Step> = Vec::new();
    for dark in [false, true] {
        steps.push(Box::new(move |d: &Demo| {
            adw::StyleManager::default().set_color_scheme(if dark {
                adw::ColorScheme::ForceDark
            } else {
                adw::ColorScheme::ForceLight
            });
            d.ui.window.set_default_size(1240, 800);
            with_items(d, |items| items.clear());
            d.ui.list.remove_all();
            d.ui.cards.borrow_mut().clear();
            window::refresh(&d.ui);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("empty")));
        steps.push(Box::new(|d: &Demo| load_items(d)));
        steps.push(Box::new(|d: &Demo| d.shot("queue")));
        steps.push(Box::new(|d: &Demo| {
            with_items(d, |items| {
                let n = items.len();
                if n >= 5 {
                    set_live(&mut items[0], 0.42, 3.4, 81.0, 74.0, 38_400_000);
                    set_live(&mut items[1], 0.17, 0.9, 22.0, 212.0, 9_100_000);
                    items[2].status = Status::Queued;
                    items[3].status = Status::Done {
                        output: items[3].path.with_extension("opus"),
                        size: 412_000,
                        fallback: None,
                    };
                    items[4].status = Status::Queued;
                }
            });
            let mut st = d.ui.state.borrow_mut();
            st.batch = true;
            st.batch_done = 1;
            drop(st);
            window::refresh(&d.ui);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("queue-running")));
        steps.push(Box::new(|d: &Demo| {
            with_items(d, |items| {
                for (i, it) in items.iter_mut().enumerate() {
                    let out = it.path.with_extension(if i == 3 { "opus" } else { "mp4" });
                    it.status = Status::Done {
                        output: out,
                        size: [612_000_000, 184_000_000, 96_000_000, 412_000, 230_000_000][i % 5],
                        fallback: (i == 1).then(|| {
                            "NVENC · RTX 4090 failed: NVENC session limit reached".to_string()
                        }),
                    };
                    it.live = Live::default();
                }
            });
            d.ui.state.borrow_mut().batch = false;
            window::refresh(&d.ui);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("completed")));
        steps.push(Box::new(|d: &Demo| {
            with_items(d, |items| {
                if items.len() >= 5 {
                    items[2].status = Status::Failed {
                        reason: "Driver cannot encode this codec (decode-only driver?)".into(),
                        retry_software: true,
                    };
                    items[4].status = Status::Unreadable {
                        reason: "Can't read this file: Invalid data found when processing input"
                            .into(),
                    };
                }
            });
        }));
        steps.push(Box::new(|d: &Demo| d.shot("errors")));
        steps.push(Box::new(|d: &Demo| {
            with_items(d, |items| {
                for it in items.iter_mut() {
                    it.status = Status::Ready;
                }
            });
            let dlg = crate::picker::show(&d.ui, true, |_| {});
            *d.dialog.borrow_mut() = Some(dlg);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("format-picker")));
        steps.push(Box::new(|d: &Demo| {
            d.close_dialog();
            crate::details::show(&d.ui, 1);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("details")));
        steps.push(Box::new(|d: &Demo| {
            if let Some(w) = d.ui.window.visible_dialog() {
                w.force_close();
            }
            let p = crate::prefs::show(&d.ui);
            p.set_visible_page_name("hardware");
            *d.dialog.borrow_mut() = Some(p.upcast());
        }));
        steps.push(Box::new(|d: &Demo| d.shot("hardware")));
        steps.push(Box::new(|d: &Demo| {
            d.close_dialog();
            let broken =
                Profile::load(&d.profile.dir.join("../broken-drivers")).expect("broken profile");
            let ff = broken.ffmpeg().expect("ff");
            let caps = ff.caps().expect("caps");
            let inv = broken.inventory().expect("inv");
            let report =
                retort_core::doctor::check(Some(&ff), None, Some(&caps), &inv, &broken.root());
            *d.dialog.borrow_mut() = Some(crate::syscheck::show_report(&d.ui, &report));
        }));
        steps.push(Box::new(|d: &Demo| d.shot("system-check")));
        steps.push(Box::new(|d: &Demo| {
            d.close_dialog();
            d.ui.sidebar.mode_advanced.set_active(true);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("advanced")));
        steps.push(Box::new(|d: &Demo| {
            d.ui.sidebar.mode_simple.set_active(true);
            d.ui.window.set_default_size(380, 780);
        }));
        steps.push(Box::new(|d: &Demo| d.shot("narrow")));
    }
    steps.push(Box::new(|d: &Demo| d.ui.app.quit()));
    let idx = Rc::new(RefCell::new((0usize, 0u32)));
    glib::timeout_add_local(Duration::from_millis(900), move || {
        let (i, tries) = *idx.borrow();
        let Some(step) = steps.get(i) else {
            return glib::ControlFlow::Break;
        };
        step(&demo);
        if demo.retry.get() && tries < 10 {
            *idx.borrow_mut() = (i, tries + 1);
        } else {
            if demo.retry.get() {
                eprintln!("screenshot step {i}: nothing rendered, giving up");
            }
            *idx.borrow_mut() = (i + 1, 0);
        }
        glib::ControlFlow::Continue
    });
}
