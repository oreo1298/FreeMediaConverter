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

/// An encoder the user can pin in Advanced mode.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncoderOption {
    pub encoder: String,
    /// `GpuDevice::id` for hardware encoders tied to a GPU.
    pub device: Option<String>,
    /// "NVENC — NVIDIA GeForce RTX 4090" or "x265 (CPU)".
    pub label: String,
    pub hardware: bool,
    /// Passed the 10-bit probe (hardware) / supports 10-bit (software: true).
    pub ten_bit: bool,
}

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

    /// Default settings for a container chosen in the format picker (may
    /// run a quick test mux for containers without curated rules).
    pub fn settings_for_format(&self, id: &str) -> Option<OutputSettings> {
        let f = self.format(id)?;
        Some(crate::preset::for_format(f, self.compat.defaults(f)))
    }

    /// Codecs of `kind` the container can hold and this FFmpeg can encode.
    pub fn codec_options(&self, format_id: &str, kind: MediaKind) -> Vec<String> {
        match self.format(format_id) {
            Some(f) => self.compat.codecs_for(f, kind),
            None => Vec::new(),
        }
    }

    /// Encoders that can be pinned for `codec`: hardware encoders that passed
    /// a real probe on this machine (one entry per device), then software.
    pub fn encoder_options(&self, codec: &str) -> Vec<EncoderOption> {
        let mut v: Vec<EncoderOption> = Vec::new();
        for e in self
            .inventory
            .probes
            .encoders
            .iter()
            .filter(|e| e.codec == codec && e.works(false))
        {
            v.push(EncoderOption {
                encoder: e.encoder.clone(),
                device: e.device.gpu_id.clone(),
                label: format!("{} — {}", e.backend.label(), e.device.label),
                hardware: true,
                ten_bit: e.works(true),
            });
        }
        let mut sw: Vec<&crate::ffmpeg::CodecEntry> = self
            .caps
            .encoders_for(codec)
            .filter(|e| crate::hw::Backend::from_codec_name(&e.name).is_none())
            .collect();
        // The preferred software encoder first, experimental ones last.
        let best = crate::catalog::best_software_encoder(&self.caps, codec);
        sw.sort_by_key(|e| (Some(&e.name) != best.as_ref(), e.experimental));
        for e in sw {
            let title = crate::plan::encoder_display(&e.name, codec);
            v.push(EncoderOption {
                encoder: e.name.clone(),
                device: None,
                label: format!("{title} (CPU)"),
                hardware: false,
                ten_bit: true,
            });
        }
        v
    }

    /// Switch `current` to another container, keeping the codec choices,
    /// filters, trim and track options the new container can carry.
    pub fn retarget(&self, current: &OutputSettings, format_id: &str) -> Option<OutputSettings> {
        use crate::settings::{AudioSettings, SubtitleSettings, VideoSettings};
        let f = self.format(format_id)?;
        let mut new = crate::preset::for_format(f, self.compat.defaults(f));
        let video_ok = |c: &str| self.compat.can_hold(f, MediaKind::Video, c);
        let audio_ok = |c: &str| self.compat.can_hold(f, MediaKind::Audio, c);
        match (&current.video, &new.video) {
            (VideoSettings::Encode(v), VideoSettings::Encode(_)) if video_ok(&v.codec) => {
                new.video = current.video.clone()
            }
            (VideoSettings::Copy, VideoSettings::Encode(_)) => new.video = VideoSettings::Copy,
            (VideoSettings::Encode(v), VideoSettings::Encode(n)) => {
                // Different codec: keep the picture filters and HDR choice.
                let mut n = n.clone();
                n.filters = v.filters.clone();
                n.hdr = v.hdr;
                new.video = VideoSettings::Encode(n);
            }
            _ => {}
        }
        match (&current.audio, &new.audio) {
            (AudioSettings::Encode(a), AudioSettings::Encode(_)) if audio_ok(&a.codec) => {
                new.audio = current.audio.clone()
            }
            (AudioSettings::Copy, AudioSettings::Encode(_)) => new.audio = AudioSettings::Copy,
            (AudioSettings::Drop, AudioSettings::Encode(_))
                if !matches!(new.video, VideoSettings::Drop) =>
            {
                new.audio = AudioSettings::Drop
            }
            _ => {}
        }
        let keeps_subs = new.subtitles != SubtitleSettings::Drop
            || matches!(current.subtitles, SubtitleSettings::BurnIn { .. });
        if keeps_subs && !matches!(new.video, VideoSettings::Drop) {
            new.subtitles = current.subtitles.clone();
        }
        new.audio_tracks = current.audio_tracks.clone();
        new.subtitle_tracks = current.subtitle_tracks.clone();
        new.metadata = current.metadata;
        new.speed = current.speed;
        new.trim = current.trim;
        new.extra_input_args = current.extra_input_args.clone();
        new.extra_output_args = current.extra_output_args.clone();
        new.default_audio = current.default_audio;
        new.default_subtitle = current.default_subtitle;
        Some(new)
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
