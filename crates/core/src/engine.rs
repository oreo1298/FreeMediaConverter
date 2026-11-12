// SPDX-License-Identifier: GPL-3.0-or-later
//! A convenient bundle of everything the UI and CLI need: the ffmpeg
//! installation, its capabilities, the format catalog, container
//! compatibility and the verified hardware inventory.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::catalog::{formats, Compat, FormatInfo};
use crate::error::Result;
use crate::ffmpeg::{Caps, Ffmpeg, MediaKind};
use crate::hw::{self, Inventory, ProbeOptions};
use crate::job::JobRequest;
use crate::media::{self, MediaInfo};
use crate::naming::{self, ConflictPolicy, OutputFolder, Resolution, TemplateVars};
use crate::plan::{self, HwPrefs, JobPlan, PlanContext};
use crate::process::SharedRunner;
use crate::settings::OutputSettings;

/// Shared engine state. Cheap to clone.
#[derive(Clone)]
pub struct Engine {
    pub ff: Ffmpeg,
    pub caps: Caps,
    pub formats: Arc<Vec<FormatInfo>>,
    pub compat: Arc<Compat>,
    pub inventory: Arc<Inventory>,
    pub prefs: HwPrefs,
}

impl Engine {
    /// Open ffmpeg and load capabilities. Hardware starts empty; call
    /// [`Engine::with_inventory`] once probing finished.
    pub fn open(ff: Ffmpeg, compat_cache: Option<PathBuf>) -> Result<Engine> {
        let caps = ff.caps()?;
        let fmts = formats::catalog(&caps, &|m| ff.muxer_help(m));
        let compat = Compat::new(ff.clone(), caps.clone(), compat_cache);
        let inventory = hw::empty_inventory(&ff);
        Ok(Engine {
            ff,
            caps,
            formats: Arc::new(fmts),
            compat: Arc::new(compat),
            inventory: Arc::new(inventory),
            prefs: HwPrefs::default(),
        })
    }

    /// Locate ffmpeg (preferred path, env, bundled, PATH) and open it.
    pub fn discover(preferred: Option<&Path>, runner: SharedRunner) -> Result<Engine> {
        let ff = Ffmpeg::discover(preferred, runner)?;
        Self::open(ff, Some(Compat::default_cache_path()))
    }

    pub fn with_inventory(mut self, inv: Inventory) -> Engine {
        self.inventory = Arc::new(inv);
        self
    }

    /// Probe hardware (or load the cache) for this machine.
    pub fn probe_hardware(
        &self,
        force: bool,
        progress: Option<hw::probe::ProgressFn>,
    ) -> Inventory {
        hw::inventory(
            &self.ff,
            &self.caps,
            Path::new("/"),
            Some(&hw::cache_path()),
            force,
            &ProbeOptions::default(),
            progress,
        )
    }

    pub fn analyze(&self, path: &Path) -> Result<MediaInfo> {
        media::analyze::analyze(&self.ff, path)
    }

    pub fn format(&self, id: &str) -> Option<&FormatInfo> {
        formats::find(&self.formats, id)
    }

    /// Plan a job.
    pub fn plan(&self, media: &MediaInfo, settings: &OutputSettings) -> Result<JobPlan> {
        let compat = self.compat.clone();
        let can_hold = move |f: &FormatInfo, k: MediaKind, c: &str| compat.can_hold(f, k, c);
        let ctx = PlanContext {
            ff: &self.ff,
            caps: &self.caps,
            inventory: &self.inventory,
            formats: &self.formats,
            can_hold: &can_hold,
            prefs: &self.prefs,
        };
        plan::plan(&ctx, media, settings)
    }

    /// True if every stream can be copied into `format` (offer "Instant, lossless").
    pub fn can_remux(&self, media: &MediaInfo, format_id: &str) -> bool {
        let Some(f) = self.format(format_id) else {
            return false;
        };
        let ok = |k: MediaKind, c: &str| self.compat.can_hold(f, k, c);
        media
            .video_streams()
            .all(|s| ok(MediaKind::Video, &s.codec))
            && media
                .audio_streams()
                .all(|s| ok(MediaKind::Audio, &s.codec))
            && media.has_video()
    }

    /// Template variables for an output.
    pub fn template_vars(
        &self,
        media: &MediaInfo,
        settings: &OutputSettings,
        plan: &JobPlan,
        preset: &str,
    ) -> TemplateVars {
        let res = match settings.video_encode().and_then(|v| v.filters.scale) {
            Some(s) => media
                .primary_video()
                .and_then(|v| v.video.as_ref())
                .map(|vi| {
                    let (w, h) =
                        crate::command::fit_size(vi.display_size().0, vi.display_size().1, &s);
                    format!("{}p", w.min(h))
                })
                .unwrap_or_default(),
            None => media
                .primary_video()
                .and_then(|v| v.video.as_ref())
                .map(|v| v.resolution_label())
                .unwrap_or_default(),
        };
        let codec = plan
            .chain
            .first()
            .and_then(|p| p.video.as_ref())
            .map(|v| v.codec.clone())
            .or_else(|| {
                media
                    .primary_video()
                    .filter(|_| plan.video_copy.is_some())
                    .map(|v| v.codec.clone())
            })
            .or_else(|| {
                plan.audio.first().map(|a| match &a.action {
                    crate::plan::AudioAction::Encode { codec, .. } => codec.clone(),
                    crate::plan::AudioAction::Copy => media
                        .streams
                        .iter()
                        .find(|s| s.index == a.input_stream)
                        .map(|s| s.codec.clone())
                        .unwrap_or_default(),
                })
            })
            .unwrap_or_default();
        TemplateVars {
            name: media
                .path
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_else(|| "output".into()),
            ext: plan.format.extension.clone(),
            codec,
            res: if plan.format.kind == crate::catalog::FormatKind::Audio {
                String::new()
            } else {
                res
            },
            date: naming::today(),
            preset: preset.to_string(),
        }
    }

    /// Resolve the output path for a job.
    #[allow(clippy::too_many_arguments)]
    pub fn output_path(
        &self,
        media: &MediaInfo,
        settings: &OutputSettings,
        plan: &JobPlan,
        preset: &str,
        folder: &OutputFolder,
        template: &str,
        policy: ConflictPolicy,
    ) -> Resolution {
        let vars = self.template_vars(media, settings, plan, preset);
        naming::resolve(&media.path, folder, template, &vars, policy, &|p| {
            p.exists()
        })
    }

    /// A job request with sensible defaults.
    pub fn request(
        &self,
        media: MediaInfo,
        settings: OutputSettings,
        plan: JobPlan,
        output: PathBuf,
        overwrite: bool,
    ) -> JobRequest {
        JobRequest {
            ff: self.ff.clone(),
            caps: self.caps.clone(),
            media,
            settings,
            plan,
            output,
            overwrite,
            log_path: None,
            keep_partial: false,
            preserve_timestamps: false,
            start_plan: 0,
            requeue_on_session_limit: false,
        }
    }
}
