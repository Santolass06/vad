//! API-key storage (PLANO_VAD.md §4.2). Keys never go in `config.toml` or logs.
//!
//! Lookup order: this session's memory → OS keyring (Secret Service) → env var.
//! When the keyring is unavailable (minimal/headless setups) `save` falls back to
//! session memory and says so, so the UI can tell the user the key is not persisted.
//!
//! Keyring calls talk D-Bus and **block**: never call them from the egui `update()`.

use std::collections::HashMap;
use std::fmt;
use std::sync::Mutex;

const SERVICE: &str = "vad";

/// Which credential a provider uses.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum SecretSlot {
    /// Official OpenAI endpoint.
    OpenAi,
    /// Any custom / self-hosted OpenAI-compatible endpoint.
    Custom,
    Anthropic,
    Gemini,
}

impl SecretSlot {
    pub fn env_var(self) -> &'static str {
        match self {
            Self::OpenAi => "VAD_OPENAI_KEY",
            Self::Custom => "VAD_CUSTOM_KEY",
            Self::Anthropic => "VAD_ANTHROPIC_KEY",
            Self::Gemini => "VAD_GEMINI_KEY",
        }
    }

    fn account(self) -> &'static str {
        match self {
            Self::OpenAi => "openai",
            Self::Custom => "custom",
            Self::Anthropic => "anthropic",
            Self::Gemini => "gemini",
        }
    }
}

/// Where a key came from / went to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum KeySource {
    Session,
    Keyring,
    Env,
}

impl KeySource {
    pub fn label(self) -> &'static str {
        match self {
            Self::Session => "só nesta sessão (não guardada)",
            Self::Keyring => "keyring do sistema",
            Self::Env => "variável de ambiente",
        }
    }
}

/// A secret that cannot be printed by accident.
#[derive(Clone)]
pub struct Secret(String);

impl Secret {
    pub fn new(s: impl Into<String>) -> Self {
        Self(s.into())
    }
    /// The only way to read it; call sites are the HTTP header and tests.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(<redacted>)")
    }
}

/// Persistent backend (the OS keyring in production, a fake in tests).
pub trait SecretBackend: Send + Sync {
    fn get(&self, account: &str) -> Result<Option<String>, String>;
    fn set(&self, account: &str, value: &str) -> Result<(), String>;
    fn delete(&self, account: &str) -> Result<(), String>;
}

/// OS keyring through the Secret Service.
pub struct KeyringBackend {
    service: &'static str,
}

impl KeyringBackend {
    pub fn new(service: &'static str) -> Self {
        Self { service }
    }
}

impl SecretBackend for KeyringBackend {
    fn get(&self, account: &str) -> Result<Option<String>, String> {
        let entry = keyring::Entry::new(self.service, account).map_err(|e| e.to_string())?;
        match entry.get_password() {
            Ok(p) => Ok(Some(p)),
            Err(keyring::Error::NoEntry) => Ok(None),
            Err(e) => Err(e.to_string()),
        }
    }
    fn set(&self, account: &str, value: &str) -> Result<(), String> {
        keyring::Entry::new(self.service, account)
            .and_then(|e| e.set_password(value))
            .map_err(|e| e.to_string())
    }
    fn delete(&self, account: &str) -> Result<(), String> {
        match keyring::Entry::new(self.service, account).and_then(|e| e.delete_credential()) {
            Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
            Err(e) => Err(e.to_string()),
        }
    }
}

type EnvLookup = Box<dyn Fn(&str) -> Option<String> + Send + Sync>;

pub struct SecretStore {
    backend: Box<dyn SecretBackend>,
    session: Mutex<HashMap<SecretSlot, String>>,
    env: EnvLookup,
}

impl SecretStore {
    pub fn system() -> Self {
        Self::with(Box::new(KeyringBackend::new(SERVICE)), Box::new(|k| std::env::var(k).ok()))
    }

    pub fn with(backend: Box<dyn SecretBackend>, env: EnvLookup) -> Self {
        Self { backend, session: Mutex::new(HashMap::new()), env }
    }

    /// Finds the key for `slot`, with its origin. Blocking (keyring).
    pub fn resolve(&self, slot: SecretSlot) -> Option<(Secret, KeySource)> {
        if let Some(k) = self.session.lock().ok().and_then(|m| m.get(&slot).cloned()) {
            return Some((Secret::new(k), KeySource::Session));
        }
        // A keyring failure is not fatal: the env var is still tried.
        if let Ok(Some(k)) = self.backend.get(slot.account()) {
            if !k.is_empty() {
                return Some((Secret::new(k), KeySource::Keyring));
            }
        }
        (self.env)(slot.env_var())
            .filter(|k| !k.trim().is_empty())
            .map(|k| (Secret::new(k.trim()), KeySource::Env))
    }

    /// Saves a key the user typed. Goes to the keyring; if that is unavailable it stays in
    /// session memory only and the returned source says so. Blocking (keyring).
    pub fn save(&self, slot: SecretSlot, key: &str) -> KeySource {
        let key = key.trim();
        match self.backend.set(slot.account(), key) {
            Ok(()) => {
                if let Ok(mut m) = self.session.lock() {
                    m.remove(&slot);
                }
                KeySource::Keyring
            }
            Err(e) => {
                tracing::warn!("keyring indisponível ({e}); chave mantida só em memória");
                if let Ok(mut m) = self.session.lock() {
                    m.insert(slot, key.to_string());
                }
                KeySource::Session
            }
        }
    }

    /// Forgets the key everywhere we control (not the env var). Blocking (keyring).
    pub fn forget(&self, slot: SecretSlot) {
        if let Ok(mut m) = self.session.lock() {
            m.remove(&slot);
        }
        let _ = self.backend.delete(slot.account());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct FakeKeyring(Mutex<HashMap<String, String>>);
    impl SecretBackend for FakeKeyring {
        fn get(&self, a: &str) -> Result<Option<String>, String> {
            Ok(self.0.lock().unwrap().get(a).cloned())
        }
        fn set(&self, a: &str, v: &str) -> Result<(), String> {
            self.0.lock().unwrap().insert(a.into(), v.into());
            Ok(())
        }
        fn delete(&self, a: &str) -> Result<(), String> {
            self.0.lock().unwrap().remove(a);
            Ok(())
        }
    }

    /// No Secret Service, as on a headless box.
    struct DeadKeyring;
    impl SecretBackend for DeadKeyring {
        fn get(&self, _: &str) -> Result<Option<String>, String> { Err("no secret service".into()) }
        fn set(&self, _: &str, _: &str) -> Result<(), String> { Err("no secret service".into()) }
        fn delete(&self, _: &str) -> Result<(), String> { Err("no secret service".into()) }
    }

    fn env(pairs: &'static [(&'static str, &'static str)]) -> EnvLookup {
        Box::new(move |k| pairs.iter().find(|(n, _)| *n == k).map(|(_, v)| v.to_string()))
    }

    #[test]
    fn keyring_happy_path_roundtrip_and_forget() {
        let s = SecretStore::with(Box::new(FakeKeyring::default()), env(&[]));
        assert!(s.resolve(SecretSlot::OpenAi).is_none());
        assert_eq!(s.save(SecretSlot::OpenAi, "  sk-abc "), KeySource::Keyring);
        let (k, src) = s.resolve(SecretSlot::OpenAi).unwrap();
        assert_eq!((k.expose(), src), ("sk-abc", KeySource::Keyring));
        assert!(s.resolve(SecretSlot::Custom).is_none(), "slots are independent");
        s.forget(SecretSlot::OpenAi);
        assert!(s.resolve(SecretSlot::OpenAi).is_none());
    }

    #[test]
    fn env_var_fallback_when_keyring_is_empty_or_dead() {
        for backend in [Box::new(FakeKeyring::default()) as Box<dyn SecretBackend>, Box::new(DeadKeyring)] {
            let s = SecretStore::with(backend, env(&[("VAD_OPENAI_KEY", "sk-env"), ("VAD_CUSTOM_KEY", "  ")]));
            let (k, src) = s.resolve(SecretSlot::OpenAi).unwrap();
            assert_eq!((k.expose(), src), ("sk-env", KeySource::Env));
            assert!(s.resolve(SecretSlot::Custom).is_none(), "blank env var is not a key");
        }
    }

    #[test]
    fn session_memory_fallback_when_secret_service_is_unavailable() {
        let s = SecretStore::with(Box::new(DeadKeyring), env(&[]));
        assert_eq!(s.save(SecretSlot::Custom, "sk-mem"), KeySource::Session);
        let (k, src) = s.resolve(SecretSlot::Custom).unwrap();
        assert_eq!((k.expose(), src), ("sk-mem", KeySource::Session));
        s.forget(SecretSlot::Custom);
        assert!(s.resolve(SecretSlot::Custom).is_none());
    }

    #[test]
    fn debug_never_prints_the_key() {
        let shown = format!("{:?}", Secret::new("sk-super-secret"));
        assert!(!shown.contains("super-secret"), "{shown}");
    }

    /// The real keyring on this machine: must not panic whether or not a Secret Service
    /// exists, and `save` must report honestly where the key ended up.
    #[test]
    fn system_store_degrades_without_secret_service() {
        // Separate service name: this test must never touch the user's real "vad" key.
        let s = SecretStore::with(Box::new(KeyringBackend::new("vad-test")), Box::new(|_| None));
        let src = s.save(SecretSlot::Custom, "sk-probe-do-not-keep");
        assert!(matches!(src, KeySource::Keyring | KeySource::Session));
        assert_eq!(s.resolve(SecretSlot::Custom).unwrap().0.expose(), "sk-probe-do-not-keep");
        s.forget(SecretSlot::Custom);
    }
}
