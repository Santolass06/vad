use eframe::egui::{self, vec2, Color32, CornerRadius, Ui};
use vad_core::Player;

/// Video adjustments panel corresponding to `design/VideoPanel.dc.html`.
/// Controls aspect ratio, rotation, crop/panscan, audio/sub delays, and color adjustments.
pub struct VideoPanel {
    pub audio_delay_ms: i64,
    pub sub_delay_ms: i64,
    pub brightness: i64,
    pub contrast: i64,
    pub saturation: i64,
    pub gamma: i64,
    pub panscan: f64,
    last_synced: bool,
}

impl Default for VideoPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl VideoPanel {
    pub fn new() -> Self {
        Self {
            audio_delay_ms: 0,
            sub_delay_ms: 0,
            brightness: 0,
            contrast: 0,
            saturation: 0,
            gamma: 0,
            panscan: 0.0,
            last_synced: false,
        }
    }

    /// Syncs panel state from Player if not yet initialized.
    pub fn sync_from_player(&mut self, player: &Player) {
        if !self.last_synced {
            if let Ok(a) = player.audio_delay() {
                self.audio_delay_ms = (a * 1000.0).round() as i64;
            }
            if let Ok(s) = player.sub_delay() {
                self.sub_delay_ms = (s * 1000.0).round() as i64;
            }
            if let Ok((b, c, s, g)) = player.color_adjustments() {
                self.brightness = b;
                self.contrast = c;
                self.saturation = s;
                self.gamma = g;
            }
            if let Ok(ps) = player.panscan() {
                self.panscan = ps;
            }
            self.last_synced = true;
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, player: &Player) {
        self.sync_from_player(player);

        let accent = Color32::from_rgb(139, 124, 246);
        let inactive_bg = Color32::from_rgba_premultiplied(255, 255, 255, 15);
        let active_bg = accent;

        ui.spacing_mut().item_spacing = vec2(0.0, 14.0);

        // --- SECTION: PROPORÇÃO (Aspect Ratio) ---
        ui.vertical(|ui| {
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                egui::RichText::new("PROPORÇÃO").size(11.0).strong(),
            );
            ui.add_space(4.0);

            let current_aspect = player.video_aspect_override().unwrap_or(-1.0);

            ui.columns(4, |cols| {
                let options = [
                    ("Auto", -1.0_f64),
                    ("16:9", 16.0 / 9.0),
                    ("4:3", 4.0 / 3.0),
                    ("21:9", 21.0 / 9.0),
                ];

                for (i, &(label, val)) in options.iter().enumerate() {
                    let is_active = if val < 0.0 {
                        current_aspect <= 0.0
                    } else {
                        (current_aspect - val).abs() < 0.05
                    };

                    let btn_text = egui::RichText::new(label)
                        .size(11.5)
                        .strong()
                        .color(if is_active { Color32::from_rgb(20, 20, 28) } else { Color32::from_rgb(200, 205, 225) });

                    let btn = egui::Button::new(btn_text)
                        .fill(if is_active { active_bg } else { inactive_bg })
                        .corner_radius(CornerRadius::same(6));

                    if cols[i].add_sized(vec2(cols[i].available_width(), 28.0), btn).clicked() {
                        let arg = if val < 0.0 { "-1" } else { label };
                        let _ = player.set_video_aspect_override(arg);
                    }
                }
            });
        });

        // --- SECTION: ROTAÇÃO ---
        ui.vertical(|ui| {
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                egui::RichText::new("ROTAÇÃO").size(11.0).strong(),
            );
            ui.add_space(4.0);

            let current_rotate = player.video_rotate().unwrap_or(0);

            ui.columns(4, |cols| {
                let rotations = [0, 90, 180, 270];
                for (i, &deg) in rotations.iter().enumerate() {
                    let is_active = current_rotate == deg;
                    let label = format!("{deg}°");
                    let btn_text = egui::RichText::new(label)
                        .size(11.5)
                        .strong()
                        .color(if is_active { Color32::from_rgb(20, 20, 28) } else { Color32::from_rgb(200, 205, 225) });

                    let btn = egui::Button::new(btn_text)
                        .fill(if is_active { active_bg } else { inactive_bg })
                        .corner_radius(CornerRadius::same(6));

                    if cols[i].add_sized(vec2(cols[i].available_width(), 28.0), btn).clicked() {
                        let _ = player.set_video_rotate(deg);
                    }
                }
            });
        });

        // --- SECTION: ENQUADRAMENTO / PANSCAN ---
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(150, 155, 175),
                    egui::RichText::new("ENQUADRAMENTO (CROP/PANSCAN)").size(11.0).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.monospace(format!("{:.0}%", self.panscan * 100.0));
                });
            });
            ui.add_space(2.0);

            ui.horizontal(|ui| {
                if ui.button("Original").clicked() {
                    self.panscan = 0.0;
                    let _ = player.set_panscan(0.0);
                }
                if ui.button("Preencher").clicked() {
                    self.panscan = 1.0;
                    let _ = player.set_panscan(1.0);
                }
                let slider = egui::Slider::new(&mut self.panscan, 0.0..=1.0)
                    .show_value(false);
                if ui.add(slider).changed() {
                    let _ = player.set_panscan(self.panscan);
                }
            });
        });

        // --- SECTION: SINCRONIZAÇÃO (Delays) ---
        ui.vertical(|ui| {
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                egui::RichText::new("SINCRONIZAÇÃO").size(11.0).strong(),
            );
            ui.add_space(4.0);

            // Delay de áudio
            ui.horizontal(|ui| {
                ui.label("Delay de áudio");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("+").clicked() {
                        self.audio_delay_ms += 50;
                        let _ = player.set_audio_delay(self.audio_delay_ms as f64 / 1000.0);
                    }
                    let sign = if self.audio_delay_ms > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{} ms", self.audio_delay_ms));
                    if ui.button("−").clicked() {
                        self.audio_delay_ms -= 50;
                        let _ = player.set_audio_delay(self.audio_delay_ms as f64 / 1000.0);
                    }
                });
            });

            // Delay de legendas
            ui.horizontal(|ui| {
                ui.label("Delay de legendas");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("+").clicked() {
                        self.sub_delay_ms += 50;
                        let _ = player.set_sub_delay(self.sub_delay_ms as f64 / 1000.0);
                    }
                    let sign = if self.sub_delay_ms > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{} ms", self.sub_delay_ms));
                    if ui.button("−").clicked() {
                        self.sub_delay_ms -= 50;
                        let _ = player.set_sub_delay(self.sub_delay_ms as f64 / 1000.0);
                    }
                });
            });
        });

        // --- SECTION: COR ---
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(150, 155, 175),
                    egui::RichText::new("COR").size(11.0).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Repor").clicked() {
                        self.brightness = 0;
                        self.contrast = 0;
                        self.saturation = 0;
                        self.gamma = 0;
                        let _ = player.reset_color_adjustments();
                    }
                });
            });
            ui.add_space(4.0);

            let mut color_changed = false;

            // Brilho
            ui.horizontal(|ui| {
                ui.label("Brilho");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sign = if self.brightness > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{}", self.brightness));
                    if ui.add(egui::Slider::new(&mut self.brightness, -100..=100).show_value(false)).changed() {
                        color_changed = true;
                    }
                });
            });

            // Contraste
            ui.horizontal(|ui| {
                ui.label("Contraste");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sign = if self.contrast > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{}", self.contrast));
                    if ui.add(egui::Slider::new(&mut self.contrast, -100..=100).show_value(false)).changed() {
                        color_changed = true;
                    }
                });
            });

            // Saturação
            ui.horizontal(|ui| {
                ui.label("Saturação");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sign = if self.saturation > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{}", self.saturation));
                    if ui.add(egui::Slider::new(&mut self.saturation, -100..=100).show_value(false)).changed() {
                        color_changed = true;
                    }
                });
            });

            // Gama
            ui.horizontal(|ui| {
                ui.label("Gama");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    let sign = if self.gamma > 0 { "+" } else { "" };
                    ui.monospace(format!("{sign}{}", self.gamma));
                    if ui.add(egui::Slider::new(&mut self.gamma, -100..=100).show_value(false)).changed() {
                        color_changed = true;
                    }
                });
            });

            if color_changed {
                let _ = player.set_color_adjustments(
                    self.brightness,
                    self.contrast,
                    self.saturation,
                    self.gamma,
                );
            }
        });
    }
}
