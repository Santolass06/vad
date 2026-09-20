use std::ffi::c_void;
use std::path::Path;
use std::sync::Arc;
use std::thread;

use libmpv2::{
    events::{Event, PropertyData},
    mpv_error,
    render::{OpenGLInitParams, RenderContext, RenderParam, RenderParamApiType},
    Format, GetData, Mpv,
};
use tracing::{debug, error, info, trace, warn};

use crate::error::VadError;
use crate::state::{EventSender, PlaybackState, PlayerEvent, SharedPlayerState};
use crate::util::{is_allowed_url_scheme, vad_mpv_config_dir};

/// Function pointer type for OpenGL procedure address lookup (`glXGetProcAddress` / `eglGetProcAddress`).
pub type GlProcAddressFn = Arc<dyn Fn(&str) -> *mut c_void + Send + Sync>;

fn resolve_gl_proc(gpa: &GlProcAddressFn, name: &str) -> *mut c_void {
    gpa(name)
}

/// Information about an audio or subtitle track.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackInfo {
    pub id: i64,
    pub title: Option<String>,
    pub lang: Option<String>,
    pub is_selected: bool,
}

/// Information about an audio output device.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub name: String,
    pub description: String,
}

/// Status of A-B repeat loop.
#[derive(Debug, Clone, PartialEq)]
pub enum AbLoopStatus {
    Off,
    AOnly(f64),
    Looping { a: f64, b: f64 },
}

/// Thin safe wrapper around `mpv_render_context`.
/// The OpenGL context MUST be current on the thread that calls `render` and `update` (§4.3).
pub struct VideoRenderContext {
    ctx: RenderContext<'static>,
    _mpv: Arc<Mpv>,
}

impl VideoRenderContext {
    pub fn new(mpv: Arc<Mpv>, gpa: GlProcAddressFn) -> Result<Self, VadError> {
        let render_ctx = mpv.create_render_context([
            RenderParam::ApiType(RenderParamApiType::OpenGl),
            RenderParam::InitParams(OpenGLInitParams {
                get_proc_address: resolve_gl_proc,
                ctx: Arc::clone(&gpa),
            }),
        ])?;

        // SAFETY: `RenderContext<'a>`'s lifetime is only a borrow-check marker
        // (`PhantomData<&'a Mpv>`); the raw pointer it wraps has no lifetime
        // dependency of its own. Erasing it to 'static is sound only because
        // `ctx` is declared BEFORE `_mpv` below — Rust drops fields in
        // declaration order, so `ctx` (and mpv_render_context_free) always
        // runs before `_mpv`'s Arc<Mpv> can be dropped. Reordering these two
        // fields would silently turn this into a use-after-free.
        let ctx: RenderContext<'static> = unsafe { std::mem::transmute(render_ctx) };
        Ok(Self { ctx, _mpv: mpv })
    }

    /// Set the update callback invoked by mpv when a new video frame is ready.
    /// The callback execution is wrapped in `std::panic::catch_unwind` (§4.24)
    /// so that any panic across the C->Rust FFI boundary is caught and converted
    /// into a `VadError::RenderCallbackPanic` instead of aborting the process.
    pub fn set_update_callback<F>(&mut self, callback: F, err_tx: Option<EventSender>)
    where
        F: Fn() + Send + 'static,
    {
        self.ctx.set_update_callback(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                callback();
            }));
            if let Err(payload) = result {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic in mpv render update callback".to_string()
                };
                let err = VadError::RenderCallbackPanic(msg);
                error!("Caught panic in mpv render update callback: {}", err);
                if let Some(ref tx) = err_tx {
                    let _ = tx.send(PlayerEvent::Error(err.to_string()));
                }
            }
        });
    }

    /// Invokes mpv_render_context_update to poll pending render flags (e.g. MPV_RENDER_UPDATE_FRAME).
    pub fn update(&self) -> Result<u32, VadError> {
        self.ctx.update().map_err(VadError::Mpv)
    }

    /// Renders video frame into specified OpenGL FBO.
    /// `flip` is set to true because OpenGL has positive Y upwards.
    pub fn render(&self, fbo: i32, width: i32, height: i32) -> Result<(), VadError> {
        self.ctx
            .render::<()>(fbo, width, height, true)
            .map_err(VadError::Mpv)
    }

}

/// Core player controlling playback and observing mpv events.
#[derive(Clone)]
pub struct Player {
    mpv: Arc<Mpv>,
}

impl Player {
    /// Initializes libmpv with isolated config-dir, --no-config, yt-dlp enabled, and hwdec=auto-safe.
    pub fn new() -> Result<Self, VadError> {
        Self::with_options(None)
    }

    /// Initializes libmpv with an optional custom isolated config directory.
    /// Explicitly sets:
    /// 1. `config-dir` to VAD's isolated path (never `~/.config/mpv`, §3)
    /// 2. `config=no` (--no-config) to prevent reading user configs
    /// 3. `load-scripts=yes` to enable internal lua scripts like `ytdl_hook`
    /// 4. `ytdl=yes` to enable yt-dlp URL playback
    /// 5. `network-timeout=30` explicit timeout for network streams (§4.22)
    pub fn with_options(custom_config_dir: Option<&Path>) -> Result<Self, VadError> {
        let vad_dir = match custom_config_dir {
            Some(p) => p.to_path_buf(),
            None => vad_mpv_config_dir(),
        };
        let _ = std::fs::create_dir_all(&vad_dir);

        let mpv = Mpv::with_initializer(|init| {
            let dir_str = vad_dir.to_string_lossy();
            init.set_option("config-dir", dir_str.as_ref())?;
            init.set_option("config", false)?;
            init.set_option("load-scripts", true)?;
            init.set_option("ytdl", true)?;
            init.set_option("network-timeout", 30_i64)?;
            Ok(())
        })
        .map_err(|e| VadError::PlayerInitFailed(format!("{e:?}")))?;

        // Configure player options per PLANO_VAD.md §4.3 and §6
        if cfg!(debug_assertions) {
            let _ = mpv.set_property("terminal", "yes");
        }
        mpv.set_property("vo", "libmpv")?;
        mpv.set_property("hwdec", "auto-safe")?;
        mpv.set_property("keep-open", "yes")?;
        mpv.set_property("video-timing-offset", 0.0_f64)?;
        mpv.set_property("volume-max", 200.0_f64)?;

        info!(
            "Player initialized with vo=libmpv, hwdec=auto-safe, volume-max=200, isolated config-dir={:?}, --no-config, ytdl=true",
            vad_dir
        );
        Ok(Self { mpv: Arc::new(mpv) })
    }

    /// Returns a reference to the underlying `Mpv` instance.
    pub fn mpv(&self) -> &Mpv {
        &self.mpv
    }

    /// Creates a `VideoRenderContext` using the provided OpenGL procedure address resolver.
    pub fn create_render_context(&self, gpa: GlProcAddressFn) -> Result<VideoRenderContext, VadError> {
        VideoRenderContext::new(Arc::clone(&self.mpv), gpa)
    }

    /// Defensive property getter (§4.32):
    /// Distros with different libmpv versions may not have all properties (`hwdec-current`, etc.).
    /// Returns `Ok(None)` if `MPV_ERROR_PROPERTY_NOT_FOUND` or `MPV_ERROR_PROPERTY_UNAVAILABLE` occurs.
    pub fn get_property_optional<T: GetData>(&self, name: &str) -> Result<Option<T>, VadError> {
        match self.mpv.get_property::<T>(name) {
            Ok(val) => Ok(Some(val)),
            Err(libmpv2::Error::Raw(code))
                if code == mpv_error::PropertyNotFound || code == mpv_error::PropertyUnavailable =>
            {
                trace!("Property '{}' not found or unavailable (treated as None)", name);
                Ok(None)
            }
            Err(err) => Err(VadError::Mpv(err)),
        }
    }

    /// Reads real hardware decoder in use. Returns `None` if software decode (`SW (CPU)`) is active.
    pub fn hwdec_current(&self) -> Result<Option<String>, VadError> {
        let opt = self.get_property_optional::<String>("hwdec-current")?;
        match opt {
            Some(s) if s.is_empty() || s == "no" => Ok(None),
            Some(s) => Ok(Some(s)),
            None => Ok(None),
        }
    }

    /// Dynamically sets the `hwdec` property (e.g. "auto-safe" or "no" for forced SW fallback testing).
    pub fn set_hwdec(&self, mode: &str) -> Result<(), VadError> {
        self.mpv
            .set_property("hwdec", mode)
            .map_err(VadError::Mpv)?;
        info!("Set mpv property hwdec='{}'", mode);
        Ok(())
    }

    /// Loads and opens a media file or URL (replaces current playback).
    /// If the target is an online URL, validates against the allowlisted schemes (§4.27).
    pub fn load_file(&self, path: &str) -> Result<(), VadError> {
        let trimmed = path.trim();
        if trimmed.contains("://") && !is_allowed_url_scheme(trimmed) {
            return Err(VadError::InvalidUrlScheme(format!(
                "Esquema de URL não permitido: '{trimmed}'. Apenas http://, https:// e rtsp:// são suportados (§4.27)."
            )));
        }

        info!("Loading media: {}", trimmed);
        self.mpv
            .command("loadfile", &[trimmed, "replace"])
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Explicitly loads an online stream URL with scheme validation (§4.27).
    pub fn load_url(&self, url: &str) -> Result<(), VadError> {
        self.load_file(url)
    }

    /// Starts or resumes playback.
    pub fn play(&self) -> Result<(), VadError> {
        self.mpv.set_property("pause", false).map_err(VadError::Mpv)
    }

    /// Pauses playback.
    pub fn pause(&self) -> Result<(), VadError> {
        self.mpv.set_property("pause", true).map_err(VadError::Mpv)
    }

    /// Toggles playback status between play and pause.
    pub fn toggle_pause(&self) -> Result<(), VadError> {
        let paused = self.is_paused()?;
        self.mpv.set_property("pause", !paused).map_err(VadError::Mpv)
    }

    /// Queries whether playback is paused.
    pub fn is_paused(&self) -> Result<bool, VadError> {
        Ok(self.get_property_optional::<bool>("pause")?.unwrap_or(true))
    }

    /// Stops playback and unloads current file/playlist.
    pub fn stop(&self) -> Result<(), VadError> {
        self.mpv.command("stop", &[]).map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Relative seek in seconds (positive forward, negative backward).
    pub fn seek_relative(&self, delta_secs: f64) -> Result<(), VadError> {
        let delta_str = format!("{}", delta_secs);
        self.mpv
            .command("seek", &[&delta_str, "relative"])
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Absolute seek in seconds.
    pub fn seek_absolute(&self, target_secs: f64) -> Result<(), VadError> {
        let target_str = format!("{}", target_secs.max(0.0));
        self.mpv
            .command("seek", &[&target_str, "absolute"])
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Sets playback volume in range 0.0..=200.0 (supports volume boost).
    pub fn set_volume(&self, volume: f64) -> Result<(), VadError> {
        let vol = volume.clamp(0.0, 200.0);
        self.mpv.set_property("volume", vol).map_err(VadError::Mpv)
    }

    /// Gets current volume in percent.
    pub fn volume(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("volume")?.unwrap_or(100.0))
    }

    /// Toggles mute state.
    pub fn toggle_mute(&self) -> Result<(), VadError> {
        let muted = self.is_muted()?;
        self.mpv.set_property("mute", !muted).map_err(VadError::Mpv)
    }

    /// Queries whether audio is muted.
    pub fn is_muted(&self) -> Result<bool, VadError> {
        Ok(self.get_property_optional::<bool>("mute")?.unwrap_or(false))
    }

    /// Gets current playback position in seconds.
    pub fn time_pos(&self) -> Result<Option<f64>, VadError> {
        self.get_property_optional::<f64>("time-pos")
    }

    /// Gets total media duration in seconds.
    pub fn duration(&self) -> Result<Option<f64>, VadError> {
        self.get_property_optional::<f64>("duration")
    }

    /// Gets current playback speed factor (1.0 = normal).
    pub fn speed(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("speed")?.unwrap_or(1.0))
    }

    /// Sets playback speed factor (e.g. 0.5 to 2.0).
    pub fn set_speed(&self, speed: f64) -> Result<(), VadError> {
        let clamped = speed.clamp(0.1, 8.0);
        self.mpv.set_property("speed", clamped).map_err(VadError::Mpv)
    }

    /// Takes a video frame screenshot using mpv's native screenshot engine.
    pub fn take_screenshot(&self) -> Result<(), VadError> {
        info!("Capturing video frame screenshot");
        self.mpv
            .command("screenshot", &["video"])
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Queries the current A-B loop status.
    pub fn ab_loop_status(&self) -> Result<AbLoopStatus, VadError> {
        let a_str = self.get_property_optional::<String>("ab-loop-a")?;
        let b_str = self.get_property_optional::<String>("ab-loop-b")?;

        let a_val = a_str.and_then(|s| if s == "no" { None } else { s.parse::<f64>().ok() });
        let b_val = b_str.and_then(|s| if s == "no" { None } else { s.parse::<f64>().ok() });

        match (a_val, b_val) {
            (Some(a), Some(b)) => Ok(AbLoopStatus::Looping { a, b }),
            (Some(a), None) => Ok(AbLoopStatus::AOnly(a)),
            _ => Ok(AbLoopStatus::Off),
        }
    }

    /// Cycles A-B loop state: Off -> Point A set -> Point B set (looping) -> Off.
    pub fn cycle_ab_loop(&self) -> Result<AbLoopStatus, VadError> {
        self.mpv.command("ab-loop", &[]).map_err(VadError::Mpv)?;
        self.ab_loop_status()
    }

    /// Returns list of available audio tracks.
    pub fn audio_tracks(&self) -> Result<Vec<TrackInfo>, VadError> {
        self.query_tracks_by_type("audio")
    }

    /// Returns list of available subtitle tracks.
    pub fn subtitle_tracks(&self) -> Result<Vec<TrackInfo>, VadError> {
        self.query_tracks_by_type("sub")
    }

    /// Selects an audio track by ID, or disables audio if `None`.
    pub fn set_audio_track(&self, id: Option<i64>) -> Result<(), VadError> {
        let val = match id {
            Some(track_id) => format!("{track_id}"),
            None => "no".to_string(),
        };
        self.mpv.set_property("aid", val.as_str()).map_err(VadError::Mpv)
    }

    /// Selects a subtitle track by ID, or disables subtitles if `None`.
    pub fn set_subtitle_track(&self, id: Option<i64>) -> Result<(), VadError> {
        let val = match id {
            Some(track_id) => format!("{track_id}"),
            None => "no".to_string(),
        };
        self.mpv.set_property("sid", val.as_str()).map_err(VadError::Mpv)
    }

    fn query_tracks_by_type(&self, track_type: &str) -> Result<Vec<TrackInfo>, VadError> {
        let count = self.get_property_optional::<i64>("track-list/count")?.unwrap_or(0);
        let mut tracks = Vec::new();

        for i in 0..count {
            let t_type: Option<String> = self.get_property_optional(&format!("track-list/{i}/type"))?;
            if t_type.as_deref() == Some(track_type) {
                let id = self
                    .get_property_optional::<i64>(&format!("track-list/{i}/id"))?
                    .unwrap_or(i + 1);
                let title = self.get_property_optional::<String>(&format!("track-list/{i}/title"))?;
                let lang = self.get_property_optional::<String>(&format!("track-list/{i}/lang"))?;
                let is_selected = self
                    .get_property_optional::<bool>(&format!("track-list/{i}/selected"))?
                    .unwrap_or(false);

                tracks.push(TrackInfo {
                    id,
                    title,
                    lang,
                    is_selected,
                });
            }
        }
        Ok(tracks)
    }

    /// Returns current video aspect ratio override as f64 (values <= 0.0 represent Auto).
    pub fn video_aspect_override(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("video-aspect-override")?.unwrap_or(-1.0))
    }

    /// Sets video aspect ratio override (e.g. "-1", "16:9", "4:3", "21:9").
    pub fn set_video_aspect_override(&self, aspect: &str) -> Result<(), VadError> {
        self.mpv
            .set_property("video-aspect-override", aspect)
            .map_err(VadError::Mpv)
    }

    /// Returns current video rotation angle in degrees (0, 90, 180, 270).
    pub fn video_rotate(&self) -> Result<i64, VadError> {
        Ok(self.get_property_optional::<i64>("video-rotate")?.unwrap_or(0))
    }

    /// Sets video rotation angle in degrees (0, 90, 180, 270).
    pub fn set_video_rotate(&self, degrees: i64) -> Result<(), VadError> {
        self.mpv
            .set_property("video-rotate", degrees)
            .map_err(VadError::Mpv)
    }

    /// Returns current video crop geometry string.
    pub fn video_crop(&self) -> Result<Option<String>, VadError> {
        self.get_property_optional::<String>("video-crop")
    }

    /// Sets video crop geometry string (e.g. "" to disable or "16:9", "4:3", "WxH+X+Y").
    pub fn set_video_crop(&self, crop: &str) -> Result<(), VadError> {
        self.mpv
            .set_property("video-crop", crop)
            .map_err(VadError::Mpv)
    }

    /// Checks if the currently playing media has an active video track.
    pub fn has_video(&self) -> bool {
        self.get_property_optional::<String>("video-format")
            .ok()
            .flatten()
            .is_some_and(|fmt| !fmt.trim().is_empty())
    }

    /// Gets current panscan value (0.0 = original aspect, 1.0 = pan-and-scan / fill).
    pub fn panscan(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("panscan")?.unwrap_or(0.0))
    }

    /// Sets panscan value (0.0 to 1.0).
    pub fn set_panscan(&self, val: f64) -> Result<(), VadError> {
        self.mpv
            .set_property("panscan", val.clamp(0.0, 1.0))
            .map_err(VadError::Mpv)
    }

    /// Gets audio delay in seconds.
    pub fn audio_delay(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("audio-delay")?.unwrap_or(0.0))
    }

    /// Sets audio delay in seconds (e.g. 0.1 for +100ms, -0.05 for -50ms).
    pub fn set_audio_delay(&self, delay_secs: f64) -> Result<(), VadError> {
        self.mpv
            .set_property("audio-delay", delay_secs)
            .map_err(VadError::Mpv)
    }

    /// Gets subtitle delay in seconds.
    pub fn sub_delay(&self) -> Result<f64, VadError> {
        Ok(self.get_property_optional::<f64>("sub-delay")?.unwrap_or(0.0))
    }

    /// Sets subtitle delay in seconds.
    pub fn set_sub_delay(&self, delay_secs: f64) -> Result<(), VadError> {
        self.mpv
            .set_property("sub-delay", delay_secs)
            .map_err(VadError::Mpv)
    }

    /// Gets subtitle visibility.
    pub fn sub_visibility(&self) -> Result<bool, VadError> {
        Ok(self.get_property_optional::<bool>("sub-visibility")?.unwrap_or(true))
    }

    /// Toggles or sets subtitle visibility.
    pub fn set_sub_visibility(&self, visible: bool) -> Result<(), VadError> {
        self.mpv
            .set_property("sub-visibility", visible)
            .map_err(VadError::Mpv)
    }

    /// Loads external subtitle file using mpv sub-add.
    pub fn load_subtitles(&self, path: &str) -> Result<(), VadError> {
        self.mpv
            .command("sub-add", &[path, "select"])
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Gets color adjustment properties: (brightness, contrast, saturation, gamma) in range -100..=100.
    pub fn color_adjustments(&self) -> Result<(i64, i64, i64, i64), VadError> {
        let b = self.get_property_optional::<i64>("brightness")?.unwrap_or(0);
        let c = self.get_property_optional::<i64>("contrast")?.unwrap_or(0);
        let s = self.get_property_optional::<i64>("saturation")?.unwrap_or(0);
        let g = self.get_property_optional::<i64>("gamma")?.unwrap_or(0);
        Ok((b, c, s, g))
    }

    /// Sets color adjustment properties in range -100..=100.
    pub fn set_color_adjustments(&self, b: i64, c: i64, s: i64, g: i64) -> Result<(), VadError> {
        self.mpv
            .set_property("brightness", b.clamp(-100, 100))
            .map_err(VadError::Mpv)?;
        self.mpv
            .set_property("contrast", c.clamp(-100, 100))
            .map_err(VadError::Mpv)?;
        self.mpv
            .set_property("saturation", s.clamp(-100, 100))
            .map_err(VadError::Mpv)?;
        self.mpv
            .set_property("gamma", g.clamp(-100, 100))
            .map_err(VadError::Mpv)?;
        Ok(())
    }

    /// Resets color adjustments to neutral 0.
    pub fn reset_color_adjustments(&self) -> Result<(), VadError> {
        self.set_color_adjustments(0, 0, 0, 0)
    }

    /// Returns the list of audio devices reported by mpv (`audio-device-list`).
    pub fn audio_devices(&self) -> Result<Vec<AudioDevice>, VadError> {
        let count = self.get_property_optional::<i64>("audio-device-list/count")?.unwrap_or(0);
        let mut devices = Vec::new();
        for i in 0..count {
            let name = self
                .get_property_optional::<String>(&format!("audio-device-list/{i}/name"))?
                .unwrap_or_default();
            let description = self
                .get_property_optional::<String>(&format!("audio-device-list/{i}/description"))?
                .unwrap_or_else(|| name.clone());
            devices.push(AudioDevice { name, description });
        }
        Ok(devices)
    }

    /// Returns the currently active audio device name (e.g. "auto", "pipewire", etc.).
    pub fn audio_device(&self) -> Result<String, VadError> {
        Ok(self.get_property_optional::<String>("audio-device")?.unwrap_or_else(|| "auto".to_string()))
    }

    /// Sets the active audio device by name.
    pub fn set_audio_device(&self, name: &str) -> Result<(), VadError> {
        self.mpv
            .set_property("audio-device", name)
            .map_err(VadError::Mpv)
    }

    /// Sets the audio filter chain (`af`) combining the 10-band equalizer and RNNoise (`arnndn`).
    /// Frequencies: 32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000 Hz.
    pub fn set_audio_filters(&self, eq_gains: &[f64; 10], rnnoise: bool) -> Result<(), VadError> {
        const FREQS: [u32; 10] = [32, 64, 125, 250, 500, 1000, 2000, 4000, 8000, 16000];
        let mut parts = Vec::new();

        // Check if any EQ gain differs from 0.0 dB
        let has_eq = eq_gains.iter().any(|&g| g.abs() > 0.01);
        if has_eq {
            for (f, &g) in FREQS.iter().zip(eq_gains.iter()) {
                parts.push(format!("lavfi=[equalizer=f={f}:width_type=o:w=1:g={g:.1}]"));
            }
        }

        if rnnoise {
            parts.push("lavfi=[arnndn]".to_string());
        }

        let af_string = parts.join(",");
        self.mpv.set_property("af", af_string.as_str()).map_err(VadError::Mpv)
    }

    /// Spawns a background thread listening for mpv events and updating `SharedPlayerState`.
    /// High-frequency `time-pos` updates write directly to `AtomicU64` without triggering
    /// channel messages or breaking reactive sleep (§5).
    pub fn start_event_loop(
        &self,
        event_sender: EventSender,
        shared_state: Arc<SharedPlayerState>,
    ) -> Result<thread::JoinHandle<()>, VadError> {
        let client = self
            .mpv
            .create_client(Some("vad_event_listener"))
            .map_err(VadError::Mpv)?;

        // Property IDs
        const PROP_TIME_POS: u64 = 1;
        const PROP_PAUSE: u64 = 2;
        const PROP_DURATION: u64 = 3;
        const PROP_HWDEC_CURRENT: u64 = 4;
        const PROP_VOLUME: u64 = 5;
        const PROP_MUTE: u64 = 6;

        let _ = client.observe_property("time-pos", Format::Double, PROP_TIME_POS);
        let _ = client.observe_property("pause", Format::Flag, PROP_PAUSE);
        let _ = client.observe_property("duration", Format::Double, PROP_DURATION);
        let _ = client.observe_property("hwdec-current", Format::String, PROP_HWDEC_CURRENT);
        let _ = client.observe_property("volume", Format::Double, PROP_VOLUME);
        let _ = client.observe_property("mute", Format::Flag, PROP_MUTE);

        let handle = thread::Builder::new()
            .name("vad-mpv-events".to_string())
            .spawn(move || {
                debug!("mpv background event loop started");
                loop {
                    // Wait up to 250ms for events
                    let event = client.wait_event(0.25);
                    let Some(event_res) = event else {
                        continue;
                    };

                    match event_res {
                        Ok(Event::Shutdown) => {
                            info!("mpv shutdown event received, terminating event loop");
                            break;
                        }
                        Ok(Event::PropertyChange {
                            reply_userdata,
                            change,
                            ..
                        }) => match reply_userdata {
                            PROP_TIME_POS => {
                                if let PropertyData::Double(pos) = change {
                                    // High-frequency update: write atomic ONLY, NO channel event
                                    shared_state.set_time_pos(pos);
                                }
                            }
                            PROP_PAUSE => {
                                if let PropertyData::Flag(paused) = change {
                                    shared_state.set_paused(paused);
                                    let state = if paused {
                                        PlaybackState::Paused
                                    } else {
                                        PlaybackState::Playing
                                    };
                                    let _ = event_sender
                                        .send(PlayerEvent::PlaybackStateChanged(state));
                                }
                            }
                            PROP_DURATION => {
                                if let PropertyData::Double(dur) = change {
                                    shared_state.set_duration(dur);
                                }
                            }
                            PROP_HWDEC_CURRENT => {
                                if let PropertyData::Str(s) = change {
                                    let hw = if s.is_empty() || s == "no" {
                                        None
                                    } else {
                                        Some(s.to_string())
                                    };
                                    let _ = event_sender.send(PlayerEvent::HwdecChanged(hw));
                                }
                            }
                            PROP_VOLUME => {
                                if let PropertyData::Double(vol) = change {
                                    let _ = event_sender.send(PlayerEvent::VolumeChanged(vol));
                                }
                            }
                            PROP_MUTE => {
                                if let PropertyData::Flag(muted) = change {
                                    let _ = event_sender.send(PlayerEvent::MutedChanged(muted));
                                }
                            }
                            _ => {}
                        },
                        Ok(Event::FileLoaded) => {
                            let title = client.get_property::<String>("media-title").ok();
                            let path = client.get_property::<String>("path").unwrap_or_default();
                            let dur = client.get_property::<f64>("duration").ok();
                            if let Some(dur_val) = dur {
                                shared_state.set_duration(dur_val);
                            }
                            let _ = event_sender.send(PlayerEvent::FileLoaded {
                                path,
                                title,
                                duration: dur,
                            });
                        }
                        Ok(Event::EndFile(_reason)) => {
                            let _ = event_sender.send(PlayerEvent::EndOfFile);
                            let _ = event_sender
                                .send(PlayerEvent::PlaybackStateChanged(PlaybackState::Idle));
                        }
                        Ok(Event::Seek) => {
                            let pos = shared_state.get_time_pos();
                            let _ = event_sender.send(PlayerEvent::SeekOccurred(pos));
                        }
                        Err(e) => {
                            warn!("mpv event error: {:?}", e);
                        }
                        _ => {}
                    }
                }
                debug!("mpv background event loop exited");
            })
            .map_err(|e| VadError::Playback(format!("Failed to spawn event loop thread: {e}")))?;

        Ok(handle)
    }
}
