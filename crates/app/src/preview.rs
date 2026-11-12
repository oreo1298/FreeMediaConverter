// SPDX-License-Identifier: GPL-3.0-or-later
//! "Preview": encode 10 seconds from the middle of the focused file with its
//! settings, show the extrapolated size and time, and offer to play it.

use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, tr_fmt};
use crate::util::{self, spawn_worker};
use crate::window::Ui;
use retort_core::job::{self, JobEvent, JobHandle, JobOutcome};
use retort_core::preview;
use retort_core::settings::OutputSettings;

/// The current or last preview.
pub struct State {
    pub item: u64,
    pub settings: OutputSettings,
    pub handle: Option<JobHandle>,
    pub output: Option<PathBuf>,
    pub running: bool,
}

/// The file the sidebar's plan describes: first selected, else the first
/// unfinished analysed file, else any analysed file.
pub fn focused(ui: &Ui) -> Option<u64> {
    let sel = ui.selected_ids();
    let st = ui.state.borrow();
    sel.first()
        .and_then(|id| st.items.iter().find(|i| i.id == *id))
        .or_else(|| {
            st.items
                .iter()
                .find(|i| i.media.is_some() && !i.status.is_finished())
        })
        .or_else(|| st.items.iter().find(|i| i.media.is_some()))
        .map(|i| i.id)
}

fn remove_output(p: &Option<PathBuf>) {
    // Each preview lives in its own temporary directory.
    if let Some(dir) = p.as_ref().and_then(|p| p.parent()) {
        let _ = std::fs::remove_dir_all(dir);
    }
}

/// Delete the last preview file (window closing).
pub fn cleanup(ui: &Ui) {
    if let Some(mut s) = ui.preview.borrow_mut().take() {
        if let Some(h) = s.handle.take() {
            h.cancel();
        }
        remove_output(&s.output);
    }
}

fn set_row(ui: &Ui, subtitle: &str, running: bool, playable: bool) {
    let sb = &ui.sidebar;
    sb.preview_row.set_visible(!subtitle.is_empty());
    sb.preview_row
        .set_subtitle(&glib::markup_escape_text(subtitle));
    sb.preview_btn.set_label(&if running {
        gettext("Stop")
    } else {
        gettext("Preview")
    });
    sb.preview_play.set_visible(playable);
}

/// Reset the row when the focused file or its settings changed.
pub fn refresh(ui: &Ui) {
    let sb = &ui.sidebar;
    let target = focused(ui);
    let current = target.and_then(|id| {
        let st = ui.state.borrow();
        st.items
            .iter()
            .find(|i| i.id == id)
            .map(|i| (i.settings.clone(), i.media.is_some()))
    });
    sb.preview_btn
        .set_sensitive(matches!(current, Some((_, true))) && ui.engine().is_some());
    let stale = {
        let p = ui.preview.borrow();
        match (&*p, &current) {
            (Some(s), Some((settings, _))) => {
                !s.running && (Some(s.item) != target || &s.settings != settings)
            }
            (Some(s), None) => !s.running,
            (None, _) => true,
        }
    };
    if stale {
        set_row(ui, "", false, false);
    }
}

/// Start a preview of the focused file, or stop the running one.
pub fn start_or_stop(ui: &Ui) {
    if let Some(s) = ui.preview.borrow_mut().as_mut() {
        if s.running {
            if let Some(h) = s.handle.take() {
                h.cancel();
            }
            return;
        }
    }
    let Some(id) = focused(ui) else { return };
    let Some(engine) = ui.engine() else { return };
    let Some((media, settings, stem)) = ({
        let st = ui.state.borrow();
        st.items.iter().find(|i| i.id == id).and_then(|i| {
            i.media.clone().map(|m| {
                let stem = i
                    .path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_else(|| "preview".into());
                (m, i.settings.clone(), stem)
            })
        })
    }) else {
        return;
    };
    if let Some(old) = ui.preview.borrow_mut().take() {
        remove_output(&old.output);
    }
    *ui.preview.borrow_mut() = Some(State {
        item: id,
        settings: settings.clone(),
        handle: None,
        output: None,
        running: true,
    });
    set_row(ui, &gettext("Preparing…"), true, false);
    let u = ui.clone();
    let no_preview = gettext("A preview needs a video or audio file with a known duration");
    spawn_worker(
        move || -> Result<(job::JobRequest, f64, f64), String> {
            let plan = engine.plan(&media, &settings).map_err(|e| e.to_string())?;
            let sample = preview::sample_settings(&settings, &media, &plan).ok_or(no_preview)?;
            let (_, len) =
                preview::sample_range(&settings, &media).unwrap_or((0.0, preview::SAMPLE_SECONDS));
            let total = preview::output_duration(&settings, &media).unwrap_or(len);
            let plan = engine.plan(&media, &sample).map_err(|e| e.to_string())?;
            let dir = retort_core::paths::temp_dir("preview").map_err(|e| e.to_string())?;
            let out = dir.join(format!("{stem}-preview.{}", plan.format.extension));
            Ok((engine.request(media, sample, plan, out, true), len, total))
        },
        move |res| match res {
            Err(e) => {
                if let Some(s) = u.preview.borrow_mut().as_mut() {
                    s.running = false;
                }
                set_row(&u, &e, false, false);
            }
            Ok((req, len, total)) => run(&u, req, len, total),
        },
    );
}

fn run(ui: &Ui, req: job::JobRequest, len: f64, total: f64) {
    // The full job's limit (the sample itself runs at the planned bitrate).
    let cap = ui
        .preview
        .borrow()
        .as_ref()
        .and_then(|s| preview::size_cap(&s.settings));
    let (tx, rx) = async_channel::unbounded::<JobEvent>();
    let handle = job::start(req, move |e| {
        let _ = tx.send_blocking(e);
    });
    if let Some(s) = ui.preview.borrow_mut().as_mut() {
        s.handle = Some(handle);
    }
    set_row(ui, &gettext("Encoding the sample…"), true, false);
    let u = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            match ev {
                JobEvent::Progress {
                    fraction: Some(f), ..
                } => {
                    set_row(
                        &u,
                        &tr_fmt("Encoding the sample… {}%", &[&format!("{:.0}", f * 100.0)]),
                        true,
                        false,
                    );
                }
                JobEvent::Finished { outcome } => {
                    let text = match &outcome {
                        JobOutcome::Completed {
                            size,
                            seconds,
                            summary,
                            ..
                        } => match preview::estimate(*size, len, *seconds, total, cap) {
                            Some(e) => tr_fmt(
                                "Whole file: about {}, takes about {} ({})",
                                &[
                                    &util::human_size(e.bytes),
                                    &util::eta(e.seconds.max(1.0)),
                                    summary,
                                ],
                            ),
                            None => gettext("Sample ready"),
                        },
                        JobOutcome::Failed { reason, .. } => {
                            tr_fmt("Preview failed: {}", &[reason])
                        }
                        JobOutcome::Cancelled => gettext("Preview stopped"),
                        JobOutcome::Requeue { reason } => reason.clone(),
                    };
                    let output = match &outcome {
                        JobOutcome::Completed { output, .. } => Some(output.clone()),
                        _ => None,
                    };
                    if let Some(s) = u.preview.borrow_mut().as_mut() {
                        s.running = false;
                        s.handle = None;
                        s.output = output.clone();
                    }
                    set_row(&u, &text, false, output.is_some());
                    break;
                }
                _ => {}
            }
        }
    });
}

/// Open the last preview in the default player.
pub fn play(ui: &Ui) {
    let out = ui.preview.borrow().as_ref().and_then(|s| s.output.clone());
    if let Some(p) = out {
        let launcher = gtk::FileLauncher::new(Some(&gio::File::for_path(&p)));
        launcher.launch(Some(&ui.window), gio::Cancellable::NONE, |_| {});
    }
}
