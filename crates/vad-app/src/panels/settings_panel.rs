//! Definições → IA (design `Settings.dc.html`, PLANO_VAD.md §4.2/§4.23).
//!
//! Three provider cards (OpenAI-compatible, Anthropic, Gemini), each with its own key and a
//! "Testar ligação" button. Testing and saving are separate steps: the test uses what is typed
//! in the card (a draft) and persists nothing; "Aplicar" is what writes `AiConfig`.
//!
//! Keyring access (D-Bus) and HTTP both block, so neither runs inside `ui()`: each goes to a
//! short-lived thread and its result comes back over a channel polled once per frame. A typed
//! key lives in `key_input` only until the click and is never stored in `AiConfig`.

use std::sync::Arc;
use std::thread;
use std::time::Duration;

use crossbeam_channel::{Receiver, Sender};
use eframe::egui::{self, RichText, Ui};
use vad_ai::endpoint::{is_loopback, is_official_openai, validate_base_url};
use vad_ai::{diagnose, test_connection, AiPrivacyBadge, Draft, KeySource, Secret, SecretSlot, SecretStore};
use vad_core::config::{AiConfig, AiProviderKind};

use super::whisper_panel::render_privacy_badge;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
enum Which {
    OpenAi,
    Anthropic,
    Gemini,
}

impl Which {
    const ALL: [Which; 3] = [Which::OpenAi, Which::Anthropic, Which::Gemini];

    /// Long name for the provider radio list (wraps).
    fn title(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI-compatible (OpenAI, Ollama, OpenRouter…)",
            Self::Anthropic => "Anthropic Claude",
            Self::Gemini => "Google Gemini",
        }
    }

    /// Short name for the card header, which also holds the privacy badge on one row.
    fn short(self) -> &'static str {
        match self {
            Self::OpenAi => "OpenAI-compatible",
            Self::Anthropic => "Anthropic Claude",
            Self::Gemini => "Google Gemini",
        }
    }

    fn kind(self) -> AiProviderKind {
        match self {
            Self::OpenAi => AiProviderKind::OpenaiCompatible,
            Self::Anthropic => AiProviderKind::Anthropic,
            Self::Gemini => AiProviderKind::Gemini,
        }
    }
}

enum Event {
    KeyChecked(Which, SecretSlot, Option<KeySource>),
    KeySaved(Which, SecretSlot, KeySource),
    KeyForgotten(Which, SecretSlot),
    /// `u64` is the card's edit generation when the test started: a result for fields the
    /// user has since changed is stale and ignored.
    TestDone(Which, u64, Result<u64, String>),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum KeyState {
    Unknown,
    Busy,
    Absent,
    Present(KeySource),
}

/// The four explicit states of §4.23.
#[derive(Debug, Clone, PartialEq, Eq)]
enum TestState {
    Idle,
    Testing,
    Ok { ms: u64 },
    Failed(String),
}

struct Card {
    key_input: String,
    key_state: KeyState,
    key_slot: Option<SecretSlot>,
    test: TestState,
    /// Bumped whenever a field that affects the test changes.
    generation: u64,
}

impl Card {
    fn new() -> Self {
        Self { key_input: String::new(), key_state: KeyState::Unknown, key_slot: None, test: TestState::Idle, generation: 0 }
    }
    fn invalidate(&mut self) {
        self.generation += 1;
        self.test = TestState::Idle;
    }
}

pub struct SettingsPanel {
    secrets: Arc<SecretStore>,
    tx: Sender<Event>,
    rx: Receiver<Event>,
    base_url_input: String,
    openai_model_input: String,
    anthropic_model_input: String,
    gemini_model_input: String,
    endpoint_error: Option<String>,
    cards: [Card; 3],
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

/// Which stored key an OpenAI-compatible URL uses: official OpenAI has its own key,
/// anything else the custom one.
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

fn idx(w: Which) -> usize {
    Which::ALL.iter().position(|x| *x == w).unwrap_or(0)
}

impl SettingsPanel {
    pub fn new(secrets: Arc<SecretStore>) -> Self {
        let (tx, rx) = crossbeam_channel::unbounded();
        Self {
            secrets,
            tx,
            rx,
            base_url_input: String::new(),
            openai_model_input: String::new(),
            anthropic_model_input: String::new(),
            gemini_model_input: String::new(),
            endpoint_error: None,
            cards: [Card::new(), Card::new(), Card::new()],
            synced: false,
        }
    }

    pub fn secrets(&self) -> &Arc<SecretStore> {
        &self.secrets
    }

    fn spawn(&self, job: impl FnOnce(&SecretStore, &Sender<Event>) + Send + 'static) {
        let (secrets, tx) = (Arc::clone(&self.secrets), self.tx.clone());
        thread::spawn(move || job(&secrets, &tx));
    }

    fn slot(&self, w: Which, ai: &AiConfig) -> SecretSlot {
        match w {
            Which::OpenAi => slot_for(&ai.openai_base_url),
            Which::Anthropic => SecretSlot::Anthropic,
            Which::Gemini => SecretSlot::Gemini,
        }
    }

    fn poll(&mut self) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                Event::KeyChecked(w, slot, src) => {
                    let c = &mut self.cards[idx(w)];
                    // Stale if the card moved to another slot meanwhile (endpoint changed).
                    if c.key_slot == Some(slot) {
                        c.key_state = src.map_or(KeyState::Absent, KeyState::Present);
                    }
                }
                Event::KeySaved(w, slot, src) => {
                    let c = &mut self.cards[idx(w)];
                    if c.key_slot == Some(slot) {
                        c.key_state = KeyState::Present(src);
                    }
                }
                Event::KeyForgotten(w, slot) => {
                    let c = &mut self.cards[idx(w)];
                    if c.key_slot == Some(slot) {
                        c.key_state = KeyState::Absent;
                    }
                }
                Event::TestDone(w, generation, res) => {
                    let c = &mut self.cards[idx(w)];
                    if c.generation == generation {
                        c.test = match res {
                            Ok(ms) => TestState::Ok { ms },
                            Err(msg) => TestState::Failed(msg),
                        };
                    }
                }
            }
        }
    }

    fn draft(&self, w: Which) -> Draft {
        match w {
            Which::OpenAi => Draft::OpenAi { base_url: self.base_url_input.clone(), model: self.openai_model_input.clone() },
            Which::Anthropic => Draft::Anthropic { model: self.anthropic_model_input.clone() },
            Which::Gemini => Draft::Gemini { model: self.gemini_model_input.clone() },
        }
    }

    /// Starts a connection test of the card's draft. The typed key wins; otherwise the stored
    /// one is looked up (on the worker). Nothing is saved.
    fn start_test(&mut self, w: Which, ai: &AiConfig) {
        let slot = self.slot(w, ai);
        let typed = self.cards[idx(w)].key_input.trim().to_string();
        let generation = self.cards[idx(w)].generation;
        self.cards[idx(w)].test = TestState::Testing;
        let draft = self.draft(w);
        self.spawn(move |secrets, tx| {
            let key = if typed.is_empty() { secrets.resolve(slot).map(|(k, _)| k) } else { Some(Secret::new(typed)) };
            let res = test_connection(&draft, key).map_err(|e| diagnose(&e));
            let _ = tx.send(Event::TestDone(w, generation, res));
        });
    }

    pub fn ui(&mut self, ui: &mut Ui, ai: &mut AiConfig) -> Option<()> {
        let mut changed = false;
        if !self.synced {
            self.base_url_input = ai.openai_base_url.clone();
            self.openai_model_input = ai.openai_model.clone();
            self.anthropic_model_input = ai.anthropic_model.clone();
            self.gemini_model_input = ai.gemini_model.clone();
            self.synced = true;
        }
        self.poll();
        let busy = self.cards.iter().any(|c| c.test == TestState::Testing || c.key_state == KeyState::Busy);
        if busy {
            ui.ctx().request_repaint_after(Duration::from_millis(100));
        }
        // Look each card's key up (in the background) when first shown or when its slot changes.
        for w in Which::ALL {
            let slot = self.slot(w, ai);
            if self.cards[idx(w)].key_slot != Some(slot) {
                let c = &mut self.cards[idx(w)];
                c.key_slot = Some(slot);
                c.key_state = KeyState::Busy;
                self.spawn(move |s, tx| {
                    let _ = tx.send(Event::KeyChecked(w, slot, s.resolve(slot).map(|(_, src)| src)));
                });
            }
        }

        // The side panel is a fixed 340 px: nothing here may ask for more, or the panel is
        // pushed past the window edge (seen on screen in Sprint_15).
        ui.set_max_width(ui.available_width());
        ui.label(RichText::new("IA — fornecedor de resumo e tradução").strong());
        ui.add_space(6.0);
        let before = ai.provider;
        ui.radio_value(&mut ai.provider, AiProviderKind::Local, "🔒 Local (Qwen2.5 0.5B) — nada sai do PC");
        for w in Which::ALL {
            ui.radio_value(&mut ai.provider, w.kind(), w.title());
        }
        changed |= ai.provider != before;
        ui.add_space(8.0);

        for w in Which::ALL {
            changed |= self.card(ui, w, ai);
            ui.add_space(6.0);
        }
        changed.then_some(())
    }

    fn card(&mut self, ui: &mut Ui, w: Which, ai: &mut AiConfig) -> bool {
        let mut changed = false;
        let slot = self.slot(w, ai);
        egui::Frame::group(ui.style()).show(ui, |ui| {
            ui.set_max_width(ui.available_width());
            ui.horizontal(|ui| {
                ui.label(RichText::new(w.short()).strong());
                let badge = match w {
                    Which::OpenAi => match validate_base_url(&ai.openai_base_url) {
                        Ok(u) if is_loopback(&u) => AiPrivacyBadge::Local,
                        _ => AiPrivacyBadge::Cloud,
                    },
                    _ => AiPrivacyBadge::Cloud,
                };
                render_privacy_badge(ui, badge);
            });
            ui.add_space(4.0);

            let mut edited = false;
            if w == Which::OpenAi {
                ui.label("Endpoint (base_url)");
                edited |= ui.add(egui::TextEdit::singleline(&mut self.base_url_input).desired_width(ui.available_width())).changed();
            }
            ui.label("Modelo");
            let model = match w {
                Which::OpenAi => &mut self.openai_model_input,
                Which::Anthropic => &mut self.anthropic_model_input,
                Which::Gemini => &mut self.gemini_model_input,
            };
            edited |= ui.add(egui::TextEdit::singleline(model).desired_width(ui.available_width())).changed();

            ui.label("Chave de API (guardada no keyring, nunca em config.toml)");
            edited |= ui
                .add(egui::TextEdit::singleline(&mut self.cards[idx(w)].key_input).password(true).hint_text("sk-…").desired_width(ui.available_width()))
                .changed();
            if edited {
                self.cards[idx(w)].invalidate();
            }

            ui.horizontal(|ui| {
                let testing = self.cards[idx(w)].test == TestState::Testing;
                if ui.add_enabled(!testing, egui::Button::new("Testar ligação")).clicked() {
                    self.start_test(w, ai);
                }
                let can_save = !self.cards[idx(w)].key_input.trim().is_empty();
                if ui.add_enabled(can_save, egui::Button::new("Guardar chave")).clicked() {
                    // Moved out of the field at once: it must not linger in the widget.
                    let key = std::mem::take(&mut self.cards[idx(w)].key_input);
                    self.cards[idx(w)].key_state = KeyState::Busy;
                    self.spawn(move |s, tx| {
                        let _ = tx.send(Event::KeySaved(w, slot, s.save(slot, &key)));
                    });
                }
                if ui.button("Esquecer").clicked() {
                    self.cards[idx(w)].key_state = KeyState::Busy;
                    self.spawn(move |s, tx| {
                        s.forget(slot);
                        let _ = tx.send(Event::KeyForgotten(w, slot));
                    });
                }
            });

            // Key status.
            let (text, color) = match self.cards[idx(w)].key_state {
                KeyState::Unknown | KeyState::Busy => ("A verificar a chave…".to_string(), crate::theme::TEXT_MUTED),
                KeyState::Absent => (format!("Sem chave. Define-a aqui ou em {}.", slot.env_var()), crate::theme::WARN),
                KeyState::Present(src) => (format!("Chave presente — {}", src.label()), crate::theme::OK),
            };
            ui.colored_label(color, text);

            // The four explicit states of "Testar ligação" (§4.23).
            match &self.cards[idx(w)].test {
                TestState::Idle => {
                    ui.colored_label(crate::theme::TEXT_MUTED, "Repouso — ainda não testada.");
                }
                TestState::Testing => {
                    ui.horizontal(|ui| {
                        ui.spinner();
                        ui.label("A testar…");
                    });
                }
                TestState::Ok { ms } => {
                    ui.colored_label(crate::theme::OK, format!("✔ Ligação OK — {ms} ms"));
                }
                TestState::Failed(msg) => {
                    ui.colored_label(crate::theme::ERROR, format!("✖ {msg}"));
                }
            }

            // "Aplicar" is the persisting step, distinct from testing.
            if ui.button("Aplicar (gravar nas definições)").clicked() {
                let mut ok = true;
                match w {
                    Which::OpenAi => match apply_endpoint(ai, &self.base_url_input) {
                        Ok(()) => {
                            self.base_url_input = ai.openai_base_url.clone();
                            self.endpoint_error = None;
                            ai.openai_model = self.openai_model_input.trim().to_string();
                        }
                        Err(e) => {
                            self.endpoint_error = Some(e);
                            ok = false;
                        }
                    },
                    Which::Anthropic => ai.anthropic_model = self.anthropic_model_input.trim().to_string(),
                    Which::Gemini => ai.gemini_model = self.gemini_model_input.trim().to_string(),
                }
                changed |= ok;
            }
            if w == Which::OpenAi {
                if let Some(err) = &self.endpoint_error {
                    ui.colored_label(crate::theme::ERROR, err);
                }
                // Custom endpoint: the Sprint_10 gate does not transfer to it (§4.1).
                if needs_confirmation(ai) && validate_base_url(&ai.openai_base_url).is_ok() {
                    ui.colored_label(
                        crate::theme::WARN,
                        "⚠ Endpoint personalizado: a qualidade de resumo/tradução deste modelo não foi validada pelo VAD e o texto das transcrições é enviado para lá.",
                    );
                    let mut ack = false;
                    if ui.checkbox(&mut ack, "Percebo e quero usar este endpoint").changed() && ack {
                        if let Ok(u) = validate_base_url(&ai.openai_base_url) {
                            ai.custom_endpoint_acknowledged = Some(u.as_str().to_string());
                            changed = true;
                        }
                    }
                }
            }
        });
        changed
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
        let cfg = vad_core::VadConfig::default();
        let dir = std::env::temp_dir().join(format!("vad-ai-cfg-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("config.toml");
        cfg.save_to_path(&path).unwrap();
        let toml = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(dir);
        assert!(toml.contains("openai_base_url") && toml.contains("anthropic_model"), "{toml}");
        assert!(!toml.to_lowercase().contains("key") && !toml.to_lowercase().contains("secret"), "{toml}");
    }

    /// Testing must never write the configuration (§4.23 known risk): a test of a draft leaves
    /// `AiConfig` untouched, and a stale result for edited fields is discarded.
    #[test]
    fn test_result_is_discarded_when_the_fields_changed_meanwhile() {
        let mut p = SettingsPanel::new(Arc::new(SecretStore::with(Box::new(NoKeyring), Box::new(|_| None))));
        let gen_at_start = p.cards[0].generation;
        p.cards[0].test = TestState::Testing;
        p.cards[0].invalidate(); // user edited a field
        p.tx.send(Event::TestDone(Which::OpenAi, gen_at_start, Ok(42))).unwrap();
        p.poll();
        assert_eq!(p.cards[0].test, TestState::Idle, "stale success must not show as current");
        p.tx.send(Event::TestDone(Which::OpenAi, p.cards[0].generation, Err("x".into()))).unwrap();
        p.poll();
        assert_eq!(p.cards[0].test, TestState::Failed("x".into()));
    }

    struct NoKeyring;
    impl vad_ai::secrets::SecretBackend for NoKeyring {
        fn get(&self, _: &str) -> Result<Option<String>, String> { Ok(None) }
        fn set(&self, _: &str, _: &str) -> Result<(), String> { Err("no".into()) }
        fn delete(&self, _: &str) -> Result<(), String> { Ok(()) }
    }
}
