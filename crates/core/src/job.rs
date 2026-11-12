// SPDX-License-Identifier: GPL-3.0-or-later
//! Running a planned job: spawn ffmpeg (argv only), parse progress, log
//! stderr, pause/resume/cancel, and walk the fallback chain on failure.
//!
//! Outputs are written to a hidden temporary file next to the destination
//! and renamed into place only on success, so failed or cancelled jobs never
//! leave partial files behind (unless asked to) and an existing file is never
//! touched unless the user chose to overwrite it.

use std::ffi::OsString;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, ChildStdin, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use serde::{Deserialize, Serialize};

use crate::command::{self, CommandSet, JobPaths, StepKind};
use crate::ffmpeg::{Caps, Ffmpeg};
use crate::hw::{probe::classify_failure, Backend};
use crate::media::MediaInfo;
use crate::naming;
use crate::plan::JobPlan;
use crate::progress::{Parser, Progress};
use crate::settings::OutputSettings;

/// Everything needed to run one job.
#[derive(Clone)]
pub struct JobRequest {
    pub ff: Ffmpeg,
    pub caps: Caps,
    pub media: MediaInfo,
    pub settings: OutputSettings,
    pub plan: JobPlan,
    pub output: PathBuf,
    pub overwrite: bool,
    /// Per-job log file (full stderr + commands).
    pub log_path: Option<PathBuf>,
    /// Keep what was written on cancel/failure (renamed with " (partial)").
    pub keep_partial: bool,
    pub preserve_timestamps: bool,
    /// Index into `plan.chain` to start from ("Retry with software" = last).
    pub start_plan: usize,
    /// Report NVENC session-limit failures as [`JobOutcome::Requeue`].
    pub requeue_on_session_limit: bool,
}

/// Progress/lifecycle events.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "event", rename_all = "snake_case")]
pub enum JobEvent {
    PlanStarted {
        index: usize,
        summary: String,
        badge: String,
        /// Copyable command (with the real output path).
        command: String,
    },
    Progress {
        /// Whole-job fraction 0–1 if known.
        fraction: Option<f64>,
        /// Whole-job ETA seconds if known.
        eta: Option<f64>,
        progress: Progress,
        step: usize,
        steps: usize,
    },
    Log {
        line: String,
    },
    FallingBack {
        reason: String,
        next_summary: String,
    },
    Paused,
    Resumed,
    Finished {
        outcome: JobOutcome,
    },
}

/// How a job ended.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "result", rename_all = "snake_case")]
pub enum JobOutcome {
    Completed {
        output: PathBuf,
        plan_index: usize,
        summary: String,
        /// Why we fell back (None = first plan worked).
        fallback_reason: Option<String>,
        size: u64,
        seconds: f64,
    },
    Failed {
        reason: String,
        log_tail: String,
        /// The encoder was pinned: offer "Retry with software".
        can_retry_software: bool,
    },
    Cancelled,
    /// Hardware session limit hit while other jobs used the encoder: retry later.
    Requeue {
        reason: String,
    },
}

struct Control {
    pid: Mutex<Option<i32>>,
    stdin: Mutex<Option<ChildStdin>>,
    cancel: AtomicBool,
    paused: AtomicBool,
}

/// Handle to a running job.
pub struct JobHandle {
    ctl: Arc<Control>,
    thread: Option<std::thread::JoinHandle<JobOutcome>>,
}

fn signal_group(pid: i32, sig: i32) -> bool {
    // SAFETY: kill(2) on the child's process group (it is its own group leader).
    unsafe { libc::kill(-pid, sig) == 0 }
}

impl JobHandle {
    /// Suspend ffmpeg (SIGSTOP). Returns false if nothing is running.
    pub fn pause(&self) -> bool {
        let pid = self.ctl.pid.lock().ok().and_then(|p| *p);
        match pid {
            Some(p) if signal_group(p, libc::SIGSTOP) => {
                self.ctl.paused.store(true, Ordering::SeqCst);
                true
            }
            _ => false,
        }
    }

    /// Continue ffmpeg (SIGCONT).
    pub fn resume(&self) -> bool {
        let pid = self.ctl.pid.lock().ok().and_then(|p| *p);
        self.ctl.paused.store(false, Ordering::SeqCst);
        matches!(pid, Some(p) if signal_group(p, libc::SIGCONT))
    }

    pub fn is_paused(&self) -> bool {
        self.ctl.paused.load(Ordering::SeqCst)
    }

    /// Graceful cancel: `q` on stdin, then SIGTERM, then SIGKILL.
    pub fn cancel(&self) {
        self.ctl.cancel.store(true, Ordering::SeqCst);
        let pid = self.ctl.pid.lock().ok().and_then(|p| *p);
        if let Some(p) = pid {
            if self.ctl.paused.swap(false, Ordering::SeqCst) {
                signal_group(p, libc::SIGCONT);
            }
        }
        if let Ok(mut s) = self.ctl.stdin.lock() {
            if let Some(stdin) = s.as_mut() {
                let _ = stdin.write_all(b"q\n");
                let _ = stdin.flush();
            }
        }
        let ctl = self.ctl.clone();
        std::thread::spawn(move || {
            for (wait, sig) in [(3000, libc::SIGTERM), (3000, libc::SIGKILL)] {
                let t = Instant::now();
                while t.elapsed() < Duration::from_millis(wait) {
                    if ctl.pid.lock().ok().and_then(|p| *p).is_none() {
                        return;
                    }
                    std::thread::sleep(Duration::from_millis(50));
                }
                if let Some(p) = ctl.pid.lock().ok().and_then(|p| *p) {
                    signal_group(p, sig);
                }
            }
        });
    }

    /// Block until the job ends.
    pub fn wait(mut self) -> JobOutcome {
        self.thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or(JobOutcome::Failed {
                reason: "job thread panicked".into(),
                log_tail: String::new(),
                can_retry_software: false,
            })
    }
}

/// Hidden temp path next to the destination: `.<name>.retort-<pid>-<n>.part.<ext>`.
pub fn temp_output_path(final_path: &Path) -> PathBuf {
    use std::sync::atomic::AtomicU64;
    static N: AtomicU64 = AtomicU64::new(0);
    let n = N.fetch_add(1, Ordering::Relaxed);
    let dir = final_path.parent().unwrap_or(Path::new("."));
    let stem = final_path
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_else(|| "output".into());
    let ext = final_path
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    dir.join(format!(
        ".{stem}.{}-{}-{n}.part{ext}",
        crate::branding::APP_BIN,
        std::process::id()
    ))
}

/// Pick a free path near `p` (" (1)", " (2)"…) without ever returning an existing file.
fn free_path(p: &Path) -> PathBuf {
    if !p.exists() {
        return p.to_path_buf();
    }
    let dir = p.parent().unwrap_or(Path::new("."));
    let stem = p
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
        .unwrap_or_default();
    let ext = p
        .extension()
        .map(|e| format!(".{}", e.to_string_lossy()))
        .unwrap_or_default();
    (1..100_000)
        .map(|n| dir.join(format!("{stem} ({n}){ext}")))
        .find(|c| !c.exists())
        .unwrap_or_else(|| p.with_extension("new"))
}

struct StepResult {
    ok: bool,
    cancelled: bool,
    stderr_tail: String,
    stderr_all: String,
}

type Sink = Arc<dyn Fn(JobEvent) + Send + Sync>;

fn run_step(
    ff: &Ffmpeg,
    args: &[OsString],
    ctl: &Arc<Control>,
    sink: &Sink,
    log: &Mutex<Option<std::fs::File>>,
    duration: Option<f64>,
    step: usize,
    steps: usize,
    weights: &[f32],
) -> StepResult {
    let mut cmd = Command::new(&ff.bins.ffmpeg);
    cmd.args(args);
    crate::process::base_env(&mut cmd);
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    {
        use std::os::unix::process::CommandExt;
        cmd.process_group(0);
    }
    let mut child: Child = match cmd.spawn() {
        Ok(c) => c,
        Err(e) => {
            return StepResult {
                ok: false,
                cancelled: false,
                stderr_tail: format!("could not start ffmpeg: {e}"),
                stderr_all: String::new(),
            }
        }
    };
    if let Ok(mut p) = ctl.pid.lock() {
        *p = Some(child.id() as i32);
    }
    if let Ok(mut s) = ctl.stdin.lock() {
        *s = child.stdin.take();
    }
    // A cancel that arrived before spawn: ask the new process to quit now.
    if ctl.cancel.load(Ordering::SeqCst) {
        if let Ok(mut s) = ctl.stdin.lock() {
            if let Some(stdin) = s.as_mut() {
                let _ = stdin.write_all(b"q\n");
            }
        }
        signal_group(child.id() as i32, libc::SIGTERM);
    }
    let stdout = child.stdout.take().expect("piped");
    let stderr = child.stderr.take().expect("piped");
    let before: f64 = weights[..step].iter().map(|w| *w as f64).sum();
    let weight = weights[step] as f64;
    let sink_p = sink.clone();
    let t_out = std::thread::spawn(move || {
        let mut parser = Parser::new(duration);
        for line in BufReader::new(stdout).lines().map_while(Result::ok) {
            if let Some(p) = parser.feed(&line) {
                let fraction = p.fraction.map(|f| before + weight * f);
                let eta = match (p.eta, fraction) {
                    (Some(e), Some(f)) if f > 0.0 && weight > 0.0 => {
                        Some(e + e / weight * (1.0 - before - weight))
                    }
                    (e, _) => e,
                };
                sink_p(JobEvent::Progress {
                    fraction,
                    eta,
                    progress: p,
                    step,
                    steps,
                });
            }
        }
    });
    let sink_e = sink.clone();
    let tail = Arc::new(Mutex::new(Vec::<String>::new()));
    let tail_e = tail.clone();
    let all = Arc::new(Mutex::new(String::new()));
    let all_e = all.clone();
    let (tx, rx) = std::sync::mpsc::channel::<String>();
    let t_err = std::thread::spawn(move || {
        let mut reader = BufReader::new(stderr);
        let mut buf = Vec::new();
        loop {
            buf.clear();
            match reader.read_until(b'\n', &mut buf) {
                Ok(0) | Err(_) => break,
                Ok(_) => {
                    let line = String::from_utf8_lossy(&buf).trim_end().to_string();
                    if line.is_empty() {
                        continue;
                    }
                    if let Ok(mut t) = tail_e.lock() {
                        t.push(line.clone());
                        let excess = t.len().saturating_sub(40);
                        t.drain(..excess);
                    }
                    if let Ok(mut a) = all_e.lock() {
                        if a.len() < 4 << 20 {
                            a.push_str(&line);
                            a.push('\n');
                        }
                    }
                    let _ = tx.send(line.clone());
                    sink_e(JobEvent::Log { line });
                }
            }
        }
    });
    let status = child.wait();
    let _ = t_out.join();
    let _ = t_err.join();
    if let Ok(mut f) = log.lock() {
        if let Some(f) = f.as_mut() {
            for line in rx.try_iter() {
                let _ = writeln!(f, "{line}");
            }
        }
    }
    if let Ok(mut p) = ctl.pid.lock() {
        *p = None;
    }
    if let Ok(mut s) = ctl.stdin.lock() {
        *s = None;
    }
    let cancelled = ctl.cancel.load(Ordering::SeqCst);
    let ok = status.map(|s| s.success()).unwrap_or(false) && !cancelled;
    let stderr_tail = tail.lock().map(|t| t.join("\n")).unwrap_or_default();
    let stderr_all = all.lock().map(|a| a.clone()).unwrap_or_default();
    StepResult {
        ok,
        cancelled,
        stderr_tail,
        stderr_all,
    }
}

fn remove_quiet(p: &Path) {
    let _ = std::fs::remove_file(p);
}

/// Start a job on a background thread; events go to `sink`.
pub fn start(req: JobRequest, sink: impl Fn(JobEvent) + Send + Sync + 'static) -> JobHandle {
    let ctl = Arc::new(Control {
        pid: Mutex::new(None),
        stdin: Mutex::new(None),
        cancel: AtomicBool::new(false),
        paused: AtomicBool::new(false),
    });
    let sink: Sink = Arc::new(sink);
    let ctl_t = ctl.clone();
    let thread = std::thread::spawn(move || {
        let outcome = run(req, &ctl_t, &sink);
        sink(JobEvent::Finished {
            outcome: outcome.clone(),
        });
        outcome
    });
    JobHandle {
        ctl,
        thread: Some(thread),
    }
}

/// Build the command set for one plan of a request (real output path).
pub fn commands_for(
    req: &JobRequest,
    plan_index: usize,
    output: &Path,
    work_dir: &Path,
) -> crate::Result<CommandSet> {
    let plan = &req.plan.chain[plan_index];
    command::build(
        &req.ff,
        &req.caps,
        &req.media,
        &req.settings,
        &req.plan,
        plan,
        &JobPaths {
            input: req.media.path.clone(),
            output: output.to_path_buf(),
            overwrite: req.overwrite,
            work_dir: work_dir.to_path_buf(),
        },
    )
}

fn run(req: JobRequest, ctl: &Arc<Control>, sink: &Sink) -> JobOutcome {
    let started = Instant::now();
    let log_file = req.log_path.as_ref().and_then(|p| {
        if let Some(d) = p.parent() {
            let _ = std::fs::create_dir_all(d);
        }
        std::fs::File::create(p).ok()
    });
    let log = Mutex::new(log_file);
    let logln = |s: &str| {
        if let Ok(mut f) = log.lock() {
            if let Some(f) = f.as_mut() {
                let _ = writeln!(f, "{s}");
            }
        }
    };
    logln(&format!(
        "# {} {} — ffmpeg {} ({})",
        crate::branding::APP_NAME,
        crate::branding::VERSION,
        req.ff.version.raw,
        req.ff.bins.ffmpeg.display()
    ));
    logln(&format!("# input: {}", req.media.path.display()));
    for n in &req.plan.notes {
        logln(&format!("# note: {n}"));
    }
    let work_dir = match crate::paths::temp_dir("job") {
        Ok(d) => d,
        Err(e) => {
            return JobOutcome::Failed {
                reason: format!("cannot create a work directory: {e}"),
                log_tail: String::new(),
                can_retry_software: false,
            }
        }
    };
    let duration = req
        .settings
        .trim
        .and_then(|t| t.duration(req.media.duration))
        .or(req.media.duration);
    let mut fallback_reason: Option<String> = None;
    let chain_len = req.plan.chain.len();
    let mut index = req.start_plan.min(chain_len.saturating_sub(1));
    let outcome = loop {
        let plan = &req.plan.chain[index];
        let tmp = temp_output_path(&req.output);
        let display = commands_for(&req, index, &req.output, &work_dir);
        let set = commands_for(&req, index, &tmp, &work_dir);
        let (display, mut set) = match (display, set) {
            (Ok(d), Ok(s)) => (d, s),
            (Err(e), _) | (_, Err(e)) => {
                if index + 1 < chain_len && !req.plan.pinned {
                    let next = &req.plan.chain[index + 1];
                    sink(JobEvent::FallingBack {
                        reason: e.to_string(),
                        next_summary: next.summary.clone(),
                    });
                    fallback_reason.get_or_insert(e.to_string());
                    index += 1;
                    continue;
                }
                break JobOutcome::Failed {
                    reason: e.to_string(),
                    log_tail: String::new(),
                    can_retry_software: req.plan.pinned,
                };
            }
        };
        let shown = display.display(&req.ff.bins.ffmpeg);
        logln(&format!(
            "\n# plan {}: {}\n{shown}\n",
            index + 1,
            plan.summary
        ));
        sink(JobEvent::PlanStarted {
            index,
            summary: plan.summary.clone(),
            badge: plan.badge.clone(),
            command: shown,
        });

        let weights: Vec<f32> = set.steps.iter().map(|s| s.weight).collect();
        let steps = set.steps.len();
        let mut failed: Option<(String, String)> = None;
        let mut cancelled = false;
        for i in 0..steps {
            let step_duration = duration;
            let args = set.steps[i].args.clone();
            let r = run_step(
                &req.ff,
                &args,
                ctl,
                sink,
                &log,
                step_duration,
                i,
                steps,
                &weights,
            );
            if r.cancelled {
                cancelled = true;
                break;
            }
            if !r.ok {
                failed = Some((classify_failure(&r.stderr_all), r.stderr_tail));
                break;
            }
            if let StepKind::LoudnormMeasure { index: j } = set.steps[i].kind {
                let last = set.steps.len() - 1;
                if let Err(e) = command::apply_loudnorm(&mut set.steps[last].args, j, &r.stderr_all)
                {
                    failed = Some((e.to_string(), r.stderr_tail));
                    break;
                }
            }
        }
        if cancelled {
            if req.keep_partial && tmp.exists() {
                let partial = req.output.with_file_name(format!(
                    "{} (partial).{}",
                    req.output
                        .file_stem()
                        .map(|s| s.to_string_lossy().into_owned())
                        .unwrap_or_default(),
                    req.output
                        .extension()
                        .map(|e| e.to_string_lossy().into_owned())
                        .unwrap_or_default()
                ));
                let _ = std::fs::rename(&tmp, free_path(&partial));
            } else {
                remove_quiet(&tmp);
            }
            break JobOutcome::Cancelled;
        }
        match failed {
            None => {
                // Success: move into place.
                let final_path = if req.overwrite {
                    req.output.clone()
                } else {
                    free_path(&req.output)
                };
                if let Err(e) = std::fs::rename(&tmp, &final_path) {
                    remove_quiet(&tmp);
                    break JobOutcome::Failed {
                        reason: format!("could not move the output into place: {e}"),
                        log_tail: String::new(),
                        can_retry_software: false,
                    };
                }
                if req.preserve_timestamps {
                    let _ = naming::preserve_timestamps(&req.media.path, &final_path);
                }
                let size = std::fs::metadata(&final_path).map(|m| m.len()).unwrap_or(0);
                logln(&format!(
                    "# completed: {} ({} bytes)",
                    final_path.display(),
                    size
                ));
                break JobOutcome::Completed {
                    output: final_path,
                    plan_index: index,
                    summary: plan.summary.clone(),
                    fallback_reason: fallback_reason.clone(),
                    size,
                    seconds: started.elapsed().as_secs_f64(),
                };
            }
            Some((reason, tail)) => {
                remove_quiet(&tmp);
                logln(&format!("# plan {} failed: {reason}", index + 1));
                let is_nvenc = plan.video.as_ref().and_then(|v| v.backend) == Some(Backend::Nvenc);
                if is_nvenc
                    && reason == "NVENC session limit reached"
                    && req.requeue_on_session_limit
                {
                    break JobOutcome::Requeue { reason };
                }
                if index + 1 < chain_len && !req.plan.pinned {
                    let next = &req.plan.chain[index + 1];
                    let why = if plan.hardware {
                        format!("{} failed: {reason}", plan.badge.trim_start_matches("⚡ "))
                    } else {
                        reason.clone()
                    };
                    sink(JobEvent::FallingBack {
                        reason: why.clone(),
                        next_summary: next.summary.clone(),
                    });
                    fallback_reason.get_or_insert(why);
                    index += 1;
                    continue;
                }
                break JobOutcome::Failed {
                    reason,
                    log_tail: tail,
                    can_retry_software: req.plan.pinned && plan.hardware,
                };
            }
        }
    };
    let _ = std::fs::remove_dir_all(&work_dir);
    outcome
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn temp_paths_are_hidden_and_unique() {
        let a = temp_output_path(Path::new("/out/My Film.mp4"));
        let b = temp_output_path(Path::new("/out/My Film.mp4"));
        assert_ne!(a, b);
        let name = a.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with(".My Film."), "{name}");
        assert!(name.ends_with(".part.mp4"), "{name}");
        assert_eq!(a.parent(), Some(Path::new("/out")));
    }

    #[test]
    fn free_path_never_returns_existing() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("x.mp4");
        assert_eq!(free_path(&p), p);
        std::fs::write(&p, b"").unwrap();
        assert_eq!(free_path(&p), dir.path().join("x (1).mp4"));
    }
}
