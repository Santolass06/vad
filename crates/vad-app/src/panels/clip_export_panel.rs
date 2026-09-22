use std::path::Path;
use std::time::Instant;

use eframe::egui::{
    self, pos2, vec2, Color32, CornerRadius, Rect, RichText, Sense, Stroke, StrokeKind, Ui,
};
use vad_audio_tools::{keyframe_at_or_before, WaveformPyramid};


/// Action emitted by the Clip Export interface.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipExportAction {
    Close,
    Export {
        start_seconds: f64,
        end_seconds: f64,
        exact_cut: bool,
        output_filename: String,
    },
    PlaySelection {
        start_seconds: f64,
        end_seconds: f64,
    },
    PauseSelection,
    SeekTo(f64),
    CancelExport,
}

/// Selection preview in progress: pause once the playhead reaches `end`.
///
/// The playhead only counts once it has been seen *before* `end`. Right after the seek the shared
/// position can still be the old one (e.g. 200 s for a 10–20 s selection); without this guard that
/// stale value read as "already past the end" and the preview paused itself instantly.
#[derive(Debug, Clone, PartialEq)]
pub struct SelectionPlayback {
    end: f64,
    armed: bool,
}

impl SelectionPlayback {
    pub fn new(end: f64) -> Self {
        Self { end, armed: false }
    }

    /// Feeds the current playhead position; true when the preview must stop now.
    pub fn reached_end(&mut self, current_time: f64) -> bool {
        if current_time < self.end {
            self.armed = true;
            false
        } else {
            self.armed
        }
    }
}

/// Identifies which waveform handle is currently being dragged.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveDragHandle {
    Start,
    End,
}

/// Clip Export interface corresponding to `design/ClipExport.dc.html`.
pub struct ClipExportPanel {
    pub start_seconds: f64,
    pub end_seconds: f64,
    pub start_input_text: String,
    pub end_input_text: String,
    pub exact_cut: bool,
    pub filename_input: String,
    pub is_exporting: bool,
    pub export_status: Option<(String, bool, Instant)>, // (message, is_error, timestamp)
    active_drag: Option<ActiveDragHandle>,
    last_media_path: Option<String>,
    /// Keyframe times of the current media: `None` while unknown/being probed, empty for
    /// audio-only files.
    keyframes: Option<Vec<f64>>,
    keyframes_for: Option<String>,
}

impl Default for ClipExportPanel {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipExportPanel {
    pub fn new() -> Self {
        Self {
            start_seconds: 0.0,
            end_seconds: 10.0,
            start_input_text: "00:00:00".to_string(),
            end_input_text: "00:00:10".to_string(),
            exact_cut: false,
            filename_input: "clip.mp4".to_string(),
            is_exporting: false,
            export_status: None,
            active_drag: None,
            last_media_path: None,
            keyframes: None,
            keyframes_for: None,
        }
    }

    /// Formats seconds into HH:MM:SS text.
    pub fn format_hms(seconds: f64) -> String {
        let total = seconds.max(0.0).round() as u64;
        let h = total / 3600;
        let m = (total % 3600) / 60;
        let s = total % 60;
        format!("{h:02}:{m:02}:{s:02}")
    }

    /// Parses user-entered text (HH:MM:SS, MM:SS, or seconds) into seconds.
    pub fn parse_hms(text: &str) -> Option<f64> {
        let trimmed = text.trim();
        if trimmed.is_empty() {
            return None;
        }

        let parts: Vec<&str> = trimmed.split(':').collect();
        match parts.len() {
            3 => {
                let h: f64 = parts[0].parse().ok()?;
                let m: f64 = parts[1].parse().ok()?;
                let s: f64 = parts[2].parse().ok()?;
                if h < 0.0 || !(0.0..60.0).contains(&m) || !(0.0..60.0).contains(&s) {
                    None
                } else {
                    Some(h * 3600.0 + m * 60.0 + s)
                }
            }
            2 => {
                let m: f64 = parts[0].parse().ok()?;
                let s: f64 = parts[1].parse().ok()?;
                if m < 0.0 || !(0.0..60.0).contains(&s) {
                    None
                } else {
                    Some(m * 60.0 + s)
                }
            }
            1 => parts[0].parse::<f64>().ok().filter(|&v| v >= 0.0),
            _ => None,
        }

    }

    /// Prepares state for a newly opened media file.
    pub fn reset_for_media(&mut self, media_path: &str, duration: f64) {
        let path = Path::new(media_path);
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("media");
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("mp4");

        let default_output = format!("{stem}_clip.{ext}");

        let duration_safe = duration.max(1.0);
        let start = 0.0;
        let end = (duration_safe * 0.25).max(1.0).min(duration_safe);

        self.start_seconds = start;
        self.end_seconds = end;
        self.start_input_text = Self::format_hms(start);
        self.end_input_text = Self::format_hms(end);
        self.filename_input = default_output;
        self.exact_cut = false;
        // `is_exporting` belongs to the export task, not to the media: an export that is still
        // running must keep the button locked (its result clears the flag).
        self.export_status = None;
        self.active_drag = None;
        self.last_media_path = Some(media_path.to_string());
    }

    /// Initialises the panel for `media_path` unless it already is: reopening the panel, or
    /// marking `I`/`O` before opening it, must not throw the selection away.
    pub fn ensure_media(&mut self, media_path: &str, duration: f64) {
        if self.last_media_path.as_deref() != Some(media_path) {
            self.reset_for_media(media_path, duration);
        }
    }

    /// Drops the per-media state; the next `ensure_media` re-initialises it with the real duration.
    pub fn forget_media(&mut self) {
        self.last_media_path = None;
        self.keyframes = None;
        self.keyframes_for = None;
    }

    /// Whether keyframes for `media_path` were already probed (or are being probed).
    pub fn has_keyframes_for(&self, media_path: &str) -> bool {
        self.keyframes_for.as_deref() == Some(media_path)
    }

    /// Marks the keyframe probe of `media_path` as started.
    pub fn begin_keyframes(&mut self, media_path: &str) {
        self.keyframes_for = Some(media_path.to_string());
        self.keyframes = None;
    }

    /// Stores the probe result (ignored if the panel has moved on to other media).
    pub fn set_keyframes(&mut self, media_path: &str, keyframes: Vec<f64>) {
        if self.has_keyframes_for(media_path) {
            self.keyframes = Some(keyframes);
        }
    }

    /// Applies a typed time to `current` only when the text differs from what is displayed:
    /// the box shows whole seconds, so re-parsing an untouched box would round a dragged handle.
    fn typed_time(text: &str, current: f64) -> Option<f64> {
        let secs = Self::parse_hms(text)?;
        (Self::format_hms(current) != Self::format_hms(secs)).then_some(secs)
    }

    /// Sets the In-point (start) from current playhead position (`I` shortcut).
    pub fn set_in_point(&mut self, current_time: f64) {
        let clamped = current_time.max(0.0).min(self.end_seconds - 0.1);
        self.start_seconds = clamped;
        self.start_input_text = Self::format_hms(clamped);
    }

    /// Sets the Out-point (end) from current playhead position (`O` shortcut).
    pub fn set_out_point(&mut self, current_time: f64, max_duration: f64) {
        let clamped = current_time.max(self.start_seconds + 0.1).min(max_duration.max(self.start_seconds + 0.1));
        self.end_seconds = clamped;
        self.end_input_text = Self::format_hms(clamped);
    }

    /// Updates status notification.
    pub fn set_status(&mut self, message: impl Into<String>, is_error: bool) {
        self.export_status = Some((message.into(), is_error, Instant::now()));
    }

    /// Renders the complete Clip Export panel.
    pub fn ui(
        &mut self,
        ui: &mut Ui,
        media_path: &str,
        duration: f64,
        current_time: f64,
        is_playing_selection: bool,
        mut waveform: Option<&mut WaveformPyramid>,
    ) -> Option<ClipExportAction> {

        let mut action = None;
        let duration_safe = duration.max(0.1);

        // Auto-initialize if media changed. Keyboard shortcuts (I/O/C/Escape) live in
        // `VadApp::handle_shortcuts`, behind the "is a text field focused?" guard; a second copy
        // here fired while typing a file name such as "intro_clip.mp4".
        self.ensure_media(media_path, duration_safe);

        let accent = Color32::from_rgb(139, 124, 246); // #8b7cf6
        let bg_card = Color32::from_rgba_unmultiplied(32, 34, 46, 220);
        let border_color = Color32::from_rgba_unmultiplied(255, 255, 255, 18);

        // --- ROOT CONTAINER (ClipExport.dc.html) ---
        ui.vertical(|ui| {
            ui.spacing_mut().item_spacing = vec2(0.0, 16.0);

            // 1. TOP HEADER BAR (44px)
            ui.horizontal(|ui| {
                ui.strong(RichText::new("VAD").size(15.0).color(accent));
                ui.separator();

                let file_name = Path::new(media_path)
                    .file_name()
                    .and_then(|n| n.to_str())
                    .unwrap_or(media_path);

                ui.colored_label(
                    Color32::from_rgb(185, 190, 210),
                    RichText::new(format!("Cortar clip — {file_name}")).size(13.0).strong(),
                );

                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button(RichText::new("Fechar").size(12.0)).clicked() {
                        action = Some(ClipExportAction::Close);
                    }
                });
            });

            ui.separator();

            ui.add_space(8.0);

            // 2. MAIN SECTION: SELECIONAR TROÇO (Waveform with handles & Keyframe marks)
            ui.colored_label(
                Color32::from_rgb(150, 155, 175),
                RichText::new("SELECIONAR TROÇO").size(11.5).strong(),
            );

            let card_height = 135.0_f32;
            let available_w = ui.available_width().max(320.0);
            let (card_rect, card_resp) = ui.allocate_exact_size(vec2(available_w, card_height), Sense::click_and_drag());
            let painter = ui.painter_at(card_rect);

            // Card background & border
            painter.rect_filled(card_rect, CornerRadius::same(12), bg_card);
            painter.rect_stroke(card_rect, CornerRadius::same(12), Stroke::new(1.0, border_color), StrokeKind::Inside);

            let waveform_area = card_rect.shrink2(vec2(20.0, 16.0));
            let wave_h = 68.0_f32;
            let wave_rect = Rect::from_min_size(waveform_area.min, vec2(waveform_area.width(), wave_h));
            let mid_y = wave_rect.center().y;

            // Render 110 bars (matching ClipExport.dc.html)
            let n_bars = 110;
            let bar_w = (wave_rect.width() / n_bars as f32).max(1.0);

            let start_fraction = (self.start_seconds / duration_safe).clamp(0.0, 1.0) as f32;
            let end_fraction = (self.end_seconds / duration_safe).clamp(0.0, 1.0) as f32;
            let start_x = wave_rect.left() + start_fraction * wave_rect.width();
            let end_x = wave_rect.left() + end_fraction * wave_rect.width();

            // Draw waveform bars
            if let Some(ref mut pyr) = waveform {
                let pts = pyr.get_visible_points(0.0, duration_safe, n_bars);
                for (i, pt) in pts.iter().enumerate() {
                    let bx = wave_rect.left() + (i as f32 * bar_w);
                    let in_sel = bx + bar_w >= start_x && bx <= end_x;
                    let bar_color = if in_sel {
                        accent
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 30)
                    };

                    let half_h = (wave_h * 0.44).max(3.0);
                    let top_y = mid_y - (pt.max * half_h);
                    let bot_y = mid_y - (pt.min * half_h);
                    let y_min = top_y.min(bot_y);
                    let y_max = top_y.max(bot_y).max(y_min + 1.0);

                    painter.rect_filled(
                        Rect::from_min_max(pos2(bx, y_min), pos2(bx + (bar_w - 1.0).max(1.0), y_max)),
                        CornerRadius::same(1),
                        bar_color,
                    );
                }
            } else {
                // Synthetic placeholder waveform bars matching mock seed
                let seed = [22.0_f32, 40.0, 60.0, 35.0, 18.0, 48.0, 70.0, 52.0, 30.0, 44.0, 26.0, 58.0, 66.0, 38.0, 20.0, 46.0, 62.0, 34.0, 28.0, 50.0];
                for i in 0..n_bars {
                    let bx = wave_rect.left() + (i as f32 * bar_w);
                    let in_sel = bx + bar_w >= start_x && bx <= end_x;
                    let bar_color = if in_sel {
                        accent
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 30)
                    };
                    let h_pct = seed[i % seed.len()] / 100.0;
                    let bh = (wave_h * 0.85 * h_pct).max(4.0);
                    let top_y = mid_y - (bh / 2.0);
                    painter.rect_filled(
                        Rect::from_min_size(pos2(bx, top_y), vec2((bar_w - 1.0).max(1.0), bh)),
                        CornerRadius::same(1),
                        bar_color,
                    );
                }
            }

            // Selection shaded overlay
            let sel_rect = Rect::from_min_max(
                pos2(start_x, wave_rect.top() - 4.0),
                pos2(end_x, wave_rect.bottom() + 4.0),
            );
            painter.rect_filled(
                sel_rect,
                CornerRadius::same(4),
                Color32::from_rgba_unmultiplied(139, 124, 246, 35),
            );

            // Left In Handle
            painter.line_segment(
                [pos2(start_x, wave_rect.top() - 6.0), pos2(start_x, wave_rect.bottom() + 6.0)],
                Stroke::new(2.0, accent),
            );
            let knob_size = 12.0_f32;
            let left_knob_rect = Rect::from_center_size(pos2(start_x, wave_rect.top() - 3.0), vec2(knob_size, knob_size));
            painter.rect_filled(left_knob_rect, CornerRadius::same(3), accent);

            // Right Out Handle
            painter.line_segment(
                [pos2(end_x, wave_rect.top() - 6.0), pos2(end_x, wave_rect.bottom() + 6.0)],
                Stroke::new(2.0, accent),
            );
            let right_knob_rect = Rect::from_center_size(pos2(end_x, wave_rect.top() - 3.0), vec2(knob_size, knob_size));
            painter.rect_filled(right_knob_rect, CornerRadius::same(3), accent);

            // Playhead indicator if inside selection or near
            let playhead_fraction = (current_time / duration_safe).clamp(0.0, 1.0) as f32;
            let playhead_x = wave_rect.left() + playhead_fraction * wave_rect.width();
            painter.line_segment(
                [pos2(playhead_x, wave_rect.top()), pos2(playhead_x, wave_rect.bottom())],
                Stroke::new(1.5, Color32::from_rgb(250, 204, 21)),
            );

            // Handle dragging interaction
            let hit_margin = 12.0_f32;
            if card_resp.drag_started() {
                if let Some(pos) = card_resp.interact_pointer_pos() {
                    let dist_start = (pos.x - start_x).abs();
                    let dist_end = (pos.x - end_x).abs();
                    if dist_start <= hit_margin {
                        self.active_drag = Some(ActiveDragHandle::Start);
                    } else if dist_end <= hit_margin {
                        self.active_drag = Some(ActiveDragHandle::End);
                    } else if dist_start < dist_end {
                        self.active_drag = Some(ActiveDragHandle::Start);
                    } else {
                        self.active_drag = Some(ActiveDragHandle::End);
                    }
                }
            }

            if card_resp.dragged() {
                if let Some(pos) = card_resp.interact_pointer_pos() {
                    let u = ((pos.x - wave_rect.left()) / wave_rect.width()).clamp(0.0, 1.0) as f64;
                    let target_secs = u * duration_safe;
                    match self.active_drag {
                        Some(ActiveDragHandle::Start) => {
                            self.start_seconds = target_secs.min(self.end_seconds - 0.1);
                            self.start_input_text = Self::format_hms(self.start_seconds);
                        }
                        Some(ActiveDragHandle::End) => {
                            self.end_seconds = target_secs.max(self.start_seconds + 0.1);
                            self.end_input_text = Self::format_hms(self.end_seconds);
                        }
                        None => {}
                    }
                }
            }

            if card_resp.clicked() {
                if let Some(pos) = card_resp.interact_pointer_pos() {
                    let u = ((pos.x - wave_rect.left()) / wave_rect.width()).clamp(0.0, 1.0) as f64;
                    let target_secs = u * duration_safe;
                    action = Some(ClipExportAction::SeekTo(target_secs));
                }
            }

            if card_resp.drag_stopped() {
                self.active_drag = None;
            }


            // Keyframe ticks (real ones, from ffprobe). A fast cut can only start on one of them.
            let kf_y = wave_rect.bottom() + 10.0;
            if let Some(ref keyframes) = self.keyframes {
                let mut last_x = f32::NEG_INFINITY;
                for &kf in keyframes {
                    let kf_x = wave_rect.left()
                        + (kf / duration_safe).clamp(0.0, 1.0) as f32 * wave_rect.width();
                    // dense GOPs would paint a solid bar: keep ticks at least 3 px apart
                    if kf_x - last_x < 3.0 {
                        continue;
                    }
                    last_x = kf_x;
                    painter.line_segment(
                        [pos2(kf_x, kf_y), pos2(kf_x, kf_y + 6.0)],
                        Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 70)),
                    );
                }
            }

            // Time labels row (00:00:00 | keyframes note | duration)
            let labels_y = kf_y + 10.0;
            painter.text(
                pos2(wave_rect.left(), labels_y),
                egui::Align2::LEFT_TOP,
                "00:00:00",
                egui::FontId::monospace(10.5),
                Color32::from_rgb(140, 145, 165),
            );
            let kf_note = match self.keyframes {
                None => "a analisar keyframes…",
                Some(ref k) if k.is_empty() => "sem vídeo: não há keyframes",
                Some(_) => "| marcas = keyframes",
            };
            painter.text(
                pos2(wave_rect.center().x, labels_y),
                egui::Align2::CENTER_TOP,
                kf_note,
                egui::FontId::proportional(10.5),
                Color32::from_rgb(120, 125, 145),
            );
            painter.text(
                pos2(wave_rect.right(), labels_y),
                egui::Align2::RIGHT_TOP,
                Self::format_hms(duration_safe),
                egui::FontId::monospace(10.5),
                Color32::from_rgb(140, 145, 165),
            );

            ui.add_space(8.0);

            // 3. CONTROLS GRID: TIMESTAMPS, EXACT CUT, FILENAME & EXPORT
            ui.columns(2, |cols| {
                // --- LEFT COLUMN: Início / Fim / Duração & Checkbox & Preview ---
                let left = &mut cols[0];
                left.vertical(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 12.0);

                    // Inputs Início, Fim, Duração
                    ui.horizontal(|ui| {
                        ui.spacing_mut().item_spacing = vec2(12.0, 0.0);

                        // Início
                        ui.vertical(|ui| {
                            ui.colored_label(Color32::from_rgb(150, 155, 175), RichText::new("Início").size(11.0).strong());
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.start_input_text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(90.0),
                            );
                            if resp.lost_focus() {
                                if let Some(secs) = Self::typed_time(&self.start_input_text, self.start_seconds) {
                                    self.start_seconds = secs.min(self.end_seconds - 0.1).max(0.0);
                                }
                                self.start_input_text = Self::format_hms(self.start_seconds);
                            }
                        });

                        // Fim
                        ui.vertical(|ui| {
                            ui.colored_label(Color32::from_rgb(150, 155, 175), RichText::new("Fim").size(11.0).strong());
                            let resp = ui.add(
                                egui::TextEdit::singleline(&mut self.end_input_text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(90.0),
                            );
                            if resp.lost_focus() {
                                if let Some(secs) = Self::typed_time(&self.end_input_text, self.end_seconds) {
                                    self.end_seconds = secs.max(self.start_seconds + 0.1).min(duration_safe);
                                }
                                self.end_input_text = Self::format_hms(self.end_seconds);
                            }
                        });

                        // Duração (Calculada e Desativada)
                        ui.vertical(|ui| {
                            ui.colored_label(Color32::from_rgb(150, 155, 175), RichText::new("Duração").size(11.0).strong());
                            let clip_duration = (self.end_seconds - self.start_seconds).max(0.0);
                            let mut dur_text = Self::format_hms(clip_duration);
                            ui.add_enabled(
                                false,
                                egui::TextEdit::singleline(&mut dur_text)
                                    .font(egui::TextStyle::Monospace)
                                    .desired_width(90.0),
                            );
                        });
                    });

                    // Quick shortcut tip
                    ui.colored_label(
                        Color32::from_rgb(130, 135, 155),
                        RichText::new("Atalhos: Tecla 'I' define Início, Tecla 'O' define Fim na posição atual do leitor.")
                            .size(10.5),
                    );

                    // Checkbox "Corte exato (recodificar)"
                    ui.group(|ui| {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut self.exact_cut, "");
                            ui.vertical(|ui| {
                                ui.strong("Corte exato (recodificar)");
                                ui.colored_label(
                                    Color32::from_rgb(150, 155, 175),
                                    RichText::new(
                                        "Desligado: corte instantâneo no keyframe mais próximo, sem perda de qualidade.\n\
                                         Ligado: preciso ao frame, mais lento, ficheiro pode crescer.",
                                    )
                                    .size(10.5),
                                );
                            });
                        });
                    });

                    // Where the fast cut really starts (told before exporting, §8 risk note)
                    if !self.exact_cut {
                        let hint = match self.keyframes.as_deref() {
                            Some([]) => Some("Sem vídeo: não há keyframes; o corte rápido segue os limites dos pacotes de áudio.".to_string()),
                            Some(kf) => keyframe_at_or_before(kf, self.start_seconds)
                                .filter(|k| self.start_seconds - k >= 0.5)
                                .map(|k| format!(
                                    "Corte rápido: o clip começa no keyframe anterior, {} (e não em {}). Liga «Corte exato» para começar mesmo em {}.",
                                    Self::format_hms(k),
                                    Self::format_hms(self.start_seconds),
                                    Self::format_hms(self.start_seconds),
                                )),
                            None => None,
                        };
                        if let Some(hint) = hint {
                            ui.colored_label(Color32::from_rgb(250, 204, 21), RichText::new(hint).size(10.5));
                        }
                    }

                    // Botão "▶ Reproduzir Seleção" / "⏸ Pausa"
                    let play_label = if is_playing_selection {
                        "⏸ Pausar Seleção"
                    } else {
                        "▶ Reproduzir Seleção"
                    };
                    if ui.button(RichText::new(play_label).strong()).clicked() {
                        if is_playing_selection {
                            action = Some(ClipExportAction::PauseSelection);
                        } else {
                            action = Some(ClipExportAction::PlaySelection {
                                start_seconds: self.start_seconds,
                                end_seconds: self.end_seconds,
                            });
                        }
                    }
                });

                // --- RIGHT COLUMN: Nome do ficheiro & Botão Exportar ---
                let right = &mut cols[1];
                right.vertical(|ui| {
                    ui.spacing_mut().item_spacing = vec2(0.0, 12.0);

                    ui.colored_label(
                        Color32::from_rgb(150, 155, 175),
                        RichText::new("Nome do ficheiro").size(11.0).strong(),
                    );
                    ui.add(
                        egui::TextEdit::singleline(&mut self.filename_input)
                            .desired_width(ui.available_width() - 10.0),
                    );

                    let parent_dir = Path::new(media_path)
                        .parent()
                        .and_then(|p| p.to_str())
                        .unwrap_or(".");
                    ui.colored_label(
                        Color32::from_rgb(120, 125, 145),
                        RichText::new(format!("Pasta de destino: {parent_dir}")).size(10.5),
                    );

                    ui.add_space(8.0);

                    // Botão Exportar Clip
                    let btn_text = if self.is_exporting {
                        "A exportar clip..."
                    } else {
                        "💾 Exportar clip"
                    };

                    let export_btn = egui::Button::new(
                        RichText::new(btn_text)
                            .size(13.0)
                            .strong()
                            .color(Color32::from_rgb(20, 20, 28)),
                    )
                    .fill(accent)
                    .corner_radius(CornerRadius::same(8));

                    let btn_enabled = !self.is_exporting && !self.filename_input.trim().is_empty();
                    if ui.add_enabled(btn_enabled, export_btn).clicked() {
                        action = Some(ClipExportAction::Export {
                            start_seconds: self.start_seconds,
                            end_seconds: self.end_seconds,
                            exact_cut: self.exact_cut,
                            output_filename: self.filename_input.trim().to_string(),
                        });
                    }
                    if self.is_exporting && ui.button("Cancelar exportação").clicked() {
                        action = Some(ClipExportAction::CancelExport);
                    }

                    // Result of the last export, under the button so nothing above it shifts.
                    if let Some((ref msg, is_err, at)) = self.export_status {
                        const SHOWN_FOR: std::time::Duration = std::time::Duration::from_secs(8);
                        // an error stays until the next attempt; a success fades out
                        if is_err || at.elapsed() < SHOWN_FOR {
                            let col = if is_err {
                                Color32::from_rgb(239, 68, 68)
                            } else {
                                Color32::from_rgb(34, 197, 94)
                            };
                            ui.colored_label(col, RichText::new(msg).size(11.5).strong());
                            if !is_err {
                                ui.ctx().request_repaint_after(SHOWN_FOR.saturating_sub(at.elapsed()));
                            }
                        }
                    }
                });
            });
        });

        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_and_parse_hms() {
        assert_eq!(ClipExportPanel::format_hms(0.0), "00:00:00");
        assert_eq!(ClipExportPanel::format_hms(65.0), "00:01:05");
        assert_eq!(ClipExportPanel::format_hms(3665.0), "01:01:05");

        assert_eq!(ClipExportPanel::parse_hms("00:00:00"), Some(0.0));
        assert_eq!(ClipExportPanel::parse_hms("00:01:05"), Some(65.0));
        assert_eq!(ClipExportPanel::parse_hms("01:01:05"), Some(3665.0));
        assert_eq!(ClipExportPanel::parse_hms("02:30"), Some(150.0));
        assert_eq!(ClipExportPanel::parse_hms("45"), Some(45.0));
        assert_eq!(ClipExportPanel::parse_hms("invalid"), None);
    }

    #[test]
    fn test_in_out_point_clamping() {
        let mut panel = ClipExportPanel::new();
        panel.reset_for_media("/tmp/test.mp4", 100.0);

        panel.set_in_point(15.0);
        assert_eq!(panel.start_seconds, 15.0);

        // In point cannot exceed end point - 0.1
        panel.end_seconds = 20.0;
        panel.set_in_point(25.0);
        assert_eq!(panel.start_seconds, 19.9);

        // Out point cannot be less than start point + 0.1
        panel.start_seconds = 30.0;
        panel.set_out_point(25.0, 100.0);
        assert_eq!(panel.end_seconds, 30.1);

        // Out point clamped to max duration
        panel.set_out_point(150.0, 100.0);
        assert_eq!(panel.end_seconds, 100.0);
    }

    #[test]
    fn test_reset_for_media_defaults() {
        let mut panel = ClipExportPanel::new();
        panel.reset_for_media("/home/user/videos/entrevista_direcao.opus", 2292.0);

        assert_eq!(panel.filename_input, "entrevista_direcao_clip.opus");
        assert!(!panel.exact_cut);
        assert_eq!(panel.start_seconds, 0.0);
        assert_eq!(panel.start_input_text, "00:00:00");
        assert!(panel.end_seconds > 0.0);
        assert!(!panel.is_exporting);
        assert!(panel.export_status.is_none());
    }

    #[test]
    fn test_ensure_media_keeps_the_selection_of_the_same_media() {
        let mut panel = ClipExportPanel::new();
        panel.ensure_media("/tmp/a.mp4", 100.0);
        panel.set_in_point(12.0);
        panel.set_out_point(40.0, 100.0);
        panel.exact_cut = true;

        // Reopening (or pressing I/O before the panel was ever shown) must not reset it
        panel.ensure_media("/tmp/a.mp4", 100.0);
        assert_eq!((panel.start_seconds, panel.end_seconds, panel.exact_cut), (12.0, 40.0, true));

        // Another file starts from its own defaults
        panel.ensure_media("/tmp/b.mkv", 200.0);
        assert_eq!((panel.start_seconds, panel.end_seconds, panel.exact_cut), (0.0, 50.0, false));
        assert_eq!(panel.filename_input, "b_clip.mkv");
    }

    #[test]
    fn test_running_export_stays_locked_when_media_changes() {
        let mut panel = ClipExportPanel::new();
        panel.is_exporting = true;
        panel.forget_media();
        panel.ensure_media("/tmp/next.mp4", 60.0);
        assert!(panel.is_exporting);
    }

    #[test]
    fn test_keyframes_for_other_media_are_ignored() {
        let mut panel = ClipExportPanel::new();
        panel.begin_keyframes("/tmp/a.mp4");
        panel.set_keyframes("/tmp/old.mp4", vec![1.0]);
        assert!(panel.keyframes.is_none());
        panel.set_keyframes("/tmp/a.mp4", vec![0.0, 2.0]);
        assert_eq!(panel.keyframes, Some(vec![0.0, 2.0]));
        panel.forget_media();
        assert!(!panel.has_keyframes_for("/tmp/a.mp4"));
    }

    #[test]
    fn test_typed_time_ignores_untouched_box() {
        // A dragged handle at 3.7 s shows "00:00:04"; re-reading that text must not round it.
        assert_eq!(ClipExportPanel::typed_time("00:00:04", 3.7), None);
        assert_eq!(ClipExportPanel::typed_time("00:00:09", 3.7), Some(9.0));
        assert_eq!(ClipExportPanel::typed_time("lixo", 3.7), None);
    }

    #[test]
    fn test_selection_playback_ignores_stale_position_after_seek() {
        // Selection 10–20 s, playhead was at 200 s when the preview started.
        let mut preview = SelectionPlayback::new(20.0);
        assert!(!preview.reached_end(200.0), "stale position must not stop the preview");
        assert!(!preview.reached_end(10.1));
        assert!(!preview.reached_end(19.9));
        assert!(preview.reached_end(20.02));
    }

    #[test]
    fn test_status_notification() {
        let mut panel = ClipExportPanel::new();
        panel.set_status("Sucesso no teste", false);
        assert!(panel.export_status.is_some());
        let (msg, is_err, _) = panel.export_status.unwrap();
        assert_eq!(msg, "Sucesso no teste");
        assert!(!is_err);
    }
}


