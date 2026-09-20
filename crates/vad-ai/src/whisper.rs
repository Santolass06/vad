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

/// Trampoline for progress callback wrapped in `std::panic::catch_unwind` (§4.24).
/// Ensures that Rust panics never cross the FFI boundary, preventing process abort.
unsafe extern "C" fn safe_progress_trampoline<F>(
    _ctx: *mut whisper_rs_sys::whisper_context,
    _state: *mut whisper_rs_sys::whisper_state,
    progress: c_int,
    user_data: *mut c_void,
) where
    F: FnMut(i32),
{
    let _ = catch_unwind(AssertUnwindSafe(|| {
        if !user_data.is_null() {
            let callback = &mut *(user_data as *mut F);
            callback(progress);
        }
    }));
}

/// Trampoline for abort callback wrapped in `std::panic::catch_unwind` (§4.24).
/// In case of panic inside Rust callback, safely aborts Whisper inference without crashing.
unsafe extern "C" fn safe_abort_trampoline<F>(user_data: *mut c_void) -> bool
where
    F: FnMut() -> bool,
{
    let result = catch_unwind(AssertUnwindSafe(|| {
        if !user_data.is_null() {
            let callback = &mut *(user_data as *mut F);
            callback()
        } else {
            false
        }
    }));

    // If panic occurred, signal abort to whisper.cpp safely
    result.unwrap_or(true)
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
        mut progress_cb: Option<P>,
        mut abort_cb: Option<A>,
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

        if let Some(lang) = language {
            params.set_language(Some(lang));
        } else {
            // Auto-detect language
            params.set_language(Some("auto"));
        }

        // Set safe progress callback wrapped in catch_unwind (§4.24)
        if let Some(ref mut cb) = progress_cb {
            unsafe {
                params.set_progress_callback(Some(safe_progress_trampoline::<P>));
                params.set_progress_callback_user_data(cb as *mut P as *mut c_void);
            }
        }

        // Set safe abort callback wrapped in catch_unwind (§4.24)
        if let Some(ref mut cb) = abort_cb {
            unsafe {
                params.set_abort_callback(Some(safe_abort_trampoline::<A>));
                params.set_abort_callback_user_data(cb as *mut A as *mut c_void);
            }
        }

        let f32_samples = audio.to_f32_samples();

        debug!(
            "Starting Whisper inference with {} samples ({:.2}s)",
            f32_samples.len(),
            audio.duration_seconds
        );

        state
            .full(params, &f32_samples)
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

    /// Formats transcription segments into clean Markdown format.
    pub fn export_to_markdown(title: &str, segments: &[TranscriptionSegment]) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Transcrição — {}\n\n", title));
        md.push_str("Gerado automaticamente pelo VAD com Whisper AI.\n\n---\n\n");

        for seg in segments {
            let start = TranscriptionSegment::format_timestamp(seg.start_ms);
            let end = TranscriptionSegment::format_timestamp(seg.end_ms);
            md.push_str(&format!("- **[{start} → {end}]** {}\n", seg.text));
        }

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
        let mut panicking_progress: ProgressCb = Box::new(|_pct: i32| {
            panic!("Intentional test panic inside progress callback");
        });

        unsafe {
            safe_progress_trampoline::<ProgressCb>(
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                50,
                &mut panicking_progress as *mut _ as *mut c_void,
            );
        }

        // If we reached here, catch_unwind caught the panic successfully without aborting the process!

        type AbortCb = Box<dyn FnMut() -> bool>;
        let mut panicking_abort: AbortCb = Box::new(|| -> bool {
            panic!("Intentional test panic inside abort callback");
        });

        let aborted = unsafe {
            safe_abort_trampoline::<AbortCb>(&mut panicking_abort as *mut _ as *mut c_void)
        };

        assert!(aborted, "Panic in abort callback must safely abort inference without crashing");
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
