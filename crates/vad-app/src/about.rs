//! "Sobre" dialog (Sprint_17, task 7): license, version/commit, source link, third-party
//! notices and the licenses of downloadable models. Evidence of what the binary carries, not
//! legal advice.

use std::sync::OnceLock;

use eframe::egui::{self, RichText};
use vad_ai::{PRESET_LLM_MODELS, PRESET_MODELS};

/// Project repository, where the corresponding source lives (GPL).
pub const SOURCE_URL: &str = "https://github.com/santolass06/vad";
/// Third-party notices, embedded so they travel with the binary.
pub const NOTICES: &str = include_str!("../../../THIRD_PARTY_NOTICES.md");
pub const LICENSE_TEXT: &str = include_str!("../../../LICENSE");

pub fn version_line() -> String {
    format!("VAD {} (commit {})", env!("CARGO_PKG_VERSION"), env!("VAD_GIT_COMMIT"))
}

/// The plain-text part of the dialog (also what the tests check).
pub fn summary() -> String {
    format!(
        "{}\n\n\
         Licença: GPL-3.0 (texto completo no ficheiro LICENSE). A variante «only» ou «or-later» \
         ainda está por decidir pelo autor.\n\
         Código-fonte: {SOURCE_URL}\n\n\
         O VAD usa a libmpv (GPL), embebe fontes de terceiros (egui) e muitas bibliotecas; os avisos \
         e licenças estão abaixo. Os modelos de IA são descarregados do anfitrião original a pedido \
         do utilizador e nunca são redistribuídos pelo VAD.\n\n\
         VLC e VideoLAN são marcas dos respetivos titulares; OpenAI, Anthropic, Google/Gemini e Qwen \
         são referidos apenas de forma nominativa. O VAD não é afiliado nem aprovado por nenhuma delas.",
        version_line()
    )
}

/// Model licenses shown before downloading and repeated here.
pub fn model_license_lines() -> Vec<String> {
    PRESET_MODELS
        .iter()
        .chain(PRESET_LLM_MODELS.iter())
        .map(|p| format!("{} — {}", p.display_name, p.license_line()))
        .collect()
}

fn notice_lines() -> &'static [&'static str] {
    static LINES: OnceLock<Vec<&'static str>> = OnceLock::new();
    LINES.get_or_init(|| NOTICES.lines().collect())
}

fn license_lines() -> &'static [&'static str] {
    static LINES: OnceLock<Vec<&'static str>> = OnceLock::new();
    LINES.get_or_init(|| LICENSE_TEXT.lines().collect())
}

/// Long text, drawn row by row so only the visible lines are laid out.
fn text_viewer(ui: &mut egui::Ui, lines: &[&str]) {
    let row_h = ui.text_style_height(&egui::TextStyle::Monospace);
    egui::ScrollArea::vertical().max_height(280.0).show_rows(ui, row_h, lines.len(), |ui, range| {
        for i in range {
            ui.monospace(lines[i]);
        }
    });
}

#[derive(Default)]
pub struct AboutDialog {
    pub open: bool,
}

impl AboutDialog {
    pub fn show(&mut self, ctx: &egui::Context) {
        if !self.open {
            return;
        }
        let mut open = self.open;
        egui::Window::new("Sobre o VAD")
            .open(&mut open)
            .collapsible(false)
            .default_width(560.0)
            .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
            .show(ctx, |ui| {
                ui.label(RichText::new(summary()).size(crate::theme::MIN_TEXT_SIZE));
                ui.add_space(6.0);
                if ui.link("Abrir o código-fonte no navegador").clicked() {
                    ctx.open_url(egui::OpenUrl::new_tab(SOURCE_URL));
                }
                ui.separator();
                ui.label(RichText::new("Licenças dos modelos de IA").strong());
                for l in model_license_lines() {
                    ui.label(RichText::new(l).size(crate::theme::MIN_TEXT_SIZE).color(crate::theme::TEXT_MUTED));
                }
                ui.separator();
                ui.collapsing("Texto da licença (GPL-3.0, ficheiro LICENSE)", |ui| text_viewer(ui, license_lines()));
                ui.collapsing("Avisos de terceiros (THIRD_PARTY_NOTICES.md)", |ui| text_viewer(ui, notice_lines()));
            });
        self.open = open;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn about_states_license_source_version_and_non_affiliation() {
        let s = summary();
        for must in ["GPL-3.0", SOURCE_URL, env!("CARGO_PKG_VERSION"), "não é afiliado", "nunca são redistribuídos", "VideoLAN"] {
            assert!(s.contains(must), "About lost: {must}");
        }
        assert!(!env!("VAD_GIT_COMMIT").is_empty());
    }

    #[test]
    fn notices_are_embedded_and_cover_egui_fonts() {
        assert!(NOTICES.len() > 100_000, "notices not embedded");
        assert!(NOTICES.contains("epaint_default_fonts"));
        assert!(NOTICES.contains("SIL OPEN FONT LICENSE") || NOTICES.contains("OFL"));
        assert!(LICENSE_TEXT.contains("GNU GENERAL PUBLIC LICENSE"));
    }

    #[test]
    fn about_lists_every_model_with_its_license() {
        let lines = model_license_lines();
        assert_eq!(lines.len(), PRESET_MODELS.len() + PRESET_LLM_MODELS.len());
        assert!(lines.iter().all(|l| l.contains("Licença:")));
    }
}
