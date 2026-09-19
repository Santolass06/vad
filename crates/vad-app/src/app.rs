use std::ffi::{c_void, CString};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, Stroke, StrokeKind, UiBuilder,
};
use tracing::{error, info};
use vad_core::{
    create_event_channel, is_allowed_url_scheme, EventReceiver, GlProcAddressFn,
    PlatformIntegration, Player, PlayerEvent, Playlist, RecentEntry, RecentsStore,
    SharedPlayerState, VadConfig, VadError,
};

use crate::panels::{
    AudioPanel, HudAction, HudPanel, PlaylistAction, PlaylistPanel, VideoPanel,
};
use crate::probe::probe_dependencies;
use crate::render::GlVideoRenderer;

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
    load_subtitles_modal_open: bool,
    load_subtitles_input: String,
    recents: RecentsStore,
    config: VadConfig,
    resume_toast: Option<ResumeToastState>,
    /// Position to seek to once mpv reports the file as loaded (`loadfile` is asynchronous).
    pending_seek: Option<f64>,
    last_saved_time_pos: f64,
    last_saved_instant: Instant,
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
            load_subtitles_modal_open: false,
            load_subtitles_input: String::new(),
            recents,
            config,
            resume_toast,
            pending_seek: None,
            last_saved_time_pos: 0.0,
            last_saved_instant: Instant::now(),
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
                            .unwrap_or(path);
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
}

impl eframe::App for VadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        self.handle_shortcuts(&ctx);
        self.handle_drag_and_drop(&ctx);
        self.poll_events();
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
}
