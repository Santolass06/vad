use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use crossbeam_channel::{unbounded, Receiver};
use tracing::{debug, error, info, warn};
use vad_core::VadError;


/// Parameters for clipping and exporting media segments.
#[derive(Debug, Clone, PartialEq)]
pub struct ClipExportOptions {
    /// Path to input media file.
    pub input_path: String,
    /// Target path for the exported clip.
    pub output_path: String,
    /// Start timestamp in seconds.
    pub start_seconds: f64,
    /// End timestamp in seconds.
    pub end_seconds: f64,
    /// If `false` (default): fast keyframe copy (`-c copy`).
    /// If `true`: frame-accurate re-encoding (`-c:v libx264 -crf 18 -c:a aac`).
    pub exact_cut: bool,
}

/// Status emitted during clip export.
#[derive(Debug, Clone, PartialEq)]
pub enum ClipExportStatus {
    Starting,
    Completed {
        output_path: PathBuf,
        duration_seconds: f64,
    },
    Failed(String),
    Cancelled,
}

/// Handle allowing cancellation of an active clip export process (§4.17).
#[derive(Clone)]
pub struct ClipExportHandle {
    cancelled: Arc<AtomicBool>,
    child_pid: Arc<Mutex<Option<u32>>>,
}

impl Default for ClipExportHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl ClipExportHandle {
    pub fn new() -> Self {
        Self {
            cancelled: Arc::new(AtomicBool::new(false)),
            child_pid: Arc::new(Mutex::new(None)),
        }
    }

    pub fn is_cancelled(&self) -> bool {
        self.cancelled.load(Ordering::SeqCst)
    }

    /// Requests cancellation and kills the FFmpeg subprocess to prevent orphan processes (§4.17).
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::SeqCst);
        if let Ok(guard) = self.child_pid.lock() {
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
        if let Ok(mut guard) = self.child_pid.lock() {
            *guard = Some(pid);
        }
    }

    fn clear_pid(&self) {
        if let Ok(mut guard) = self.child_pid.lock() {
            *guard = None;
        }
    }
}

/// Formats seconds into HH:MM:SS.mmm for FFmpeg CLI seeking.
pub fn format_timestamp(seconds: f64) -> String {
    let s_total = seconds.max(0.0);
    let h = (s_total / 3600.0).floor() as u64;
    let m = ((s_total % 3600.0) / 60.0).floor() as u64;
    let s = (s_total % 60.0).floor() as u64;
    let millis = ((s_total.fract()) * 1000.0).round() as u64;

    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

/// Sanitizes a file path to prevent CLI option confusion (§4.26).
/// If a relative path starts with `-`, prepends `./` so FFmpeg treats it as a file.
pub fn sanitize_path(path: &str) -> String {
    if path.starts_with('-') && !path.starts_with("./") && !path.starts_with('/') {
        format!("./{}", path)
    } else {
        path.to_string()
    }
}

/// Determines audio codec flags for exact cut re-encoding based on destination file extension.
fn audio_recode_flags(output_path: &str) -> Vec<&'static str> {
    let path = Path::new(output_path);
    let ext = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase();

    match ext.as_str() {
        "opus" => vec!["-c:a", "libopus"],
        "mp3" => vec!["-c:a", "libmp3lame"],
        "wav" => vec!["-c:a", "pcm_s16le"],
        _ => vec!["-c:a", "aac"],
    }
}

/// Validates clip export options before spawning FFmpeg.
pub fn validate_options(options: &ClipExportOptions) -> Result<(), VadError> {
    if options.input_path.trim().is_empty() {
        return Err(VadError::ExtractionFailed(
            "Caminho do ficheiro de entrada não especificado".to_string(),
        ));
    }
    if options.output_path.trim().is_empty() {
        return Err(VadError::ExtractionFailed(
            "Caminho do ficheiro de saída não especificado".to_string(),
        ));
    }
    if options.start_seconds < 0.0 {
        return Err(VadError::ExtractionFailed(
            "Timestamp inicial não pode ser negativo".to_string(),
        ));
    }
    if options.end_seconds <= options.start_seconds {
        return Err(VadError::ExtractionFailed(
            "Timestamp final deve ser estritamente superior ao timestamp inicial".to_string(),
        ));
    }
    if !Path::new(&options.input_path).exists() {
        return Err(VadError::ExtractionFailed(format!(
            "Ficheiro de entrada não encontrado: {}",
            options.input_path
        )));
    }

    Ok(())
}


/// Builds the secure `ffmpeg` command per PLANO_VAD.md §4.26 and §8.
/// - Argument vector (`Command::args`), never shell.
/// - Sanitized paths (`sanitize_path`).
/// - Option delimiter `--` before the arbitrary user-specified output path.
pub fn build_ffmpeg_clip_command(options: &ClipExportOptions) -> Command {
    let safe_input = sanitize_path(&options.input_path);
    let safe_output = sanitize_path(&options.output_path);

    let start_str = format_timestamp(options.start_seconds);
    let end_str = format_timestamp(options.end_seconds);

    let mut cmd = Command::new("ffmpeg");

    // Standard safety and non-interactive flags
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"]);

    // Timestamps placed before input for fast seek (§8)
    cmd.args(["-ss", &start_str, "-to", &end_str]);

    // Input file
    cmd.args(["-i", &safe_input]);

    if options.exact_cut {
        // Frame-accurate re-encode: -c:v libx264 -crf 18 {audio_codec} -avoid_negative_ts 1 (§8)
        cmd.args(["-c:v", "libx264", "-crf", "18"]);
        for flag in audio_recode_flags(&safe_output) {
            cmd.arg(flag);
        }
    } else {
        // Fast keyframe stream copy: -c copy -avoid_negative_ts 1 (§8)
        cmd.args(["-c", "copy"]);
    }

    cmd.args(["-avoid_negative_ts", "1"]);

    // Terminate options with `--` before user output path (§4.26)
    cmd.arg("--");
    cmd.arg(&safe_output);

    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());

    cmd
}

/// Spawns asynchronous clip export on a background worker thread.
/// Returns a receiver for status events and a cancellation handle (§4.17).
pub fn export_clip_async(
    options: ClipExportOptions,
) -> (Receiver<ClipExportStatus>, ClipExportHandle) {
    let (tx, rx) = unbounded();
    let handle = ClipExportHandle::new();
    let thread_handle = handle.clone();

    thread::spawn(move || {
        let duration_seconds = options.end_seconds - options.start_seconds;

        if let Err(err) = validate_options(&options) {
            warn!("Clip export validation failed: {:?}", err);
            let _ = tx.send(ClipExportStatus::Failed(err.to_string()));
            return;
        }

        let _ = tx.send(ClipExportStatus::Starting);

        let mut cmd = build_ffmpeg_clip_command(&options);
        debug!("Spawning FFmpeg clip export: {:?}", cmd);

        let child = match cmd.spawn() {
            Ok(c) => c,
            Err(e) => {
                error!("Failed to spawn FFmpeg clip export: {:?}", e);
                let _ = tx.send(ClipExportStatus::Failed(format!(
                    "Falha ao executar FFmpeg: {e}"
                )));
                return;
            }
        };

        thread_handle.set_pid(child.id());

        let output = match child.wait_with_output() {
            Ok(out) => out,
            Err(e) => {
                thread_handle.clear_pid();
                if thread_handle.is_cancelled() {
                    let _ = tx.send(ClipExportStatus::Cancelled);
                } else {
                    let _ = tx.send(ClipExportStatus::Failed(format!(
                        "Erro ao aguardar pelo FFmpeg: {e}"
                    )));
                }
                return;
            }
        };

        thread_handle.clear_pid();

        if thread_handle.is_cancelled() {
            info!("Clip export was cancelled by user");
            // Clean up partially written output file if present
            let _ = std::fs::remove_file(&options.output_path);
            let _ = tx.send(ClipExportStatus::Cancelled);
            return;
        }

        let status = output.status;
        if status.success() {
            let out_path = PathBuf::from(&options.output_path);
            info!(
                "Clip exported successfully to {:?} ({:.2}s)",
                out_path, duration_seconds
            );
            let _ = tx.send(ClipExportStatus::Completed {
                output_path: out_path,
                duration_seconds,
            });
        } else {
            let stderr_msg = String::from_utf8_lossy(&output.stderr).trim().to_string();
            let err_text = if !stderr_msg.is_empty() {
                stderr_msg
            } else {
                #[cfg(unix)]
                if let Some(sig) = status.signal() {
                    format!("FFmpeg terminado por sinal {sig}")
                } else {
                    format!("FFmpeg terminou com código {:?}", status.code())
                }
                #[cfg(not(unix))]
                format!("FFmpeg terminou com código {:?}", status.code())
            };

            error!("FFmpeg clip export failed: {}", err_text);
            let _ = std::fs::remove_file(&options.output_path);
            let _ = tx.send(ClipExportStatus::Failed(format!(
                "Exportação falhou: {err_text}"
            )));
        }
    });

    (rx, handle)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0.0), "00:00:00.000");
        assert_eq!(format_timestamp(5.25), "00:00:05.250");
        assert_eq!(format_timestamp(65.5), "00:01:05.500");
        assert_eq!(format_timestamp(3661.125), "01:01:01.125");
    }

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("-foo.mp4"), "./-foo.mp4");
        assert_eq!(sanitize_path("./-foo.mp4"), "./-foo.mp4");
        assert_eq!(sanitize_path("/tmp/-foo.mp4"), "/tmp/-foo.mp4");
        assert_eq!(sanitize_path("video.mp4"), "video.mp4");
    }

    #[test]
    fn test_build_ffmpeg_clip_command_copy_mode() {
        let opts = ClipExportOptions {
            input_path: "-input.mp4".to_string(),
            output_path: "-output.mp4".to_string(),
            start_seconds: 10.0,
            end_seconds: 25.0,
            exact_cut: false,
        };

        let cmd = build_ffmpeg_clip_command(&opts);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Must pass -c copy for fast keyframe cut (§8)
        assert!(args.contains(&"-c".to_string()));
        assert!(args.contains(&"copy".to_string()));
        assert!(args.contains(&"-avoid_negative_ts".to_string()));

        // Must sanitize paths and end with -- before output (§4.26)
        let double_dash_idx = args.iter().position(|a| a == "--").expect("Must contain --");
        assert_eq!(args[double_dash_idx + 1], "./-output.mp4");

        let input_idx = args.iter().position(|a| a == "-i").expect("Must contain -i");
        assert_eq!(args[input_idx + 1], "./-input.mp4");
    }

    #[test]
    fn test_build_ffmpeg_clip_command_exact_mode() {
        let opts = ClipExportOptions {
            input_path: "input.mp4".to_string(),
            output_path: "output.mp4".to_string(),
            start_seconds: 5.0,
            end_seconds: 15.0,
            exact_cut: true,
        };

        let cmd = build_ffmpeg_clip_command(&opts);
        let args: Vec<String> = cmd
            .get_args()
            .map(|a| a.to_string_lossy().to_string())
            .collect();

        // Must re-encode with libx264 and aac (§8)
        assert!(args.contains(&"libx264".to_string()));
        assert!(args.contains(&"-crf".to_string()));
        assert!(args.contains(&"18".to_string()));
        assert!(args.contains(&"aac".to_string()));
    }

    #[test]
    fn test_validate_options() {
        let valid_file = "/tmp/vad_clip_test_existing.tmp";
        File::create(valid_file).unwrap();

        let ok_opts = ClipExportOptions {
            input_path: valid_file.to_string(),
            output_path: "/tmp/clip.mp4".to_string(),
            start_seconds: 2.0,
            end_seconds: 10.0,
            exact_cut: false,
        };
        assert!(validate_options(&ok_opts).is_ok());

        let invalid_time = ClipExportOptions {
            input_path: valid_file.to_string(),
            output_path: "/tmp/clip.mp4".to_string(),
            start_seconds: 10.0,
            end_seconds: 2.0,
            exact_cut: false,
        };
        assert!(validate_options(&invalid_time).is_err());

        let non_existent = ClipExportOptions {
            input_path: "/tmp/vad_non_existent_12345.mp4".to_string(),
            output_path: "/tmp/clip.mp4".to_string(),
            start_seconds: 0.0,
            end_seconds: 5.0,
            exact_cut: false,
        };
        assert!(validate_options(&non_existent).is_err());

        let _ = std::fs::remove_file(valid_file);
    }

    #[test]
    fn test_export_real_clip_keyframe_and_exact() {
        // Generate a synthetic 3-second media file with FFmpeg in /tmp
        let src_file = "/tmp/vad_test_clip_src.mp4";
        let clip_copy = "/tmp/vad_test_clip_copy.mp4";
        let clip_exact = "/tmp/vad_test_clip_exact.mp4";

        let mut gen_cmd = Command::new("ffmpeg");
        gen_cmd.args([
            "-nostdin",
            "-hide_banner",
            "-loglevel",
            "error",
            "-y",
            "-f",
            "lavfi",
            "-i",
            "testsrc2=duration=3:size=320x240:rate=30",
            "-f",
            "lavfi",
            "-i",
            "sine=frequency=440:duration=3",
            "-c:v",
            "libx264",
            "-c:a",
            "aac",
            "--",
            src_file,
        ]);

        let gen_status = gen_cmd.status().expect("FFmpeg must be available for test");
        assert!(gen_status.success(), "Failed to generate synthetic source media");

        // 1. Test Keyframe Copy Export
        let copy_opts = ClipExportOptions {
            input_path: src_file.to_string(),
            output_path: clip_copy.to_string(),
            start_seconds: 0.5,
            end_seconds: 2.0,
            exact_cut: false,
        };

        let (rx_copy, _handle) = export_clip_async(copy_opts);
        let mut completed_copy = false;
        while let Ok(status) = rx_copy.recv() {
            if let ClipExportStatus::Completed { output_path, .. } = status {
                assert_eq!(output_path, PathBuf::from(clip_copy));
                assert!(output_path.exists());
                assert!(output_path.metadata().unwrap().len() > 100);
                completed_copy = true;
                break;
            } else if let ClipExportStatus::Failed(err) = status {
                panic!("Keyframe export failed: {}", err);
            }
        }
        assert!(completed_copy, "Keyframe export did not complete");

        // 2. Test Exact Recode Export
        let exact_opts = ClipExportOptions {
            input_path: src_file.to_string(),
            output_path: clip_exact.to_string(),
            start_seconds: 0.5,
            end_seconds: 2.0,
            exact_cut: true,
        };

        let (rx_exact, _handle) = export_clip_async(exact_opts);
        let mut completed_exact = false;
        while let Ok(status) = rx_exact.recv() {
            if let ClipExportStatus::Completed { output_path, .. } = status {
                assert_eq!(output_path, PathBuf::from(clip_exact));
                assert!(output_path.exists());
                assert!(output_path.metadata().unwrap().len() > 100);
                completed_exact = true;
                break;
            } else if let ClipExportStatus::Failed(err) = status {
                panic!("Exact export failed: {}", err);
            }
        }
        assert!(completed_exact, "Exact export did not complete");

        // Clean up test files (§4 of workflow.md)
        let _ = std::fs::remove_file(src_file);
        let _ = std::fs::remove_file(clip_copy);
        let _ = std::fs::remove_file(clip_exact);
    }
}
