use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, Sense, Stroke, StrokeKind, Ui,
};
use vad_core::{AudioDevice, Player, VadError};

/// Equalizer and audio configuration panel corresponding to `design/Equalizer.dc.html`.
/// Features a 10-band graphic equalizer with continuous spectral curve, 0 dB baseline,
/// presets, volume boost, RNNoise toggle, and output device selection.
pub struct AudioPanel {
    pub gains: [f64; 10],
    pub active_preset: String,
    pub volume_boost: f64,
    pub rnnoise: bool,
    /// Why the last filter update could not be fully applied (shown next to the RNNoise toggle).
    pub filter_error: Option<String>,
    pub cached_devices: Vec<AudioDevice>,
    pub selected_device: String,
    last_device_query: Option<std::time::Instant>,
}

impl Default for AudioPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioPanel {
    pub const FREQ_LABELS: [&'static str; 10] =
        ["32", "64", "125", "250", "500", "1k", "2k", "4k", "8k", "16k"];

    pub fn new() -> Self {
        Self {
            gains: [0.0; 10],
            active_preset: "Plano".to_string(),
            volume_boost: 100.0,
            rnnoise: false,
            filter_error: None,
            cached_devices: Vec::new(),
            selected_device: "auto".to_string(),
            last_device_query: None,
        }
    }

    /// Pushes the equalizer + RNNoise state to mpv. If RNNoise cannot run (no model file), the
    /// toggle is switched back off and the reason is kept in `filter_error`; the equalizer is
    /// applied regardless. Every caller goes through here so a failure is never swallowed.
    pub fn apply_filters(&mut self, player: &Player) {
        self.filter_error = None;
        match player.set_audio_filters(&self.gains, self.rnnoise) {
            Ok(()) => {}
            Err(err @ VadError::RnnoiseModelMissing(_)) => {
                self.rnnoise = false;
                self.filter_error = Some(err.to_string());
            }
            Err(err) => self.filter_error = Some(format!("Filtros de áudio: {err}")),
        }
    }

    /// Applies a named preset to the equalizer bands.
    pub fn apply_preset(&mut self, preset_name: &str) {
        self.active_preset = preset_name.to_string();
        match preset_name {
            "Plano" => self.gains = [0.0; 10],
            "Voz clara" => {
                self.gains = [-3.0, -2.0, 0.0, 1.0, 3.0, 4.0, 3.0, 2.0, 0.0, -1.0];
            }
            "Música" => {
                self.gains = [3.0, 2.0, 1.0, 0.0, -1.0, 0.0, 1.0, 2.0, 3.0, 2.0];
            }
            "Cinema" => {
                self.gains = [4.0, 3.0, 1.0, 0.0, 1.0, 3.0, 2.0, 1.0, 2.0, 3.0];
            }
            _ => {}
        }
    }

    /// Queries audio output devices with rate limiting.
    fn refresh_devices(&mut self, player: &Player) {
        let should_query = self
            .last_device_query
            .is_none_or(|t| t.elapsed() > std::time::Duration::from_secs(4));

        if should_query {
            if let Ok(devs) = player.audio_devices() {
                self.cached_devices = devs;
            }
            if let Ok(cur) = player.audio_device() {
                self.selected_device = cur;
            }
            self.last_device_query = Some(std::time::Instant::now());
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, player: &Player) {
        self.refresh_devices(player);

        let accent = Color32::from_rgb(139, 124, 246);
        let inactive_bg = Color32::from_rgba_premultiplied(255, 255, 255, 12);

        ui.spacing_mut().item_spacing = vec2(0.0, 14.0);

        // --- SECTION: PREDEFINIÇÃO (Presets) ---
        ui.vertical(|ui| {
            ui.horizontal(|ui| {
                ui.colored_label(
                    Color32::from_rgb(150, 155, 175),
                    egui::RichText::new("PREDEFINIÇÃO").size(11.0).strong(),
                );
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.small_button("Repor").clicked() {
                        self.apply_preset("Plano");
                        self.apply_filters(player);
                    }
                });
            });
            ui.add_space(4.0);

            ui.columns(4, |cols| {
                let presets = ["Voz clara", "Música", "Cinema", "Plano"];
                for (i, &p) in presets.iter().enumerate() {
                    let is_active = self.active_preset == p;
                    let btn_text = egui::RichText::new(p)
                        .size(11.0)
                        .strong()
                        .color(if is_active {
                            Color32::from_rgb(20, 20, 28)
                        } else {
                            Color32::from_rgb(200, 205, 225)
                        });

                    let btn = egui::Button::new(btn_text)
                        .fill(if is_active { accent } else { inactive_bg })
                        .corner_radius(CornerRadius::same(6));

                    if cols[i].add_sized(vec2(cols[i].available_width(), 26.0), btn).clicked() {
                        self.apply_preset(p);
                        self.apply_filters(player);
                    }
                }
            });
        });

        // --- SECTION: EQUALIZADOR DE 10 BANDAS (Curva Contínua e 0 dB Baseline) ---
        ui.vertical(|ui| {
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                egui::RichText::new("EQUALIZADOR GRÁFICO (10 BANDAS)").size(11.0).strong(),
            );
            ui.add_space(6.0);

            // Container for graphical EQ
            let eq_height = 180.0_f32;
            let available_w = ui.available_width().max(280.0);
            let (rect, _resp) = ui.allocate_exact_size(vec2(available_w, eq_height), Sense::hover());

            let painter = ui.painter_at(rect);

            // Background surface
            painter.rect_filled(
                rect,
                CornerRadius::same(12),
                Color32::from_rgba_premultiplied(22, 24, 34, 200),
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(12),
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 18)),
                StrokeKind::Inside,
            );

            let inner_margin = 16.0_f32;
            let plot_rect = rect.shrink2(vec2(inner_margin, 24.0));
            let baseline_y = plot_rect.center().y;

            // 0 dB reference line (subtle horizontal guide)
            painter.line_segment(
                [
                    pos2(plot_rect.left(), baseline_y),
                    pos2(plot_rect.right(), baseline_y),
                ],
                Stroke::new(1.0, Color32::from_rgba_premultiplied(255, 255, 255, 35)),
            );

            // 0 dB text badge
            painter.text(
                pos2(rect.right() - 8.0, baseline_y),
                egui::Align2::RIGHT_CENTER,
                "0 dB",
                egui::FontId::monospace(9.0),
                Color32::from_rgba_premultiplied(255, 255, 255, 60),
            );

            // Calculate band X positions and node points for continuous spectral curve
            let n_bands = 10;
            let col_step = plot_rect.width() / (n_bands as f32);
            let mut curve_points = Vec::with_capacity(n_bands);
            let mut band_changed = false;

            for i in 0..n_bands {
                let x = plot_rect.left() + (i as f32 + 0.5) * col_step;
                let gain = self.gains[i];
                // Y maps -12 dB to bottom and +12 dB to top
                let norm = (gain / 12.0).clamp(-1.0, 1.0) as f32;
                let half_h = plot_rect.height() / 2.0;
                let y = baseline_y - norm * half_h;
                curve_points.push(pos2(x, y));

                // Vertical slider track behind point
                let track_top = baseline_y - half_h;
                let track_bottom = baseline_y + half_h;
                painter.line_segment(
                    [pos2(x, track_top), pos2(x, track_bottom)],
                    Stroke::new(3.0, Color32::from_rgba_premultiplied(255, 255, 255, 15)),
                );

                // Vertical fill from baseline to current gain
                let fill_color = if gain >= 0.0 {
                    Color32::from_rgba_premultiplied(139, 124, 246, 180)
                } else {
                    Color32::from_rgba_premultiplied(100, 110, 140, 160)
                };
                painter.line_segment([pos2(x, baseline_y), pos2(x, y)], Stroke::new(3.0, fill_color));

                // Frequency label at bottom
                painter.text(
                    pos2(x, rect.bottom() - 10.0),
                    egui::Align2::CENTER_CENTER,
                    Self::FREQ_LABELS[i],
                    egui::FontId::monospace(10.0),
                    Color32::from_rgb(140, 145, 165),
                );

                // Gain readout above
                let gain_str = if gain > 0.0 {
                    format!("+{:.0}", gain)
                } else {
                    format!("{:.0}", gain)
                };
                let val_color = if gain > 0.0 {
                    accent
                } else if gain < 0.0 {
                    Color32::from_rgb(140, 145, 165)
                } else {
                    Color32::from_rgb(180, 185, 200)
                };
                painter.text(
                    pos2(x, rect.top() + 10.0),
                    egui::Align2::CENTER_CENTER,
                    gain_str,
                    egui::FontId::monospace(9.5),
                    val_color,
                );

                // Interactive slider handle
                let handle_radius = 6.0_f32;
                let hit_rect = Rect::from_center_size(pos2(x, baseline_y), vec2(col_step, plot_rect.height()));
                let hit_resp = ui.interact(
                    hit_rect,
                    ui.id().with(("eq_band", i)),
                    Sense::click_and_drag(),
                );

                if hit_resp.dragged() || hit_resp.clicked() {
                    if let Some(ptr) = hit_resp.interact_pointer_pos() {
                        let delta_from_base = baseline_y - ptr.y;
                        let new_gain = ((delta_from_base / half_h) * 12.0).clamp(-12.0, 12.0);
                        self.gains[i] = ((new_gain * 2.0).round() / 2.0) as f64; // Round to 0.5 dB
                        self.active_preset = "Personalizado".to_string();
                        band_changed = true;
                    }
                }

                // Draw thumb handle circle
                painter.circle_filled(pos2(x, y), handle_radius, Color32::from_rgb(245, 245, 255));
                painter.circle_stroke(pos2(x, y), handle_radius, Stroke::new(1.5, accent));
            }

            // Draw continuous spectral curve connecting all bands
            if curve_points.len() > 1 {
                // Polygon fill between curve and baseline for glowing spectral visual
                let mut fill_polygon = Vec::with_capacity(curve_points.len() + 2);
                fill_polygon.push(pos2(curve_points[0].x, baseline_y));
                fill_polygon.extend(curve_points.iter().cloned());
                fill_polygon.push(pos2(curve_points.last().unwrap().x, baseline_y));

                painter.add(egui::Shape::convex_polygon(
                    fill_polygon,
                    Color32::from_rgba_premultiplied(139, 124, 246, 28),
                    Stroke::NONE,
                ));

                // Smooth spectral line
                painter.add(egui::Shape::line(
                    curve_points,
                    Stroke::new(2.2, accent),
                ));
            }

            if band_changed {
                self.apply_filters(player);
            }
        });

        // --- SECTION: VOLUME BOOST ---
        ui.vertical(|ui| {
            if let Ok(current_vol) = player.volume() {
                self.volume_boost = current_vol;
            }
            ui.horizontal(|ui| {
                ui.label("Volume Boost");
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    ui.monospace(format!("{:.0}%", self.volume_boost));
                    if ui.add(egui::Slider::new(&mut self.volume_boost, 100.0..=200.0).show_value(false)).changed() {
                        let _ = player.set_volume(self.volume_boost);
                    }
                });
            });
        });

        // --- SECTION: REDUÇÃO DE RUÍDO (RNNoise) ---
        ui.horizontal(|ui| {
            let rn_label = if self.rnnoise {
                "Redução de ruído (RNNoise): Ativa"
            } else {
                "Redução de ruído (RNNoise): Inativa"
            };

            let chk = ui.checkbox(&mut self.rnnoise, rn_label);
            if chk.changed() {
                self.apply_filters(player);
            }
        });
        if let Some(ref err) = self.filter_error {
            ui.colored_label(Color32::from_rgb(239, 68, 68), egui::RichText::new(err).size(10.5));
        }

        // --- SECTION: DISPOSITIVO DE SAÍDA ---
        ui.vertical(|ui| {
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                egui::RichText::new("DISPOSITIVO DE SAÍDA").size(11.0).strong(),
            );
            ui.add_space(2.0);

            let display_name = if self.selected_device == "auto" {
                "Automático (PipeWire / SO)".to_string()
            } else {
                self.cached_devices
                    .iter()
                    .find(|d| d.name == self.selected_device)
                    .map(|d| d.description.clone())
                    .unwrap_or_else(|| self.selected_device.clone())
            };

            egui::ComboBox::from_id_salt("audio_output_device_combo")
                .selected_text(display_name)
                .width(ui.available_width() - 8.0)
                .show_ui(ui, |ui| {
                    let is_auto = self.selected_device == "auto";
                    if ui.selectable_label(is_auto, "Automático (PipeWire / SO)").clicked() {
                        self.selected_device = "auto".to_string();
                        let _ = player.set_audio_device("auto");
                    }

                    for dev in &self.cached_devices {
                        if dev.name == "auto" {
                            continue;
                        }
                        let is_sel = self.selected_device == dev.name;
                        let label = if dev.description.is_empty() {
                            &dev.name
                        } else {
                            &dev.description
                        };
                        if ui.selectable_label(is_sel, label).clicked() {
                            self.selected_device = dev.name.clone();
                            let _ = player.set_audio_device(&dev.name);
                        }
                    }
                });
        });
    }
}
