//! Definições → IA (design `Settings.dc.html`, PLANO_VAD.md §4.2/§4.23).
//!
//! Sprint_11 scaffold: the OpenAI-compatible card works (endpoint, model, key); Anthropic and
//! Gemini are visual placeholders until Sprint_12.
//!
//! Keyring access is D-Bus and blocks, so it never runs inside `ui()`: every operation goes to
//! a short-lived thread and its result comes back over a channel polled once per frame. The
//! key typed by the user lives in `key_input` only until the click and is never stored in
//! `AiConfig`/`config.toml`.

use std::sync::Arc;
use std::thread;

use crossbeam_channel::{Receiver, TryRecvError};
use eframe::egui::{self, Color32, RichText, Ui};
use vad_ai::endpoint::{is_loopback, is_official_openai, validate_base_url};
use vad_ai::{AiPrivacyBadge, KeySource, SecretSlot, SecretStore};
use vad_core::config::{AiConfig, AiProviderKind};

use super::whisper_panel::render_privacy_badge;

/// Result of a background keyring operation.
enum KeyEvent {
    Checked(SecretSlot, Option<KeySource>),
    Saved(SecretSlot, KeySource),
    Forgotten(SecretSlot),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Unknown,
    Busy,
    Absent,
    Present(KeySource),
}

/// Emitted when `AiConfig` changed and must be saved and propagated.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SettingsAction {
    ConfigChanged,
}

pub struct SettingsPanel {
    secrets: Arc<SecretStore>,
    base_url_input: String,
    model_input: String,
    key_input: String,
    endpoint_error: Option<String>,
    key_state: KeyState,
    key_slot: Option<SecretSlot>,
    rx: Option<Receiver<KeyEvent>>,
    synced: bool,
}

/// Validates `raw` and stores it normalised. Changing the URL drops the custom-endpoint
/// confirmation, which is tied to the exact URL (§4.1).
pub fn apply_endpoint(ai: &mut AiConfig, raw: &str) -> Result<(), String> {
    let url = validate_base_url(raw).map_err(|e| e.to_string())?;
    if ai.openai_base_url != url.as_str() {
        ai.custom_endpoint_acknowledged = None;
    }
    ai.openai_base_url = url.as_str().to_string();
    Ok(())
}

/// Which stored key a URL uses: official OpenAI has its own, anything else the custom one.
pub fn slot_for(base_url: &str) -> SecretSlot {
    match validate_base_url(base_url) {
        Ok(u) if is_official_openai(&u) => SecretSlot::OpenAi,
        _ => SecretSlot::Custom,
    }
}

/// True if the endpoint is custom and not yet confirmed by the user.
pub fn needs_confirmation(ai: &AiConfig) -> bool {
    match validate_base_url(&ai.openai_base_url) {
        Ok(u) => !is_official_openai(&u) && ai.custom_endpoint_acknowledged.as_deref() != Some(u.as_str()),
        Err(_) => true,
    }
}

impl SettingsPanel {
    pub fn new(secrets: Arc<SecretStore>) -> Self {
        Self {
            secrets,
            base_url_input: String::new(),
            model_input: String::new(),
            key_input: String::new(),
            endpoint_error: None,
            key_state: KeyState::Unknown,
            key_slot: None,
            rx: None,
            synced: false,
        }
    }

    pub fn secrets(&self) -> &Arc<SecretStore> {
        &self.secrets
    }

    fn spawn(&mut self, job: impl FnOnce(&SecretStore) -> KeyEvent + Send + 'static) {
        let (tx, rx) = crossbeam_channel::bounded(1);
        let secrets = Arc::clone(&self.secrets);
        self.key_state = KeyState::Busy;
        thread::spawn(move || {
            let _ = tx.send(job(&secrets));
        });
        self.rx = Some(rx);
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        match rx.try_recv() {
            Ok(ev) => {
                self.rx = None;
                let (KeyEvent::Checked(slot, ..) | KeyEvent::Saved(slot, _) | KeyEvent::Forgotten(slot)) = ev;
                // Result for a slot we no longer show (endpoint changed meanwhile): drop it
                // and look the current slot up again.
                if Some(slot) != self.key_slot {
                    self.key_slot = None;
                    self.key_state = KeyState::Unknown;
                    return;
                }
                self.key_state = match ev {
                    KeyEvent::Checked(_, Some(src)) | KeyEvent::Saved(_, src) => KeyState::Present(src),
                    KeyEvent::Checked(_, None) | KeyEvent::Forgotten(_) => KeyState::Absent,
                };
            }
            Err(TryRecvError::Empty) => {}
            // The worker died (panic): do not leave the UI in "busy" forever.
            Err(TryRecvError::Disconnected) => {
                self.rx = None;
                self.key_state = KeyState::Unknown;
            }
        }
    }

    pub fn ui(&mut self, ui: &mut Ui, ai: &mut AiConfig) -> Option<SettingsAction> {
        let mut action = None;
        if !self.synced {
            self.base_url_input = ai.openai_base_url.clone();
            self.model_input = ai.openai_model.clone();
            self.synced = true;
        }
        self.poll();
        if self.rx.is_some() {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        }
        // Look the key up (in the background) whenever the slot in use changes.
        let slot = slot_for(&ai.openai_base_url);
        if self.key_slot != Some(slot) && self.rx.is_none() {
            self.key_slot = Some(slot);
            self.spawn(move |s| KeyEvent::Checked(slot, s.resolve(slot).map(|(_, src)| src)));
        }

        ui.label(RichText::new("IA — fornecedor de resumo e tradução").strong());
        ui.add_space(6.0);

        // --- Provider choice ---
        let before = ai.provider;
        ui.radio_value(&mut ai.provider, AiProviderKind::Local, "🔒 Local (Qwen2.5 0.5B) — nada sai do PC");
        ui.radio_value(&mut ai.provider, AiProviderKind::OpenaiCompatible, "OpenAI-compatible (OpenAI, Ollama, OpenRouter…)");
        if ai.provider != before {
            action = Some(SettingsAction::ConfigChanged);
        }
        ui.add_space(8.0);

        // --- OpenAI-compatible card ---
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("OpenAI-compatible").strong());
                let badge = match validate_base_url(&ai.openai_base_url) {
                    Ok(u) if is_loopback(&u) => AiPrivacyBadge::Local,
                    _ => AiPrivacyBadge::Cloud,
                };
                render_privacy_badge(ui, badge);
            });
            ui.add_space(4.0);
            ui.label("Endpoint (base_url)");
            ui.add(egui::TextEdit::singleline(&mut self.base_url_input).desired_width(f32::INFINITY));
            ui.label("Modelo");
            ui.add(egui::TextEdit::singleline(&mut self.model_input).desired_width(f32::INFINITY));
            if ui.button("Aplicar endpoint e modelo").clicked() {
                match apply_endpoint(ai, &self.base_url_input) {
                    Ok(()) => {
                        self.base_url_input = ai.openai_base_url.clone();
                        self.endpoint_error = None;
                        let model = self.model_input.trim();
                        if !model.is_empty() {
                            ai.openai_model = model.to_string();
                        }
                        action = Some(SettingsAction::ConfigChanged);
                    }
                    Err(e) => self.endpoint_error = Some(e),
                }
            }
            if let Some(err) = &self.endpoint_error {
                ui.colored_label(Color32::from_rgb(239, 68, 68), err);
            }

            // Custom endpoint: the Sprint_10 gate does not transfer to it (§4.1).
            if needs_confirmation(ai) && validate_base_url(&ai.openai_base_url).is_ok() {
                ui.add_space(4.0);
                ui.colored_label(
                    Color32::from_rgb(250, 204, 21),
                    "⚠ Endpoint personalizado: a qualidade de resumo/tradução deste modelo não foi validada pelo VAD e o texto das transcrições é enviado para lá.",
                );
                let mut ack = false;
                if ui.checkbox(&mut ack, "Percebo e quero usar este endpoint").changed() && ack {
                    if let Ok(u) = validate_base_url(&ai.openai_base_url) {
                        ai.custom_endpoint_acknowledged = Some(u.as_str().to_string());
                        action = Some(SettingsAction::ConfigChanged);
                    }
                }
            }

            ui.add_space(6.0);
            ui.label("Chave de API (guardada no keyring do sistema, nunca em config.toml)");
            ui.add(
                egui::TextEdit::singleline(&mut self.key_input)
                    .password(true)
                    .hint_text("sk-…")
                    .desired_width(f32::INFINITY),
            );
            ui.horizontal(|ui| {
                let busy = self.key_state == KeyState::Busy;
                if ui.add_enabled(!busy && !self.key_input.trim().is_empty(), egui::Button::new("Guardar chave")).clicked() {
                    // Moved out of the field at once: it must not linger in the widget.
                    let key = std::mem::take(&mut self.key_input);
                    self.spawn(move |s| KeyEvent::Saved(slot, s.save(slot, &key)));
                }
                if ui.add_enabled(!busy, egui::Button::new("Esquecer")).clicked() {
                    self.spawn(move |s| {
                        s.forget(slot);
                        KeyEvent::Forgotten(slot)
                    });
                }
            });
            let (text, color) = match self.key_state {
                KeyState::Unknown | KeyState::Busy => ("A verificar…".to_string(), Color32::GRAY),
                KeyState::Absent => (
                    "Sem chave. Define-a aqui ou na variável de ambiente VAD_OPENAI_KEY / VAD_CUSTOM_KEY.".to_string(),
                    Color32::from_rgb(250, 204, 21),
                ),
                KeyState::Present(src) => (format!("Chave presente — {}", src.label()), Color32::from_rgb(74, 222, 128)),
            };
            ui.colored_label(color, text);
        });

        ui.add_space(8.0);
        // --- Placeholders (Sprint_12) ---
        for name in ["Anthropic Claude", "Google Gemini"] {
            ui.add_enabled_ui(false, |ui| {
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.label(RichText::new(name).strong());
                    ui.label("Disponível na próxima versão.");
                });
            });
        }
        action
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn apply_endpoint_validates_normalises_and_resets_confirmation() {
        let mut ai = AiConfig {
            openai_base_url: "http://10.0.0.5:11434/v1".into(),
            custom_endpoint_acknowledged: Some("http://10.0.0.5:11434/v1".into()),
            ..AiConfig::default()
        };
        assert!(!needs_confirmation(&ai));
        apply_endpoint(&mut ai, " http://10.0.0.5:11434/v1/ ").unwrap();
        assert!(ai.custom_endpoint_acknowledged.is_some(), "same URL keeps the confirmation");
        apply_endpoint(&mut ai, "http://10.0.0.9:11434/v1").unwrap();
        assert!(ai.custom_endpoint_acknowledged.is_none(), "a different URL drops it");
        assert!(needs_confirmation(&ai));
    }

    #[test]
    fn apply_endpoint_rejects_bad_urls_without_touching_config() {
        let mut ai = AiConfig::default();
        let before = ai.clone();
        for bad in ["file:///etc/passwd", "http://169.254.169.254/latest", "not a url", "https://u:p@x.org/v1"] {
            assert!(apply_endpoint(&mut ai, bad).is_err(), "{bad}");
        }
        assert_eq!(ai, before);
    }

    #[test]
    fn official_openai_needs_no_confirmation_and_uses_its_own_key_slot() {
        let ai = AiConfig::default();
        assert!(!needs_confirmation(&ai));
        assert_eq!(slot_for(&ai.openai_base_url), SecretSlot::OpenAi);
        assert_eq!(slot_for("http://localhost:11434/v1"), SecretSlot::Custom);
        assert_eq!(slot_for("garbage"), SecretSlot::Custom);
    }

    /// Settings must round-trip through config.toml without ever carrying a secret.
    #[test]
    fn ai_config_serialises_no_key_material() {
        let toml = toml_like(&AiConfig::default());
        assert!(toml.contains("openai_base_url") && !toml.to_lowercase().contains("key"), "{toml}");
    }

    fn toml_like(ai: &AiConfig) -> String {
        let cfg = vad_core::VadConfig { ai: ai.clone(), ..Default::default() };
        let dir = std::env::temp_dir().join(format!("vad-ai-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        cfg.save_to_path(&path).unwrap();
        let out = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        out
    }
}
