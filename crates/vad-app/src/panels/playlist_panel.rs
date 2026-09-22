use eframe::egui::{self, pos2, vec2, Color32, CornerRadius, Rect, Sense, Stroke, Ui};
use vad_core::{Playlist, RepeatMode};

/// Actions emitted by the playlist panel to be handled by `VadApp`.
pub enum PlaylistAction {
    PlayItem(usize),
    AddFileRequest,
    AddUrlRequest,
}

/// Lateral playlist panel corresponding to `design/Playlist.dc.html`.
/// Fits strictly within the 340px uniform sidebar.
pub struct PlaylistPanel;

impl PlaylistPanel {
    /// Formats seconds into HH:MM:SS or MM:SS.
    fn format_duration(seconds: f64) -> String {
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

    pub fn ui(
        ui: &mut Ui,
        playlist: &mut Playlist,
        current_time: f64,
        total_duration: f64,
    ) -> Option<PlaylistAction> {
        let mut action = None;
        let accent = Color32::from_rgb(139, 124, 246);
        let inactive_bg = Color32::from_rgba_unmultiplied(255, 255, 255, 12);

        ui.spacing_mut().item_spacing = vec2(0.0, 10.0);

        // --- CONTROLS ROW: SHUFFLE & REPEAT ---
        ui.horizontal(|ui| {
            // Shuffle toggle
            let is_shuffle = playlist.shuffle();
            let shuffle_text = if is_shuffle {
                "🔀 Aleatório: ATIVO"
            } else {
                "🔀 Aleatório: DESLIGADO"
            };
            let shuffle_btn = egui::Button::new(
                egui::RichText::new(shuffle_text)
                    .size(11.0)
                    .strong()
                    .color(if is_shuffle {
                        Color32::from_rgb(20, 20, 28)
                    } else {
                        Color32::from_rgb(200, 205, 225)
                    }),
            )
            .fill(if is_shuffle { accent } else { inactive_bg })
            .corner_radius(CornerRadius::same(6));

            if ui.add(shuffle_btn).clicked() {
                playlist.toggle_shuffle();
            }

            // Repeat toggle
            let rep_label = match playlist.repeat() {
                RepeatMode::Off => "🔁 Repetir: Desat.",
                RepeatMode::All => "🔁 Repetir: Lista",
                RepeatMode::Single => "🔂 Repetir: Faixa",
            };
            let is_rep_active = playlist.repeat() != RepeatMode::Off;
            let rep_btn = egui::Button::new(
                egui::RichText::new(rep_label)
                    .size(11.0)
                    .strong()
                    .color(if is_rep_active {
                        accent
                    } else {
                        Color32::from_rgb(200, 205, 225)
                    }),
            )
            .fill(inactive_bg)
            .corner_radius(CornerRadius::same(6));

            if ui.add(rep_btn).clicked() {
                playlist.cycle_repeat();
            }
        });

        // --- ACTIONS ROW: ADD FILE / URL ---
        ui.horizontal(|ui| {
            if ui.button("➕ Ficheiro...").clicked() {
                action = Some(PlaylistAction::AddFileRequest);
            }
            if ui.button("🌐 Colar URL...").clicked() {
                action = Some(PlaylistAction::AddUrlRequest);
            }
            if !playlist.is_empty() && ui.small_button("Limpar").clicked() {
                playlist.clear();
            }
        });

        ui.separator();

        // --- ITEMS LIST ---
        let list_h = ui.available_height() - 90.0;
        egui::ScrollArea::vertical()
            .max_height(list_h.max(120.0))
            .show(ui, |ui| {
                if playlist.is_empty() {
                    ui.add_space(30.0);
                    ui.vertical_centered(|ui| {
                        ui.colored_label(
                            Color32::from_rgb(140, 145, 165),
                            "A lista de reprodução está vazia",
                        );
                        ui.add_space(4.0);
                        ui.label("Adiciona ficheiros locais ou URLs acima.");
                    });
                    return;
                }

                let current_idx = playlist.current_index();
                let mut remove_idx = None;

                for i in 0..playlist.len() {
                    let is_active = current_idx == Some(i);
                    let item = &playlist.items()[i];

                    let (rect, response) = ui.allocate_exact_size(
                        vec2(ui.available_width(), 36.0),
                        Sense::click(),
                    );

                    let painter = ui.painter_at(rect);

                    // Row background
                    let row_bg = if is_active {
                        Color32::from_rgba_unmultiplied(139, 124, 246, 35)
                    } else if response.hovered() {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 14)
                    } else {
                        Color32::from_rgba_unmultiplied(255, 255, 255, 5)
                    };
                    painter.rect_filled(rect, CornerRadius::same(6), row_bg);

                    if is_active {
                        painter.rect_stroke(
                            rect,
                            CornerRadius::same(6),
                            Stroke::new(1.0, Color32::from_rgba_unmultiplied(139, 124, 246, 120)),
                            eframe::egui::StrokeKind::Inside,
                        );
                    }

                    // Icon
                    let icon_str = if item.is_url() { "🌐" } else { "🎬" };
                    painter.text(
                        pos2(rect.left() + 8.0, rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        icon_str,
                        egui::FontId::proportional(14.0),
                        Color32::WHITE,
                    );

                    // Title
                    let title_text = item.title();
                    let title_color = if is_active {
                        accent
                    } else {
                        Color32::from_rgb(220, 225, 240)
                    };

                    let title_rect = Rect::from_min_max(
                        pos2(rect.left() + 30.0, rect.min.y),
                        pos2(rect.right() - 65.0, rect.max.y),
                    );
                    painter.text(
                        pos2(title_rect.left(), title_rect.center().y),
                        egui::Align2::LEFT_CENTER,
                        title_text,
                        egui::FontId::proportional(12.5),
                        title_color,
                    );

                    // Duration if available
                    if let Some(dur) = item.duration() {
                        painter.text(
                            pos2(rect.right() - 26.0, rect.center().y),
                            egui::Align2::RIGHT_CENTER,
                            Self::format_duration(dur),
                            egui::FontId::monospace(10.5),
                            Color32::from_rgb(140, 145, 165),
                        );
                    }

                    // Delete button (✕)
                    let del_rect = Rect::from_center_size(
                        pos2(rect.right() - 12.0, rect.center().y),
                        vec2(18.0, 18.0),
                    );
                    let del_resp = ui.interact(
                        del_rect,
                        ui.id().with(("del_item", i)),
                        Sense::click(),
                    );
                    let del_color = if del_resp.hovered() {
                        Color32::from_rgb(240, 90, 90)
                    } else {
                        Color32::from_rgb(120, 125, 140)
                    };
                    painter.text(
                        del_rect.center(),
                        egui::Align2::CENTER_CENTER,
                        "✕",
                        egui::FontId::monospace(11.0),
                        del_color,
                    );
                    if del_resp.clicked() {
                        remove_idx = Some(i);
                    } else if response.clicked() {
                        action = Some(PlaylistAction::PlayItem(i));
                    }

                    ui.add_space(2.0);
                }

                if let Some(idx) = remove_idx {
                    playlist.remove(idx);
                }
            });

        // --- NOW PLAYING CARD AT BOTTOM ---
        if let Some(current_item) = playlist.current() {
            ui.add_space(4.0);
            let card_rect = ui.available_rect_before_wrap();
            let card_h = 60.0_f32;
            let (rect, _) = ui.allocate_exact_size(vec2(card_rect.width(), card_h), Sense::hover());

            let painter = ui.painter_at(rect);
            painter.rect_filled(
                rect,
                CornerRadius::same(8),
                Color32::from_rgba_unmultiplied(26, 28, 40, 240),
            );
            painter.rect_stroke(
                rect,
                CornerRadius::same(8),
                Stroke::new(1.0, Color32::from_rgba_unmultiplied(255, 255, 255, 20)),
                eframe::egui::StrokeKind::Inside,
            );

            painter.text(
                pos2(rect.left() + 10.0, rect.top() + 10.0),
                egui::Align2::LEFT_TOP,
                "A TOCAR AGORA",
                egui::FontId::monospace(9.5),
                Color32::from_rgb(140, 145, 165),
            );

            let title = current_item.title();
            painter.text(
                pos2(rect.left() + 10.0, rect.top() + 25.0),
                egui::Align2::LEFT_TOP,
                title,
                egui::FontId::proportional(12.5),
                accent,
            );

            let time_str = format!(
                "{} / {}",
                Self::format_duration(current_time),
                Self::format_duration(total_duration)
            );
            painter.text(
                pos2(rect.left() + 10.0, rect.top() + 42.0),
                egui::Align2::LEFT_TOP,
                time_str,
                egui::FontId::monospace(10.5),
                Color32::from_rgb(160, 165, 185),
            );
        }

        action
    }
}
