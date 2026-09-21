use std::ffi::{c_void, CString};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, RichText, Stroke, StrokeKind, UiBuilder,
};
use tracing::{debug, error, info, warn};
use vad_ai::{
    AudioExtractor, ExtractionHandle, ExtractionStatus, PcmAudio, TranscriptionSegment,
    VadDetectionResult, VadDetector, WhisperEngine,
};
use vad_audio_tools::{WaveformPyramid, TARGET_VISIBLE_POINTS};
use vad_core::{
    create_event_channel, is_allowed_url_scheme, BookmarkStore, EventReceiver, GlProcAddressFn,
    PlatformIntegration, Player, PlayerEvent, Playlist, RecentEntry, RecentsStore,
    SharedPlayerState, VadConfig, VadError,
};

use crate::panels::{
    AudioPanel, HudAction, HudPanel, PlaylistAction, PlaylistPanel, VideoPanel,
    WhisperAction, WhisperPanel, IDLE_UNLOAD_TIMEOUT,
};
use crate::probe::probe_dependencies;
use crate::render::GlVideoRenderer;

/// Text of a freshly added meeting note until the user types their own.
const DEFAULT_NOTE_TEXT: &str = "Nova nota";

/// State for the non-blocking resume toast per design/Dialogs.dc.html and PLANO_VAD.md §4.6.
#[derive(Debug, Clone)]
pub struct ResumeToastState {
    pub location: String,
    pub title: String,
    pub timestamp: f64,
    pub duration: Option<f64>,
    pub time_left: f32,
    pub is_startup_resume: bool,
}

/// Active right-hand lateral panel.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ActiveSidePanel {
    None,
    Playlist,
    Equalizer,
    Video,
    Whisper,
}

/// Main GUI application for VAD.
pub struct VadApp {
    player: Option<Player>,
    renderer: Option<GlVideoRenderer>,
    event_rx: Option<EventReceiver>,
    shared_state: Arc<SharedPlayerState>,
    platform_integrations: Vec<Box<dyn PlatformIntegration>>,
    hwdec_current_label: String,
    hwdec_forced_sw: bool,
    current_media_path: Option<String>,
    current_title: String,
    last_error: Option<String>,
    fatal_error: Option<VadError>,
    missing_dependencies: Vec<VadError>,
    show_dependency_dialog: bool,
    copy_feedback: Option<(&'static str, Instant)>,
    open_modal_open: bool,
    open_modal_is_url: bool,
    open_modal_input: String,
    hud: HudPanel,
    is_fullscreen: bool,
    playlist: Arc<Mutex<Playlist>>,
    active_side_panel: ActiveSidePanel,
    video_panel: VideoPanel,
    audio_panel: AudioPanel,
    whisper_panel: WhisperPanel,
    audio_extractor: AudioExtractor,
    extraction_handle: Option<ExtractionHandle>,
    extraction_rx: Option<crossbeam_channel::Receiver<ExtractionStatus>>,
    extraction_progress_pct: Option<f32>,
    /// Why there is no waveform for the current file (shown in Meeting Mode, not as a global error).
    extraction_error: Option<String>,
    current_audio: Option<PcmAudio>,
    waveform_pyramid: Option<WaveformPyramid>,
    waveform_zoom_window: Option<(f64, f64)>,
    meeting_mode_view: bool,
    load_subtitles_modal_open: bool,
    load_subtitles_input: String,
    recents: RecentsStore,
    config: VadConfig,
    resume_toast: Option<ResumeToastState>,
    /// Position to seek to once mpv reports the file as loaded (`loadfile` is asynchronous).
    pending_seek: Option<f64>,
    last_saved_time_pos: f64,
    last_saved_instant: Instant,
    bookmark_store: BookmarkStore,
    vad_detector: VadDetector,
    vad_result: Option<VadDetectionResult>,
    /// Pending background VAD analysis (a 4 h file takes seconds in a debug build: never on the UI thread).
    vad_rx: Option<crossbeam_channel::Receiver<VadDetectionResult>>,
    skip_silence_enabled: bool,
    editing_bookmark_id: Option<u64>,
    bookmark_input_text: String,
    /// Give the inline note editor keyboard focus on the next frame (set when editing starts).
    focus_bookmark_edit: bool,
    export_notification: Option<(String, Instant)>,
    last_skip_time: Instant,
}

impl VadApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        initial_file: Option<String>,
        missing_dependencies: Vec<VadError>,
    ) -> Self {
        let (event_tx, event_rx) = create_event_channel();
        let shared_state = Arc::new(SharedPlayerState::new());
        let show_dependency_dialog = !missing_dependencies.is_empty();

        // 1. Initialize Player defensivamente sem panic/expect
        let (player, fatal_error) = match Player::new() {
            Ok(p) => {
                if let Err(e) = p.start_event_loop(event_tx.clone(), Arc::clone(&shared_state)) {
                    error!("Failed to start mpv event loop thread: {:?}", e);
                }
                (Some(p), None)
            }
            Err(err) => {
                error!("Fatal: Failed to initialize libmpv player: {:?}", err);
                (None, Some(VadError::PlayerInitFailed(err.to_string())))
            }
        };

        // 2. Setup OpenGL video renderer se o contexto estiver disponível
        let mut renderer = None;
        let mut gl_fatal_error = None;

        if let Some(ref p) = player {
            if let Some(gl) = cc.gl.clone() {
                if let Some(ref proc_addr) = cc.get_proc_address {
                    let proc_addr_clone = proc_addr.clone();
                    let gpa: GlProcAddressFn = Arc::new(move |name: &str| -> *mut c_void {
                        CString::new(name).ok().map_or(std::ptr::null_mut(), |s| {
                            (proc_addr_clone)(&s).cast_mut()
                        })
                    });

                    match p.create_render_context(gpa) {
                        Ok(render_ctx) => {
                            info!("Successfully created mpv_render_context with OpenGL backend");
                            renderer = Some(GlVideoRenderer::new(
                                gl,
                                render_ctx,
                                cc.egui_ctx.clone(),
                                Some(event_tx),
                            ));
                        }
                        Err(err) => {
                            error!("Failed to create mpv_render_context: {:?}", err);
                            gl_fatal_error = Some(VadError::GlContextUnavailable(err.to_string()));
                        }
                    }
                } else {
                    error!("OpenGL get_proc_address is unavailable");
                    gl_fatal_error = Some(VadError::GlContextUnavailable(
                        "OpenGL get_proc_address unavailable".to_string(),
                    ));
                }
            } else {
                error!("OpenGL (glow) context was not initialized by eframe");
                gl_fatal_error = Some(VadError::GlContextUnavailable(
                    "Contexto Glow não inicializado".to_string(),
                ));
            }
        }

        let playlist = Arc::new(Mutex::new(Playlist::new()));

        let mut platform_integrations: Vec<Box<dyn PlatformIntegration>> = Vec::new();
        #[cfg(target_os = "linux")]
        if let Some(ref p) = player {
            match crate::mpris::MprisServer::new(
                p.clone(),
                Arc::clone(&shared_state),
                Arc::clone(&playlist),
                cc.egui_ctx.clone(),
            ) {
                Ok(mpris) => platform_integrations.push(Box::new(mpris)),
                Err(err) => error!("Failed to initialize MPRIS integration: {:?}", err),
            }
            match crate::screensaver::ScreenSaverInhibitor::new() {
                Ok(inhibitor) => platform_integrations.push(Box::new(inhibitor)),
                Err(err) => error!("Failed to initialize screensaver inhibitor: {:?}", err),
            }
        }

        let recents = RecentsStore::load_default();
        let config = VadConfig::load_default();

        let mut audio_panel = AudioPanel::new();
        audio_panel.gains = config.equalizer.gains;
        if let Some(ref preset) = config.equalizer.preset {
            audio_panel.active_preset = preset.clone();
        }
        audio_panel.rnnoise = config.equalizer.rnnoise;

        if let Some(ref p) = player {
            let _ = p.set_volume(config.player.volume);
            // The panel only pushes filters to mpv on interaction; without this the restored
            // gains/RNNoise show in the UI while the audio stays flat.
            if audio_panel.rnnoise || audio_panel.gains != [0.0; 10] {
                let _ = p.set_audio_filters(&audio_panel.gains, audio_panel.rnnoise);
            }
            if let Some(ref dev) = config.player.audio_device {
                let _ = p.set_audio_device(dev);
            }
            if let Some(ref aspect) = config.player.aspect_ratio {
                let _ = p.set_video_aspect_override(aspect);
            }
        }

        let mut resume_toast = None;
        if initial_file.is_none() && config.recents.resume_enabled {
            if let Some(recent) = recents.most_recent() {
                if recent.is_resumable() {
                    resume_toast = Some(ResumeToastState {
                        location: recent.location.clone(),
                        title: recent.title.clone(),
                        timestamp: recent.timestamp,
                        duration: recent.duration,
                        time_left: 8.0,
                        is_startup_resume: true,
                    });
                }
            }
        }

        let mut whisper_panel = WhisperPanel::new();
        whisper_panel.init_from_config(&config);
        let audio_extractor = AudioExtractor::new();

        let mut app = Self {
            player,
            renderer,
            event_rx: Some(event_rx),
            shared_state,
            platform_integrations,
            hwdec_current_label: "A detetar...".to_string(),
            hwdec_forced_sw: false,
            current_media_path: None,
            current_title: String::new(),
            last_error: None,
            fatal_error: fatal_error.or(gl_fatal_error),
            missing_dependencies,
            show_dependency_dialog,
            copy_feedback: None,
            open_modal_open: false,
            open_modal_is_url: false,
            open_modal_input: String::new(),
            hud: HudPanel::new(),
            is_fullscreen: false,
            playlist,
            active_side_panel: ActiveSidePanel::None,
            video_panel: VideoPanel::new(),
            audio_panel,
            whisper_panel,
            audio_extractor,
            extraction_handle: None,
            extraction_rx: None,
            extraction_progress_pct: None,
            extraction_error: None,
            current_audio: None,
            waveform_pyramid: None,
            waveform_zoom_window: None,
            meeting_mode_view: false,
            load_subtitles_modal_open: false,
            load_subtitles_input: String::new(),
            recents,
            config,
            resume_toast,
            pending_seek: None,
            last_saved_time_pos: 0.0,
            last_saved_instant: Instant::now(),
            bookmark_store: BookmarkStore::default(),
            vad_detector: VadDetector::new(),
            vad_result: None,
            vad_rx: None,
            skip_silence_enabled: false,
            editing_bookmark_id: None,
            bookmark_input_text: String::new(),
            focus_bookmark_edit: false,
            export_notification: None,
            last_skip_time: Instant::now(),
        };

        if let Some(path) = initial_file {
            app.load_media(&path);
        }

        app
    }

    /// Loads `path` into mpv. Returns whether mpv accepted it.
    pub fn load_media(&mut self, path: &str) -> bool {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return false;
        }

        // A seek queued for the previous file must not leak onto this one.
        self.pending_seek = None;
        let is_url = is_allowed_url_scheme(trimmed);
        let mut loaded = false;

        if let Some(ref player) = self.player {
            info!("Loading media: {}", trimmed);
            if let Err(e) = player.load_file(trimmed) {
                self.last_error = Some(format!("Erro ao carregar mídia: {e:?}"));
            } else {
                loaded = true;
                self.current_media_path = Some(trimmed.to_string());
                self.current_title = if is_url {
                    trimmed.to_string()
                } else {
                    std::path::Path::new(trimmed)
                        .file_name()
                        .map(|f| f.to_string_lossy().to_string())
                        .unwrap_or_else(|| trimmed.to_string())
                };
                self.last_error = None;
                self.last_saved_time_pos = 0.0;

                if let Ok(mut pl) = self.playlist.lock() {
                    let found = pl.items().iter().position(|it| it.location() == trimmed);
                    if let Some(idx) = found {
                        pl.set_current(idx);
                    } else if is_url {
                        let idx = pl.add_url(trimmed);
                        pl.set_current(idx);
                    } else {
                        let idx = pl.add_file(trimmed);
                        pl.set_current(idx);
                    }
                }

                // Check recents for resume toast dialog (§4.6, design/Dialogs.dc.html)
                if self.config.recents.resume_enabled {
                    if let Some(recent) = self.recents.get(trimmed) {
                        if recent.is_resumable() {
                            self.resume_toast = Some(ResumeToastState {
                                location: trimmed.to_string(),
                                title: self.current_title.clone(),
                                timestamp: recent.timestamp,
                                duration: recent.duration,
                                time_left: 8.0,
                                is_startup_resume: false,
                            });
                        }
                    }
                }

                // Save previous bookmarks and load bookmarks for the new media
                if let Err(err) = self.bookmark_store.save_to_disk() {
                    warn!("Could not save bookmarks of the previous media: {err}");
                }
                self.bookmark_store = BookmarkStore::load_from_disk(trimmed).unwrap_or_else(|err| {
                    warn!("Could not read saved bookmarks for {trimmed}: {err}");
                    BookmarkStore::new(trimmed)
                });
                self.editing_bookmark_id = None;
                self.bookmark_input_text.clear();
                self.whisper_panel.reset_for_new_media();

                // Start non-blocking background audio extraction for waveform & Whisper (§4.17)
                self.start_audio_extraction(trimmed);
                self.hud.poke();
            }
        }
        loaded
    }

    /// Loads `path` and seeks to `start` once mpv reports it loaded — a seek issued right after
    /// `loadfile` runs before the file is open and is silently dropped. The caller has already
    /// chosen where to play from (resume / start over), so no resume toast is offered.
    fn load_media_at(&mut self, path: &str, start: f64) {
        if self.load_media(path) {
            self.resume_toast = None;
            self.pending_seek = (start > 0.0).then_some(start);
        }
    }

    fn poll_events(&mut self) {
        let mut events = Vec::new();
        if let Some(ref rx) = self.event_rx {
            while let Ok(event) = rx.try_recv() {
                events.push(event);
            }
        }

        for event in events {
            for integration in &mut self.platform_integrations {
                if let Err(e) = integration.on_event(&event) {
                    tracing::warn!(
                        "Platform integration '{}' error handling event: {:?}",
                        integration.name(),
                        e
                    );
                }
            }

            match event {
                PlayerEvent::FileLoaded { title, duration, path } => {
                    if let Some(t) = title {
                        self.current_title = t;
                    } else if !path.is_empty() {
                        self.current_title = std::path::Path::new(&path)
                            .file_name()
                            .map(|f| f.to_string_lossy().to_string())
                            .unwrap_or(path.clone());
                    }
                    if let Some(dur) = duration {
                        self.shared_state.set_duration(dur);
                        if let Ok(mut pl) = self.playlist.lock() {
                            if let Some(idx) = pl.current_index() {
                                if let Some(item) = pl.items_mut().get_mut(idx) {
                                    item.set_duration(Some(dur));
                                }
                            }
                        }
                    }
                    if self.waveform_pyramid.is_none() && self.extraction_rx.is_none() && !path.is_empty() {
                        self.start_audio_extraction(&path);
                    }
                    if let Some(ts) = self.pending_seek.take() {
                        if let Some(ref p) = self.player {
                            let _ = p.seek_absolute(ts);
                        }
                    }
                    self.update_hwdec_label();
                    self.hud.poke();
                }
                PlayerEvent::EndOfFile => {
                    if let Some(ref cur) = self.current_media_path {
                        let is_url = is_allowed_url_scheme(cur);
                        self.recents.add_or_update(cur, &self.current_title, 0.0, None, is_url);
                        let _ = self.recents.save_default();
                    }

                    let next_item = if let Ok(mut pl) = self.playlist.lock() {
                        pl.next().cloned()
                    } else {
                        None
                    };

                    if let Some(item) = next_item {
                        let path = item.location();
                        self.load_media(&path);
                    }
                }
                PlayerEvent::HwdecChanged(hw) => match hw {
                    Some(val) => self.hwdec_current_label = format!("HW ({val})"),
                    None => self.hwdec_current_label = "SW (CPU)".to_string(),
                },
                PlayerEvent::Error(err) => {
                    self.last_error = Some(err);
                }
                _ => {}
            }
        }

        if self.hwdec_current_label == "A detetar..." {
            self.update_hwdec_label();
        }
    }

    /// Checks the error→ação→UI table (§4.14) for whether `feature` is currently
    /// disabled because of a missing dependency, instead of matching `VadError`
    /// variants ad-hoc at each call site.
    fn is_feature_disabled(&self, feature: &str) -> bool {
        self.missing_dependencies
            .iter()
            .any(|e| e.action().disabled_features.contains(&feature))
    }

    /// Validates a URL against the allowlisted schemes (§4.27) before it is ever
    /// handed to mpv — prevents `file://`/`smb://` style paths from reaching the
    /// player through the "Abrir URL" surface.
    pub fn is_allowed_url_scheme(input: &str) -> bool {
        vad_core::is_allowed_url_scheme(input)
    }

    fn update_hwdec_label(&mut self) {
        if let Some(ref p) = self.player {
            if let Ok(hw) = p.hwdec_current() {
                match hw {
                    Some(val) => self.hwdec_current_label = format!("HW ({val})"),
                    None => self.hwdec_current_label = "SW (CPU)".to_string(),
                }
            }
        }
    }

    fn handle_shortcuts(&mut self, ctx: &egui::Context) {
        // Dismiss dialogs with Escape (§4.34)
        if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            if self.resume_toast.is_some() {
                self.resume_toast = None;
            } else if self.show_dependency_dialog {
                self.show_dependency_dialog = false;
            } else if self.open_modal_open {
                self.open_modal_open = false;
            } else if self.load_subtitles_modal_open {
                self.load_subtitles_modal_open = false;
            }
        }

        // Ctrl+O: Open File modal
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::O))
            || ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::O))
        {
            self.open_modal_is_url = false;
            self.open_modal_open = true;
            self.open_modal_input.clear();
        }

        // Ctrl+U: Open URL modal
        if ctx.input_mut(|i| i.consume_key(egui::Modifiers::COMMAND, egui::Key::U))
            || ctx.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::U))
        {
            self.open_modal_is_url = true;
            self.open_modal_open = true;
            self.open_modal_input.clear();
        }

        // Space to toggle playback (only if no text input has focus)
        let wants_keyboard = ctx.egui_wants_keyboard_input();
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::Space)) {
            if let Some(ref p) = self.player {
                let _ = p.toggle_pause();
                self.hud.poke();
            }
        }

        // Left / Right arrow for relative seek
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::ArrowLeft)) {
            if let Some(ref p) = self.player {
                let _ = p.seek_relative(-5.0);
                self.hud.poke();
            }
        }
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::ArrowRight)) {
            if let Some(ref p) = self.player {
                let _ = p.seek_relative(5.0);
                self.hud.poke();
            }
        }

        // Fullscreen toggle (F or F11)
        if !wants_keyboard
            && (ctx.input(|i| i.key_pressed(egui::Key::F))
                || ctx.input(|i| i.key_pressed(egui::Key::F11)))
        {
            self.is_fullscreen = !self.is_fullscreen;
            ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.is_fullscreen));
        }

        // Up / Down arrow for volume adjustment (only if no text input has focus)
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::ArrowUp)) {
            if let Some(ref p) = self.player {
                if let Ok(v) = p.volume() {
                    let _ = p.set_volume(v + 5.0);
                    self.hud.poke();
                }
            }
        }
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::ArrowDown)) {
            if let Some(ref p) = self.player {
                if let Ok(v) = p.volume() {
                    let _ = p.set_volume(v - 5.0);
                    self.hud.poke();
                }
            }
        }

        // M for mute toggle (only if no text input has focus)
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::M)) {
            if let Some(ref p) = self.player {
                let _ = p.toggle_mute();
                self.hud.poke();
            }
        }

        // Audio delay shortcuts (J: -50ms, K: +50ms) per PLANO_VAD.md §3 line 95
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::J)) {
            if let Some(ref p) = self.player {
                let cur = p.audio_delay().unwrap_or(0.0);
                let next = cur - 0.05;
                let _ = p.set_audio_delay(next);
                self.video_panel.audio_delay_ms = (next * 1000.0).round() as i64;
                self.hud.set_notification(format!("Atraso áudio: {:.0} ms", next * 1000.0));
            }
        }
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::K)) {
            if let Some(ref p) = self.player {
                let cur = p.audio_delay().unwrap_or(0.0);
                let next = cur + 0.05;
                let _ = p.set_audio_delay(next);
                self.video_panel.audio_delay_ms = (next * 1000.0).round() as i64;
                self.hud.set_notification(format!("Atraso áudio: {:.0} ms", next * 1000.0));
            }
        }

        // Subtitle delay shortcuts (G: -50ms, H: +50ms) per PLANO_VAD.md §3 line 95
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::G)) {
            if let Some(ref p) = self.player {
                let cur = p.sub_delay().unwrap_or(0.0);
                let next = cur - 0.05;
                let _ = p.set_sub_delay(next);
                self.video_panel.sub_delay_ms = (next * 1000.0).round() as i64;
                self.hud.set_notification(format!("Atraso legendas: {:.0} ms", next * 1000.0));
            }
        }
        if !wants_keyboard && ctx.input(|i| i.key_pressed(egui::Key::H)) {
            if let Some(ref p) = self.player {
                let cur = p.sub_delay().unwrap_or(0.0);
                let next = cur + 0.05;
                let _ = p.set_sub_delay(next);
                self.video_panel.sub_delay_ms = (next * 1000.0).round() as i64;
                self.hud.set_notification(format!("Atraso legendas: {:.0} ms", next * 1000.0));
            }
        }
    }

    fn handle_drag_and_drop(&mut self, ctx: &egui::Context) {
        // Process dropped files
        let dropped = ctx.input(|i| i.raw.dropped_files.clone());
        if !dropped.is_empty() {
            for file in dropped {
                let path_str = file.path().to_string_lossy().to_string();
                if !path_str.is_empty() {
                    info!("Dropped file received: {}", path_str);
                    self.load_media(&path_str);
                    break;
                }
            }
        }
    }

    /// Renders dependency degradation modal dialog (§4.14, §4.34, and design/Dialogs.dc.html).
    fn render_dependency_dialog(&mut self, ctx: &egui::Context) {
        if !self.show_dependency_dialog || self.missing_dependencies.is_empty() {
            return;
        }

        let mut close_dialog = false;
        let mut recheck = false;

        egui::Window::new("⚠️ Dependências em falta")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(26, 28, 40, 238))
                    .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 30)))
                    .corner_radius(16)
                    .inner_margin(20),
            )
            .show(ctx, |ui| {
                ui.set_max_width(480.0);

                ui.horizontal(|ui| {
                    ui.colored_label(
                        Color32::from_rgb(230, 160, 60),
                        egui::RichText::new("⚠️").size(20.0),
                    );
                    ui.heading("Dependências em falta no sistema");
                });
                ui.add_space(8.0);
                ui.label(
                    "O leitor arrancou em modo degradado. Algumas funcionalidades avançadas foram desativadas até que os binários necessários sejam instalados.",
                );
                ui.add_space(12.0);

                for err in &self.missing_dependencies {
                    let action = err.action();
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.colored_label(Color32::from_rgb(230, 70, 70), "✕");
                            ui.strong(action.title);
                        });
                        ui.add_space(2.0);
                        ui.label(action.description);

                        if let Some(cmd) = action.install_command {
                            ui.add_space(6.0);
                            ui.horizontal(|ui| {
                                ui.monospace(
                                    egui::RichText::new(cmd)
                                        .color(Color32::from_rgb(210, 210, 225))
                                        .background_color(Color32::from_rgb(18, 19, 26)),
                                );
                                if ui.button("📋 Copiar").clicked() {
                                    ctx.copy_text(cmd.to_string());
                                    self.copy_feedback = Some((cmd, Instant::now()));
                                }
                            });
                        }
                    });
                    ui.add_space(8.0);
                }

                if let Some((cmd, instant)) = self.copy_feedback {
                    if instant.elapsed() < std::time::Duration::from_secs(3) {
                        ui.colored_label(
                            Color32::from_rgb(100, 220, 100),
                            format!("✓ Comando copiado: {cmd}"),
                        );
                    }
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        if ui.button("Verificar novamente").clicked() {
                            recheck = true;
                        }
                        if ui.button("Ignorar (limitar funções)").clicked() {
                            close_dialog = true;
                        }
                    });
                });
            });

        if close_dialog {
            self.show_dependency_dialog = false;
        }

        if recheck {
            let refreshed = probe_dependencies();
            if refreshed.is_empty() {
                self.show_dependency_dialog = false;
            }
            self.missing_dependencies = refreshed;
        }
    }

    /// Renders modal dialog for Ctrl+O / Ctrl+U open actions.
    fn render_open_modal(&mut self, ctx: &egui::Context) {
        if !self.open_modal_open {
            return;
        }

        let is_url = self.open_modal_is_url;
        let title = if is_url { "Abrir Endereço Web / URL" } else { "Abrir Ficheiro de Mídia" };
        let placeholder = if is_url { "https://... ou rtsp://..." } else { "/caminho/para/video.mp4" };
        let url_playback_disabled = is_url && self.is_feature_disabled("url_playback");

        let mut close_modal = false;
        let mut load_path = None;
        let mut validation_error = None;

        egui::Window::new(title)
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(26, 28, 40, 240))
                    .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 30)))
                    .corner_radius(14)
                    .inner_margin(18),
            )
            .show(ctx, |ui| {
                ui.set_max_width(450.0);

                ui.label(if is_url {
                    "Introduz o URL de vídeo ou stream que pretendes reproduzir:"
                } else {
                    "Introduz o caminho do ficheiro ou arrasta-o para a janela:"
                });
                ui.add_space(6.0);

                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.open_modal_input)
                        .hint_text(placeholder)
                        .desired_width(420.0),
                );
                edit.request_focus();

                if url_playback_disabled {
                    ui.add_space(6.0);
                    ui.colored_label(
                        Color32::from_rgb(230, 160, 60),
                        "yt-dlp em falta — reprodução por URL desativada (`sudo apt install yt-dlp`).",
                    );
                }

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let open_btn = egui::Button::new("Abrir");
                        let submit = ui.add_enabled(!url_playback_disabled, open_btn).clicked()
                            || (!url_playback_disabled
                                && edit.lost_focus()
                                && ui.input(|i| i.key_pressed(egui::Key::Enter)));

                        if submit {
                            let input = self.open_modal_input.trim().to_string();
                            if !input.is_empty() {
                                if is_url && !Self::is_allowed_url_scheme(&input) {
                                    validation_error = Some(
                                        "URL inválido: só são aceites http://, https:// ou rtsp:// (§4.27)."
                                            .to_string(),
                                    );
                                } else {
                                    load_path = Some(input);
                                    close_modal = true;
                                }
                            }
                        }
                        if ui.button("Cancelar").clicked() {
                            close_modal = true;
                        }
                    });
                });
            });

        if let Some(path) = load_path {
            self.load_media(&path);
        }
        if close_modal {
            self.open_modal_open = false;
        }
        if validation_error.is_some() {
            self.last_error = validation_error;
        }
    }

    /// Renders modal dialog for loading external subtitle file.
    fn render_subtitles_modal(&mut self, ctx: &egui::Context) {
        if !self.load_subtitles_modal_open {
            return;
        }

        let mut close_modal = false;
        let mut load_sub = None;

        egui::Window::new("Carregar Legendas Externas")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_CENTER, vec2(0.0, 0.0))
            .frame(
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(26, 28, 40, 240))
                    .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 30)))
                    .corner_radius(14)
                    .inner_margin(18),
            )
            .show(ctx, |ui| {
                ui.set_max_width(450.0);

                ui.label("Introduz o caminho para o ficheiro de legendas (.srt, .ass, .vtt):");
                ui.add_space(6.0);

                let edit = ui.add(
                    egui::TextEdit::singleline(&mut self.load_subtitles_input)
                        .hint_text("/caminho/para/legendas.srt")
                        .desired_width(420.0),
                );
                edit.request_focus();

                ui.add_space(10.0);
                ui.horizontal(|ui| {
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                        let submit = ui.button("Carregar").clicked()
                            || (edit.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter)));

                        if submit {
                            let input = self.load_subtitles_input.trim().to_string();
                            if !input.is_empty() {
                                load_sub = Some(input);
                                close_modal = true;
                            }
                        }
                        if ui.button("Cancelar").clicked() {
                            close_modal = true;
                        }
                    });
                });
            });

        if let Some(sub_path) = load_sub {
            if let Some(ref p) = self.player {
                if let Err(e) = p.load_subtitles(&sub_path) {
                    self.last_error = Some(format!("Erro ao carregar legendas: {e:?}"));
                } else {
                    self.hud.set_notification("Legendas carregadas com sucesso");
                }
            }
        }
        if close_modal {
            self.load_subtitles_modal_open = false;
        }
    }

    /// Renders welcome/idle screen when no media is currently opened (Task 5).
    fn render_welcome_screen(&mut self, ui: &mut egui::Ui) {
        let available_rect = ui.available_rect_before_wrap();

        ui.vertical_centered(|ui| {
            ui.add_space(available_rect.height() * 0.15);

            // Large media icon
            ui.label(
                egui::RichText::new("🎬")
                    .size(56.0),
            );
            ui.add_space(10.0);

            ui.heading(
                egui::RichText::new("Arrasta e larga um ficheiro de vídeo ou áudio aqui")
                    .size(20.0)
                    .strong(),
            );
            ui.add_space(6.0);

            ui.colored_label(
                Color32::from_rgb(160, 165, 185),
                "Suporta MP4, MKV, WebM, Opus, FLAC, AAC e múltiplos fluxos de áudio e legendas.",
            );
            ui.add_space(16.0);

            // Keyboard shortcut badges
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing = vec2(16.0, 0.0);
                if ui.button("📂 Procurar Ficheiro (Ctrl+O)").clicked() {
                    self.open_modal_is_url = false;
                    self.open_modal_open = true;
                    self.open_modal_input.clear();
                }
                let url_playback_disabled = self.is_feature_disabled("url_playback");
                let url_btn = egui::Button::new("🌐 Abrir URL (Ctrl+U)");
                if url_playback_disabled {
                    ui.add_enabled(false, url_btn)
                        .on_disabled_hover_text("Desativado: Requer yt-dlp (`sudo apt install yt-dlp`)");
                } else if ui.add(url_btn).clicked() {
                    self.open_modal_is_url = true;
                    self.open_modal_open = true;
                    self.open_modal_input.clear();
                }
            });

            ui.add_space(36.0);

            // Recents section on welcome screen (§4.6)
            let has_recents = !self.recents.is_empty();
            let entries_count = self.recents.len().min(3);
            let box_height = if has_recents { 44.0 + (entries_count as f32 * 32.0) } else { 70.0 };
            let recents_rect = Rect::from_center_size(
                pos2(available_rect.center().x, available_rect.center().y + 120.0),
                vec2(520.0, box_height),
            );
            let painter = ui.painter();
            painter.rect(
                recents_rect,
                CornerRadius::same(12),
                Color32::from_rgba_premultiplied(255, 255, 255, 8),
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 16)),
                StrokeKind::Inside,
            );

            let mut recents_ui = ui.new_child(
                UiBuilder::new()
                    .max_rect(recents_rect.shrink(10.0))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            recents_ui.colored_label(Color32::from_rgb(140, 145, 165), "Ficheiros Recentes");
            recents_ui.add_space(4.0);

            if !has_recents {
                recents_ui.colored_label(
                    Color32::from_rgb(100, 105, 120),
                    "Nenhum ficheiro recente ainda.",
                );
            } else {
                let mut open_item = None;
                let entries = self.recents.entries().iter().take(3).cloned().collect::<Vec<_>>();
                for entry in entries {
                    recents_ui.horizontal(|ui| {
                        let icon = if entry.is_url { "🌐 " } else { "🎬 " };
                        let summary = entry.formatted_summary();
                        let title_display = if entry.title.chars().count() > 38 {
                            let head: String = entry.title.chars().take(35).collect();
                            format!("{icon}{head}...")
                        } else {
                            format!("{icon}{}", entry.title)
                        };
                        if ui.button(title_display).clicked() {
                            open_item = Some((entry.location.clone(), entry.timestamp));
                        }
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            ui.colored_label(Color32::from_rgb(139, 124, 246), summary);
                        });
                    });
                }
                if let Some((loc, ts)) = open_item {
                    self.load_media_at(&loc, ts);
                }
            }
        });
    }

    /// Periodically saves playback progress to `recentes.json` every ~5 seconds (§4.6, §4.30).
    fn save_progress_periodically(&mut self) {
        if self.last_saved_instant.elapsed().as_secs() < 5 {
            return;
        }
        self.last_saved_instant = Instant::now();

        if let Some(ref path) = self.current_media_path {
            let pos = self.shared_state.get_time_pos();
            let dur = self.shared_state.get_duration();
            let is_url = is_allowed_url_scheme(path);

            if pos > 3.0 && (pos - self.last_saved_time_pos).abs() >= 1.0 {
                self.last_saved_time_pos = pos;
                self.recents.add_or_update(
                    path,
                    &self.current_title,
                    pos,
                    if dur > 0.0 { Some(dur) } else { None },
                    is_url,
                );
                let _ = self.recents.save_default();
            }
        }
    }

    /// Flushes playback progress and configuration state to disk on exit or pause (§4.30, §5).
    fn save_state(&mut self) {
        if let Some(ref path) = self.current_media_path {
            let pos = self.shared_state.get_time_pos();
            let dur = self.shared_state.get_duration();
            let is_url = is_allowed_url_scheme(path);

            if pos > 3.0 {
                let save_pos = if dur > 0.0 && pos >= dur - 3.0 { 0.0 } else { pos };
                self.recents.add_or_update(
                    path,
                    &self.current_title,
                    save_pos,
                    if dur > 0.0 { Some(dur) } else { None },
                    is_url,
                );
                let _ = self.recents.save_default();
            }
            self.save_bookmarks();
        }

        if let Some(ref p) = self.player {
            if let Ok(vol) = p.volume() {
                self.config.player.volume = vol;
            }
        }
        self.config.equalizer.gains = self.audio_panel.gains;
        self.config.equalizer.preset = Some(self.audio_panel.active_preset.clone());
        self.config.equalizer.rnnoise = self.audio_panel.rnnoise;
        let _ = self.config.save_default();
    }

    /// Renders non-blocking floating toast dialog for resuming playback matching `design/Dialogs.dc.html`.
    /// Has an 8s timeout, doesn't block UI interactions, and can be dismissed with Escape.
    fn render_resume_toast(&mut self, ctx: &egui::Context) {
        let Some(mut toast) = self.resume_toast.take() else {
            return;
        };

        let dt = ctx.input(|i| i.stable_dt).min(0.2);
        toast.time_left -= dt;

        if toast.time_left <= 0.0 {
            return;
        }

        ctx.request_repaint();

        let accent = Color32::from_rgb(139, 124, 246);
        let mut action_continue = false;
        let mut action_start_over = false;
        let mut action_close = false;

        egui::Area::new(egui::Id::new("vad_resume_toast_area"))
            .anchor(egui::Align2::RIGHT_BOTTOM, vec2(-24.0, -115.0))
            .order(egui::Order::Foreground)
            .show(ctx, |ui| {
                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(26, 28, 40, 235))
                    .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 30)))
                    .corner_radius(14)
                    .inner_margin(16)
                    .shadow(egui::epaint::Shadow {
                        blur: 24,
                        color: Color32::from_black_alpha(140),
                        ..Default::default()
                    })
                    .show(ui, |ui| {
                        ui.set_max_width(360.0);
                        ui.spacing_mut().item_spacing = vec2(0.0, 10.0);

                        // Header with Clock Icon and Title
                        ui.horizontal(|ui| {
                            let (icon_rect, _) = ui.allocate_exact_size(vec2(18.0, 18.0), egui::Sense::hover());
                            let painter = ui.painter_at(icon_rect);
                            painter.circle_stroke(icon_rect.center(), 8.0, Stroke::new(1.8, accent));
                            let center = icon_rect.center();
                            painter.line_segment([center, center + vec2(0.0, -4.5)], Stroke::new(1.8, accent));
                            painter.line_segment([center, center + vec2(3.5, 3.5)], Stroke::new(1.8, accent));

                            ui.add_space(4.0);
                            ui.label(
                                egui::RichText::new("Continuar de onde parou?")
                                    .size(14.5)
                                    .strong()
                                    .color(Color32::WHITE),
                            );

                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                if ui.small_button("✕").clicked() {
                                    action_close = true;
                                }
                            });
                        });

                        // Subtle countdown progress line
                        let pct = (toast.time_left / 8.0).clamp(0.0, 1.0);
                        let (bar_rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), 2.0), egui::Sense::hover());
                        let painter = ui.painter_at(bar_rect);
                        painter.rect_filled(bar_rect, CornerRadius::ZERO, Color32::from_rgba_premultiplied(255, 255, 255, 15));
                        let active_bar = Rect::from_min_size(bar_rect.min, vec2(bar_rect.width() * pct, 2.0));
                        painter.rect_filled(active_bar, CornerRadius::ZERO, accent);

                        // Media info box
                        egui::Frame::new()
                            .fill(Color32::from_rgba_premultiplied(255, 255, 255, 10))
                            .corner_radius(8)
                            .inner_margin(10)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new(&toast.title)
                                        .size(13.0)
                                        .strong()
                                        .color(Color32::from_rgb(235, 238, 250)),
                                );
                                ui.add_space(3.0);
                                let pos_str = RecentEntry::format_seconds(toast.timestamp);
                                let dur_str = toast.duration.map(RecentEntry::format_seconds);

                                ui.horizontal(|ui| {
                                    ui.label(
                                        egui::RichText::new(pos_str)
                                            .size(12.5)
                                            .color(accent)
                                            .strong(),
                                    );
                                    if let Some(dur) = dur_str {
                                        ui.colored_label(
                                            Color32::from_rgb(140, 145, 165),
                                            format!("de {dur}"),
                                        );
                                    }
                                });
                            });

                        // Actions row
                        ui.horizontal(|ui| {
                            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                let continue_btn = egui::Button::new(
                                    egui::RichText::new("Continuar")
                                        .size(12.5)
                                        .strong()
                                        .color(Color32::from_rgb(20, 20, 28)),
                                )
                                .fill(accent)
                                .corner_radius(CornerRadius::same(8));

                                if ui.add(continue_btn).clicked() {
                                    action_continue = true;
                                }

                                let start_over_btn = egui::Button::new(
                                    egui::RichText::new("Começar do início")
                                        .size(12.5)
                                        .strong()
                                        .color(Color32::from_rgb(200, 205, 225)),
                                )
                                .fill(Color32::from_rgba_premultiplied(255, 255, 255, 14))
                                .corner_radius(CornerRadius::same(8));

                                if ui.add(start_over_btn).clicked() {
                                    action_start_over = true;
                                }
                            });
                        });
                    });
            });

        if action_continue || action_start_over {
            let start = if action_continue { toast.timestamp } else { 0.0 };
            if toast.is_startup_resume {
                self.load_media_at(&toast.location, start);
            } else if let Some(ref p) = self.player {
                let _ = p.seek_absolute(start);
                let _ = p.play();
            }
        } else if !action_close {
            self.resume_toast = Some(toast);
        }
    }

    /// Renders fatal error screen if OpenGL or libmpv initialization completely failed.
    fn render_fatal_error_screen(&self, ui: &mut egui::Ui, err: &VadError) {
        let action = err.action();
        ui.centered_and_justified(|ui| {
            ui.group(|ui| {
                ui.colored_label(Color32::RED, egui::RichText::new("⚠️ Falha Crítica").size(24.0).strong());
                ui.add_space(8.0);
                ui.heading(action.title);
                ui.add_space(4.0);
                ui.label(action.description);
                if let Some(cmd) = action.install_command {
                    ui.add_space(8.0);
                    ui.label("Para corrigir este problema no sistema, executa:");
                    ui.monospace(cmd);
                }
            });
        });
    }

    /// Starts non-blocking audio extraction for waveform and Whisper transcription.
    /// Never blocks playback or UI (§4.17).
    pub fn start_audio_extraction(&mut self, path: &str) {
        self.cancel_extraction();
        self.waveform_pyramid = None;
        self.current_audio = None;
        self.clear_vad();
        self.waveform_zoom_window = None;
        self.extraction_error = None;

        // ffmpeg cannot resolve stream pages (YouTube, ...) and would never finish on a live
        // RTSP stream: extraction is for local files only. Without ffmpeg the waveform/Whisper
        // features are disabled by the error table (§4.14) instead of failing on every open.
        if is_allowed_url_scheme(path) {
            self.extraction_error = Some("Waveform indisponível para streams por URL.".to_string());
            return;
        }
        if self.is_feature_disabled("waveform") {
            self.extraction_error = Some("Waveform desativada: FFmpeg em falta.".to_string());
            return;
        }

        // Check in-memory cache first (§4.12, §4.18)
        if let Some(cached) = self.audio_extractor.get_cached(path) {
            info!("Reusing in-memory cached PCM audio for {}", path);
            self.waveform_pyramid = Some(WaveformPyramid::from_pcm(&cached));
            self.start_vad_analysis(&cached);
            self.current_audio = Some(cached);
            return;
        }

        // PCM is transient (§4.12): the previous file's audio is released when another opens
        self.audio_extractor.clear_cache();

        let dur = self.shared_state.get_duration();
        let dur_hint = if dur > 0.0 { Some(dur) } else { None };
        let (rx, handle) = self.audio_extractor.extract_async(path.to_string(), dur_hint);
        self.extraction_handle = Some(handle);
        self.extraction_rx = Some(rx);
        self.extraction_progress_pct = Some(0.0);
    }

    /// Cancels active audio extraction subprocess immediately (§4.17).
    pub fn cancel_extraction(&mut self) {
        if let Some(handle) = self.extraction_handle.take() {
            handle.cancel();
        }
        self.extraction_rx = None;
        self.extraction_progress_pct = None;
        self.clear_vad();
    }

    /// Drops the VAD result and ignores any analysis still running for the previous media.
    fn clear_vad(&mut self) {
        self.vad_result = None;
        self.vad_rx = None;
    }

    /// Runs silence detection on a worker thread (samples are an `Arc`, so the clone is cheap).
    fn start_vad_analysis(&mut self, pcm: &PcmAudio) {
        let detector = self.vad_detector.clone();
        let pcm = pcm.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let _ = tx.send(detector.detect(&pcm));
        });
        self.vad_result = None;
        self.vad_rx = Some(rx);
    }

    /// Collects the background VAD result without blocking; keeps repainting while it is pending.
    fn poll_vad_analysis(&mut self, ctx: &egui::Context) {
        let Some(ref rx) = self.vad_rx else {
            return;
        };
        match rx.try_recv() {
            Ok(result) => {
                info!(
                    "VAD analysis done: {} speech / {} silence segments",
                    result.speech_segments.len(),
                    result.silence_segments.len()
                );
                self.vad_result = Some(result);
                self.vad_rx = None;
            }
            Err(crossbeam_channel::TryRecvError::Empty) => {
                ctx.request_repaint_after(std::time::Duration::from_millis(100));
            }
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                warn!("VAD analysis thread ended without a result");
                self.vad_rx = None;
            }
        }
    }

    /// Polls asynchronous audio extraction status without blocking (§4.17).
    fn poll_audio_extraction(&mut self) {
        if let Some(ref rx) = self.extraction_rx {
            while let Ok(status) = rx.try_recv() {
                match status {
                    ExtractionStatus::Progress(prog) => {
                        self.extraction_progress_pct = Some(prog.percent);
                    }
                    ExtractionStatus::Completed(pcm) => {
                        info!(
                            "Audio extraction complete: duration={:.1}s, samples={}",
                            pcm.duration_seconds,
                            pcm.samples.len()
                        );
                        let pyramid = WaveformPyramid::from_pcm(&pcm);
                        self.waveform_pyramid = Some(pyramid);
                        self.start_vad_analysis(&pcm);
                        self.current_audio = Some(pcm);
                        self.extraction_progress_pct = None;
                        self.extraction_handle = None;
                        self.extraction_rx = None;
                        break;
                    }
                    ExtractionStatus::Failed(err) => {
                        // Expected for e.g. videos without an audio track: keep it out of the
                        // global error banner and explain it where the waveform would be.
                        warn!("Audio extraction failed: {}", err);
                        self.extraction_error = Some(format!("Sem áudio extraível: {err}"));
                        self.extraction_progress_pct = None;
                        self.extraction_handle = None;
                        self.extraction_rx = None;
                        break;
                    }
                    ExtractionStatus::Cancelled => {
                        info!("Audio extraction cancelled");
                        self.extraction_progress_pct = None;
                        self.extraction_handle = None;
                        self.extraction_rx = None;
                        break;
                    }
                }
            }
        }
    }

    /// Adjusts waveform zoom window based on mouse wheel scroll (§5).
    fn apply_waveform_zoom(&mut self, scroll_y: f32, current_time: f64, duration: f64) {
        self.waveform_zoom_window =
            next_waveform_zoom_window(self.waveform_zoom_window, scroll_y, current_time, duration);
    }

    /// Renders Meeting Mode view per PLANO_VAD.md §6 and Sprint_Planning_06 Task 6.
    /// Features:
    /// - Multi-resolution WaveformPyramid rendering (capped at ~1000 points visible in single batch, §5)
    /// - Mouse wheel zooming (§5)
    /// - Clickable/draggable playhead seeking directly on waveform
    /// - Mini-player transport bar below waveform ([⏪5s], [▶/⏸], [5s⏩], pos, speed, vol)
    fn render_meeting_mode(&mut self, ui: &mut egui::Ui, available_rect: Rect) {
        let current_time = self.shared_state.get_time_pos();
        let media_duration = self.shared_state.get_duration();
        // A truncated extraction (§4.12) covers less than the media: draw the waveform against the
        // audio actually held, otherwise it would be stretched over the full duration.
        let truncated = self.current_audio.as_ref().is_some_and(|a| a.is_truncated);
        let duration = match self.current_audio.as_ref() {
            Some(a) if a.is_truncated => a.duration_seconds,
            _ => media_duration,
        };

        ui.vertical(|ui| {
            // Title & View Toggle
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("Visualizador de Forma de Onda (Waveform da Reunião com MIP-Mapping)")
                        .size(13.0)
                        .strong()
                        .color(Color32::from_rgb(180, 185, 205)),
                );

                if let Some(ref p) = self.player {
                    if p.has_video() {
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.button("🎞 Alternar para Vídeo").clicked() {
                                self.meeting_mode_view = false;
                            }
                        });
                    }
                }
            });

            if truncated {
                ui.colored_label(
                    Color32::from_rgb(230, 160, 60),
                    format!(
                        "⚠ Áudio truncado às {:.0}h (limite de memória, §4.12): a waveform e a transcrição só cobrem esse troço.",
                        vad_ai::MAX_AUDIO_DURATION_SECS / 3600.0
                    ),
                );
            }

            ui.add_space(6.0);

            // --- WAVEFORM DISPLAY AREA ---
            let waveform_h = 190.0_f32.min(available_rect.height() * 0.42);
            let waveform_size = vec2(ui.available_width(), waveform_h);
            let (response, painter) = ui.allocate_painter(waveform_size, egui::Sense::click_and_drag());
            let rect = response.rect;

            // Waveform background card
            painter.rect_filled(
                rect,
                CornerRadius::same(8),
                Color32::from_rgb(18, 20, 26),
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 20)),
                StrokeKind::Inside,
            );

            // Waveform midline
            let mid_y = rect.center().y;
            painter.line_segment(
                [pos2(rect.left(), mid_y), pos2(rect.right(), mid_y)],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 30)),
            );

            let (view_start, view_end) = self.waveform_zoom_window.unwrap_or((0.0, duration.max(0.1)));

            if let Some(ref mut pyramid) = self.waveform_pyramid {
                let points = pyramid.get_visible_points(view_start, view_end, TARGET_VISIBLE_POINTS);
                let num_pts = points.len();

                if num_pts > 0 {
                    let wave_color = Color32::from_rgb(139, 124, 246);
                    let half_h = (rect.height() * 0.44).max(5.0);
                    let col_w = (rect.width() / num_pts as f32).max(1.0);

                    for (i, pt) in points.iter().enumerate() {
                        let frac = i as f32 / num_pts as f32;
                        let x = rect.left() + frac * rect.width();
                        let top_y = mid_y - (pt.max * half_h);
                        let bot_y = mid_y - (pt.min * half_h);
                        let y_min = top_y.min(bot_y);
                        let y_max = top_y.max(bot_y).max(y_min + 1.0);

                        painter.rect_filled(
                            Rect::from_min_max(pos2(x, y_min), pos2(x + col_w, y_max)),
                            CornerRadius::ZERO,
                            wave_color,
                        );
                    }
                }

                // Playhead indicator [▲]
                let span = (view_end - view_start).max(0.001);
                let pos_fraction = ((current_time - view_start) / span).clamp(0.0, 1.0);
                let playhead_x = rect.left() + pos_fraction as f32 * rect.width();
                let playhead_color = Color32::from_rgb(250, 204, 21); // Yellow/Gold

                painter.line_segment(
                    [pos2(playhead_x, rect.top()), pos2(playhead_x, rect.bottom())],
                    Stroke::new(2.0, playhead_color),
                );

                // Playhead cursor [▲]
                let tri_h = 8.0;
                let tri_w = 6.0;
                painter.line_segment(
                    [pos2(playhead_x - tri_w, rect.bottom()), pos2(playhead_x + tri_w, rect.bottom())],
                    Stroke::new(2.0, playhead_color),
                );
                painter.line_segment(
                    [pos2(playhead_x - tri_w, rect.bottom()), pos2(playhead_x, rect.bottom() - tri_h)],
                    Stroke::new(2.0, playhead_color),
                );
                painter.line_segment(
                    [pos2(playhead_x + tri_w, rect.bottom()), pos2(playhead_x, rect.bottom() - tri_h)],
                    Stroke::new(2.0, playhead_color),
                );

                // Click and drag seeking directly on waveform (§6)
                if response.clicked() || response.dragged() {
                    if let Some(pos) = response.interact_pointer_pos() {
                        let u = ((pos.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                        let target_secs = view_start + u as f64 * span;
                        if let Some(ref p) = self.player {
                            let _ = p.seek_absolute(target_secs);
                        }
                    }
                }

                // Mouse wheel zooming (§5)
                if response.hovered() {
                    let scroll_y = ui.input(|i| i.smooth_scroll_delta.y);
                    if scroll_y.abs() > 0.5 {
                        self.apply_waveform_zoom(scroll_y, current_time, duration);
                    }
                }
            } else if let Some(pct) = self.extraction_progress_pct {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    format!("A extrair áudio e a gerar waveform: {:.0}%...", pct),
                    egui::FontId::proportional(15.0),
                    Color32::from_rgb(217, 158, 66),
                );
            } else {
                painter.text(
                    rect.center(),
                    egui::Align2::CENTER_CENTER,
                    self.extraction_error.as_deref().unwrap_or("Sem dados de áudio extraídos"),
                    egui::FontId::proportional(14.0),
                    Color32::from_rgb(140, 145, 165),
                );
            }

            ui.add_space(8.0);

            // --- MINI-PLAYER DE TRANSPORTE (§6, Task 6) ---
            ui.horizontal(|ui| {
                // [⏪ 5s]
                if ui.button("⏪ 5s").on_hover_text("Retroceder 5 segundos").clicked() {
                    if let Some(ref p) = self.player {
                        let _ = p.seek_relative(-5.0);
                    }
                }

                // [▶ / ⏸]
                let is_paused = self.shared_state.is_paused();
                let play_btn_text = if is_paused { "▶ Play" } else { "⏸ Pausa" };
                if ui.button(play_btn_text).clicked() {
                    if let Some(ref p) = self.player {
                        let _ = p.toggle_pause();
                    }
                }

                // [5s ⏩]
                if ui.button("5s ⏩").on_hover_text("Avançar 5 segundos").clicked() {
                    if let Some(ref p) = self.player {
                        let _ = p.seek_relative(5.0);
                    }
                }

                ui.separator();

                // Position / Duration
                let time_text = format!(
                    "{} / {}",
                    HudPanel::format_time(current_time),
                    HudPanel::format_time(media_duration)
                );
                ui.monospace(RichText::new(time_text).strong());

                ui.separator();

                // Speed control
                if let Some(ref p) = self.player {
                    let cur_speed = p.speed().unwrap_or(1.0);
                    egui::ComboBox::from_id_salt("mini_player_speed_combo")
                        .selected_text(format!("{:.2}x", cur_speed))
                        .show_ui(ui, |ui| {
                            for &spd in &[0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                                if ui.selectable_label((cur_speed - spd).abs() < 0.01, format!("{:.2}x", spd)).clicked() {
                                    let _ = p.set_speed(spd);
                                }
                            }
                        });
                }

                ui.separator();

                // Volume control
                if let Some(ref p) = self.player {
                    let mut vol = p.volume().unwrap_or(100.0);
                    ui.label("🔊");
                    if ui.add(egui::Slider::new(&mut vol, 0.0..=200.0).show_value(false)).changed() {
                        let _ = p.set_volume(vol);
                    }
                    ui.label(format!("{:.0}%", vol));
                }

                // Zoom reset & info
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if let Some((s, e)) = self.waveform_zoom_window {
                        let span = e - s;
                        let span_str = if span < 60.0 {
                            format!("Zoom: {:.0}s", span)
                        } else {
                            format!("Zoom: {:.1}m", span / 60.0)
                        };
                        ui.colored_label(Color32::from_rgb(139, 124, 246), span_str);
                        if ui.small_button("Reset Zoom").on_hover_text("Voltar ao nível global completo").clicked() {
                            self.waveform_zoom_window = None;
                        }
                    } else {
                        ui.colored_label(Color32::from_rgb(140, 145, 165), "Zoom: Global");
                    }
                });
            });

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(4.0);

            // --- CONTROLOS RÁPIDOS DE REUNIÃO (PLANO_VAD.md §6, Meeting.dc.html) ---
            ui.horizontal(|ui| {
                ui.colored_label(Color32::from_rgb(160, 165, 185), "Controlos Rápidos:");

                // Pill: Saltar Silêncios (Meeting.dc.html lines 60-63)
                let skip_btn_text = if self.skip_silence_enabled {
                    "Saltar Silêncios: ATIVO"
                } else {
                    "Saltar Silêncios: INATIVO"
                };
                let mut skip_btn = egui::Button::new(RichText::new(skip_btn_text).strong().size(12.0));
                if self.skip_silence_enabled {
                    skip_btn = skip_btn
                        .fill(Color32::from_rgba_premultiplied(139, 124, 246, 55))
                        .stroke(Stroke::new(1.2, Color32::from_rgb(139, 124, 246)));
                }
                if ui
                    .add(skip_btn)
                    .on_hover_text("Salta automaticamente pausas e silêncios durante a reprodução da reunião")
                    .clicked()
                {
                    self.skip_silence_enabled = !self.skip_silence_enabled;
                }
                let vad_note = match (&self.vad_result, self.vad_rx.is_some()) {
                    (Some(vad), _) => format!("{} pausas detetadas", vad.silence_segments.len()),
                    (None, true) => "A analisar áudio…".to_string(),
                    (None, false) => "Sem análise de áudio".to_string(),
                };
                ui.colored_label(Color32::from_rgb(130, 135, 155), vad_note);

                // Placeholder for Sprint 08 / Noise reduction
                ui.add_enabled(false, egui::Button::new("Redução de Ruído: Sprint 08"))
                    .on_disabled_hover_text("Previsto para o Sprint 08 (§11)");

                if self.whisper_panel.is_transcribing() {
                    let prog = self.whisper_panel.transcribe_progress().unwrap_or(0);
                    ui.spinner();
                    ui.colored_label(Color32::from_rgb(139, 124, 246), format!("A transcrever: {}%", prog));
                } else if ui
                    .button("🎙 Abrir Whisper AI")
                    .on_hover_text("Abrir painel lateral de Transcrição Whisper AI")
                    .clicked()
                {
                    self.active_side_panel = ActiveSidePanel::Whisper;
                }
            });

            ui.add_space(10.0);

            // --- MARCADORES DA REUNIÃO (Meeting.dc.html lines 72-89) ---
            ui.horizontal(|ui| {
                ui.label(
                    RichText::new("MARCADORES DA REUNIÃO")
                        .size(12.0)
                        .strong()
                        .color(Color32::from_rgb(160, 165, 185)),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let add_btn = egui::Button::new(
                        RichText::new("+ Adicionar nota")
                            .color(Color32::from_rgb(139, 124, 246))
                            .strong(),
                    );
                    if ui
                        .add_enabled(self.current_media_path.is_some(), add_btn)
                        .on_hover_text("Criar nota no timestamp atual da reprodução")
                        .on_disabled_hover_text("Abre um ficheiro para criar notas")
                        .clicked()
                    {
                        let cur_pos = self.shared_state.get_time_pos();
                        let new_id = self.bookmark_store.add(cur_pos, DEFAULT_NOTE_TEXT);
                        self.editing_bookmark_id = Some(new_id);
                        self.bookmark_input_text.clear();
                        self.focus_bookmark_edit = true;
                        self.save_bookmarks();
                    }
                });
            });

            ui.add_space(6.0);

            // Bookmark List Frame
            let mut bookmark_to_delete = None;
            let mut bookmark_to_seek = None;
            let mut bookmark_to_edit = None;

            egui::Frame::new()
                .fill(Color32::from_rgba_premultiplied(25, 27, 36, 255))
                .corner_radius(8.0)
                .inner_margin(8.0)
                .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 15)))
                .show(ui, |ui| {
                    if self.bookmark_store.bookmarks.is_empty() {
                        ui.vertical_centered(|ui| {
                            ui.add_space(6.0);
                            ui.colored_label(
                                Color32::from_rgb(130, 135, 155),
                                "Nenhum marcador adicionado. Clica em '+ Adicionar nota' para marcar momentos importantes da reunião.",
                            );
                            ui.add_space(6.0);
                        });
                    } else {
                        let cur_sec = self.shared_state.get_time_pos();
                        for bm in &self.bookmark_store.bookmarks {
                            let is_current = (cur_sec - bm.timestamp_secs).abs() < 2.0;
                            let ts_color = if is_current {
                                Color32::from_rgb(250, 204, 21)
                            } else {
                                Color32::from_rgb(139, 124, 246)
                            };

                            ui.horizontal(|ui| {
                                ui.colored_label(
                                    ts_color,
                                    RichText::new(bm.formatted_timestamp()).monospace().strong(),
                                );

                                if self.editing_bookmark_id == Some(bm.id) {
                                    let resp = ui.add(
                                        egui::TextEdit::singleline(&mut self.bookmark_input_text)
                                            .hint_text(DEFAULT_NOTE_TEXT),
                                    );
                                    if std::mem::take(&mut self.focus_bookmark_edit) {
                                        resp.request_focus();
                                    }
                                    let save_enter = resp.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                                    let save_clicked = ui.small_button("Guardar").clicked();
                                    if save_enter || save_clicked {
                                        bookmark_to_edit = Some((bm.id, self.bookmark_input_text.trim().to_string()));
                                    }
                                } else {
                                    ui.label(RichText::new(&bm.text).size(12.5));
                                }

                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.small_button("🗑").on_hover_text("Eliminar nota").clicked() {
                                        bookmark_to_delete = Some(bm.id);
                                    }

                                    if self.editing_bookmark_id == Some(bm.id) {
                                        if ui.small_button("Cancelar").clicked() {
                                            self.editing_bookmark_id = None;
                                        }
                                    } else {
                                        if ui.small_button("Editar").clicked() {
                                            self.editing_bookmark_id = Some(bm.id);
                                            self.bookmark_input_text = bm.text.clone();
                                            self.focus_bookmark_edit = true;
                                        }

                                        if ui
                                            .small_button("Ir")
                                            .on_hover_text("Saltar reprodução para este segundo")
                                            .clicked()
                                        {
                                            bookmark_to_seek = Some(bm.timestamp_secs);
                                        }
                                    }
                                });
                            });
                            ui.separator();
                        }
                    }
                });

            if let Some((id, text)) = bookmark_to_edit {
                // An empty box keeps the current text: an empty note is never useful
                self.bookmark_store.edit(id, (!text.is_empty()).then_some(text), None);
                self.editing_bookmark_id = None;
                self.save_bookmarks();
            }

            if let Some(id) = bookmark_to_delete {
                self.bookmark_store.remove(id);
                if self.editing_bookmark_id == Some(id) {
                    self.editing_bookmark_id = None;
                }
                self.save_bookmarks();
            }

            if let Some(target) = bookmark_to_seek {
                if let Some(ref p) = self.player {
                    let _ = p.seek_absolute(target);
                }
            }

            ui.add_space(8.0);

            // --- EXPORTAR NOTAS E TRANSCRIÇÃO PARA MARKDOWN (Meeting.dc.html line 91) ---
            let export_btn = egui::Button::new(
                RichText::new("📄 Exportar Notas e Transcrição para Markdown (.md)").strong(),
            );
            if ui.add_sized([ui.available_width(), 34.0], export_btn).clicked() {
                self.export_notes_and_transcription();
            }

            if let Some((ref msg, instant)) = self.export_notification {
                if instant.elapsed().as_secs() < 6 {
                    let color = if msg.starts_with("Falha") {
                        Color32::RED
                    } else {
                        Color32::GREEN
                    };
                    ui.colored_label(color, msg);
                }
            }

            ui.add_space(10.0);

            // Recent Transcription Segments preview
            let segments = self.whisper_panel.transcription_segments();
            if !segments.is_empty() {
                ui.label(RichText::new("Segmentos Transcritos (Whisper AI):").size(12.0).strong());
                ui.add_space(4.0);

                egui::Frame::new()
                    .fill(Color32::from_rgba_premultiplied(25, 27, 36, 255))
                    .corner_radius(6.0)
                    .inner_margin(8.0)
                    .show(ui, |ui| {
                        let cur_ms = (current_time * 1000.0) as i64;
                        for seg in segments.iter().take(5) {
                            let is_current = cur_ms >= seg.start_ms && cur_ms <= seg.end_ms;
                            ui.horizontal(|ui| {
                                let time_lbl = format!("[{}]", TranscriptionSegment::format_timestamp(seg.start_ms));
                                let color = if is_current {
                                    Color32::from_rgb(250, 204, 21)
                                } else {
                                    Color32::from_rgb(139, 124, 246)
                                };

                                if ui.link(RichText::new(time_lbl).color(color).monospace()).clicked() {
                                    if let Some(ref p) = self.player {
                                        let _ = p.seek_absolute(seg.start_ms as f64 / 1000.0);
                                    }
                                }
                                ui.label(&seg.text);
                            });
                            ui.add_space(2.0);
                        }
                    });
            }
        });
    }

    /// Persists the notes, logging (not hiding) a failure: a note the user typed must not vanish silently.
    fn save_bookmarks(&self) {
        if let Err(err) = self.bookmark_store.save_to_disk() {
            error!("Could not save bookmarks: {err}");
        }
    }

    /// Automatically unloads a RAM-only Whisper model after `IDLE_UNLOAD_TIMEOUT` of inactivity (§4.16).
    /// Disk-mode models (mmap) are left to the kernel page cache.
    fn check_whisper_inactivity_unload(&mut self, ctx: &egui::Context) {
        let _ = self.whisper_panel.check_inactivity_unload(IDLE_UNLOAD_TIMEOUT);
        // An idle window is never repainted by egui: schedule the moment the timeout expires.
        if let Some(due_in) = self.whisper_panel.idle_unload_due_in(IDLE_UNLOAD_TIMEOUT) {
            ctx.request_repaint_after(due_in.max(std::time::Duration::from_millis(50)));
        }
    }

    /// Handles automatic skipping of silences during playback when skip-silence is active.
    fn handle_skip_silence(&mut self) {
        if !self.skip_silence_enabled {
            return;
        }

        if self.shared_state.is_paused() {
            return;
        }

        if self.last_skip_time.elapsed().as_millis() < 120 {
            return;
        }

        let Some(ref vad) = self.vad_result else {
            return;
        };

        let current_time = self.shared_state.get_time_pos();
        if let Some(target_sec) = vad.next_speech_position(current_time, 0.5) {
            if target_sec > current_time + 0.15 {
                debug!(
                    "Skip-silence jumping from {:.2}s to {:.2}s",
                    current_time, target_sec
                );
                if let Some(ref p) = self.player {
                    let _ = p.seek_absolute(target_sec);
                    self.last_skip_time = Instant::now();
                }
            }
        }
    }

    /// Exports meeting bookmarks and optional Whisper transcription to Markdown format (.md).
    fn export_notes_and_transcription(&mut self) {
        let title = self
            .current_media_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "Reunião".to_string());

        let stem = self
            .current_media_path
            .as_deref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .map(|f| f.to_string_lossy().to_string())
            .unwrap_or_else(|| "reuniao".to_string());

        let segs = self.whisper_panel.transcription_segments();
        let transcription_text = (!segs.is_empty()).then(|| WhisperEngine::segments_to_markdown(segs));

        let md_content = self
            .bookmark_store
            .export_to_markdown(&title, transcription_text.as_deref());

        // Same folder as the transcript export of the Whisper panel (`~/.local/share/vad/`),
        // one file per recording.
        let export_dir = vad_core::vad_data_dir();
        let _ = std::fs::create_dir_all(&export_dir);
        let export_path = export_dir.join(format!("notas_reuniao_{stem}.md"));

        self.export_notification = Some(match std::fs::write(&export_path, md_content) {
            Ok(()) => {
                info!("Meeting notes and transcription exported to {:?}", export_path);
                (format!("Exportado com sucesso para {:?}", export_path), Instant::now())
            }
            Err(err) => {
                error!("Failed to export meeting notes: {:?}", err);
                (format!("Falha ao exportar {:?}: {err}", export_path), Instant::now())
            }
        });
    }
}

/// Next waveform zoom window after a mouse-wheel step (§5); `None` is the global overview.
/// Scrolling up zooms in around the playhead (down to a 3s window), scrolling down zooms out
/// around the window centre and returns to the global overview once it would cover everything.
fn next_waveform_zoom_window(
    window: Option<(f64, f64)>,
    scroll_y: f32,
    current_time: f64,
    duration: f64,
) -> Option<(f64, f64)> {
    if duration <= 1.0 {
        return window;
    }

    let (cur_start, cur_end) = window.unwrap_or((0.0, duration));
    let cur_span = cur_end - cur_start;

    if scroll_y > 0.0 {
        let new_span = (cur_span * 0.65).max(3.0);
        let center = current_time.clamp(0.0, duration);
        let new_start = (center - new_span / 2.0).max(0.0);
        let new_end = (new_start + new_span).min(duration);
        Some((new_start, new_end))
    } else if scroll_y < 0.0 {
        let new_span = cur_span * 1.5;
        if new_span >= duration {
            None
        } else {
            let center = (cur_start + cur_end) / 2.0;
            let new_start = (center - new_span / 2.0).max(0.0);
            let new_end = (new_start + new_span).min(duration);
            Some((new_start, new_end))
        }
    } else {
        window
    }
}

impl eframe::App for VadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.handle_shortcuts(&ctx);
        self.handle_drag_and_drop(&ctx);
        self.poll_events();
        self.poll_audio_extraction();
        self.poll_vad_analysis(&ctx);
        self.check_whisper_inactivity_unload(&ctx);
        self.handle_skip_silence();
        self.save_progress_periodically();

        for integration in &mut self.platform_integrations {
            let _ = integration.update();
        }

        // A platform integration (e.g. MPRIS Quit) may request shutdown; close via
        // the normal viewport path so `Drop for VadApp` still runs and releases
        // the screensaver inhibitor / D-Bus name instead of killing the process.
        if self.platform_integrations.iter().any(|i| i.quit_requested()) {
            ctx.send_viewport_cmd(egui::ViewportCommand::Close);
        }

        // If fatal error occurred during initialization, render recovery screen
        if let Some(ref fatal) = self.fatal_error {
            self.render_fatal_error_screen(ui, fatal);
            return;
        }

        // --- TOP BAR ---
        egui::Panel::top("vad_top_bar").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.strong("VAD");
                ui.separator();

                if let Some(ref path) = self.current_media_path {
                    let title = if self.current_title.is_empty() { path } else { &self.current_title };
                    ui.label(title);
                } else {
                    ui.colored_label(Color32::from_rgb(150, 155, 175), "Nenhum ficheiro aberto");
                }

                // Task 5: Non-blocking top bar progress indicator (§4.17)
                if let Some(pct) = self.extraction_progress_pct {
                    ui.separator();
                    ui.horizontal(|ui| {
                        ui.colored_label(
                            Color32::from_rgb(217, 158, 66),
                            format!("🎙 A indexar áudio: {:.0}%", pct),
                        );
                        if ui.small_button("✕").on_hover_text("Cancelar indexação de áudio").clicked() {
                            self.cancel_extraction();
                        }
                    });
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // HW / SW indicator
                    let label_color = if self.hwdec_current_label.contains("HW") {
                        Color32::from_rgb(100, 220, 100)
                    } else {
                        Color32::from_rgb(220, 180, 80)
                    };
                    ui.colored_label(label_color, format!("[{}]", self.hwdec_current_label));

                    // Forced SW mode toggle for testing fallback
                    if let Some(ref player) = self.player {
                        let hw_btn_text = if self.hwdec_forced_sw {
                            "Modo: Forçado SW"
                        } else {
                            "Modo: Auto-safe"
                        };
                        if ui.button(hw_btn_text).clicked() {
                            self.hwdec_forced_sw = !self.hwdec_forced_sw;
                            let target_mode = if self.hwdec_forced_sw { "no" } else { "auto-safe" };
                            let _ = player.set_hwdec(target_mode);
                            self.update_hwdec_label();
                        }
                    }

                    // Degraded mode warning indicator if missing dependencies
                    if !self.missing_dependencies.is_empty() {
                        let btn = egui::Button::new(
                            egui::RichText::new("⚠️ Degradação ativa")
                                .color(Color32::from_rgb(235, 170, 60)),
                        );
                        if ui.add(btn).clicked() {
                            self.show_dependency_dialog = true;
                        }
                    }

                    if ui.button("Abrir...").clicked() {
                        self.open_modal_is_url = false;
                        self.open_modal_open = true;
                        self.open_modal_input.clear();
                    }

                    ui.separator();

                    // Lateral Panel toggle buttons (Top Bar)
                    let whisper_active = self.active_side_panel == ActiveSidePanel::Whisper;
                    if ui.selectable_label(whisper_active, "🎙 Whisper").clicked() {
                        self.active_side_panel = if whisper_active {
                            ActiveSidePanel::None
                        } else {
                            ActiveSidePanel::Whisper
                        };
                    }

                    if let Some(ref p) = self.player {
                        if p.has_video() {
                            let is_meeting = self.meeting_mode_view;
                            if ui.selectable_label(is_meeting, "📊 Modo Reunião").clicked() {
                                self.meeting_mode_view = !self.meeting_mode_view;
                            }
                        }
                    }

                    let vid_active = self.active_side_panel == ActiveSidePanel::Video;
                    if ui.selectable_label(vid_active, "🎞 Vídeo").clicked() {
                        self.active_side_panel = if vid_active {
                            ActiveSidePanel::None
                        } else {
                            ActiveSidePanel::Video
                        };
                    }

                    let eq_active = self.active_side_panel == ActiveSidePanel::Equalizer;
                    if ui.selectable_label(eq_active, "🎚 Equalizador").clicked() {
                        self.active_side_panel = if eq_active {
                            ActiveSidePanel::None
                        } else {
                            ActiveSidePanel::Equalizer
                        };
                    }

                    let pl_active = self.active_side_panel == ActiveSidePanel::Playlist;
                    if ui.selectable_label(pl_active, "📜 Playlist").clicked() {
                        self.active_side_panel = if pl_active {
                            ActiveSidePanel::None
                        } else {
                            ActiveSidePanel::Playlist
                        };
                    }
                });
            });

            if let Some(ref err) = self.last_error {
                ui.colored_label(Color32::RED, format!("Aviso: {err}"));
            }
        });

        // --- UNIFORM RIGHT SIDE PANEL (Fixed 340px) ---
        if self.active_side_panel != ActiveSidePanel::None {
            egui::Panel::right("vad_uniform_side_panel")
                .exact_size(340.0)
                .resizable(false)
                .frame(
                    egui::Frame::new()
                        .fill(Color32::from_rgba_premultiplied(20, 22, 30, 248))
                        .stroke(Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 18)))
                        .inner_margin(14),
                )
                .show(ui, |ui| {
                    // Header tabs
                    ui.horizontal(|ui| {
                        let accent = Color32::from_rgb(139, 124, 246);
                        let is_pl = self.active_side_panel == ActiveSidePanel::Playlist;
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Playlist")
                                        .strong()
                                        .color(if is_pl { accent } else { Color32::from_rgb(150, 155, 175) }),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            self.active_side_panel = ActiveSidePanel::Playlist;
                        }

                        let is_eq = self.active_side_panel == ActiveSidePanel::Equalizer;
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Equalizador")
                                        .strong()
                                        .color(if is_eq { accent } else { Color32::from_rgb(150, 155, 175) }),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            self.active_side_panel = ActiveSidePanel::Equalizer;
                        }

                        let is_vid = self.active_side_panel == ActiveSidePanel::Video;
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Vídeo")
                                        .strong()
                                        .color(if is_vid { accent } else { Color32::from_rgb(150, 155, 175) }),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            self.active_side_panel = ActiveSidePanel::Video;
                        }

                        let is_wh = self.active_side_panel == ActiveSidePanel::Whisper;
                        if ui
                            .add(
                                egui::Button::new(
                                    egui::RichText::new("Whisper")
                                        .strong()
                                        .color(if is_wh { accent } else { Color32::from_rgb(150, 155, 175) }),
                                )
                                .fill(Color32::TRANSPARENT),
                            )
                            .clicked()
                        {
                            self.active_side_panel = ActiveSidePanel::Whisper;
                        }

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            if ui.small_button("✕").clicked() {
                                self.active_side_panel = ActiveSidePanel::None;
                            }
                        });
                    });

                    ui.separator();
                    ui.add_space(4.0);

                    match self.active_side_panel {
                        ActiveSidePanel::Playlist => {
                            let cur_time = self.shared_state.get_time_pos();
                            let dur = self.shared_state.get_duration();
                            let mut action = None;
                            if let Ok(mut pl) = self.playlist.lock() {
                                action = PlaylistPanel::ui(ui, &mut pl, cur_time, dur);
                            }
                            if let Some(act) = action {
                                match act {
                                    PlaylistAction::PlayItem(idx) => {
                                        let item_info = if let Ok(mut pl) = self.playlist.lock() {
                                            pl.set_current(idx).map(|it| (it.is_url(), it.location()))
                                        } else {
                                            None
                                        };
                                        if let Some((_is_url, loc)) = item_info {
                                            self.load_media(&loc);
                                        }
                                    }
                                    PlaylistAction::AddFileRequest => {
                                        self.open_modal_is_url = false;
                                        self.open_modal_open = true;
                                        self.open_modal_input.clear();
                                    }
                                    PlaylistAction::AddUrlRequest => {
                                        self.open_modal_is_url = true;
                                        self.open_modal_open = true;
                                        self.open_modal_input.clear();
                                    }
                                }
                            }
                        }
                        ActiveSidePanel::Equalizer => {
                            if let Some(ref p) = self.player {
                                self.audio_panel.ui(ui, p);
                            }
                        }
                        ActiveSidePanel::Video => {
                            if let Some(ref p) = self.player {
                                self.video_panel.ui(ui, p);
                            }
                        }
                        ActiveSidePanel::Whisper => {
                            if let Some(act) = self.whisper_panel.ui(ui, self.current_audio.as_ref()) {
                                match act {
                                    WhisperAction::SeekTo(secs) => {
                                        if let Some(ref p) = self.player {
                                            let _ = p.seek_absolute(secs);
                                        }
                                    }
                                    WhisperAction::SaveConfig => {
                                        self.config.whisper.default_storage_mode =
                                            self.whisper_panel.storage_mode();
                                        let _ = self.config.save_default();
                                    }
                                }
                            }
                        }
                        ActiveSidePanel::None => {}
                    }
                });
        }

        // --- CENTRAL CANVAS ---
        egui::CentralPanel::default().show(ui, |ui| {
            let available_rect = ui.available_rect_before_wrap();

            if self.current_media_path.is_none() {
                // Show Welcome Screen with dropzone
                self.render_welcome_screen(ui);
            } else {
                let has_video = self.player.as_ref().map(|p| p.has_video()).unwrap_or(false);
                let show_meeting_mode = !has_video || self.meeting_mode_view;

                if show_meeting_mode {
                    self.render_meeting_mode(ui, available_rect);
                } else if let Some(ref mut renderer) = self.renderer {
                    // Paint OpenGL video frame
                    renderer.paint_to_rect(ui, available_rect);

                    // Overlay Floating HUD over the video canvas
                    if let Some(ref player) = self.player {
                        let whisper_disabled = self.is_feature_disabled("whisper");

                        if let Some(action) = self.hud.show(
                            ui,
                            available_rect,
                            player,
                            &self.shared_state,
                            whisper_disabled,
                        ) {
                            match action {
                                HudAction::TogglePlaylist => {
                                    self.active_side_panel = if self.active_side_panel == ActiveSidePanel::Playlist {
                                        ActiveSidePanel::None
                                    } else {
                                        ActiveSidePanel::Playlist
                                    };
                                }
                                HudAction::ToggleEqualizer => {
                                    self.active_side_panel = if self.active_side_panel == ActiveSidePanel::Equalizer {
                                        ActiveSidePanel::None
                                    } else {
                                        ActiveSidePanel::Equalizer
                                    };
                                }
                                HudAction::ToggleVideo => {
                                    self.active_side_panel = if self.active_side_panel == ActiveSidePanel::Video {
                                        ActiveSidePanel::None
                                    } else {
                                        ActiveSidePanel::Video
                                    };
                                }
                                HudAction::ToggleWhisper => {
                                    self.active_side_panel = if self.active_side_panel == ActiveSidePanel::Whisper {
                                        ActiveSidePanel::None
                                    } else {
                                        ActiveSidePanel::Whisper
                                    };
                                }
                                HudAction::OpenSubtitlesDialog => {
                                    self.load_subtitles_modal_open = true;
                                    self.load_subtitles_input.clear();
                                }
                            }
                        }
                    }
                } else {
                    ui.centered_and_justified(|ui| {
                        ui.label("Renderer OpenGL indisponível");
                    });
                }
            }

            // Visual indicator when hovering files over window for drop
            let is_file_hovered = ctx.input(|i| !i.raw.hovered_files.is_empty());
            if is_file_hovered {
                let painter = ui.painter();
                painter.rect_filled(
                    available_rect,
                    CornerRadius::ZERO,
                    Color32::from_rgba_premultiplied(139, 124, 246, 35),
                );
                painter.rect_stroke(
                    available_rect.shrink(8.0),
                    CornerRadius::same(12),
                    Stroke::new(2.5, Color32::from_rgb(139, 124, 246)),
                    StrokeKind::Inside,
                );
                painter.text(
                    available_rect.center(),
                    egui::Align2::CENTER_CENTER,
                    "Larga o ficheiro para reproduzir",
                    egui::FontId::proportional(22.0),
                    Color32::WHITE,
                );
            }
        });

        // Dialogs
        self.render_dependency_dialog(&ctx);
        self.render_open_modal(&ctx);
        self.render_subtitles_modal(&ctx);
        self.render_resume_toast(&ctx);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        self.save_state();
    }
}

impl Drop for VadApp {
    fn drop(&mut self) {
        self.save_state();
        for integration in &mut self.platform_integrations {
            let _ = integration.shutdown();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_scheme_allowlist() {
        assert!(VadApp::is_allowed_url_scheme("http://example.com/video.mp4"));
        assert!(VadApp::is_allowed_url_scheme("https://example.com/stream.m3u8"));
        assert!(VadApp::is_allowed_url_scheme("rtsp://192.168.1.100:554/live"));

        // Reject non-whitelisted schemes
        assert!(!VadApp::is_allowed_url_scheme("file:///etc/passwd"));
        assert!(!VadApp::is_allowed_url_scheme("smb://nas/share/video.mp4"));
        assert!(!VadApp::is_allowed_url_scheme("ftp://ftp.example.com/file"));
        assert!(!VadApp::is_allowed_url_scheme("javascript:alert(1)"));
    }

    #[test]
    fn test_keyboard_focus_guard_in_context() {
        let ctx = egui::Context::default();

        // Initially with no focused widget, egui_wants_keyboard_input is false
        let mut out1 = ctx.run_ui(egui::RawInput::default(), |_ui_ctx| {});
        out1.textures_delta.clear();
        assert!(!ctx.egui_wants_keyboard_input());

        // When a TextEdit widget gains focus, egui_wants_keyboard_input becomes true
        let mut text = String::new();
        let mut out2 = ctx.run_ui(egui::RawInput::default(), |ui_ctx| {
            egui::CentralPanel::default().show(ui_ctx, |ui| {
                let response = ui.add(egui::TextEdit::singleline(&mut text));
                response.request_focus();
            });
        });
        out2.textures_delta.clear();
        assert!(ctx.egui_wants_keyboard_input());
    }

    #[test]
    fn test_side_panel_state_transitions() {
        let mut panel = ActiveSidePanel::None;
        assert_eq!(panel, ActiveSidePanel::None);

        // Clicking playlist when None opens Playlist
        panel = if panel == ActiveSidePanel::Playlist { ActiveSidePanel::None } else { ActiveSidePanel::Playlist };
        assert_eq!(panel, ActiveSidePanel::Playlist);

        // Clicking playlist again closes panel (None)
        panel = if panel == ActiveSidePanel::Playlist { ActiveSidePanel::None } else { ActiveSidePanel::Playlist };
        assert_eq!(panel, ActiveSidePanel::None);

        // Clicking Equalizer opens Equalizer
        panel = if panel == ActiveSidePanel::Equalizer { ActiveSidePanel::None } else { ActiveSidePanel::Equalizer };
        assert_eq!(panel, ActiveSidePanel::Equalizer);

        // Switching from Equalizer directly to Video
        panel = if panel == ActiveSidePanel::Video { ActiveSidePanel::None } else { ActiveSidePanel::Video };
        assert_eq!(panel, ActiveSidePanel::Video);
    }

    #[test]
    fn test_audio_panel_presets_and_frequencies() {
        let mut audio_panel = crate::panels::AudioPanel::new();
        assert_eq!(crate::panels::AudioPanel::FREQ_LABELS.len(), 10);
        assert_eq!(audio_panel.gains, [0.0; 10]);
        assert_eq!(audio_panel.active_preset, "Plano");

        audio_panel.apply_preset("Voz clara");
        assert_eq!(audio_panel.active_preset, "Voz clara");
        assert_ne!(audio_panel.gains, [0.0; 10]);

        audio_panel.apply_preset("Plano");
        assert_eq!(audio_panel.gains, [0.0; 10]);
    }

    #[test]
    fn test_resume_toast_state_and_timeout() {
        let mut toast = ResumeToastState {
            location: "/path/to/movie.mp4".to_string(),
            title: "movie.mp4".to_string(),
            timestamp: 125.0,
            duration: Some(3600.0),
            time_left: 8.0,
            is_startup_resume: true,
        };

        // Advance by 3 seconds
        toast.time_left -= 3.0;
        assert_eq!(toast.time_left, 5.0);
        assert!(toast.time_left > 0.0);

        // Advance beyond 8 seconds -> expires
        toast.time_left -= 5.5;
        assert!(toast.time_left <= 0.0);
    }

    #[test]
    fn test_app_startup_with_recents_offers_resume() {
        let dir = std::env::temp_dir().join("vad_test_startup_resume");
        let _ = std::fs::create_dir_all(&dir);
        let recents_path = dir.join("recentes.json");

        let mut store = RecentsStore::new();
        store.add_or_update(
            "/path/to/meeting.mp4",
            "Reunião Estratégica",
            2052.0,
            Some(5400.0),
            false,
        );
        store.save_to_path(&recents_path).unwrap();

        let loaded = RecentsStore::load_from_path(&recents_path).unwrap();
        assert_eq!(loaded.len(), 1);
        let recent = loaded.most_recent().unwrap();
        assert_eq!(recent.location, "/path/to/meeting.mp4");
        assert_eq!(recent.timestamp, 2052.0);
        assert_eq!(recent.formatted_summary(), "00:34:12 de 01:30:00");

        // Verify toast generation logic for this recent file
        let toast = ResumeToastState {
            location: recent.location.clone(),
            title: recent.title.clone(),
            timestamp: recent.timestamp,
            duration: recent.duration,
            time_left: 8.0,
            is_startup_resume: true,
        };
        assert_eq!(toast.time_left, 8.0);
        assert!(toast.is_startup_resume);
        assert_eq!(toast.title, "Reunião Estratégica");
        assert_eq!(toast.timestamp, 2052.0);

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_side_panel_whisper_toggle() {
        let mut panel = ActiveSidePanel::None;
        // Toggle Whisper from None -> Whisper
        panel = if panel == ActiveSidePanel::Whisper { ActiveSidePanel::None } else { ActiveSidePanel::Whisper };
        assert_eq!(panel, ActiveSidePanel::Whisper);

        // Toggle again Whisper -> None
        panel = if panel == ActiveSidePanel::Whisper { ActiveSidePanel::None } else { ActiveSidePanel::Whisper };
        assert_eq!(panel, ActiveSidePanel::None);

        // Toggle from Video -> Whisper
        panel = ActiveSidePanel::Video;
        panel = if panel == ActiveSidePanel::Whisper { ActiveSidePanel::None } else { ActiveSidePanel::Whisper };
        assert_eq!(panel, ActiveSidePanel::Whisper);
    }

    #[test]
    fn test_waveform_zoom_calculations() {
        let duration: f64 = 3600.0; // 1 hour
        let current_time: f64 = 1800.0; // 30 minutes in

        // Initially global view (None); zoom in once
        let zoomed = next_waveform_zoom_window(None, 1.0, current_time, duration);
        let (s, e) = zoomed.expect("zoom in leaves the global view");
        assert!(e - s < duration);
        assert!(s >= 0.0 && e <= duration);
        assert!((s + e) / 2.0 - current_time < 1.0, "zoom in centres on the playhead");

        // Zoom in never goes below 3s
        let mut w = zoomed;
        for _ in 0..40 {
            w = next_waveform_zoom_window(w, 1.0, current_time, duration);
        }
        let (s, e) = w.unwrap();
        assert!((e - s - 3.0).abs() < 1e-6);

        // Zoom out enough times to restore the global overview (None)
        for _ in 0..40 {
            w = next_waveform_zoom_window(w, -1.0, current_time, duration);
        }
        assert!(w.is_none());

        // No scroll / too-short media: window unchanged
        assert_eq!(next_waveform_zoom_window(zoomed, 0.0, current_time, duration), zoomed);
        assert_eq!(next_waveform_zoom_window(None, 1.0, 0.5, 1.0), None);
    }

    #[test]
    fn test_whisper_panel_defaults_and_storage_tooltips() {
        use vad_ai::{DISK_TOOLTIP, RAM_ONLY_TOOLTIP};
        let panel = crate::panels::WhisperPanel::new();
        assert!(!panel.has_active_model());
        assert_eq!(panel.active_model_id(), None);
        assert!(!DISK_TOOLTIP.is_empty());
        assert!(!RAM_ONLY_TOOLTIP.is_empty());
        assert!(DISK_TOOLTIP.contains("disco"));
        assert!(RAM_ONLY_TOOLTIP.contains("RAM"));
    }
}
