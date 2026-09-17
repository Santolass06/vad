use std::ffi::c_void;
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

/// Function pointer type for OpenGL procedure address lookup (`glXGetProcAddress` / `eglGetProcAddress`).
pub type GlProcAddressFn = Arc<dyn Fn(&str) -> *mut c_void + Send + Sync>;

fn resolve_gl_proc(gpa: &GlProcAddressFn, name: &str) -> *mut c_void {
    gpa(name)
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

        // SAFETY: The static lifetime is erased safely because `_mpv` keeps the Mpv instance
        // alive for the entire lifetime of `VideoRenderContext`.
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
                error!("Caught panic in mpv render update callback: {}", msg);
                if let Some(ref tx) = err_tx {
                    let _ = tx.send(PlayerEvent::Error(format!("Callback panic: {msg}")));
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

    /// Reports swap buffers to mpv for frame timing accuracy.
    pub fn report_swap(&self) {
        self.ctx.report_swap();
    }
}

/// Core player controlling playback and observing mpv events.
#[derive(Clone)]
pub struct Player {
    mpv: Arc<Mpv>,
}

impl Player {
    /// Initializes libmpv with `hwdec=auto-safe` and minimal default configuration.
    pub fn new() -> Result<Self, VadError> {
        let mpv = Mpv::new()?;

        // Configure player options per PLANO_VAD.md §4.3 and §6
        if cfg!(debug_assertions) {
            let _ = mpv.set_property("terminal", "yes");
        }
        mpv.set_property("vo", "libmpv")?;
        mpv.set_property("hwdec", "auto-safe")?;
        mpv.set_property("keep-open", "yes")?;
        mpv.set_property("video-timing-offset", 0.0_f64)?;

        info!("Player initialized with vo=libmpv and hwdec=auto-safe");
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

    /// Loads and opens a media file (replaces current playback).
    pub fn load_file(&self, path: &str) -> Result<(), VadError> {
        info!("Loading file: {}", path);
        self.mpv
            .command("loadfile", &[path, "replace"])
            .map_err(VadError::Mpv)?;
        Ok(())
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

    /// Sets playback volume in range 0.0..=100.0.
    pub fn set_volume(&self, volume: f64) -> Result<(), VadError> {
        let vol = volume.clamp(0.0, 100.0);
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
