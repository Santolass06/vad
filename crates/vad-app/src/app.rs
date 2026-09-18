use std::ffi::{c_void, CString};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, Stroke, StrokeKind, UiBuilder,
};
use tracing::{error, info};
use vad_core::{
    create_event_channel, EventReceiver, GlProcAddressFn, PlatformIntegration, Player,
    PlayerEvent, Playlist, SharedPlayerState, VadError,
};

use crate::panels::{
    AudioPanel, HudAction, HudPanel, PlaylistAction, PlaylistPanel, VideoPanel,
};
use crate::probe::probe_dependencies;
use crate::render::GlVideoRenderer;

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
            audio_panel: AudioPanel::new(),
            load_subtitles_modal_open: false,
            load_subtitles_input: String::new(),
        };

        if let Some(path) = initial_file {
            app.load_media(&path);
        }

        app
    }

    pub fn load_media(&mut self, path: &str) {
        let trimmed = path.trim();
        if trimmed.is_empty() {
            return;
        }

        if let Some(ref player) = self.player {
            info!("Loading media: {}", trimmed);
            if let Err(e) = player.load_file(trimmed) {
                self.last_error = Some(format!("Erro ao carregar ficheiro: {e:?}"));
            } else {
                self.current_media_path = Some(trimmed.to_string());
                self.current_title = std::path::Path::new(trimmed)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| trimmed.to_string());
                self.last_error = None;

                if let Ok(mut pl) = self.playlist.lock() {
                    let found = pl.items().iter().position(|it| it.location() == trimmed);
                    if let Some(idx) = found {
                        pl.set_current(idx);
                    } else {
                        let idx = pl.add_file(trimmed);
                        pl.set_current(idx);
                    }
                }

                self.hud.poke();
            }
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
                    self.update_hwdec_label();
                    self.hud.poke();
                }
                PlayerEvent::EndOfFile => {
                    let next_item = if let Ok(mut pl) = self.playlist.lock() {
                        pl.next().cloned()
                    } else {
                        None
                    };

                    if let Some(item) = next_item {
                        if item.is_url() {
                            self.hud.set_notification("A reprodução de URLs requer a Sprint 05");
                        } else {
                            let path = item.location();
                            self.load_media(&path);
                        }
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
    fn is_allowed_url_scheme(input: &str) -> bool {
        const ALLOWED_SCHEMES: [&str; 3] = ["http://", "https://", "rtsp://"];
        ALLOWED_SCHEMES.iter().any(|scheme| input.starts_with(scheme))
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
            if self.show_dependency_dialog {
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

            // Reserved layout space for recents.rs (Sprint 05)
            let recents_rect = Rect::from_center_size(
                pos2(available_rect.center().x, available_rect.center().y + 110.0),
                vec2(480.0, 80.0),
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
                    .max_rect(recents_rect.shrink(12.0))
                    .layout(egui::Layout::top_down(egui::Align::Center)),
            );
            recents_ui.colored_label(Color32::from_rgb(140, 145, 165), "Ficheiros Recentes");
            recents_ui.add_space(4.0);
            recents_ui.colored_label(
                Color32::from_rgb(100, 105, 120),
                "(Histórico e ponto de retoma disponíveis na Sprint 05)",
            );
        });
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
                                        if let Some((is_url, loc)) = item_info {
                                            if is_url {
                                                self.hud.set_notification("A reprodução de URLs requer a Sprint 05");
                                            } else {
                                                self.load_media(&loc);
                                            }
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
    }
}

impl Drop for VadApp {
    fn drop(&mut self) {
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
}
