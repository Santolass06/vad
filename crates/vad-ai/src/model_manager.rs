use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use tracing::{error, info};
use vad_core::{vad_models_dir, ModelStorageMode, VadError};

/// Connection timeout for model downloads (§4.22 spirit: never leave network waits implicit).
const CONNECT_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(30);
/// Upper bound for a whole model download; large enough for the ~1.5 GB `medium` model.
const DOWNLOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(2 * 3600);

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

/// Preset specifications for official local LLM models (Qwen 2.5 0.5B Instruct GGUF).
pub const PRESET_LLM_MODELS: [ModelPreset; 2] = [
    ModelPreset {
        id: "qwen2.5-0.5b",
        display_name: "Qwen2.5 0.5B Instruct Q4_K_M (~398 MB)",
        filename: "qwen2.5-0.5b-instruct-q4_k_m.gguf",
        url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct-GGUF/resolve/main/qwen2.5-0.5b-instruct-q4_k_m.gguf",
        approx_size_mb: 398.0,
    },
    ModelPreset {
        id: "qwen2.5-tokenizer",
        display_name: "Qwen2.5 Tokenizer (~7 MB)",
        filename: "qwen2.5-tokenizer.json",
        url: "https://huggingface.co/Qwen/Qwen2.5-0.5B-Instruct/resolve/main/tokenizer.json",
        approx_size_mb: 7.0,
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

/// Finds a known LLM preset by ID or filename.
pub fn find_llm_preset(id_or_filename: &str) -> Option<&'static ModelPreset> {
    PRESET_LLM_MODELS.iter().find(|p| {
        p.id.eq_ignore_ascii_case(id_or_filename)
            || p.filename.eq_ignore_ascii_case(id_or_filename)
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

    /// Lists all `.gguf` local LLM models currently saved on disk in `~/.local/share/vad/models/`.
    pub fn list_disk_llm_models(&self) -> Vec<DiskModelInfo> {
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

                if filename.ends_with(".gguf") && !filename.ends_with(".tmp") {
                    let size_bytes = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let id = find_llm_preset(&filename)
                        .map(|p| p.id.to_string())
                        .unwrap_or_else(|| {
                            filename
                                .trim_end_matches(".gguf")
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

    /// Checks if both the GGUF model and its tokenizer exist on disk.
    pub fn is_llm_model_ready(&self, model_filename: &str, tokenizer_filename: &str) -> bool {
        self.is_model_on_disk(model_filename) && self.is_model_on_disk(tokenizer_filename)
    }

    /// Returns the paths to both the GGUF model and tokenizer if present.
    pub fn get_llm_model_paths(
        &self,
        model_filename: &str,
        tokenizer_filename: &str,
    ) -> Option<(PathBuf, PathBuf)> {
        let model_path = self.get_disk_model_path(model_filename)?;
        let tokenizer_path = self.get_disk_model_path(tokenizer_filename)?;
        Some((model_path, tokenizer_path))
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

                if let Err(err) = self.download_to_file(preset.url, &tmp_path, &mut progress_cb) {
                    // Never leave a partial download behind (it would look like a model on disk)
                    let _ = fs::remove_file(&tmp_path);
                    return Err(err);
                }

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

    /// Sends the GET request for a model download and returns the response with its size.
    ///
    /// Only the connection has a short timeout: a total timeout would abort the larger models
    /// (`medium` is ~1.5 GB) on ordinary connections. `DOWNLOAD_TIMEOUT` is just an upper bound
    /// so a stalled transfer cannot hang the download thread forever.
    fn open_download(url: &str) -> Result<(reqwest::blocking::Response, u64), VadError> {
        let client = reqwest::blocking::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(DOWNLOAD_TIMEOUT)
            .build()
            .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;

        let response = client
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
        Ok((response, total_size))
    }

    /// Streams `response` into `sink` in chunks, reporting progress; returns the bytes copied.
    fn copy_with_progress<F>(
        mut response: reqwest::blocking::Response,
        total_size: u64,
        progress_cb: &mut F,
        mut sink: impl FnMut(&[u8]) -> Result<(), VadError>,
    ) -> Result<u64, VadError>
    where
        F: FnMut(f32),
    {
        let mut chunk = [0u8; 32768];
        let mut downloaded: u64 = 0;

        loop {
            let n = response
                .read(&mut chunk)
                .map_err(|e| VadError::ModelDownloadFailed(e.to_string()))?;
            if n == 0 {
                break;
            }

            sink(&chunk[..n])?;
            downloaded += n as u64;

            if total_size > 0 {
                let pct = (downloaded as f32 / total_size as f32 * 100.0).clamp(0.0, 99.0);
                progress_cb(pct);
            }
        }

        Ok(downloaded)
    }

    /// Internal helper: downloads URL to a file with progress reporting.
    fn download_to_file<F>(&self, url: &str, target_path: &Path, progress_cb: &mut F) -> Result<(), VadError>
    where
        F: FnMut(f32),
    {
        let (response, total_size) = Self::open_download(url)?;
        let mut file = File::create(target_path).map_err(VadError::Io)?;

        Self::copy_with_progress(response, total_size, progress_cb, |bytes| {
            file.write_all(bytes).map_err(VadError::Io)
        })?;

        file.sync_all().map_err(VadError::Io)?;
        progress_cb(100.0);
        Ok(())
    }

    /// Internal helper: downloads URL directly to in-memory bytes with progress reporting.
    fn download_to_memory<F>(&self, url: &str, progress_cb: &mut F) -> Result<Vec<u8>, VadError>
    where
        F: FnMut(f32),
    {
        let (response, total_size) = Self::open_download(url)?;
        let mut buffer = Vec::with_capacity(total_size as usize);

        Self::copy_with_progress(response, total_size, progress_cb, |bytes| {
            buffer.extend_from_slice(bytes);
            Ok(())
        })?;

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

    /// Minimal HTTP server on localhost serving `body` for every request; counts the requests.
    fn serve_model(body: &'static [u8]) -> (String, Arc<std::sync::atomic::AtomicUsize>) {
        use std::io::{BufRead, BufReader};
        use std::sync::atomic::{AtomicUsize, Ordering};

        let listener = std::net::TcpListener::bind("127.0.0.1:0").expect("bind");
        let url = format!("http://{}/ggml-test.bin", listener.local_addr().unwrap());
        let hits = Arc::new(AtomicUsize::new(0));
        let hits_srv = Arc::clone(&hits);

        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                // Consume the request head
                while reader.read_line(&mut line).map(|n| n > 2).unwrap_or(false) {
                    line.clear();
                }
                hits_srv.fetch_add(1, Ordering::SeqCst);
                let mut stream = reader.into_inner();
                let head = format!(
                    "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                    body.len()
                );
                let _ = stream.write_all(head.as_bytes());
                let _ = stream.write_all(body);
            }
        });

        (url, hits)
    }

    fn test_preset(url: String) -> ModelPreset {
        ModelPreset {
            id: "test",
            display_name: "test",
            filename: "ggml-test.bin",
            url: Box::leak(url.into_boxed_str()),
            approx_size_mb: 0.0,
        }
    }

    fn files_in(dir: &Path) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(dir)
            .map(|rd| rd.flatten().map(|e| e.file_name().to_string_lossy().to_string()).collect())
            .unwrap_or_default();
        names.sort();
        names
    }

    #[test]
    fn test_ram_only_mode_zero_files_created_section_10_3() {
        const BODY: &[u8] = b"RAM_ONLY_MODEL_BYTES";
        let (url, hits) = serve_model(BODY);
        let sandbox_dir = PathBuf::from(format!("/tmp/vad_test_ramonly_{}", std::process::id()));
        let manager = ModelManager::with_dir(sandbox_dir.clone());
        let tmp_before = files_in(Path::new("/tmp"));

        let source = manager
            .load_or_download_model(&test_preset(url), ModelStorageMode::RamOnly, |_| {})
            .expect("RAM-only download from local server");

        match source {
            ModelSource::Ram(buf) => assert_eq!(&*buf, BODY),
            ModelSource::Disk(_) => panic!("Expected ModelSource::Ram"),
        }
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Zero files: nothing in the models dir (not even a .tmp) and no model-looking file in /tmp
        assert!(files_in(&sandbox_dir).is_empty(), "RAM-only left files in models dir");
        let new_in_tmp: Vec<String> = files_in(Path::new("/tmp"))
            .into_iter()
            .filter(|n| !tmp_before.contains(n) && (n.contains("ggml") || n.ends_with(".tmp")))
            .collect();
        assert!(new_in_tmp.is_empty(), "RAM-only left files in /tmp: {new_in_tmp:?}");

        let _ = fs::remove_dir_all(&sandbox_dir);
    }

    #[test]
    fn test_disk_mode_downloads_once_then_reuses_section_10_4() {
        const BODY: &[u8] = b"DISK_MODEL_BYTES";
        let (url, hits) = serve_model(BODY);
        let sandbox_dir = PathBuf::from(format!("/tmp/vad_test_diskdl_{}", std::process::id()));
        let manager = ModelManager::with_dir(sandbox_dir.clone());
        let preset = test_preset(url);

        // First activation downloads and persists atomically (no .tmp left behind)
        let first = manager
            .load_or_download_model(&preset, ModelStorageMode::Disk, |_| {})
            .expect("first download");
        let ModelSource::Disk(path) = first else { panic!("Expected ModelSource::Disk") };
        assert_eq!(path, sandbox_dir.join("ggml-test.bin"));
        assert_eq!(fs::read(&path).unwrap(), BODY);
        assert_eq!(files_in(&sandbox_dir), vec!["ggml-test.bin".to_string()]);
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1);

        // Next activation reuses the file: no new request
        manager
            .load_or_download_model(&preset, ModelStorageMode::Disk, |_| {})
            .expect("reuse");
        assert_eq!(hits.load(std::sync::atomic::Ordering::SeqCst), 1, "model was downloaded again");

        let _ = fs::remove_dir_all(&sandbox_dir);
    }

    #[test]
    fn test_failed_download_leaves_no_partial_file() {
        // Port 1 refuses connections: the request fails after the .tmp path was chosen
        let sandbox_dir = PathBuf::from(format!("/tmp/vad_test_faildl_{}", std::process::id()));
        let manager = ModelManager::with_dir(sandbox_dir.clone());
        let preset = test_preset("http://127.0.0.1:1/ggml-test.bin".to_string());

        let res = manager.load_or_download_model(&preset, ModelStorageMode::Disk, |_| {});
        assert!(matches!(res, Err(VadError::ModelDownloadFailed(_))));
        assert!(files_in(&sandbox_dir).is_empty());

        let _ = fs::remove_dir_all(&sandbox_dir);
    }
}
