use std::ffi::{c_void, CString};
use std::sync::Arc;
use eframe::egui;
use tracing::{error, info};
use vad_core::{
    create_event_channel, EventReceiver, GlProcAddressFn, Player,
    PlayerEvent, SharedPlayerState,
};

use crate::render::GlVideoRenderer;

/// Main GUI application for VAD.
pub struct VadApp {
    player: Player,
    renderer: Option<GlVideoRenderer>,
    event_rx: EventReceiver,
    shared_state: Arc<SharedPlayerState>,
    hwdec_current_label: String,
    hwdec_forced_sw: bool,
    file_path_input: String,
    _current_title: String,
    last_error: Option<String>,
    volume: f32,
    muted: bool,
}

impl VadApp {
    pub fn new(cc: &eframe::CreationContext<'_>, initial_file: Option<String>) -> Self {
        let (event_tx, event_rx) = create_event_channel();
        let shared_state = Arc::new(SharedPlayerState::new());

        let player = Player::new().expect("Failed to initialize libmpv player");
        let _ = player.start_event_loop(event_tx.clone(), Arc::clone(&shared_state));

        // Setup OpenGL video renderer if glow context is present
        let renderer = if let Some(gl) = cc.gl.clone() {
            let proc_addr = cc
                .get_proc_address
                .clone()
                .expect("OpenGL get_proc_address unavailable");
            let gpa: GlProcAddressFn = Arc::new(move |name: &str| -> *mut c_void {
                CString::new(name).ok().map_or(std::ptr::null_mut(), |s| {
                    (proc_addr)(&s).cast_mut()
                })
            });

            match player.create_render_context(gpa) {
                Ok(render_ctx) => {
                    info!("Successfully created mpv_render_context with OpenGL backend");
                    Some(GlVideoRenderer::new(
                        gl,
                        render_ctx,
                        cc.egui_ctx.clone(),
                        Some(event_tx),
                    ))
                }
                Err(err) => {
                    error!("Failed to create mpv_render_context: {:?}", err);
                    None
                }
            }
        } else {
            error!("OpenGL (glow) context was not initialized by eframe");
            None
        };

        let app = Self {
            player,
            renderer,
            event_rx,
            shared_state,
            hwdec_current_label: "Detecting...".to_string(),
            hwdec_forced_sw: false,
            file_path_input: initial_file
                .clone()
                .unwrap_or_else(|| "/tmp/M0_test_1080p_h264_aac.mp4".to_string()),
            _current_title: String::new(),
            last_error: None,
            volume: 100.0,
            muted: false,
        };

        if let Some(path) = initial_file {
            if let Err(e) = app.player.load_file(&path) {
                error!("Failed to load initial file {path}: {:?}", e);
            }
        }

        app
    }

    fn poll_events(&mut self) {
        while let Ok(event) = self.event_rx.try_recv() {
            match event {
                PlayerEvent::FileLoaded { title, duration, path } => {
                    self._current_title = title.unwrap_or_else(|| path.clone());
                    if let Some(dur) = duration {
                        self.shared_state.set_duration(dur);
                    }
                    self.update_hwdec_label();
                }
                PlayerEvent::HwdecChanged(hw) => match hw {
                    Some(val) => self.hwdec_current_label = format!("HW ({val})"),
                    None => self.hwdec_current_label = "SW (CPU)".to_string(),
                },
                PlayerEvent::VolumeChanged(vol) => {
                    self.volume = vol as f32;
                }
                PlayerEvent::MutedChanged(mute) => {
                    self.muted = mute;
                }
                PlayerEvent::Error(err) => {
                    self.last_error = Some(err);
                }
                _ => {}
            }
        }

        // Periodic sync of hwdec label if still in default state
        if self.hwdec_current_label == "Detecting..." {
            self.update_hwdec_label();
        }
    }

    fn update_hwdec_label(&mut self) {
        if let Ok(hw) = self.player.hwdec_current() {
            match hw {
                Some(val) => self.hwdec_current_label = format!("HW ({val})"),
                None => self.hwdec_current_label = "SW (CPU)".to_string(),
            }
        }
    }

    fn format_time(seconds: f64) -> String {
        let s = seconds.max(0.0) as u64;
        let m = s / 60;
        let s = s % 60;
        let h = m / 60;
        let m = m % 60;
        if h > 0 {
            format!("{h:02}:{m:02}:{s:02}")
        } else {
            format!("{m:02}:{s:02}")
        }
    }
}

impl eframe::App for VadApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        self.poll_events();

        egui::Panel::top("vad_top_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.heading("VAD Video Player (M0.5)");
                ui.separator();

                ui.label("Ficheiro:");
                ui.text_edit_singleline(&mut self.file_path_input);
                if ui.button("Carregar").clicked() {
                    let path = self.file_path_input.trim().to_string();
                    if !path.is_empty() {
                        if let Err(e) = self.player.load_file(&path) {
                            self.last_error = Some(format!("Erro ao carregar: {e:?}"));
                        }
                    }
                }

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    // HW / SW forced test button for gate M0.5 validation
                    let hw_btn_text = if self.hwdec_forced_sw {
                        "Modo: Forçado SW [Restaurar HW]"
                    } else {
                        "Modo: Auto-safe [Forçar SW]"
                    };

                    if ui.button(hw_btn_text).clicked() {
                        self.hwdec_forced_sw = !self.hwdec_forced_sw;
                        let target_mode = if self.hwdec_forced_sw { "no" } else { "auto-safe" };
                        if let Err(e) = self.player.set_hwdec(target_mode) {
                            self.last_error = Some(format!("Falha ao alternar hwdec: {e:?}"));
                        }
                        self.update_hwdec_label();
                    }

                    // Real hwdec-current indicator
                    let label_color = if self.hwdec_current_label.contains("HW") {
                        egui::Color32::from_rgb(100, 220, 100)
                    } else {
                        egui::Color32::from_rgb(220, 180, 80)
                    };
                    ui.colored_label(label_color, format!("[Descodificação: {}]", self.hwdec_current_label));
                });
            });

            if let Some(ref err) = self.last_error {
                ui.colored_label(egui::Color32::RED, format!("Aviso: {err}"));
            }
        });

        egui::Panel::bottom("vad_controls_panel").show(ui, |ui| {
            ui.horizontal(|ui| {
                // Play / Pause toggle
                let is_paused = self.shared_state.is_paused();
                let play_pause_icon = if is_paused { "▶ Play" } else { "⏸ Pause" };
                if ui.button(play_pause_icon).clicked() {
                    let _ = self.player.toggle_pause();
                }

                // Seek buttons (-5s / +5s)
                if ui.button("⏮ -5s").clicked() {
                    let _ = self.player.seek_relative(-5.0);
                }
                if ui.button("+5s ⏭").clicked() {
                    let _ = self.player.seek_relative(5.0);
                }

                // Time and duration display
                let current_pos = self.shared_state.get_time_pos();
                let duration = self.shared_state.get_duration();
                ui.label(format!("{} / {}", Self::format_time(current_pos), Self::format_time(duration)));

                // Seekbar scrubber
                let mut seek_pos = current_pos;
                let slider = egui::Slider::new(&mut seek_pos, 0.0..=duration.max(1.0))
                    .show_value(false);
                let response = ui.add_sized(egui::vec2(ui.available_width() - 180.0, 20.0), slider);
                if response.drag_stopped() || (response.changed() && !response.dragged()) {
                    let _ = self.player.seek_absolute(seek_pos);
                }

                // Volume controls
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let mut vol = self.volume;
                    if ui.add(egui::Slider::new(&mut vol, 0.0..=100.0).show_value(false)).changed() {
                        let _ = self.player.set_volume(vol as f64);
                    }

                    let mute_text = if self.muted { "🔇" } else { "🔊" };
                    if ui.button(mute_text).clicked() {
                        let _ = self.player.toggle_mute();
                    }
                });
            });
        });

        egui::CentralPanel::default().show(ui, |ui| {
            let available_rect = ui.available_rect_before_wrap();
            if let Some(ref mut renderer) = self.renderer {
                renderer.paint_to_rect(ui, available_rect);
            } else {
                ui.centered_and_justified(|ui| {
                    ui.label("Renderer OpenGL indisponível");
                });
            }
        });
    }
}
