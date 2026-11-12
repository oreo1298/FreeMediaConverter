// SPDX-License-Identifier: GPL-3.0-or-later
//! Running the queue: planning, per-resource concurrency (one job per GPU
//! plus one CPU job by default), NVENC session-limit requeueing, suspend
//! inhibition, notifications, pause/resume/cancel.

use std::collections::HashMap;
use std::path::PathBuf;

use adw::prelude::*;
use gtk::{gio, glib};

use crate::i18n::{gettext, ntr_fmt, tr_fmt};
use crate::model::{Prepared, Status};
use crate::util::spawn_worker;
use crate::window::{refresh, Ui};
use retort_core::job::{self, JobEvent, JobOutcome};
use retort_core::naming::{ConflictPolicy, OutputFolder, Resolution};

/// Start converting every convertible item.
pub fn start_batch(ui: &Ui) {
    let any = {
        let mut st = ui.state.borrow_mut();
        let mut any = false;
        for it in st.items.iter_mut() {
            if it.status.is_convertible() {
                it.status = Status::Queued;
                it.prepared = None;
                it.live = Default::default();
                any = true;
            }
        }
        if any && !st.batch {
            st.batch = true;
            st.batch_done = 0;
            st.batch_failed = 0;
            st.batch_fallback = 0;
            st.last_output = None;
        }
        any
    };
    if !any {
        return;
    }
    if !ui.demo && ui.state.borrow().inhibit.is_none() {
        let cookie = ui.app.inhibit(
            Some(&ui.window),
            gtk::ApplicationInhibitFlags::SUSPEND | gtk::ApplicationInhibitFlags::LOGOUT,
            Some(&gettext("Converting files")),
        );
        ui.state.borrow_mut().inhibit = Some(cookie).filter(|c| *c != 0);
    }
    let ids: Vec<u64> = ui
        .state
        .borrow()
        .items
        .iter()
        .filter(|i| i.status == Status::Queued)
        .map(|i| i.id)
        .collect();
    for id in ids {
        ui.update_card(id);
    }
    refresh(ui);
    pump(ui);
}

fn limits(ui: &Ui) -> (usize, usize) {
    let max = ui.settings.int("max-jobs").max(0) as usize;
    if max > 0 {
        return (max, max);
    }
    let gpus = ui
        .state
        .borrow()
        .engine
        .as_ref()
        .map(|e| {
            e.inventory
                .device_summaries()
                .iter()
                .filter(|d| !d.encode.is_empty())
                .count()
        })
        .unwrap_or(0);
    // Auto: one job per GPU encode engine plus one CPU job.
    (gpus + 1, 1)
}

/// Start whatever can start now; plan queued items that are not planned yet.
pub fn pump(ui: &Ui) {
    let (total_limit, per_resource) = limits(ui);
    // Plan up to `total_limit` items ahead.
    let to_plan: Vec<u64> = {
        let st = ui.state.borrow();
        st.items
            .iter()
            .filter(|i| i.status == Status::Queued && i.prepared.is_none() && !i.preparing)
            .take(total_limit.max(1))
            .map(|i| i.id)
            .collect()
    };
    for id in to_plan {
        prepare(ui, id);
    }
    loop {
        let next = {
            let st = ui.state.borrow();
            let running: Vec<&str> = st
                .items
                .iter()
                .filter(|i| matches!(i.status, Status::Running | Status::Paused))
                .filter_map(|i| i.prepared.as_ref().map(|p| p.resource.as_str()))
                .collect();
            if running.len() >= total_limit {
                None
            } else {
                let mut per: HashMap<&str, usize> = HashMap::new();
                for r in &running {
                    *per.entry(r).or_default() += 1;
                }
                st.items
                    .iter()
                    .filter(|i| i.status == Status::Queued)
                    .find(|i| {
                        i.prepared.as_ref().is_some_and(|p| {
                            per.get(p.resource.as_str()).copied().unwrap_or(0) < per_resource
                        })
                    })
                    .map(|i| i.id)
            }
        };
        match next {
            Some(id) => launch(ui, id),
            None => break,
        }
    }
    maybe_finish(ui);
}

/// Plan an item and resolve its output path (worker thread).
fn prepare(ui: &Ui, id: u64) {
    let Some(mut engine) = ui.engine() else {
        return;
    };
    let Some(idx) = ui.item_index(id) else { return };
    let (media, settings, preset, force_sw) = {
        let mut st = ui.state.borrow_mut();
        let it = &mut st.items[idx];
        let Some(m) = it.media.clone() else { return };
        it.preparing = true;
        (m, it.settings.clone(), it.preset.clone(), it.force_software)
    };
    if force_sw {
        engine.prefs.disable_hardware = true;
    }
    let folder = match ui.settings.string("output-folder").as_str() {
        "" => OutputFolder::NextToSource,
        p => OutputFolder::Custom {
            path: PathBuf::from(p),
        },
    };
    let template = ui.settings.string("filename-template").to_string();
    let policy = match ui.settings.string("conflict-policy").as_str() {
        "skip" => ConflictPolicy::Skip,
        "ask" => ConflictPolicy::Ask,
        "overwrite" => ConflictPolicy::Overwrite,
        _ => ConflictPolicy::Rename,
    };
    // Paths already claimed by other queued items (avoid two jobs writing the same name).
    let claimed: Vec<PathBuf> = ui
        .state
        .borrow()
        .items
        .iter()
        .filter_map(|i| i.prepared.as_ref().map(|p| p.output.clone()))
        .collect();
    let u = ui.clone();
    spawn_worker(
        move || {
            let plan = engine.plan(&media, &settings).map_err(|e| e.to_string())?;
            let vars = engine.template_vars(&media, &settings, &plan, &preset);
            let res = retort_core::naming::resolve(
                &media.path,
                &folder,
                &template,
                &vars,
                policy,
                &|p| p.exists() || claimed.iter().any(|c| c == p),
            );
            let resource = match plan.chain.first().and_then(|p| p.video.as_ref()) {
                Some(v) if v.backend.is_some() => v
                    .device
                    .as_ref()
                    .map(|d| d.gpu_id.clone().unwrap_or_else(|| d.label.clone()))
                    .unwrap_or_else(|| "gpu".into()),
                _ => "cpu".into(),
            };
            Ok::<_, String>((plan, res, resource))
        },
        move |res| {
            let Some(idx) = u.item_index(id) else { return };
            {
                let mut st = u.state.borrow_mut();
                st.items[idx].preparing = false;
                if st.items[idx].status != Status::Queued {
                    return;
                }
            }
            match res {
                Err(e) => finish_item(
                    &u,
                    id,
                    Status::Failed {
                        reason: e,
                        retry_software: false,
                    },
                ),
                Ok((plan, res, resource)) => match res {
                    Resolution::Path(p) => set_prepared(&u, id, plan, p, false, resource),
                    Resolution::Overwrite(p) => set_prepared(&u, id, plan, p, true, resource),
                    Resolution::Skip(p) => finish_item(
                        &u,
                        id,
                        Status::Skipped {
                            reason: tr_fmt(
                                "Skipped: {} already exists",
                                &[&p.file_name()
                                    .map(|n| n.to_string_lossy().into_owned())
                                    .unwrap_or_default()],
                            ),
                        },
                    ),
                    Resolution::Ask(p) => ask_overwrite(&u, id, plan, p, resource),
                },
            }
            pump(&u);
        },
    );
}

fn set_prepared(
    ui: &Ui,
    id: u64,
    plan: retort_core::plan::JobPlan,
    output: PathBuf,
    overwrite: bool,
    resource: String,
) {
    if let Some(idx) = ui.item_index(id) {
        let mut st = ui.state.borrow_mut();
        let it = &mut st.items[idx];
        it.summary = Some(plan.chain[0].summary.clone());
        it.badge = Some(plan.chain[0].badge.clone());
        it.hardware = plan.chain[0].hardware;
        it.notes = plan.notes.clone();
        it.prepared = Some(Prepared {
            plan,
            output,
            overwrite,
            resource,
        });
    }
    ui.update_card(id);
}

fn ask_overwrite(
    ui: &Ui,
    id: u64,
    plan: retort_core::plan::JobPlan,
    path: PathBuf,
    resource: String,
) {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let dialog = adw::AlertDialog::new(
        Some(&gettext("File Already Exists")),
        Some(&tr_fmt(
            "“{}” already exists. What should happen?",
            &[&name],
        )),
    );
    dialog.add_response("skip", &gettext("Skip"));
    dialog.add_response("rename", &gettext("Keep Both"));
    dialog.add_response("overwrite", &gettext("Replace"));
    dialog.set_response_appearance("overwrite", adw::ResponseAppearance::Destructive);
    dialog.set_default_response(Some("rename"));
    dialog.set_close_response("skip");
    let u = ui.clone();
    dialog.connect_response(None, move |_, resp| {
        match resp {
            "overwrite" => set_prepared(&u, id, plan.clone(), path.clone(), true, resource.clone()),
            "rename" => {
                let mut n = 1;
                let stem = path
                    .file_stem()
                    .map(|s| s.to_string_lossy().into_owned())
                    .unwrap_or_default();
                let ext = path
                    .extension()
                    .map(|e| format!(".{}", e.to_string_lossy()))
                    .unwrap_or_default();
                let mut p = path.clone();
                while p.exists() {
                    p = path.with_file_name(format!("{stem} ({n}){ext}"));
                    n += 1;
                }
                set_prepared(&u, id, plan.clone(), p, false, resource.clone());
            }
            _ => finish_item(
                &u,
                id,
                Status::Skipped {
                    reason: gettext("Skipped: the output already exists"),
                },
            ),
        }
        pump(&u);
    });
    dialog.present(Some(&ui.window));
}

fn log_path(id: u64) -> PathBuf {
    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    retort_core::paths::log_dir().join(format!("{ts}-{id}.log"))
}

fn launch(ui: &Ui, id: u64) {
    let Some(engine) = ui.engine() else { return };
    let Some(idx) = ui.item_index(id) else { return };
    let req = {
        let mut st = ui.state.borrow_mut();
        let others_nvenc = st
            .items
            .iter()
            .filter(|i| i.id != id && matches!(i.status, Status::Running | Status::Paused))
            .any(|i| {
                i.prepared
                    .as_ref()
                    .and_then(|p| p.plan.chain[0].video.as_ref())
                    .and_then(|v| v.backend)
                    == Some(retort_core::hw::Backend::Nvenc)
            });
        let it = &mut st.items[idx];
        let Some(p) = it.prepared.as_ref() else {
            return;
        };
        let Some(media) = it.media.clone() else {
            return;
        };
        let mut req = engine.request(
            media,
            it.settings.clone(),
            p.plan.clone(),
            p.output.clone(),
            p.overwrite,
        );
        req.log_path = Some(log_path(id));
        req.keep_partial = ui.settings.boolean("keep-partial");
        req.preserve_timestamps = ui.settings.boolean("preserve-timestamps");
        req.start_plan = it.start_plan.unwrap_or(0);
        req.requeue_on_session_limit = others_nvenc;
        it.status = Status::Running;
        it.log.clear();
        it.live = Default::default();
        req
    };
    let (tx, rx) = async_channel::unbounded::<JobEvent>();
    let handle = job::start(req, move |e| {
        let _ = tx.send_blocking(e);
    });
    if let Some(idx) = ui.item_index(id) {
        ui.state.borrow_mut().items[idx].handle = Some(handle);
    }
    ui.update_card(id);
    refresh(ui);
    let u = ui.clone();
    glib::spawn_future_local(async move {
        while let Ok(ev) = rx.recv().await {
            let done = matches!(ev, JobEvent::Finished { .. });
            on_event(&u, id, ev);
            if done {
                break;
            }
        }
    });
}

fn on_event(ui: &Ui, id: u64, ev: JobEvent) {
    let Some(idx) = ui.item_index(id) else { return };
    match ev {
        JobEvent::PlanStarted {
            summary,
            badge,
            command,
            index,
        } => {
            let mut st = ui.state.borrow_mut();
            let it = &mut st.items[idx];
            it.summary = Some(summary);
            it.badge = Some(badge);
            it.hardware = it
                .prepared
                .as_ref()
                .and_then(|p| p.plan.chain.get(index))
                .map(|p| p.hardware)
                .unwrap_or(false);
            it.command = Some(command);
        }
        JobEvent::Progress {
            fraction,
            eta,
            progress,
            ..
        } => {
            let mut st = ui.state.borrow_mut();
            let it = &mut st.items[idx];
            it.live.fraction = fraction;
            it.live.eta = eta;
            it.live.progress = progress;
        }
        JobEvent::Log { line } => {
            ui.state.borrow_mut().items[idx].push_log(line);
            return;
        }
        JobEvent::FallingBack {
            reason,
            next_summary,
        } => {
            let name = ui.state.borrow().items[idx].file_name();
            ui.toast(&tr_fmt(
                "{}: {} — retrying: {}",
                &[&name, &reason, &next_summary],
            ));
        }
        JobEvent::Paused | JobEvent::Resumed => {}
        JobEvent::Finished { outcome } => {
            let status = match outcome {
                JobOutcome::Completed {
                    output,
                    size,
                    fallback_reason,
                    ..
                } => {
                    let mut st = ui.state.borrow_mut();
                    st.batch_done += 1;
                    if fallback_reason.is_some() {
                        st.batch_fallback += 1;
                    }
                    st.last_output = Some(output.clone());
                    Status::Done {
                        output,
                        size,
                        fallback: fallback_reason,
                    }
                }
                JobOutcome::Failed {
                    reason,
                    log_tail,
                    can_retry_software,
                } => {
                    let mut st = ui.state.borrow_mut();
                    st.batch_failed += 1;
                    if !log_tail.is_empty() {
                        st.items[idx].push_log(log_tail);
                    }
                    Status::Failed {
                        reason,
                        retry_software: can_retry_software,
                    }
                }
                JobOutcome::Cancelled => Status::Cancelled,
                JobOutcome::Requeue { .. } => Status::Queued,
            };
            finish_item(ui, id, status);
            pump(ui);
            return;
        }
    }
    ui.update_card(id);
    refresh(ui);
}

fn finish_item(ui: &Ui, id: u64, status: Status) {
    if let Some(idx) = ui.item_index(id) {
        let mut st = ui.state.borrow_mut();
        let it = &mut st.items[idx];
        it.handle = None;
        if status != Status::Queued {
            it.live = Default::default();
            it.start_plan = None;
            it.force_software = false;
        }
        it.status = status;
    }
    ui.update_card(id);
    refresh(ui);
    ui.save_queue();
}

fn maybe_finish(ui: &Ui) {
    let (batch, active) = {
        let st = ui.state.borrow();
        (st.batch, st.items.iter().any(|i| i.status.is_active()))
    };
    if !batch || active {
        return;
    }
    let (done, failed, fallback, last, cookie) = {
        let mut st = ui.state.borrow_mut();
        st.batch = false;
        (
            st.batch_done,
            st.batch_failed,
            st.batch_fallback,
            st.last_output.clone(),
            st.inhibit.take(),
        )
    };
    if let Some(c) = cookie {
        ui.app.uninhibit(c);
    }
    refresh(ui);
    if done == 0 && failed == 0 {
        return;
    }
    let mut body = ntr_fmt(
        "{} file converted",
        "{} files converted",
        done,
        &[&done.to_string()],
    );
    if fallback > 0 {
        body.push_str(" · ");
        body.push_str(&ntr_fmt(
            "{} with software fallback",
            "{} with software fallback",
            fallback,
            &[&fallback.to_string()],
        ));
    }
    if failed > 0 {
        body.push_str(" · ");
        body.push_str(&ntr_fmt(
            "{} failed",
            "{} failed",
            failed,
            &[&failed.to_string()],
        ));
    }
    ui.toast(&body);
    if ui.settings.boolean("notify-on-complete") && !ui.demo {
        let n = gio::Notification::new(&if failed > 0 && done == 0 {
            gettext("Conversion Failed")
        } else {
            gettext("Conversion Finished")
        });
        n.set_body(Some(&body));
        if let Some(out) = last {
            let path = out.to_string_lossy().to_string();
            n.add_button_with_target_value(
                &gettext("Show in Files"),
                "app.show-file",
                Some(&path.to_variant()),
            );
            if let Some(dir) = out.parent() {
                n.add_button_with_target_value(
                    &gettext("Open Folder"),
                    "app.open-folder",
                    Some(&dir.to_string_lossy().to_string().to_variant()),
                );
            }
        }
        ui.app.send_notification(Some("batch-finished"), &n);
    }
}

/// Stop the batch: unstarted items go back to Ready, running ones are cancelled.
pub fn stop_all(ui: &Ui) {
    let running: Vec<u64> = {
        let mut st = ui.state.borrow_mut();
        let mut r = Vec::new();
        for it in st.items.iter_mut() {
            match it.status {
                Status::Queued => {
                    it.status = Status::Ready;
                    it.prepared = None;
                }
                Status::Running | Status::Paused => r.push(it.id),
                _ => {}
            }
        }
        r
    };
    for id in running {
        cancel_item(ui, id);
    }
    let ids: Vec<u64> = ui.state.borrow().items.iter().map(|i| i.id).collect();
    for id in ids {
        ui.update_card(id);
    }
    maybe_finish(ui);
    refresh(ui);
}

pub fn cancel_item(ui: &Ui, id: u64) {
    let Some(idx) = ui.item_index(id) else { return };
    let queued = ui.state.borrow().items[idx].status == Status::Queued;
    if queued {
        {
            let mut st = ui.state.borrow_mut();
            st.items[idx].status = Status::Ready;
            st.items[idx].prepared = None;
        }
        ui.update_card(id);
        maybe_finish(ui);
        return;
    }
    if let Some(h) = ui.state.borrow().items[idx].handle.as_ref() {
        h.cancel();
    }
}

pub fn toggle_pause_item(ui: &Ui, id: u64) {
    let Some(idx) = ui.item_index(id) else { return };
    {
        let mut st = ui.state.borrow_mut();
        let it = &mut st.items[idx];
        let Some(h) = it.handle.as_ref() else { return };
        if it.status == Status::Paused {
            if h.resume() {
                it.status = Status::Running;
            }
        } else if h.pause() {
            it.status = Status::Paused;
        }
    }
    ui.update_card(id);
    refresh(ui);
}

pub fn toggle_pause_all(ui: &Ui) {
    let (running, paused): (Vec<u64>, Vec<u64>) = {
        let st = ui.state.borrow();
        (
            st.items
                .iter()
                .filter(|i| i.status == Status::Running)
                .map(|i| i.id)
                .collect(),
            st.items
                .iter()
                .filter(|i| i.status == Status::Paused)
                .map(|i| i.id)
                .collect(),
        )
    };
    let ids = if running.is_empty() { paused } else { running };
    for id in ids {
        toggle_pause_item(ui, id);
    }
}

/// "Retry with software" for a job whose pinned hardware encoder failed.
pub fn retry_software(ui: &Ui, id: u64) {
    let Some(idx) = ui.item_index(id) else { return };
    {
        let mut st = ui.state.borrow_mut();
        let it = &mut st.items[idx];
        if let Some(v) = it.settings.video_encode_mut() {
            v.encoder = retort_core::settings::EncoderChoice::Auto;
        }
        it.force_software = true;
        it.start_plan = None;
        it.status = Status::Ready;
    }
    ui.update_card(id);
    start_batch(ui);
}
