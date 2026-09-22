use std::sync::Arc;
use std::time::{Duration, Instant};

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, UiBuilder,
};
use vad_core::{AbLoopStatus, Player, SharedPlayerState, TrackInfo};

/// Actions emitted by the HUD to toggle lateral panels or open external assets.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudAction {
    TogglePlaylist,
    ToggleEqualizer,
    ToggleVideo,
    ToggleWhisper,
    ToggleClipExport,
    OpenSubtitlesDialog,
}


/// Floating HUD panel overlaying the video canvas.
/// Complies with PLANO_VAD.md §4.29 (fixed ~92% opacity, no blur shader)
/// and implements 2-second auto-hide with HiDPI-friendly 20px hit targets.
pub struct HudPanel {
    last_activity: Instant,
    is_visible: bool,
    scrub_drag_time: Option<f64>,
    cached_audio_tracks: Vec<TrackInfo>,
    cached_sub_tracks: Vec<TrackInfo>,
    last_tracks_query: Instant,
    notification: Option<(String, Instant)>,
}

impl Default for HudPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl HudPanel {
    pub fn new() -> Self {
        Self {
            last_activity: Instant::now(),
            is_visible: true,
            scrub_drag_time: None,
            cached_audio_tracks: Vec::new(),
            cached_sub_tracks: Vec::new(),
            last_tracks_query: Instant::now() - Duration::from_secs(10),
            notification: None,
        }
    }

    /// Resets activity timer, forcing HUD to stay visible.
    pub fn poke(&mut self) {
        self.last_activity = Instant::now();
        self.is_visible = true;
    }

    pub fn set_notification(&mut self, text: impl Into<String>) {
        self.notification = Some((text.into(), Instant::now()));
        self.poke();
    }

    /// Formats seconds into HH:MM:SS or MM:SS.
    pub fn format_time(seconds: f64) -> String {
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

    /// Renders the custom scrubber with ~20px touch target, 4px visual height,
    /// 8px on hover, and 16px handle during dragging (§4.29).
    /// Returns `Some(target_seconds)` if seek was triggered.
    fn render_scrubber(
        &mut self,
        ui: &mut egui::Ui,
        current_time: f64,
        duration: f64,
    ) -> Option<f64> {
        let available_w = ui.available_width().max(100.0);
        let (rect, response) = ui.allocate_exact_size(vec2(available_w, 20.0), Sense::click_and_drag());

        let is_hovered = response.hovered();
        let is_dragged = response.dragged();

        let track_h = if is_hovered || is_dragged { 8.0 } else { 4.0 };
        let track_rect = Rect::from_center_size(rect.center(), vec2(rect.width(), track_h));

        let duration_safe = duration.max(0.001);
        let mut target_seek = None;

        // Calculate progress fraction
        let fraction = if is_dragged {
            if let Some(ptr) = response.interact_pointer_pos() {
                let frac = ((ptr.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                self.scrub_drag_time = Some(frac as f64 * duration_safe);
                frac
            } else {
                (current_time / duration_safe).clamp(0.0, 1.0) as f32
            }
        } else if let Some(preview) = self.scrub_drag_time {
            (preview / duration_safe).clamp(0.0, 1.0) as f32
        } else {
            (current_time / duration_safe).clamp(0.0, 1.0) as f32
        };

        if response.drag_stopped() {
            if let Some(target) = self.scrub_drag_time.take() {
                target_seek = Some(target);
            }
        } else if response.clicked() {
            if let Some(ptr) = response.interact_pointer_pos() {
                let frac = ((ptr.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let target = frac as f64 * duration_safe;
                self.scrub_drag_time = None;
                target_seek = Some(target);
            }
        }

        // Painter rendering
        let painter = ui.painter_at(rect);
        let radius_u8 = (track_h / 2.0).round() as u8;

        // Background track
        painter.rect_filled(
            track_rect,
            CornerRadius::same(radius_u8),
            Color32::from_rgba_unmultiplied(255, 255, 255, 30),
        );

        // Filled progress (accent color #8b7cf6)
        let filled_w = track_rect.width() * fraction;
        let filled_rect = Rect::from_min_max(
            track_rect.min,
            pos2(track_rect.left() + filled_w, track_rect.max.y),
        );
        let accent_color = Color32::from_rgb(139, 124, 246);
        painter.rect_filled(filled_rect, CornerRadius::same(radius_u8), accent_color);

        // Scrubber thumb handle (16px diameter when dragged, 12px when hovered)
        if is_dragged || is_hovered {
            let radius = if is_dragged { 8.0 } else { 6.0 };
            let center = pos2(track_rect.left() + filled_w, track_rect.center().y);

            // Subtle outer halo
            painter.circle_filled(
                center,
                radius + 3.0,
                Color32::from_rgba_unmultiplied(139, 124, 246, 70),
            );
            // Thumb inner circle
            painter.circle_filled(center, radius, Color32::from_rgb(245, 245, 250));
            painter.circle_stroke(center, radius, Stroke::new(1.5, accent_color));
        }

        target_seek
    }

    /// Renders custom volume slider with 20px hit area and expanding visual bar.
    fn render_volume_slider(&mut self, ui: &mut egui::Ui, volume: f64) -> Option<f64> {
        let (rect, response) = ui.allocate_exact_size(vec2(76.0, 20.0), Sense::click_and_drag());
        let is_hovered = response.hovered();
        let is_dragged = response.dragged();

        let track_h = if is_hovered || is_dragged { 6.0 } else { 3.5 };
        let track_rect = Rect::from_center_size(rect.center(), vec2(rect.width(), track_h));

        // Player::set_volume/volume() operate on a unified 0..=200 scale (the
        // Equalizer panel's "Volume Boost" writes the same property up to 200%),
        // so this slider must span the same range — otherwise dragging it here
        // would silently clamp a boosted volume back down to 100%.
        let max_vol = 200.0_f64;
        let mut new_vol = None;

        let fraction = if is_dragged || response.clicked() {
            if let Some(ptr) = response.interact_pointer_pos() {
                let frac = ((ptr.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
                let vol = (frac as f64 * max_vol).clamp(0.0, max_vol);
                new_vol = Some(vol);
                frac
            } else {
                (volume / max_vol).clamp(0.0, 1.0) as f32
            }
        } else {
            (volume / max_vol).clamp(0.0, 1.0) as f32
        };

        let painter = ui.painter_at(rect);
        let radius_u8 = (track_h / 2.0).round() as u8;

        // Background track
        painter.rect_filled(
            track_rect,
            CornerRadius::same(radius_u8),
            Color32::from_rgba_unmultiplied(255, 255, 255, 30),
        );

        // Filled track
        let filled_w = track_rect.width() * fraction;
        let filled_rect = Rect::from_min_max(
            track_rect.min,
            pos2(track_rect.left() + filled_w, track_rect.max.y),
        );
        painter.rect_filled(
            filled_rect,
            CornerRadius::same(radius_u8),
            Color32::from_rgb(200, 200, 215),
        );

        // Thumb
        if is_dragged || is_hovered {
            let radius = if is_dragged { 7.0 } else { 5.0 };
            let center = pos2(track_rect.left() + filled_w, track_rect.center().y);
            painter.circle_filled(center, radius, Color32::from_rgb(245, 245, 250));
            painter.circle_stroke(
                center,
                radius,
                Stroke::new(1.0, Color32::from_rgb(139, 124, 246)),
            );
        }

        new_vol
    }

    /// Refreshes track listings periodically or on media changes.
    fn update_tracks_cache(&mut self, player: &Player) {
        if self.last_tracks_query.elapsed() > Duration::from_secs(3) {
            if let Ok(tracks) = player.audio_tracks() {
                self.cached_audio_tracks = tracks;
            }
            if let Ok(tracks) = player.subtitle_tracks() {
                self.cached_sub_tracks = tracks;
            }
            self.last_tracks_query = Instant::now();
        }
    }

    /// Renders the floating HUD inside the provided container rect.
    pub fn show(
        &mut self,
        ui: &mut egui::Ui,
        container_rect: Rect,
        player: &Player,
        shared_state: &Arc<SharedPlayerState>,
        whisper_disabled: bool,
    ) -> Option<HudAction> {
        let mut action = None;
        let is_paused = shared_state.is_paused();
        let current_time = shared_state.get_time_pos();
        let duration = shared_state.get_duration();

        // 1. Auto-hide logic (2 seconds timeout during playback)
        let pointer_pos = ui.input(|i| i.pointer.hover_pos());
        let pointer_moved = ui.input(|i| i.pointer.delta() != egui::Vec2::ZERO || i.pointer.any_down());

        if pointer_moved {
            self.poke();
        }

        // Keep visible when paused or when active
        if is_paused {
            self.is_visible = true;
        } else if self.last_activity.elapsed() > Duration::from_secs(2) {
            self.is_visible = false;
        }

        // Request repaint while visible during playback to animate/trigger auto-hide
        if self.is_visible && !is_paused {
            ui.ctx().request_repaint_after(Duration::from_millis(150));
        }

        if !self.is_visible {
            return None;
        }

        self.update_tracks_cache(player);

        // 2. Compute Floating HUD positioning (centered bottom, 24px margin)
        let hud_max_width = 1000.0_f32.min(container_rect.width() - 40.0);
        let hud_width = hud_max_width.max(340.0);
        let hud_bottom = container_rect.max.y - 24.0;
        let hud_left = container_rect.center().x - (hud_width / 2.0);

        let hud_rect = Rect::from_min_size(pos2(hud_left, hud_bottom - 128.0), vec2(hud_width, 128.0));

        // Hover over HUD keeps it visible
        if let Some(pos) = pointer_pos {
            if hud_rect.expand(10.0).contains(pos) {
                self.poke();
            }
        }

        // 3. Render HUD background surface: Fixed ~92% opacity, NO blur shader (§4.29)
        let surface_fill = Color32::from_rgba_unmultiplied(26, 28, 40, 235);
        let surface_stroke = Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 26));

        let painter = ui.painter();
        painter.rect(
            hud_rect,
            CornerRadius::same(16),
            surface_fill,
            surface_stroke,
            StrokeKind::Inside,
        );

        // 4. Place interactive contents inside hud_rect
        let hud_inner = hud_rect.shrink2(vec2(18.0, 12.0));
        let mut child_ui = ui.new_child(
            UiBuilder::new()
                .max_rect(hud_inner)
                .layout(egui::Layout::top_down(egui::Align::Center)),
        );

        // Optional feedback notification banner
        if let Some((ref text, ref time)) = self.notification {
            if time.elapsed() < Duration::from_secs(3) {
                child_ui.colored_label(Color32::from_rgb(139, 124, 246), text);
            }
        }

        // --- ROW 1: Timestamps & Scrubber ---
        child_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(12.0, 0.0);

            // Left: Current time / Duration
            let display_time = self.scrub_drag_time.unwrap_or(current_time);
            let time_str = format!(
                "{} / {}",
                Self::format_time(display_time),
                Self::format_time(duration)
            );
            ui.monospace(time_str);

            // Center: Scrubber
            let remaining_w = ui.available_width() - 85.0;
            let target_w = remaining_w.max(60.0);
            ui.allocate_ui_with_layout(
                vec2(target_w, 20.0),
                egui::Layout::left_to_right(egui::Align::Center),
                |ui| {
                    if let Some(target) = self.render_scrubber(ui, current_time, duration) {
                        let _ = player.seek_absolute(target);
                        self.poke();
                    }
                },
            );

            // Right: Negative remaining time
            let remaining_secs = (duration - display_time).max(0.0);
            let rem_str = format!("-{}", Self::format_time(remaining_secs));
            ui.monospace(rem_str);
        });

        child_ui.add_space(6.0);

        // --- ROW 2: Transport Controls (Centered) ---
        child_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(10.0, 0.0);

            // Recuar 10s
            if ui.button("⏮ 10s").clicked() {
                let _ = player.seek_relative(-10.0);
                self.poke();
            }

            // Recuar 5s
            if ui.button("⏪ 5s").clicked() {
                let _ = player.seek_relative(-5.0);
                self.poke();
            }

            // Play / Pause prominent toggle button
            let play_btn_text = if is_paused { " ▶ Play " } else { " ⏸ Pause " };
            let play_btn = egui::Button::new(
                egui::RichText::new(play_btn_text)
                    .strong()
                    .color(if is_paused { Color32::WHITE } else { Color32::from_rgb(139, 124, 246) }),
            );
            if ui.add(play_btn).clicked() {
                let _ = player.toggle_pause();
                self.poke();
            }

            // Avançar 5s
            if ui.button("5s ⏩").clicked() {
                let _ = player.seek_relative(5.0);
                self.poke();
            }

            // Avançar 10s
            if ui.button("10s ⏭").clicked() {
                let _ = player.seek_relative(10.0);
                self.poke();
            }

            ui.separator();

            // A-B Repeat Loop toggle button
            let ab_label = match player.ab_loop_status().unwrap_or(AbLoopStatus::Off) {
                AbLoopStatus::Off => "🔁 A-B".to_string(),
                AbLoopStatus::AOnly(a) => format!("🔁 A: {}", Self::format_time(a)),
                AbLoopStatus::Looping { a, b } => {
                    format!("🔁 A-B ({}-{})", Self::format_time(a), Self::format_time(b))
                }
            };
            if ui.button(ab_label).clicked() {
                if let Ok(status) = player.cycle_ab_loop() {
                    let msg = match status {
                        AbLoopStatus::AOnly(a) => format!("Ponto A definido em {}", Self::format_time(a)),
                        AbLoopStatus::Looping { a, b } => {
                            format!("Repetição A-B ativa ({} até {})", Self::format_time(a), Self::format_time(b))
                        }
                        AbLoopStatus::Off => "Repetição A-B desativada".to_string(),
                    };
                    self.set_notification(msg);
                }
            }

            // Frame screenshot capture button
            if ui.button("📸 Frame").clicked() && player.take_screenshot().is_ok() {
                self.set_notification("Fotograma guardado");
            }

            // Whisper AI button (prominent badge, disabled if ffmpeg is missing)
            let whisper_btn_text = egui::RichText::new("🎙 Whisper").strong();
            let whisper_btn = egui::Button::new(whisper_btn_text);

            if whisper_disabled {
                ui.add_enabled(false, whisper_btn)
                    .on_disabled_hover_text("Desativado: Requer FFmpeg (`sudo apt install ffmpeg`)");
            } else {
                let active_btn = whisper_btn.fill(Color32::from_rgba_unmultiplied(217, 158, 66, 45));
                if ui
                    .add(active_btn)
                    .on_hover_text("Abrir painel de Transcrição Whisper AI")
                    .clicked()
                {
                    action = Some(HudAction::ToggleWhisper);
                }
            }

            ui.separator();

            // Lateral Panel toggles
            if ui.button("📜 Playlist").clicked() {
                action = Some(HudAction::TogglePlaylist);
            }
            if ui.button("🎚 Equalizador").clicked() {
                action = Some(HudAction::ToggleEqualizer);
            }
            if ui.button("🎞 Vídeo").clicked() {
                action = Some(HudAction::ToggleVideo);
            }
            if ui
                .button("✂ Cortar Clip")
                .on_hover_text("Abrir painel de corte e exportação de clips (atalho: C)")
                .clicked()
            {
                action = Some(HudAction::ToggleClipExport);
            }
        });


        child_ui.add_space(6.0);

        // --- ROW 3: Secondary Controls (Speed, Tracks, Volume) ---
        child_ui.horizontal(|ui| {
            ui.spacing_mut().item_spacing = vec2(14.0, 0.0);

            // Playback speed selector
            let current_speed = player.speed().unwrap_or(1.0);
            let speed_label = format!("Velocidade: {current_speed:.2}x");
            egui::ComboBox::from_id_salt("speed_selector")
                .selected_text(speed_label)
                .show_ui(ui, |ui| {
                    for preset in [0.25_f64, 0.5, 0.75, 1.0, 1.25, 1.5, 1.75, 2.0] {
                        let text = if (preset - 1.0).abs() < 0.01 {
                            format!("{preset:.2}x (Normal)")
                        } else {
                            format!("{preset:.2}x")
                        };
                        let is_sel = (current_speed - preset).abs() < 0.01;
                        if ui.selectable_label(is_sel, text).clicked() {
                            let _ = player.set_speed(preset);
                            self.poke();
                        }
                    }
                });

            // Audio track selector
            let audio_tracks = self.cached_audio_tracks.clone();
            let current_aid_text = audio_tracks
                .iter()
                .find(|t| t.is_selected)
                .map(|t| {
                    format!(
                        "Áudio: {}",
                        t.title
                            .as_deref()
                            .or(t.lang.as_deref())
                            .unwrap_or("Faixa ativa")
                    )
                })
                .unwrap_or_else(|| "Áudio: Padrão".to_string());

            egui::ComboBox::from_id_salt("audio_track_selector")
                .selected_text(current_aid_text)
                .show_ui(ui, |ui| {
                    if audio_tracks.is_empty() {
                        ui.label("Nenhuma faixa adicional");
                    } else {
                        for track in &audio_tracks {
                            let label = format!(
                                "Faixa {} ({})",
                                track.id,
                                track.title.as_deref().or(track.lang.as_deref()).unwrap_or("—")
                            );
                            if ui.selectable_label(track.is_selected, label).clicked() {
                                let _ = player.set_audio_track(Some(track.id));
                                self.last_tracks_query = Instant::now() - Duration::from_secs(10);
                                self.poke();
                            }
                        }
                    }
                });

            // Subtitle track selector
            let sub_tracks = self.cached_sub_tracks.clone();
            let current_sub_text = sub_tracks
                .iter()
                .find(|t| t.is_selected)
                .map(|t| {
                    format!(
                        "Legendas: {}",
                        t.title
                            .as_deref()
                            .or(t.lang.as_deref())
                            .unwrap_or("Ativa")
                    )
                })
                .unwrap_or_else(|| "Legendas: Desativadas".to_string());

            egui::ComboBox::from_id_salt("sub_track_selector")
                .selected_text(current_sub_text)
                .show_ui(ui, |ui| {
                    if ui.selectable_label(sub_tracks.iter().all(|t| !t.is_selected), "Desativar legendas").clicked() {
                        let _ = player.set_subtitle_track(None);
                        self.last_tracks_query = Instant::now() - Duration::from_secs(10);
                        self.poke();
                    }
                    for track in &sub_tracks {
                        let label = format!(
                            "Legenda {} ({})",
                            track.id,
                            track.title.as_deref().or(track.lang.as_deref()).unwrap_or("—")
                        );
                        if ui.selectable_label(track.is_selected, label).clicked() {
                            let _ = player.set_subtitle_track(Some(track.id));
                            self.last_tracks_query = Instant::now() - Duration::from_secs(10);
                            self.poke();
                        }
                    }
                    ui.separator();
                    if ui.button("➕ Carregar legendas externas...").clicked() {
                        action = Some(HudAction::OpenSubtitlesDialog);
                    }
                });

            // Right-aligned: Volume control with slider & mute
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let current_vol = player.volume().unwrap_or(100.0);
                let is_muted = player.is_muted().unwrap_or(false);

                ui.monospace(format!("{:.0}%", current_vol));

                if let Some(new_vol) = self.render_volume_slider(ui, current_vol) {
                    let _ = player.set_volume(new_vol);
                    self.poke();
                }

                let mute_icon = if is_muted { "🔇" } else { "🔊" };
                if ui.button(mute_icon).clicked() {
                    let _ = player.toggle_mute();
                    self.poke();
                }
            });
        });

        action
    }
}
