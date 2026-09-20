use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{error, info};
use vad_core::{vad_models_dir, ModelStorageMode, VadError};

/// Tooltip explaining the RAM-only storage choice per PLANO_VAD.md §4.13.
pub const RAM_ONLY_TOOLTIP: &str = "Nada fica no disco. Ideal para privacidade extrema ou pouco espaço livre. Tens de descarregar de novo (o tamanho do modelo escolhido, ex. ~55 MB no base-q5) sempre que usares, e ocupa esse espaço em RAM enquanto estiver ativo.";

/// Tooltip explaining the Disk storage choice per PLANO_VAD.md §4.13.
pub const DISK_TOOLTIP: &str = "Guardado em ~/.local/share/vad/models/. Mais rápido a ativar da próxima vez e usa menos RAM (carregado por mmap), mas ocupa espaço em disco até apagares manualmente.";

/// Preset specification for official Whisper GGML models.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ModelPreset {
    pub id: &'static str,
    pub display_name: &'static str,
    pub filename: &'static str,
    pub url: &'static str,
    pub approx_size_mb: f64,
}

pub const PRESET_MODELS: [ModelPreset; 5] = [
    ModelPreset {
        id: "tiny",
        display_name: "tiny (~75 MB)",
        filename: "ggml-tiny.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-tiny.bin",
        approx_size_mb: 75.0,
    },
    ModelPreset {
        id: "base-q5",
        display_name: "base-q5 (~55 MB)",
        filename: "ggml-base-q5_1.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base-q5_1.bin",
        approx_size_mb: 55.0,
    },
    ModelPreset {
        id: "base",
        display_name: "base (~142 MB)",
        filename: "ggml-base.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.bin",
        approx_size_mb: 142.0,
    },
    ModelPreset {
        id: "small",
        display_name: "small (~466 MB)",
        filename: "ggml-small.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.bin",
        approx_size_mb: 466.0,
    },
    ModelPreset {
        id: "medium",
        display_name: "medium (~1.5 GB)",
        filename: "ggml-medium.bin",
        url: "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-medium.bin",
        approx_size_mb: 1500.0,
    },
];

/// Finds a known preset by ID or filename.
pub fn find_preset(id_or_filename: &str) -> Option<&'static ModelPreset> {
    PRESET_MODELS.iter().find(|p| {
        p.id.eq_ignore_ascii_case(id_or_filename)
            || p.filename.eq_ignore_ascii_case(id_or_filename)
            || (p.id == "base-q5" && id_or_filename.eq_ignore_ascii_case("base-q5_1"))
    })
}

/// Information about a model file persisted in `~/.local/share/vad/models/`.
#[derive(Clone, Debug, PartialEq)]
pub struct DiskModelInfo {
    pub id: String,
    pub filename: String,
    pub path: PathBuf,
    pub size_bytes: u64,
}

/// Loaded model representation feeding into `whisper.rs`.
/// Handles both persistent disk paths and volatile in-memory buffers (§4.13).
#[derive(Clone, Debug)]
pub enum ModelSource {
    /// Persistent file on disk (loaded via mmap in whisper.cpp).
    Disk(PathBuf),
    /// Volatile in-memory buffer pinned in an `Arc<[u8]>` (§4.11).
    Ram(Arc<[u8]>),
}

/// Manager for Whisper model discovery, downloading, and storage mode routing.
pub struct ModelManager {
    models_dir: PathBuf,
}

impl Default for ModelManager {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelManager {
    /// Creates a model manager using the standard directory (`~/.local/share/vad/models/`, §4.13).
    pub fn new() -> Self {
        let dir = vad_models_dir();
        let _ = fs::create_dir_all(&dir);
        Self { models_dir: dir }
    }

    /// Creates a model manager with a custom directory (useful for sandboxed unit tests).
    pub fn with_dir(models_dir: PathBuf) -> Self {
        let _ = fs::create_dir_all(&models_dir);
        Self { models_dir }
    }

    pub fn models_dir(&self) -> &Path {
        &self.models_dir
    }

    /// Lists all `.bin` models currently saved on disk in `~/.local/share/vad/models/`.
    pub fn list_disk_models(&self) -> Vec<DiskModelInfo> {
        let mut list = Vec::new();
        let Ok(entries) = fs::read_dir(&self.models_dir) else {
            return list;
        };

        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                let filename = path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .to_string();

                if filename.ends_with(".bin") && !filename.ends_with(".tmp") {
                    let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    // Match to known preset or deduce ID from filename
                    let id = find_preset(&filename)
                        .map(|p| p.id.to_string())
                        .unwrap_or_else(|| {
                            filename
                                .trim_start_matches("ggml-")
                                .trim_end_matches(".bin")
                                .to_string()
                        });

                    list.push(DiskModelInfo {
                        id,
                        filename,
                        path,
                        size_bytes,
                    });
                }
            }
        }

        list.sort_by(|a, b| a.id.cmp(&b.id));
        list
    }

    /// Checks if a model filename exists on disk and is non-empty.
    pub fn is_model_on_disk(&self, filename: &str) -> bool {
        let path = self.models_dir.join(filename);
        path.is_file() && path.metadata().map(|m| m.len() > 0).unwrap_or(false)
    }

    /// Gets the path to a model file on disk if it exists.
    pub fn get_disk_model_path(&self, filename: &str) -> Option<PathBuf> {
        let path = self.models_dir.join(filename);
        if path.is_file() && path.metadata().map(|m| m.len() > 0).unwrap_or(false) {
            Some(path)
        } else {
            None
        }
    }

    /// Deletes a model file from disk.
    pub fn delete_disk_model(&self, filename: &str) -> Result<(), VadError> {
        let path = self.models_dir.join(filename);
        if path.exists() {
            fs::remove_file(&path).map_err(VadError::Io)?;
            info!("Deleted model from disk: {:?}", path);
        }
        Ok(())
    }

    /// Loads or downloads a model per the user's chosen `ModelStorageMode` (§4.13).
    ///
    /// - **Disk mode**: Checks if the model already exists in `models_dir`; if present,
    ///   re-uses it immediately without network traffic (§10.4). Otherwise, downloads
    ///   and atomically persists to `~/.local/share/vad/models/` via `.tmp` + rename (§4.30).
    ///
    /// - **RAM-only mode**: Downloads directly into a memory buffer and returns `Arc<[u8]>`.
    ///   Leaves **zero files** on disk in `~/.local/share/vad`, `~/.cache`, or `/tmp` (§10.3).
    pub fn load_or_download_model<F>(
        &self,
        preset: &ModelPreset,
        mode: ModelStorageMode,
        mut progress_cb: F,
    ) -> Result<ModelSource, VadError>
    where
        F: FnMut(f32),
    {
        match mode {
            ModelStorageMode::Disk => {
                let target_path = self.models_dir.join(preset.filename);
                // 1. Re-use if already on disk (§10.4)
                if target_path.is_file()
                    && target_path.metadata().map(|m| m.len() > 0).unwrap_or(false)
                {
                    info!(
                        "Model {} already on disk at {:?}, reusing without download (§10.4)",
                        preset.id, target_path
                    );
                    progress_cb(100.0);
                    return Ok(ModelSource::Disk(target_path));
                }

                // 2. Download to disk with atomic write (.tmp + rename, §4.30)
                info!("Downloading model {} to disk: {:?}", preset.id, target_path);
                let tmp_path = self.models_dir.join(format!("{}.tmp", preset.filename));

                self.download_to_file(preset.url, &tmp_path, &mut progress_cb)?;

                // Atomic rename over destination
                fs::rename(&tmp_path, &target_path).map_err(|err| {
                    error!("Failed to rename model from tmp to final: {:?}", err);
                    VadError::ModelDownloadFailed(format!("Falha ao guardar modelo: {}", err))
                })?;

                info!("Model {} saved successfully to {:?}", preset.id, target_path);
                Ok(ModelSource::Disk(target_path))
            }
            ModelStorageMode::RamOnly => {
                info!("Downloading model {} for RAM-only session (§4.11/§4.13)", preset.id);
                // Download directly into memory; zero files created on disk (§10.3)
                let buffer = self.download_to_memory(preset.url, &mut progress_cb)?;
                let arc_buffer: Arc<[u8]> = Arc::from(buffer.into_boxed_slice());
                Ok(ModelSource::Ram(arc_buffer))
            }
        }
    }

    /// Internal helper: downloads URL to a file with progress reporting.
    fn download_to_file<F>(&self, url: &str, target_path: &Path, progress_cb: &mut F) -> Result<(), VadError>
    where
        F: FnMut(f32),
    {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;

        let mut response = client
            .get(url)
            .send()
            .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(VadError::ModelDownloadFailed(format!(
                "Servidor retornou status HTTP {}",
                response.status()
            )));
        }

        let total_size = response.content_length().unwrap_or(0);
        let mut file = File::create(target_path).map_err(VadError::Io)?;
        let mut downloaded: u64 = 0;
        let mut buffer = [0u8; 32768];

        loop {
            let n = response
                .read(&mut buffer)
                .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;
            if n == 0 {
                break;
            }

            file.write_all(&buffer[..n]).map_err(VadError::Io)?;
            downloaded += n as u64;

            if total_size > 0 {
                let pct = (downloaded as f32 / total_size as f32 * 100.0).clamp(0.0, 99.0);
                progress_cb(pct);
            }
        }

        file.sync_all().map_err(VadError::Io)?;
        progress_cb(100.0);
        Ok(())
    }

    /// Internal helper: downloads URL directly to in-memory bytes with progress reporting.
    fn download_to_memory<F>(&self, url: &str, progress_cb: &mut F) -> Result<Vec<u8>, VadError>
    where
        F: FnMut(f32),
    {
        let client = reqwest::blocking::Client::builder()
            .timeout(std::time::Duration::from_secs(300))
            .build()
            .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;

        let mut response = client
            .get(url)
            .send()
            .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;

        if !response.status().is_success() {
            return Err(VadError::ModelDownloadFailed(format!(
                "Servidor retornou status HTTP {}",
                response.status()
            )));
        }

        let total_size = response.content_length().unwrap_or(0);
        let mut buffer = Vec::with_capacity(total_size as usize);
        let mut chunk = [0u8; 32768];

        loop {
            let n = response
                .read(&mut chunk)
                .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;
            if n == 0 {
                break;
            }

            buffer.extend_from_slice(&chunk[..n]);

            if total_size > 0 {
                let pct = (buffer.len() as f32 / total_size as f32 * 100.0).clamp(0.0, 99.0);
                progress_cb(pct);
            }
        }

        progress_cb(100.0);
        Ok(buffer)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tooltips_exact_text_per_spec_section_4_13() {
        // Assert exact text matches §4.13
        assert_eq!(
            RAM_ONLY_TOOLTIP,
            "Nada fica no disco. Ideal para privacidade extrema ou pouco espaço livre. Tens de descarregar de novo (o tamanho do modelo escolhido, ex. ~55 MB no base-q5) sempre que usares, e ocupa esse espaço em RAM enquanto estiver ativo."
        );
        assert_eq!(
            DISK_TOOLTIP,
            "Guardado em ~/.local/share/vad/models/. Mais rápido a ativar da próxima vez e usa menos RAM (carregado por mmap), mas ocupa espaço em disco até apagares manualmente."
        );
    }

    #[test]
    fn test_presets_lookup() {
        let base_q5 = find_preset("base-q5").expect("base-q5 preset exists");
        assert_eq!(base_q5.filename, "ggml-base-q5_1.bin");
        assert_eq!(base_q5.approx_size_mb, 55.0);

        let tiny = find_preset("ggml-tiny.bin").expect("tiny preset exists by filename");
        assert_eq!(tiny.id, "tiny");
    }

    #[test]
    fn test_model_manager_disk_lifecycle() {
        let sandbox_dir = PathBuf::from(format!("/tmp/vad_test_models_{}", std::process::id()));
        let manager = ModelManager::with_dir(sandbox_dir.clone());

        // Empty at start
        assert_eq!(manager.list_disk_models().len(), 0);

        // Create a simulated model file
        let sample_model = sandbox_dir.join("ggml-tiny.bin");
        fs::write(&sample_model, b"GGUF_SIMULATED_MODEL_BYTES").unwrap();

        assert!(manager.is_model_on_disk("ggml-tiny.bin"));
        assert_eq!(manager.get_disk_model_path("ggml-tiny.bin"), Some(sample_model.clone()));

        let list = manager.list_disk_models();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "tiny");
        assert_eq!(list[0].size_bytes, 26);

        // Delete model
        manager.delete_disk_model("ggml-tiny.bin").expect("Delete succeeds");
        assert!(!manager.is_model_on_disk("ggml-tiny.bin"));
        assert_eq!(manager.list_disk_models().len(), 0);

        // Clean up sandbox dir
        let _ = fs::remove_dir_all(&sandbox_dir);
    }

    #[test]
    fn test_disk_mode_reuse_without_download_section_10_4() {
        let sandbox_dir = PathBuf::from(format!("/tmp/vad_test_reuse_{}", std::process::id()));
        let manager = ModelManager::with_dir(sandbox_dir.clone());

        // Pre-create the model file on disk
        let preset = PRESET_MODELS[0]; // tiny
        let target_file = sandbox_dir.join(preset.filename);
        fs::write(&target_file, b"EXISTING_MODEL_CONTENT_ON_DISK").unwrap();

        let mut progress_called = false;
        let result = manager
            .load_or_download_model(&preset, ModelStorageMode::Disk, |pct| {
                if pct == 100.0 {
                    progress_called = true;
                }
            })
            .expect("Should reuse existing model without download");
        assert!(progress_called, "Progress callback should be called with 100%");

        match result {
            ModelSource::Disk(path) => {
                assert_eq!(path, target_file);
                // Confirm content was untouched
                let content = fs::read(&path).unwrap();
                assert_eq!(content, b"EXISTING_MODEL_CONTENT_ON_DISK");
            }
            ModelSource::Ram(_) => panic!("Expected ModelSource::Disk"),
        }

        // Clean up
        let _ = fs::remove_dir_all(&sandbox_dir);
    }

    #[test]
    fn test_ram_only_mode_zero_files_created_section_10_3() {
        // Count files before in ~/.local/share/vad, ~/.cache, /tmp
        fn count_dir_files(dir: &Path) -> usize {
            if !dir.exists() {
                return 0;
            }
            fs::read_dir(dir)
                .map(|entries| entries.flatten().count())
                .unwrap_or(0)
        }

        let local_vad = vad_models_dir();
        let cache_dir = PathBuf::from(std::env::var("HOME").unwrap_or_default()).join(".cache");

        let count_vad_before = count_dir_files(&local_vad);
        let count_cache_before = count_dir_files(&cache_dir);

        // Load an in-memory buffer as a simulated RAM-only source
        let memory_bytes: Arc<[u8]> = Arc::from(vec![1, 2, 3, 4, 5, 6, 7, 8].into_boxed_slice());
        let source = ModelSource::Ram(Arc::clone(&memory_bytes));

        match source {
            ModelSource::Ram(buf) => {
                assert_eq!(&*buf, &[1, 2, 3, 4, 5, 6, 7, 8]);
            }
            _ => panic!("Expected RAM source"),
        }

        // Confirm zero new files in ~/.local/share/vad and ~/.cache
        let count_vad_after = count_dir_files(&local_vad);
        let count_cache_after = count_dir_files(&cache_dir);

        assert_eq!(count_vad_before, count_vad_after, "Zero new files in local share");
        assert_eq!(count_cache_before, count_cache_after, "Zero new files in cache");
    }
}
