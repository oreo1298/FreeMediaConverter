// SPDX-License-Identifier: GPL-3.0-or-later
//! Queue items and their persistence (independent of widgets).

use std::path::{Path, PathBuf};

use retort_core::job::JobHandle;
use retort_core::media::MediaInfo;
use retort_core::progress::Progress;
use retort_core::settings::OutputSettings;
use serde::{Deserialize, Serialize};

/// Lifecycle of a queue item.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum Status {
    Analyzing,
    /// Analysed and waiting for "Convert".
    Ready,
    /// Waiting for a free slot in a running batch.
    Queued,
    Running,
    Paused,
    Done {
        output: PathBuf,
        size: u64,
        /// Set when a hardware path failed and a fallback finished the job.
        fallback: Option<String>,
    },
    Failed {
        reason: String,
        /// The encoder was pinned: offer "Retry with software".
        retry_software: bool,
    },
    Cancelled,
    Skipped {
        reason: String,
    },
    /// ffprobe could not read the file.
    Unreadable {
        reason: String,
    },
}

impl Status {
    pub fn is_active(&self) -> bool {
        matches!(self, Status::Running | Status::Paused | Status::Queued)
    }
    pub fn is_finished(&self) -> bool {
        matches!(
            self,
            Status::Done { .. } | Status::Failed { .. } | Status::Cancelled | Status::Skipped { .. }
        )
    }
    /// Can be (re)started by "Convert".
    pub fn is_convertible(&self) -> bool {
        matches!(self, Status::Ready | Status::Failed { .. } | Status::Cancelled)
    }
}

/// Live progress of a running item.
#[derive(Debug, Clone, Copy, Default)]
pub struct Live {
    pub fraction: Option<f64>,
    pub eta: Option<f64>,
    pub progress: Progress,
}

/// One file in the queue.
pub struct Item {
    pub id: u64,
    pub path: PathBuf,
    pub media: Option<MediaInfo>,
    pub settings: OutputSettings,
    pub preset: String,
    pub status: Status,
    pub summary: Option<String>,
    pub badge: Option<String>,
    pub hardware: bool,
    pub notes: Vec<String>,
    pub command: Option<String>,
    pub log: Vec<String>,
    pub live: Live,
    pub handle: Option<JobHandle>,
    /// Start from this plan index next time (Retry with software).
    pub start_plan: Option<usize>,
    pub thumbnail: Option<gtk::gdk::Texture>,
    pub peaks: Option<Vec<f32>>,
}

impl Item {
    pub fn new(id: u64, path: PathBuf, settings: OutputSettings, preset: String) -> Item {
        Item {
            id,
            path,
            media: None,
            settings,
            preset,
            status: Status::Analyzing,
            summary: None,
            badge: None,
            hardware: false,
            notes: Vec::new(),
            command: None,
            log: Vec::new(),
            live: Live::default(),
            handle: None,
            start_plan: None,
            thumbnail: None,
            peaks: None,
        }
    }

    pub fn file_name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| self.path.display().to_string())
    }

    pub fn push_log(&mut self, line: String) {
        self.log.push(line);
        if self.log.len() > 2000 {
            self.log.drain(..500);
        }
    }
}

/// What we persist per item.
#[derive(Serialize, Deserialize)]
struct Saved {
    path: PathBuf,
    settings: OutputSettings,
    preset: String,
    status: Status,
}

#[derive(Serialize, Deserialize, Default)]
struct SavedQueue {
    version: u32,
    items: Vec<Saved>,
}

pub fn queue_file() -> PathBuf {
    retort_core::paths::state_dir().join("queue.json")
}

/// Persist the queue (atomically). Running/queued items are saved as Ready.
pub fn save(items: &[Item], path: &Path) {
    let q = SavedQueue {
        version: 1,
        items: items
            .iter()
            .map(|i| Saved {
                path: i.path.clone(),
                settings: i.settings.clone(),
                preset: i.preset.clone(),
                status: match &i.status {
                    Status::Running | Status::Paused | Status::Queued | Status::Analyzing => Status::Ready,
                    s => s.clone(),
                },
            })
            .collect(),
    };
    if let Some(d) = path.parent() {
        let _ = std::fs::create_dir_all(d);
    }
    if let Ok(json) = serde_json::to_vec_pretty(&q) {
        let tmp = path.with_extension("json.tmp");
        if std::fs::write(&tmp, json).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }
}

/// Load the persisted queue: (path, settings, preset, status). Files that no
/// longer exist are dropped.
pub fn load(path: &Path) -> Vec<(PathBuf, OutputSettings, String, Status)> {
    let Ok(bytes) = std::fs::read(path) else { return Vec::new() };
    let Ok(q) = serde_json::from_slice::<SavedQueue>(&bytes) else { return Vec::new() };
    q.items
        .into_iter()
        .filter(|s| s.path.exists())
        .map(|s| (s.path, s.settings, s.preset, s.status))
        .collect()
}

/// Media file extensions picked up when a folder is added.
pub const DEFAULT_EXTENSIONS: &str = "mp4,m4v,mkv,mov,avi,webm,mpg,mpeg,ts,m2ts,mts,wmv,flv,3gp,ogv,mxf,vob,dv,\
mp3,m4a,aac,flac,wav,ogg,oga,opus,wma,aiff,aif,ape,wv,mka,ac3,eac3,dts,amr,gif,webp,apng,avif";

/// Expand folders recursively (filtering by extension) and keep files as-is.
pub fn expand_inputs(paths: Vec<PathBuf>, extensions: &str) -> Vec<PathBuf> {
    let exts: Vec<String> = extensions
        .split(',')
        .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
        .filter(|e| !e.is_empty())
        .collect();
    let mut out = Vec::new();
    fn walk(dir: &Path, exts: &[String], out: &mut Vec<PathBuf>, depth: usize) {
        if depth > 32 {
            return;
        }
        let Ok(rd) = std::fs::read_dir(dir) else { return };
        let mut entries: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        entries.sort();
        for p in entries {
            let hidden = p
                .file_name()
                .map(|n| n.to_string_lossy().starts_with('.'))
                .unwrap_or(false);
            if hidden {
                continue;
            }
            if p.is_dir() {
                walk(&p, exts, out, depth + 1);
            } else if p
                .extension()
                .map(|e| exts.iter().any(|x| *x == e.to_string_lossy().to_ascii_lowercase()))
                .unwrap_or(false)
            {
                out.push(p);
            }
        }
    }
    for p in paths {
        if p.is_dir() {
            walk(&p, &exts, &mut out, 0);
        } else {
            out.push(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_folders_with_filter() {
        let d = tempfile::tempdir().unwrap();
        std::fs::create_dir_all(d.path().join("a/b")).unwrap();
        for f in ["a/x.mkv", "a/b/y.MP3", "a/notes.txt", "a/.hidden.mp4", "z.mov"] {
            std::fs::write(d.path().join(f), b"").unwrap();
        }
        let got = expand_inputs(vec![d.path().join("a"), d.path().join("z.mov")], "mkv,mp3");
        let names: Vec<_> = got.iter().map(|p| p.file_name().unwrap().to_string_lossy().into_owned()).collect();
        assert_eq!(names, ["y.MP3", "x.mkv", "z.mov"]);
    }
}
