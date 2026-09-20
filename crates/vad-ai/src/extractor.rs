use std::collections::HashMap;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use crossbeam_channel::{unbounded, Receiver, Sender};
use tracing::{debug, error, info, warn};

/// Maximum audio duration retained in memory per PLANO_VAD.md §4.12 (4 hours).
/// 4 hours at 16kHz mono i16: 4 * 3600 * 16000 = 230,400,000 samples (~460.8 MB).
pub const MAX_AUDIO_DURATION_SECS: f64 = 4.0 * 3600.0;
pub const MAX_AUDIO_SAMPLES: usize = (MAX_AUDIO_DURATION_SECS * 16_000.0) as usize;
pub const SAMPLE_RATE: u32 = 16_000;
pub const CHANNELS: u16 = 1;

/// Decoded PCM audio data in 16kHz mono i16 (§4.12).
/// Kept strictly in-memory; never persisted to disk (§4.18).
#[derive(Clone, Debug)]
pub struct PcmAudio {
    pub path: String,
    pub samples: Arc<[i16]>,
    pub sample_rate: u32,
    pub channels: u16,
    pub duration_seconds: f64,
    pub is_truncated: bool,
}

impl PcmAudio {
    /// Normalizes samples to floating-point `f32` in `[-1.0, 1.0]` for Whisper ingestion.
    pub fn to_f32_samples(&self) -> Vec<f32> {
        self.samples
            .iter()
            .map(|&s| s as f32 / 32768.0)
            .collect()
    }
}

/// Progress report for asynchronous audio extraction.
#[derive(Clone, Debug, PartialEq)]
pub struct AudioExtractionProgress {
    pub path: String,
    pub percent: f32,
    pub current_seconds: f64,
    pub total_seconds: f64,
}

/// Status emitted during extraction.
#[derive(Clone, Debug)]
pub enum ExtractionStatus {
    Progress(AudioExtractionProgress),
    Completed(PcmAudio),
    Failed(String),
    Cancelled,
}

/// Handle allowing cancellation of an active extraction process.
#[derive(Clone)]
pub struct ExtractionHandle {
    cancelled: Arc<AtomicBool>,
    child_handle: Arc<Mutex<Option<u32>>>,
}

impl Default for ExtractionHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ExtractionHandle {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            child_handle: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Requests cancellation and kills the FFmpeg subprocess to prevent orphan processes (§4.17).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.child_handle.lock() {
            if let Some(pid) = *guard {
                #[cfg(unix)]
                unsafe {
                    libc::kill(pid as i32, libc::SIGKILL);
                }
                #[cfg(not(unix))]
                let _ = pid;
            }
        }
    }

    fn set_pid(&self, pid: u32) {
        if let Ok(mut guard) = self.child_handle.lock() {
            *guard = Some(pid);
        }
    }

    fn clear_pid(&self) {
        if let Ok(mut guard) = self.child_handle.lock() {
            *guard = None;
        }
    }
}

/// In-memory audio extraction and caching service.
/// Zero disk persistence: never touches `~/.cache/vad/pcm/` (§4.18).
pub struct AudioExtractor {
    cache: Arc<Mutex<HashMap<String, PcmAudio>>>,
}

impl Default for AudioExtractor {
    fn default() -> Self {
        Self::new()
    }
}

impl AudioExtractor {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Checks if PCM for the given file is already in memory cache.
    pub fn get_cached(&self, path: &str) -> Option<PcmAudio> {
        let guard = self.cache.lock().ok()?;
        guard.get(path).cloned()
    }

    /// Clears the cache or a specific path when file is closed.
    pub fn remove_cached(&self, path: &str) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.remove(path);
        }
    }

    /// Clears all in-memory PCM audio.
    pub fn clear_cache(&self) {
        if let Ok(mut guard) = self.cache.lock() {
            guard.clear();
        }
    }

    /// Sanitizes an input path to prevent CLI option confusion (§4.26).
    /// If a relative path starts with `-`, prepends `./` so FFmpeg treats it as a file.
    pub fn sanitize_input_path(path: &str) -> String {
        if path.starts_with('-') && !path.starts_with("./") && !path.starts_with('/') {
            format!("./{}", path)
        } else {
            path.to_string()
        }
    }

    /// Builds the secure `ffmpeg` command with `--` argument delimiter per PLANO_VAD.md §4.26.
    pub fn build_ffmpeg_command(input_path: &str) -> Command {
        let safe_path = Self::sanitize_input_path(input_path);
        let mut cmd = Command::new("ffmpeg");
        // Always pass argv as vector, never via shell (§4.26).
        // Input path is sanitized against dash prefixes, and options end with `--` before output.
        cmd.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-progress",
            "pipe:2",
            "-i",
            &safe_path,
            "-vn",
            "-f",
            "s16le",
            "-acodec",
            "pcm_s16le",
            "-ac",
            "1",
            "-ar",
            "16000",
            "--",
            "pipe:1",
        ]);
        cmd.stdout(Stdio::piped());
        cmd.stderr(Stdio::piped());
        cmd
    }

    /// Starts asynchronous audio extraction on a background thread.
    /// Returns a receiver for status events and a cancellation handle (§4.17).
    pub fn extract_async(
        &self,
        input_path: String,
        duration_hint: Option<f64>,
    ) -> (Receiver<ExtractionStatus>, ExtractionHandle) {
        let (tx, rx) = unbounded();
        let handle = ExtractionHandle::new();
        let handle_clone = handle.clone();
        let cache = Arc::clone(&self.cache);

        // If already in cache, return immediately
        if let Some(cached) = self.get_cached(&input_path) {
            let _ = tx.send(ExtractionStatus::Completed(cached));
            return (rx, handle);
        }

        thread::Builder::new()
            .name(format!("vad-extractor-{}", input_path))
            .spawn(move || {
                Self::run_extraction(input_path, duration_hint, tx, handle_clone, cache);
            })
            .expect("Failed to spawn audio extraction thread");

        (rx, handle)
    }

    /// Internal synchronous extraction worker run in background thread.
    fn run_extraction(
        input_path: String,
        duration_hint: Option<f64>,
        tx: Sender<ExtractionStatus>,
        handle: ExtractionHandle,
        cache: Arc<Mutex<HashMap<String, PcmAudio>>>,
    ) {
        info!("Starting audio extraction for: {}", input_path);

        let mut cmd = Self::build_ffmpeg_command(&input_path);
        let mut child: Child = match cmd.spawn() {
            Ok(c) => c,
            Err(err) => {
                error!("Failed to spawn ffmpeg subprocess: {:?}", err);
                let _ = tx.send(ExtractionStatus::Failed(format!(
                    "Não foi possível iniciar o ffmpeg: {}",
                    err
                )));
                return;
            }
        };

        handle.set_pid(child.id());

        let stdout = child.stdout.take();
        let stderr = child.stderr.take();

        if stdout.is_none() || stderr.is_none() {
            let _ = child.kill();
            let _ = child.wait();
            handle.clear_pid();
            let _ = tx.send(ExtractionStatus::Failed(
                "Falha ao abrir pipes de E/S do ffmpeg".to_string(),
            ));
            return;
        }

        let mut stdout_pipe = stdout.unwrap();
        let stderr_pipe = stderr.unwrap();

        // 1. Thread to monitor stderr for `-progress pipe:2` key=value updates
        let tx_progress = tx.clone();
        let input_path_progress = input_path.clone();
        let handle_progress = handle.clone();
        let duration_hint_val = duration_hint.unwrap_or(0.0);

        let stderr_thread = thread::spawn(move || {
            use std::io::BufRead;
            let reader = std::io::BufReader::new(stderr_pipe);
            let mut detected_duration = duration_hint_val;

            for line in reader.lines() {
                if handle_progress.is_cancelled() {
                    break;
                }
                let Ok(line) = line else { break };
                let line = line.trim();

                // Format: out_time_us=... or out_time_ms=... or out_time=HH:MM:SS.xxx
                if let Some(rest) = line.strip_prefix("out_time_ms=") {
                    if let Ok(us) = rest.parse::<i64>() {
                        let cur_secs = us as f64 / 1_000_000.0;
                        let pct = if detected_duration > 0.0 {
                            ((cur_secs / detected_duration) * 100.0).clamp(0.0, 99.0) as f32
                        } else {
                            0.0
                        };
                        let _ = tx_progress.send(ExtractionStatus::Progress(
                            AudioExtractionProgress {
                                path: input_path_progress.clone(),
                                percent: pct,
                                current_seconds: cur_secs,
                                total_seconds: detected_duration,
                            },
                        ));
                    }
                } else if let Some(rest) = line.strip_prefix("out_time_us=") {
                    if let Ok(us) = rest.parse::<i64>() {
                        let cur_secs = us as f64 / 1_000_000.0;
                        let pct = if detected_duration > 0.0 {
                            ((cur_secs / detected_duration) * 100.0).clamp(0.0, 99.0) as f32
                        } else {
                            0.0
                        };
                        let _ = tx_progress.send(ExtractionStatus::Progress(
                            AudioExtractionProgress {
                                path: input_path_progress.clone(),
                                percent: pct,
                                current_seconds: cur_secs,
                                total_seconds: detected_duration,
                            },
                        ));
                    }
                } else if line.starts_with("Duration:") {
                    // Fallback parsing of header Duration: HH:MM:SS.xx
                    if let Some(dur_str) = line.split(',').next().and_then(|s| s.strip_prefix("Duration:")) {
                        if let Some(parsed) = parse_duration_string(dur_str.trim()) {
                            detected_duration = parsed;
                        }
                    }
                }
            }
        });

        // 2. Read raw PCM samples (s16le = 2 bytes per sample) from stdout
        let mut raw_bytes = vec![0u8; 16384];
        let mut pcm_samples: Vec<i16> = Vec::with_capacity(16000 * 60); // preallocate 1 min
        let mut is_truncated = false;

        loop {
            if handle.is_cancelled() {
                debug!("Audio extraction cancelled by user");
                let _ = child.kill();
                let _ = child.wait();
                handle.clear_pid();
                let _ = stderr_thread.join();
                let _ = tx.send(ExtractionStatus::Cancelled);
                return;
            }

            match stdout_pipe.read(&mut raw_bytes) {
                Ok(0) => break, // EOF
                Ok(n) => {
                    let mut i = 0;
                    while i + 1 < n {
                        if pcm_samples.len() >= MAX_AUDIO_SAMPLES {
                            // Enforce 4h duration cap (§4.12)
                            warn!(
                                "Audio extraction reached 4h duration cap for {}; truncating buffer",
                                input_path
                            );
                            is_truncated = true;
                            break;
                        }
                        let sample = i16::from_le_bytes([raw_bytes[i], raw_bytes[i + 1]]);
                        pcm_samples.push(sample);
                        i += 2;
                    }

                    if is_truncated {
                        // Kill child process as we've hit the safety cap (§4.12)
                        let _ = child.kill();
                        break;
                    }
                }
                Err(err) if err.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(err) => {
                    error!("Error reading stdout from ffmpeg: {:?}", err);
                    break;
                }
            }
        }

        // Wait for child process exit status (§4.31)
        let wait_result = child.wait();
        handle.clear_pid();
        let _ = stderr_thread.join();

        if handle.is_cancelled() {
            let _ = tx.send(ExtractionStatus::Cancelled);
            return;
        }

        match wait_result {
            Ok(status) => {
                // If truncated, non-zero code is expected due to SIGKILL
                if !status.success() && !is_truncated {
                    let err_msg = Self::format_exit_error(status);
                    error!("FFmpeg extraction failed: {}", err_msg);
                    let _ = tx.send(ExtractionStatus::Failed(err_msg));
                    return;
                }
            }
            Err(err) => {
                error!("Failed to wait for ffmpeg child: {:?}", err);
                let _ = tx.send(ExtractionStatus::Failed(format!(
                    "Erro ao aguardar processo ffmpeg: {}",
                    err
                )));
                return;
            }
        }

        let total_samples = pcm_samples.len();
        let duration_seconds = total_samples as f64 / SAMPLE_RATE as f64;
        let samples_arc: Arc<[i16]> = pcm_samples.into();

        let audio = PcmAudio {
            path: input_path.clone(),
            samples: samples_arc,
            sample_rate: SAMPLE_RATE,
            channels: CHANNELS,
            duration_seconds,
            is_truncated,
        };

        info!(
            "Extraction completed for {}: {:.2}s, {} samples, truncated: {}",
            input_path, duration_seconds, total_samples, is_truncated
        );

        // Cache in memory (§4.12)
        if let Ok(mut guard) = cache.lock() {
            guard.insert(input_path.clone(), audio.clone());
        }

        // Final 100% progress and Completed event
        let _ = tx.send(ExtractionStatus::Progress(AudioExtractionProgress {
            path: input_path,
            percent: 100.0,
            current_seconds: duration_seconds,
            total_seconds: duration_seconds,
        }));
        let _ = tx.send(ExtractionStatus::Completed(audio));
    }

    /// Formats ExitStatus into a descriptive error message per PLANO_VAD.md §4.31.
    fn format_exit_error(status: std::process::ExitStatus) -> String {
        #[cfg(unix)]
        {
            if let Some(sig) = status.signal() {
                return format!(
                    "FFmpeg foi terminado pelo sinal {} (possível esgotamento de memória/OOM-killer)",
                    sig
                );
            }
        }
        if let Some(code) = status.code() {
            format!("FFmpeg terminou com código de erro {}", code)
        } else {
            "FFmpeg terminou de forma anormal".to_string()
        }
    }
}

/// Parses HH:MM:SS.xx into total seconds.
fn parse_duration_string(s: &str) -> Option<f64> {
    let parts: Vec<&str> = s.split(':').collect();
    if parts.len() == 3 {
        let h: f64 = parts[0].parse().ok()?;
        let m: f64 = parts[1].parse().ok()?;
        let s: f64 = parts[2].parse().ok()?;
        Some(h * 3600.0 + m * 60.0 + s)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn test_build_ffmpeg_command_vectorized_args_and_delimiter() {
        let path_with_dashes = "-meu-ficheiro-video.mp4";
        let cmd = AudioExtractor::build_ffmpeg_command(path_with_dashes);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Input path starting with dash is sanitized with `./` to prevent option injection (§4.26)
        let i_pos = args.iter().position(|a| a == "-i").expect("-i must exist");
        assert_eq!(
            args.get(i_pos + 1),
            Some(&format!("./{}", path_with_dashes)),
            "Dash-prefixed input path must be safely prefixed with ./"
        );

        // Must contain `--` directly before the output to prevent option injection (§4.26)
        let delimiter_pos = args.iter().position(|a| a == "--");
        assert!(delimiter_pos.is_some(), "Command must include '--' separator");
        let pos = delimiter_pos.unwrap();
        assert_eq!(
            args.get(pos + 1),
            Some(&"pipe:1".to_string()),
            "Output must immediately follow '--'"
        );

        // Must specify 16kHz mono s16le
        assert!(args.iter().any(|a| a == "s16le"));
        assert!(args.iter().any(|a| a == "16000"));
        assert!(args.iter().any(|a| a == "pipe:1"));
    }

    #[test]
    fn test_pcm_audio_duration_and_normalization() {
        let samples = vec![0i16, 16384, -16384, 32767, -32768];
        let pcm = PcmAudio {
            path: "test.wav".to_string(),
            samples: samples.into(),
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 5.0 / 16000.0,
            is_truncated: false,
        };

        let f32_samples = pcm.to_f32_samples();
        assert_eq!(f32_samples.len(), 5);
        assert!((f32_samples[0] - 0.0).abs() < 1e-4);
        assert!((f32_samples[1] - 0.5).abs() < 1e-3);
        assert!((f32_samples[2] - (-0.5)).abs() < 1e-3);
        assert!((f32_samples[3] - 0.9999).abs() < 1e-3);
        assert!((f32_samples[4] - (-1.0)).abs() < 1e-3);
    }

    #[test]
    fn test_extraction_duration_cap_logic() {
        // Assert that MAX_AUDIO_SAMPLES corresponds to exactly 4 hours at 16kHz mono
        assert_eq!(MAX_AUDIO_SAMPLES, 4 * 3600 * 16000);
        assert_eq!(MAX_AUDIO_DURATION_SECS, 14400.0);
    }

    #[test]
    fn test_audio_cache_in_memory_only() {
        let extractor = AudioExtractor::new();
        assert!(extractor.get_cached("nonexistent.wav").is_none());

        let pcm = PcmAudio {
            path: "test.wav".to_string(),
            samples: Arc::new([100, 200, 300]),
            sample_rate: 16000,
            channels: 1,
            duration_seconds: 3.0 / 16000.0,
            is_truncated: false,
        };

        extractor.cache.lock().unwrap().insert("test.wav".to_string(), pcm.clone());
        assert!(extractor.get_cached("test.wav").is_some());

        extractor.remove_cached("test.wav");
        assert!(extractor.get_cached("test.wav").is_none());
    }

    #[test]
    fn test_real_ffmpeg_extraction_synthetic_audio() {
        // Generate a 1-second synthetic audio file using lavfi in /tmp
        let test_file = "/tmp/vad_test_extract_sine.wav";
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=1",
                "-c:a",
                "pcm_s16le",
                test_file,
            ])
            .status()
            .expect("Failed to execute ffmpeg");
        assert!(status.success());

        let extractor = AudioExtractor::new();
        let (rx, _handle) = extractor.extract_async(test_file.to_string(), Some(1.0));

        let mut completed = None;
        while let Ok(msg) = rx.recv_timeout(Duration::from_secs(5)) {
            match msg {
                ExtractionStatus::Completed(audio) => {
                    completed = Some(audio);
                    break;
                }
                ExtractionStatus::Failed(err) => panic!("Extraction failed: {}", err),
                _ => {}
            }
        }

        let audio = completed.expect("Should complete extraction");
        assert_eq!(audio.sample_rate, 16000);
        assert_eq!(audio.channels, 1);
        // Expect approximately 16000 samples for 1 second of audio
        assert!(audio.samples.len() >= 15500 && audio.samples.len() <= 16500);
        assert!(!audio.is_truncated);

        // Clean up test file
        let _ = std::fs::remove_file(test_file);
    }

    #[test]
    fn test_ffmpeg_extraction_invalid_file_fails_gracefully() {
        let extractor = AudioExtractor::new();
        let (rx, _handle) = extractor.extract_async("/tmp/nonexistent_file_vad_12345.mp3".to_string(), None);

        let mut failed = false;
        while let Ok(msg) = rx.recv_timeout(Duration::from_secs(5)) {
            match msg {
                ExtractionStatus::Failed(_) => {
                    failed = true;
                    break;
                }
                ExtractionStatus::Completed(_) => panic!("Nonexistent file should not succeed"),
                _ => {}
            }
        }
        assert!(failed, "Extraction must report failure for invalid/missing file (§4.31)");
    }

    #[test]
    fn test_ffmpeg_extraction_cancellation_kills_child() {
        // Generate a 10-second synthetic audio file using lavfi in /tmp
        let test_file = "/tmp/vad_test_cancel_sine.wav";
        let status = Command::new("ffmpeg")
            .args([
                "-y",
                "-f",
                "lavfi",
                "-i",
                "sine=frequency=440:duration=10",
                "-c:a",
                "pcm_s16le",
                test_file,
            ])
            .status()
            .expect("Failed to execute ffmpeg");
        assert!(status.success());

        let extractor = AudioExtractor::new();
        let (rx, handle) = extractor.extract_async(test_file.to_string(), Some(10.0));

        // Immediately request cancellation (§4.17)
        handle.cancel();

        let mut cancelled = false;
        while let Ok(msg) = rx.recv_timeout(Duration::from_secs(5)) {
            match msg {
                ExtractionStatus::Cancelled => {
                    cancelled = true;
                    break;
                }
                ExtractionStatus::Failed(_) => break,
                _ => {}
            }
        }

        assert!(cancelled, "Extraction must report Cancelled on handle.cancel()");
        assert!(handle.is_cancelled());

        // Clean up test file
        let _ = std::fs::remove_file(test_file);
    }
}
