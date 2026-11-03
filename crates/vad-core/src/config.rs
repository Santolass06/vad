use std::collections::HashMap;
use std::fs;
use std::path::Path;

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::VadError;
use crate::util::{vad_config_path, write_atomic};

fn default_true() -> bool {
    true
}

fn default_volume() -> f64 {
    100.0
}

fn default_hwdec() -> String {
    "auto-safe".to_string()
}

fn default_max_recents() -> usize {
    crate::recents::DEFAULT_MAX_RECENTS
}

fn default_gains() -> [f64; 10] {
    [0.0; 10]
}

fn default_model_storage() -> ModelStorageMode {
    ModelStorageMode::Disk
}

fn default_play_pause() -> String {
    "Space".to_string()
}

fn default_seek_forward() -> String {
    "Right".to_string()
}

fn default_seek_backward() -> String {
    "Left".to_string()
}

fn default_volume_up() -> String {
    "Up".to_string()
}

fn default_volume_down() -> String {
    "Down".to_string()
}

fn default_mute() -> String {
    "m".to_string()
}

fn default_open_file() -> String {
    "Ctrl+O".to_string()
}

fn default_open_url() -> String {
    "Ctrl+U".to_string()
}

fn default_fullscreen() -> String {
    "f".to_string()
}

/// Storage mode for Whisper models per PLANO_VAD.md §4.13.
/// - `Disk`: Persisted to `~/.local/share/vad/models/` and mmap'd (default).
/// - `RamOnly`: Kept strictly in-memory during current session, never written to disk.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ModelStorageMode {
    /// Save model to disk (`~/.local/share/vad/models/`) — default mode.
    Disk,
    /// Session-only in memory (RAM-only), zero disk footprint.
    RamOnly,
}

/// Configuration schema for Whisper model storage choices (§4.13, Sprint 05 schema / Sprint 06 feature).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct WhisperConfig {
    /// Default storage mode for new or unspecified models.
    #[serde(default = "default_model_storage")]
    pub default_storage_mode: ModelStorageMode,
    /// Explicit per-model storage choice (e.g. "base" -> Disk, "small" -> RamOnly).
    #[serde(default)]
    pub models: HashMap<String, ModelStorageMode>,
}

impl Default for WhisperConfig {
    fn default() -> Self {
        Self {
            default_storage_mode: ModelStorageMode::Disk,
            models: HashMap::new(),
        }
    }
}

/// General player settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct PlayerConfig {
    #[serde(default = "default_volume")]
    pub volume: f64,
    #[serde(default = "default_hwdec")]
    pub hwdec: String,
    #[serde(default)]
    pub audio_device: Option<String>,
    #[serde(default)]
    pub aspect_ratio: Option<String>,
}

impl Default for PlayerConfig {
    fn default() -> Self {
        Self {
            volume: 100.0,
            hwdec: "auto-safe".to_string(),
            audio_device: None,
            aspect_ratio: None,
        }
    }
}

/// Recent playback settings.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentsConfig {
    #[serde(default = "default_max_recents")]
    pub max_entries: usize,
    #[serde(default = "default_true")]
    pub resume_enabled: bool,
}

impl Default for RecentsConfig {
    fn default() -> Self {
        Self {
            max_entries: crate::recents::DEFAULT_MAX_RECENTS,
            resume_enabled: true,
        }
    }
}

/// Keyboard shortcuts schema.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ShortcutsConfig {
    #[serde(default = "default_play_pause")]
    pub play_pause: String,
    #[serde(default = "default_seek_forward")]
    pub seek_forward: String,
    #[serde(default = "default_seek_backward")]
    pub seek_backward: String,
    #[serde(default = "default_volume_up")]
    pub volume_up: String,
    #[serde(default = "default_volume_down")]
    pub volume_down: String,
    #[serde(default = "default_mute")]
    pub mute: String,
    #[serde(default = "default_open_file")]
    pub open_file: String,
    #[serde(default = "default_open_url")]
    pub open_url: String,
    #[serde(default = "default_fullscreen")]
    pub toggle_fullscreen: String,
}

impl Default for ShortcutsConfig {
    fn default() -> Self {
        Self {
            play_pause: default_play_pause(),
            seek_forward: default_seek_forward(),
            seek_backward: default_seek_backward(),
            volume_up: default_volume_up(),
            volume_down: default_volume_down(),
            mute: default_mute(),
            open_file: default_open_file(),
            open_url: default_open_url(),
            toggle_fullscreen: default_fullscreen(),
        }
    }
}

/// Audio equalizer persistent state.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct EqualizerConfig {
    #[serde(default)]
    pub preset: Option<String>,
    #[serde(default = "default_gains")]
    pub gains: [f64; 10],
    #[serde(default)]
    pub rnnoise: bool,
}

impl Default for EqualizerConfig {
    fn default() -> Self {
        Self {
            preset: None,
            gains: default_gains(),
            rnnoise: false,
        }
    }
}

/// Which LLM backend runs summaries and translations.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiProviderKind {
    /// Offline Qwen (default, nothing leaves the PC).
    #[default]
    Local,
    /// OpenAI or any OpenAI-compatible endpoint (Ollama, OpenRouter, self-hosted).
    OpenaiCompatible,
    /// Anthropic Messages API.
    Anthropic,
    /// Google Gemini API.
    Gemini,
}

fn default_openai_base_url() -> String {
    "https://api.openai.com/v1".to_string()
}

fn default_openai_model() -> String {
    "gpt-4o-mini".to_string()
}

fn default_anthropic_model() -> String {
    "claude-haiku-5-5".to_string()
}

fn default_gemini_model() -> String {
    "gemini-2.0-flash".to_string()
}

/// AI provider choice. **No secret ever lives here** (§4.2, §10.7): API keys go to the OS
/// keyring or env vars (`vad_ai::secrets`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct AiConfig {
    #[serde(default)]
    pub provider: AiProviderKind,
    #[serde(default = "default_openai_base_url")]
    pub openai_base_url: String,
    #[serde(default = "default_openai_model")]
    pub openai_model: String,
    #[serde(default = "default_anthropic_model")]
    pub anthropic_model: String,
    #[serde(default = "default_gemini_model")]
    pub gemini_model: String,
    /// Normalised custom `base_url` the user confirmed is not covered by the Sprint_10
    /// quality gate (§4.1). Changing the URL invalidates the confirmation.
    #[serde(default)]
    pub custom_endpoint_acknowledged: Option<String>,
}

impl Default for AiConfig {
    fn default() -> Self {
        Self {
            provider: AiProviderKind::Local,
            openai_base_url: default_openai_base_url(),
            openai_model: default_openai_model(),
            anthropic_model: default_anthropic_model(),
            gemini_model: default_gemini_model(),
            custom_endpoint_acknowledged: None,
        }
    }
}

/// Unified VAD configuration stored in `~/.config/vad/config.toml` per PLANO_VAD.md §5.
/// Note: API keys and cloud secrets are STRICTLY FORBIDDEN in this structure (§4.2, §10.7).
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq)]
pub struct VadConfig {
    #[serde(default)]
    pub player: PlayerConfig,
    #[serde(default)]
    pub recents: RecentsConfig,
    #[serde(default)]
    pub shortcuts: ShortcutsConfig,
    #[serde(default)]
    pub whisper: WhisperConfig,
    #[serde(default)]
    pub equalizer: EqualizerConfig,
    #[serde(default)]
    pub ai: AiConfig,
}

impl VadConfig {
    /// Saves the unified configuration to a TOML file via atomic write (.tmp + rename, §4.30).
    pub fn save_to_path(&self, path: &Path) -> Result<(), VadError> {
        let toml_str = toml::to_string_pretty(self)
            .map_err(|e| VadError::Config(format!("Failed to serialize config.toml: {e}")))?;
        write_atomic(path, toml_str.as_bytes())?;
        debug!("Saved unified config to {:?}", path);
        Ok(())
    }

    /// Loads the configuration from a TOML file.
    /// Returns default config if file does not exist.
    /// If parse fails, logs warning and returns default config without crashing the application (§5).
    pub fn load_from_path(path: &Path) -> Result<Self, VadError> {
        if !path.exists() {
            debug!("Config file {:?} does not exist, using defaults", path);
            return Ok(Self::default());
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read {:?}: {:?}, falling back to defaults", path, e);
                return Ok(Self::default());
            }
        };

        match toml::from_str::<Self>(&content) {
            Ok(cfg) => {
                debug!("Loaded unified configuration from {:?}", path);
                Ok(cfg)
            }
            Err(e) => {
                warn!(
                    "Corrupted config.toml at {:?}: {:?}, falling back to defaults",
                    path, e
                );
                Ok(Self::default())
            }
        }
    }

    /// Saves configuration to the default path `~/.config/vad/config.toml` (§5).
    pub fn save_default(&self) -> Result<(), VadError> {
        self.save_to_path(&vad_config_path())
    }

    /// Loads configuration from the default path `~/.config/vad/config.toml`.
    pub fn load_default() -> Self {
        Self::load_from_path(&vad_config_path()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static CONFIG_TEST_COUNTER: AtomicU64 = AtomicU64::new(200);

    fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
        let pid = std::process::id();
        let cnt = CONFIG_TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vad_config_test_{prefix}_{pid}_{cnt}"));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_config_defaults_and_roundtrip() {
        let dir = temp_test_dir("defaults");
        let path = dir.join("config.toml");

        let mut config = VadConfig::default();
        assert_eq!(config.player.volume, 100.0);
        assert_eq!(config.player.hwdec, "auto-safe");
        assert_eq!(config.recents.max_entries, 20);
        assert!(config.recents.resume_enabled);
        assert_eq!(config.whisper.default_storage_mode, ModelStorageMode::Disk);

        // Customize some values
        config.player.volume = 120.0;
        config.equalizer.preset = Some("Voz clara".to_string());
        config.equalizer.gains[0] = 3.5;
        config.whisper.models.insert("base".to_string(), ModelStorageMode::Disk);
        config.whisper.models.insert("tiny".to_string(), ModelStorageMode::RamOnly);

        config.save_to_path(&path).expect("Failed to save config.toml");

        let loaded = VadConfig::load_from_path(&path).expect("Failed to load config.toml");
        assert_eq!(loaded.player.volume, 120.0);
        assert_eq!(loaded.equalizer.preset, Some("Voz clara".to_string()));
        assert_eq!(loaded.equalizer.gains[0], 3.5);
        assert_eq!(
            loaded.whisper.models.get("base"),
            Some(&ModelStorageMode::Disk)
        );
        assert_eq!(
            loaded.whisper.models.get("tiny"),
            Some(&ModelStorageMode::RamOnly)
        );

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_corrupted_toml_fallback() {
        let dir = temp_test_dir("corrupt");
        let path = dir.join("config.toml");

        fs::write(&path, b"INVALID_NON_TOML_SYNTAX [[[[").unwrap();
        let loaded = VadConfig::load_from_path(&path).expect("Should degrade gracefully");
        assert_eq!(loaded, VadConfig::default());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_config_anti_leak_rule() {
        // §4.2, §10.7: No API keys or secrets should exist in config.toml
        let config = VadConfig::default();
        let toml_str = toml::to_string_pretty(&config).unwrap();
        assert!(!toml_str.contains("api_key"));
        assert!(!toml_str.contains("secret"));
        assert!(!toml_str.contains("token"));
        // Sprint_12: provider *model names* legitimately live in config; anything else that
        // mentions a provider (a stray key or URL carrying one) is still a failure.
        let stripped: String = toml_str
            .lines()
            .filter(|l| !l.starts_with("anthropic_model") && !l.starts_with("gemini_model"))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(!stripped.contains("anthropic"));
        assert!(!stripped.contains("gemini"));
        // Even with every field set, the serialised config holds no credential-shaped text.
        let cfg = VadConfig {
            ai: AiConfig { provider: AiProviderKind::Anthropic, ..AiConfig::default() },
            ..VadConfig::default()
        };
        let all = toml::to_string_pretty(&cfg).unwrap();
        assert!(!all.contains("sk-") && !all.contains("AIza") && !all.to_lowercase().contains("bearer"));
    }
}
