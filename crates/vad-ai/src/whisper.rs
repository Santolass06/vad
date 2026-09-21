use std::ffi::{c_int, c_void};
use std::panic::{catch_unwind, AssertUnwindSafe};
use std::sync::Arc;

use tracing::{debug, error, info};
use vad_core::VadError;
use whisper_rs::{FullParams, SamplingStrategy, WhisperContext, WhisperContextParameters};

use crate::model_manager::ModelSource;
use crate::PcmAudio;

/// Single transcribed subtitle/text segment with timing metadata.
#[derive(Clone, Debug, PartialEq)]
pub struct TranscriptionSegment {
    /// Start timestamp in milliseconds.
    pub start_ms: i64,
    /// End timestamp in milliseconds.
    pub end_ms: i64,
    /// Transcribed text content.
    pub text: String,
}

impl TranscriptionSegment {
    /// Formats timestamps into HH:MM:SS or MM:SS.
    pub fn format_timestamp(ms: i64) -> String {
        let total_seconds = (ms.max(0) / 1000) as u64;
        let s = total_seconds % 60;
        let m = (total_seconds / 60) % 60;
        let h = total_seconds / 3600;
        if h > 0 {
            format!("{h:02}:{m:02}:{s:02}")
        } else {
            format!("{m:02}:{s:02}")
        }
    }
}

/// User callbacks handed to whisper.cpp, plus whether one of them panicked (§4.24).
/// Both trampolines receive a pointer to this struct as `user_data`.
struct CallbackState<P, A> {
    progress: Option<P>,
    abort: Option<A>,
    panicked: bool,
}

/// Trampoline for progress callback wrapped in `std::panic::catch_unwind` (§4.24).
/// Ensures that Rust panics never cross the FFI boundary, preventing process abort.
/// A panic is recorded in the state so `transcribe` can turn it into a `VadError`.
unsafe extern "C" fn safe_progress_trampoline<P, A>(
    _ctx: *mut whisper_rs_sys::whisper_context,
    _state: *mut whisper_rs_sys::whisper_state,
    progress: c_int,
    user_data: *mut c_void,
) where
    P: FnMut(i32),
{
    if user_data.is_null() {
        return;
    }
    let state = &mut *(user_data as *mut CallbackState<P, A>);
    if state.panicked {
        return;
    }
    if let Some(cb) = state.progress.as_mut() {
        if catch_unwind(AssertUnwindSafe(|| cb(progress))).is_err() {
            state.panicked = true;
        }
    }
}

/// Trampoline for abort callback wrapped in `std::panic::catch_unwind` (§4.24).
/// Also installed when the caller has no abort callback, so a panic in the progress callback
/// stops the inference instead of letting it run to completion for nothing.
unsafe extern "C" fn safe_abort_trampoline<P, A>(user_data: *mut c_void) -> bool
where
    A: FnMut() -> bool,
{
    if user_data.is_null() {
        return false;
    }
    let state = &mut *(user_data as *mut CallbackState<P, A>);
    if state.panicked {
        return true;
    }
    let Some(cb) = state.abort.as_mut() else {
        return false;
    };
    match catch_unwind(AssertUnwindSafe(cb)) {
        Ok(abort) => abort,
        Err(_) => {
            // Signal abort to whisper.cpp safely
            state.panicked = true;
            true
        }
    }
}

/// High-level Whisper transcription engine wrapping `whisper.cpp` via `whisper-rs`.
pub struct WhisperEngine {
    ctx: WhisperContext,
    /// Retained in-memory model buffer for RAM-only mode (§4.11).
    /// Kept alive for the entire lifetime of the engine to guarantee no use-after-free in C.
    _ram_buffer: Option<Arc<[u8]>>,
    n_threads: i32,
}

impl WhisperEngine {
    /// Loads the engine from a `ModelSource` (persistent disk or volatile RAM-only).
    ///
    /// By default, sets the number of inference threads to `num_cpus::get_physical()` (§5)
    /// to avoid CPU cache thrashing on Hyper-Threading / SMT sibling cores.
    pub fn load(source: &ModelSource) -> Result<Self, VadError> {
        let physical_cpus = num_cpus::get_physical() as i32;
        let default_threads = physical_cpus.max(1);

        info!(
            "Initializing WhisperEngine with {} physical threads (logical threads: {}) (§5)",
            default_threads,
            num_cpus::get()
        );

        let params = WhisperContextParameters::default();

        match source {
            ModelSource::Disk(ref path) => {
                info!("Loading Whisper model from disk via mmap: {:?}", path);
                let ctx = WhisperContext::new_with_params(path, params).map_err(|err| {
                    error!("Failed to initialize Whisper context from disk: {:?}", err);
                    VadError::Whisper(format!("Falha ao carregar modelo do disco: {:?}", err))
                })?;

                Ok(Self {
                    ctx,
                    _ram_buffer: None,
                    n_threads: default_threads,
                })
            }
            ModelSource::Ram(ref arc_buf) => {
                info!(
                    "Loading Whisper model from RAM-only pinned buffer ({} bytes) (§4.11)",
                    arc_buf.len()
                );
                let ctx = WhisperContext::new_from_buffer_with_params(arc_buf, params).map_err(|err| {
                    error!("Failed to initialize Whisper context from buffer: {:?}", err);
                    VadError::Whisper(format!("Falha ao carregar modelo da memória: {:?}", err))
                })?;

                Ok(Self {
                    ctx,
                    // Keep Arc<[u8]> alive for the life of WhisperEngine (§4.11)
                    _ram_buffer: Some(Arc::clone(arc_buf)),
                    n_threads: default_threads,
                })
            }
        }
    }

    /// Sets the number of decoding threads.
    pub fn set_threads(&mut self, threads: i32) {
        self.n_threads = threads.max(1);
    }

    /// Current number of decoding threads.
    pub fn threads(&self) -> i32 {
        self.n_threads
    }

    /// Transcribes the provided `PcmAudio` (16kHz mono i16) into timestamped segments.
    /// Supports safe progress updates and abort callbacks protected against panics (§4.24).
    pub fn transcribe<P, A>(
        &self,
        audio: &PcmAudio,
        language: Option<&str>,
        progress_cb: Option<P>,
        abort_cb: Option<A>,
    ) -> Result<Vec<TranscriptionSegment>, VadError>
    where
        P: FnMut(i32),
        A: FnMut() -> bool,
    {
        self.transcribe_with_options(audio, language, false, progress_cb, abort_cb)
    }

    /// Transcribes and optionally translates into English using Whisper's native translation mode (§4.1).
    pub fn transcribe_with_options<P, A>(
        &self,
        audio: &PcmAudio,
        language: Option<&str>,
        translate_to_en: bool,
        progress_cb: Option<P>,
        abort_cb: Option<A>,
    ) -> Result<Vec<TranscriptionSegment>, VadError>
    where
        P: FnMut(i32),
        A: FnMut() -> bool,
    {
        let mut state = self.ctx.create_state().map_err(|err| {
            VadError::Whisper(format!("Falha ao criar estado do Whisper: {:?}", err))
        })?;

        let mut params = FullParams::new(SamplingStrategy::Greedy { best_of: 1 });
        params.set_n_threads(self.n_threads);
        params.set_print_progress(false);
        params.set_print_special(false);
        params.set_print_realtime(false);
        params.set_print_timestamps(false);
        params.set_translate(translate_to_en);

        if let Some(lang) = language {
            params.set_language(Some(lang));
        } else {
            // Auto-detect language
            params.set_language(Some("auto"));
        }

        // Callbacks go through catch_unwind trampolines (§4.24); the state must outlive `full`.
        let mut callbacks = CallbackState {
            progress: progress_cb,
            abort: abort_cb,
            panicked: false,
        };
        let user_data = &mut callbacks as *mut CallbackState<P, A> as *mut c_void;
        unsafe {
            params.set_progress_callback(Some(safe_progress_trampoline::<P, A>));
            params.set_progress_callback_user_data(user_data);
            params.set_abort_callback(Some(safe_abort_trampoline::<P, A>));
            params.set_abort_callback_user_data(user_data);
        }

        let f32_samples = audio.to_f32_samples();

        debug!(
            "Starting Whisper inference with {} samples ({:.2}s)",
            f32_samples.len(),
            audio.duration_seconds
        );

        let full_result = state.full(params, &f32_samples);

        // A panic caught in a callback is reported as such, not as a generic inference error (§4.24)
        if callbacks.panicked {
            error!("Panic caught in a Whisper callback; inference aborted");
            return Err(VadError::WhisperCallbackPanic(
                "Um callback de progresso/abort entrou em pânico; a inferência foi interrompida".to_string(),
            ));
        }
        full_result
            .map_err(|err| VadError::Whisper(format!("Erro durante a inferência do Whisper: {:?}", err)))?;

        let num_segments = state.full_n_segments();
        let mut results = Vec::with_capacity(num_segments as usize);

        for i in 0..num_segments {
            if let Some(segment) = state.get_segment(i) {
                // whisper.cpp returns timestamps in 10-millisecond units
                let t0 = segment.start_timestamp();
                let t1 = segment.end_timestamp();
                let text = segment.to_str_lossy().unwrap_or_default().trim().to_string();

                if !text.is_empty() {
                    results.push(TranscriptionSegment {
                        start_ms: t0 * 10,
                        end_ms: t1 * 10,
                        text,
                    });
                }
            }
        }

        info!("Whisper transcription finished: {} segments generated", results.len());
        Ok(results)
    }

    /// Formats transcription segments as Markdown list lines (`- **[00:04 → 00:09]** text`),
    /// shared by the transcript export and the meeting-notes export.
    pub fn segments_to_markdown(segments: &[TranscriptionSegment]) -> String {
        let mut md = String::new();
        for seg in segments {
            let start = TranscriptionSegment::format_timestamp(seg.start_ms);
            let end = TranscriptionSegment::format_timestamp(seg.end_ms);
            md.push_str(&format!("- **[{start} → {end}]** {}\n", seg.text));
        }
        md
    }

    /// Formats transcription segments into clean Markdown format.
    pub fn export_to_markdown(title: &str, segments: &[TranscriptionSegment]) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Transcrição — {}\n\n", title));
        md.push_str("Gerado automaticamente pelo VAD com Whisper AI.\n\n---\n\n");
        md.push_str(&Self::segments_to_markdown(segments));
        md
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_transcription_segment_formatting() {
        assert_eq!(TranscriptionSegment::format_timestamp(0), "00:00");
        assert_eq!(TranscriptionSegment::format_timestamp(42_000), "00:42");
        assert_eq!(TranscriptionSegment::format_timestamp(252_000), "04:12");
        assert_eq!(TranscriptionSegment::format_timestamp(3_665_000), "01:01:05");
    }

    #[test]
    fn test_physical_threads_configuration_section_5() {
        let physical = num_cpus::get_physical();
        assert!(physical >= 1, "Physical CPU count must be at least 1");
    }

    #[test]
    fn test_safe_callbacks_catch_unwind_protection_section_4_24() {
        type ProgressCb = Box<dyn FnMut(i32)>;
        type AbortCb = Box<dyn FnMut() -> bool>;

        let mut state = CallbackState::<ProgressCb, AbortCb> {
            progress: Some(Box::new(|_pct: i32| panic!("Intentional test panic inside progress callback"))),
            abort: Some(Box::new(|| -> bool { panic!("Intentional test panic inside abort callback") })),
            panicked: false,
        };
        let user_data = &mut state as *mut _ as *mut c_void;

        unsafe {
            safe_progress_trampoline::<ProgressCb, AbortCb>(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                50,
                user_data,
            );
        }
        // Reaching here means catch_unwind caught the panic; it must also be recorded
        assert!(state.panicked, "a caught panic must be recorded so transcribe() can report it");

        // Once a callback panicked, abort is requested without calling user code again
        let aborted = unsafe { safe_abort_trampoline::<ProgressCb, AbortCb>(user_data) };
        assert!(aborted, "Panic in a callback must safely abort inference without crashing");

        // A panicking abort callback aborts too
        state.panicked = false;
        let aborted = unsafe { safe_abort_trampoline::<ProgressCb, AbortCb>(user_data) };
        assert!(aborted && state.panicked);
    }

    #[test]
    fn test_abort_trampoline_without_user_callback_does_not_abort() {
        type ProgressCb = fn(i32);
        type AbortCb = fn() -> bool;
        let mut state = CallbackState::<ProgressCb, AbortCb> { progress: None, abort: None, panicked: false };
        let aborted = unsafe {
            safe_abort_trampoline::<ProgressCb, AbortCb>(&mut state as *mut _ as *mut c_void)
        };
        assert!(!aborted);
    }

    /// Exit criterion of Sprint_06 with real inputs: YouTube audio (yt-dlp) -> `AudioExtractor`
    /// -> `WhisperEngine`, loading the `tiny` model from disk and from a RAM-only buffer.
    /// Needs network, yt-dlp and ffmpeg; run with `cargo test -p vad-ai -- --ignored --nocapture`.
    #[test]
    #[ignore = "needs network access, yt-dlp and ffmpeg"]
    fn test_real_speech_transcription_disk_and_ram_only() {
        use crate::extractor::{AudioExtractor, ExtractionStatus};
        use crate::model_manager::{find_preset, ModelManager};
        use std::time::Duration;
        use vad_core::ModelStorageMode;

        let dir = std::path::PathBuf::from(format!("/tmp/vad_test_whisper_e2e_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();

        // "Me at the zoo": 19s of English speech ("...in front of the elephants...")
        let speech = dir.join("speech.m4a");
        let ok = std::process::Command::new("yt-dlp")
            .args(["--no-config", "-q", "-f", "bestaudio[ext=m4a]/bestaudio", "-o"])
            .arg(&speech)
            .arg("https://www.youtube.com/watch?v=jNQXAC9IVRw")
            .status()
            .map(|s| s.success())
            .unwrap_or(false);
        assert!(ok && speech.exists(), "yt-dlp could not fetch the speech sample");

        let extractor = AudioExtractor::new();
        let (rx, _handle) = extractor.extract_async(speech.to_string_lossy().to_string(), Some(19.0));
        let audio = loop {
            match rx.recv_timeout(Duration::from_secs(30)).expect("extraction timed out") {
                ExtractionStatus::Completed(a) => break a,
                ExtractionStatus::Failed(e) => panic!("extraction failed: {e}"),
                _ => {}
            }
        };
        assert!((15.0..25.0).contains(&audio.duration_seconds));

        // Disk mode: download once into the sandbox, then load by path
        let manager = ModelManager::with_dir(dir.join("models"));
        let preset = find_preset("tiny").unwrap();
        let disk_source = manager
            .load_or_download_model(preset, ModelStorageMode::Disk, |_| {})
            .expect("tiny model download");
        let ModelSource::Disk(model_path) = &disk_source else { panic!("expected disk source") };

        let mut progress_seen = false;
        let engine = WhisperEngine::load(&disk_source).expect("load from disk");
        let segments = engine
            .transcribe(&audio, Some("en"), Some(|_: i32| progress_seen = true), None::<fn() -> bool>)
            .expect("transcription (disk)");
        let text = segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
        println!("[disk] {} segments: {text}", segments.len());
        assert!(progress_seen, "progress callback never ran");
        assert!(text.contains("elephant"), "unexpected transcription: {text}");
        assert!(segments.iter().all(|s| s.end_ms >= s.start_ms && s.end_ms <= 25_000));

        // RAM-only path: the same model bytes held in a pinned Arc<[u8]> (§4.11)
        let bytes: std::sync::Arc<[u8]> = std::fs::read(model_path).unwrap().into();
        let ram_engine = WhisperEngine::load(&ModelSource::Ram(bytes)).expect("load from RAM buffer");
        let ram_segments = ram_engine
            .transcribe(&audio, Some("en"), None::<fn(i32)>, None::<fn() -> bool>)
            .expect("transcription (RAM-only)");
        let ram_text = ram_segments.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ").to_lowercase();
        println!("[ram] {} segments: {ram_text}", ram_segments.len());
        assert!(ram_text.contains("elephant"), "unexpected transcription: {ram_text}");

        // A callback that aborts stops the inference with an error instead of hanging
        let aborted = engine.transcribe(&audio, Some("en"), None::<fn(i32)>, Some(|| true));
        assert!(aborted.is_err(), "abort callback must stop the inference");

        // A panicking progress callback is reported as WhisperCallbackPanic, not a crash (§4.24)
        let panicked = engine.transcribe(&audio, Some("en"), Some(|_: i32| panic!("boom")), None::<fn() -> bool>);
        assert!(matches!(panicked, Err(VadError::WhisperCallbackPanic(_))), "got {panicked:?}");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_export_to_markdown() {
        let segments = vec![
            TranscriptionSegment {
                start_ms: 4200,
                end_ms: 8500,
                text: "Introdução aos objetivos do projeto".to_string(),
            },
            TranscriptionSegment {
                start_ms: 9000,
                end_ms: 15000,
                text: "Discussão de arquitetura em Rust".to_string(),
            },
        ];

        let md = WhisperEngine::export_to_markdown("Reunião Estratégica", &segments);
        assert!(md.contains("# Transcrição — Reunião Estratégica"));
        assert!(md.contains("**[00:04 → 00:08]** Introdução aos objetivos do projeto"));
        assert!(md.contains("**[00:09 → 00:15]** Discussão de arquitetura em Rust"));
    }
}
