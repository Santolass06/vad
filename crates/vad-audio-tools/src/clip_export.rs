use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, ExitStatus, Stdio};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

#[cfg(unix)]
use std::os::unix::process::ExitStatusExt;

use crossbeam_channel::{unbounded, Receiver};
use tracing::{debug, error, info, warn};
use vad_core::{vad_data_dir, VadError};

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
    /// If `true`: frame-accurate re-encoding (`-c:v libx264 -crf 18 -c:a aac`, or the codecs the
    /// destination container accepts, see [`recode_flags`]).
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
        let guard = self.child_pid.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(pid) = *guard {
            #[cfg(unix)]
            unsafe {
                libc::kill(pid as i32, libc::SIGKILL);
            }
            #[cfg(not(unix))]
            let _ = pid;
        }
    }

    fn set_pid(&self, pid: u32) {
        *self.child_pid.lock().unwrap_or_else(|e| e.into_inner()) = Some(pid);
    }

    fn clear_pid(&self) {
        *self.child_pid.lock().unwrap_or_else(|e| e.into_inner()) = None;
    }

    /// Non-blocking reap. The pid is withdrawn under the same lock `cancel()` signals under, so a
    /// late `cancel()` can never SIGKILL an unrelated process that reused the pid (same reasoning
    /// as the extractor's handle).
    fn try_reap(&self, child: &mut Child) -> std::io::Result<Option<ExitStatus>> {
        let mut guard = self.child_pid.lock().unwrap_or_else(|e| e.into_inner());
        let status = child.try_wait()?;
        if status.is_some() {
            *guard = None;
        }
        Ok(status)
    }
}

/// Formats seconds into HH:MM:SS.mmm for FFmpeg CLI seeking.
pub fn format_timestamp(seconds: f64) -> String {
    // Round to whole milliseconds first: rounding only the fraction turned 5.9996 s into
    // "00:00:05.1000", which ffmpeg reads as 5.1 s.
    let total_ms = (seconds.max(0.0) * 1000.0).round() as u64;
    let h = total_ms / 3_600_000;
    let m = (total_ms % 3_600_000) / 60_000;
    let s = (total_ms % 60_000) / 1000;
    let ms = total_ms % 1000;

    format!("{h:02}:{m:02}:{s:02}.{ms:03}")
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

/// Extension of `path`, lowercased (empty when there is none).
fn extension_lower(path: &str) -> String {
    Path::new(path)
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or("")
        .to_lowercase()
}

/// Containers that only hold audio: the video stream of a video input must be dropped (`-vn`),
/// otherwise ffmpeg refuses to write them.
fn is_audio_only_container(ext: &str) -> bool {
    matches!(ext, "mp3" | "wav" | "flac" | "ogg" | "oga" | "opus" | "m4a" | "aac")
}

/// Codec flags for the exact (re-encode) cut, chosen from the destination container: each
/// container only accepts some codecs (`-c:a aac` into `.flac`/`.ogg`, or `libx264` into `.webm`,
/// makes ffmpeg fail while writing the header).
pub fn recode_flags(output_path: &str) -> Vec<&'static str> {
    match extension_lower(output_path).as_str() {
        "webm" => vec!["-c:v", "libvpx-vp9", "-crf", "24", "-b:v", "0", "-c:a", "libopus"],
        "opus" => vec!["-vn", "-c:a", "libopus"],
        "mp3" => vec!["-vn", "-c:a", "libmp3lame"],
        "wav" => vec!["-vn", "-c:a", "pcm_s16le"],
        "flac" => vec!["-vn", "-c:a", "flac"],
        "ogg" | "oga" => vec!["-vn", "-c:a", "libvorbis"],
        "m4a" | "aac" => vec!["-vn", "-c:a", "aac"],
        _ => vec!["-c:v", "libx264", "-crf", "18", "-c:a", "aac"],
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
    if !options.start_seconds.is_finite() || !options.end_seconds.is_finite() {
        return Err(VadError::ExtractionFailed(
            "Timestamps inválidos".to_string(),
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
    let input = Path::new(&options.input_path);
    if !input.exists() {
        return Err(VadError::ExtractionFailed(format!(
            "Ficheiro de entrada não encontrado: {}",
            options.input_path
        )));
    }

    // Never write over an existing file. Besides silently destroying it, the failure path below
    // deletes the output, which for `output == input` would delete the user's original media.
    let output = Path::new(&options.output_path);
    if output.exists() {
        let same_file = match (input.canonicalize(), output.canonicalize()) {
            (Ok(a), Ok(b)) => a == b,
            _ => false,
        };
        let msg = if same_file {
            "O ficheiro de saída não pode ser o próprio ficheiro de entrada".to_string()
        } else {
            format!(
                "Já existe um ficheiro com esse nome: {} — escolhe outro nome",
                options.output_path
            )
        };
        return Err(VadError::ExtractionFailed(msg));
    }

    Ok(())
}

/// Turns the file name typed in the export panel into the final output path: absolute names are
/// kept, relative ones go next to the input (or into `<data dir>/clips` when that is not a
/// directory), and a name without extension gets the input's, so ffmpeg can pick a muxer.
pub fn resolve_output_path(input_path: &str, filename: &str) -> PathBuf {
    let mut name = filename.trim().to_string();
    if Path::new(&name).extension().is_none() {
        let ext = extension_lower(input_path);
        if !ext.is_empty() {
            name = format!("{name}.{ext}");
        }
    }

    let named = Path::new(&name);
    if named.is_absolute() {
        return named.to_path_buf();
    }
    let parent = Path::new(input_path).parent().unwrap_or(Path::new(""));
    let dir = if !parent.as_os_str().is_empty() && parent.is_dir() {
        parent.to_path_buf()
    } else {
        vad_data_dir().join("clips")
    };
    let _ = std::fs::create_dir_all(&dir);
    dir.join(named)
}

/// Builds the secure `ffmpeg` command per PLANO_VAD.md §4.26 and §8.
/// - Argument vector (`Command::args`), never shell.
/// - Sanitized paths (`sanitize_path`).
/// - Option delimiter `--` before the arbitrary user-specified output path.
/// - `-n`: ffmpeg itself refuses to overwrite (defence in depth next to `validate_options`).
pub fn build_ffmpeg_clip_command(options: &ClipExportOptions) -> Command {
    let safe_input = sanitize_path(&options.input_path);
    let safe_output = sanitize_path(&options.output_path);

    let start_str = format_timestamp(options.start_seconds);
    let end_str = format_timestamp(options.end_seconds);

    let mut cmd = Command::new("ffmpeg");

    // Standard safety and non-interactive flags
    cmd.args(["-nostdin", "-hide_banner", "-loglevel", "error", "-n"]);

    // Timestamps placed before input for fast seek (§8)
    cmd.args(["-ss", &start_str, "-to", &end_str]);

    // Input file
    cmd.args(["-i", &safe_input]);

    // FLAC is always re-encoded: a `-c copy` cut keeps the source STREAMINFO, so the clip plays
    // its real length but is reported with the original file's full duration (measured: a 3 s
    // cut out of 6 s read back as 6.01 s). Re-encoding FLAC is lossless, so nothing is lost.
    let output_ext = extension_lower(&safe_output);
    if options.exact_cut || output_ext == "flac" {
        // Frame-accurate re-encode with codecs the destination container accepts (§8)
        cmd.args(recode_flags(&safe_output));
    } else {
        // Fast keyframe stream copy: -c copy -avoid_negative_ts 1 (§8)
        cmd.args(["-c", "copy"]);
        if is_audio_only_container(&output_ext) {
            cmd.arg("-vn");
        }
    }

    cmd.args(["-avoid_negative_ts", "1"]);

    // Terminate options with `--` before user output path (§4.26)
    cmd.arg("--");
    cmd.arg(&safe_output);

    cmd.stdout(Stdio::null());
    cmd.stderr(Stdio::piped());

    cmd
}

/// How the ffmpeg child ended.
enum Outcome {
    Cancelled,
    Exited(ExitStatus),
    WaitFailed(std::io::Error),
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

        // From here on the output path is known not to exist (validate_options), so removing it
        // after a failure/cancel only ever removes what this export created.
        if let Err(err) = validate_options(&options) {
            warn!("Clip export validation failed: {:?}", err);
            // the variant's own Display says "Audio extraction failed: ..." — show just the reason
            let reason = match err {
                VadError::ExtractionFailed(reason) => reason,
                other => other.to_string(),
            };
            let _ = tx.send(ClipExportStatus::Failed(reason));
            return;
        }

        let _ = tx.send(ClipExportStatus::Starting);

        let mut cmd = build_ffmpeg_clip_command(&options);
        debug!("Spawning FFmpeg clip export: {:?}", cmd);

        let mut child = match cmd.spawn() {
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

        let stderr_reader = child.stderr.take().map(|mut pipe| {
            thread::spawn(move || {
                let mut text = String::new();
                let _ = pipe.read_to_string(&mut text);
                text
            })
        });

        let outcome = loop {
            if thread_handle.is_cancelled() {
                // Owner-side kill also covers a cancel that landed before the pid was published.
                thread_handle.clear_pid();
                let _ = child.kill();
                let _ = child.wait();
                break Outcome::Cancelled;
            }
            match thread_handle.try_reap(&mut child) {
                Ok(Some(status)) => break Outcome::Exited(status),
                Ok(None) => thread::sleep(Duration::from_millis(25)),
                Err(e) => {
                    thread_handle.clear_pid();
                    let _ = child.kill();
                    let _ = child.wait();
                    break Outcome::WaitFailed(e);
                }
            }
        };

        let stderr_msg = stderr_reader
            .and_then(|t| t.join().ok())
            .unwrap_or_default()
            .trim()
            .to_string();

        let status = match outcome {
            Outcome::Cancelled => {
                info!("Clip export was cancelled by user");
                let _ = std::fs::remove_file(&options.output_path);
                let _ = tx.send(ClipExportStatus::Cancelled);
                return;
            }
            Outcome::WaitFailed(e) => {
                let _ = std::fs::remove_file(&options.output_path);
                let _ = tx.send(ClipExportStatus::Failed(format!(
                    "Erro ao aguardar pelo FFmpeg: {e}"
                )));
                return;
            }
            Outcome::Exited(status) => status,
        };

        // A cancel that arrived while the child was finishing on its own still wins.
        if thread_handle.is_cancelled() {
            let _ = std::fs::remove_file(&options.output_path);
            let _ = tx.send(ClipExportStatus::Cancelled);
            return;
        }

        if status.success() {
            let out_path = PathBuf::from(&options.output_path);
            // A fast cut starts on the previous keyframe, so the file is longer than requested:
            // report what was really written (the requested length is only the fallback).
            let duration_seconds =
                probe_duration(&options.output_path).unwrap_or(duration_seconds);
            info!(
                "Clip exported successfully to {:?} ({:.2}s)",
                out_path, duration_seconds
            );
            let _ = tx.send(ClipExportStatus::Completed {
                output_path: out_path,
                duration_seconds,
            });
        } else {
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
            let _ = tx.send(ClipExportStatus::Failed(err_text));
        }
    });

    (rx, handle)
}

/// Duration in seconds of a media file as reported by ffprobe (`None` if it cannot be read).
pub fn probe_duration(path: &str) -> Option<f64> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-show_entries", "format=duration", "-of", "csv=p=0"])
        .args(["-i", &sanitize_path(path)])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .ok()
        .filter(|d| d.is_finite() && *d >= 0.0)
}

/// Parses `ffprobe -show_entries packet=pts_time,flags -of csv=p=0` output into the sorted
/// presentation times of the keyframe packets (flags contain `K`).
pub fn parse_keyframe_lines(text: &str) -> Vec<f64> {
    let mut times: Vec<f64> = text
        .lines()
        .filter_map(|line| {
            let (pts, flags) = line.trim().split_once(',')?;
            if !flags.contains('K') {
                return None;
            }
            pts.parse::<f64>().ok().filter(|t| t.is_finite() && *t >= 0.0)
        })
        .collect();
    times.sort_by(|a, b| a.total_cmp(b));
    times.dedup();
    times
}

/// Keyframe times of the first video stream (packet scan, no decoding). Empty for audio-only
/// files, where a stream copy cuts at packet boundaries and there are no keyframes to show.
pub fn probe_keyframes(input_path: &str) -> Result<Vec<f64>, VadError> {
    let output = Command::new("ffprobe")
        .args(["-v", "error", "-select_streams", "v:0"])
        .args(["-show_entries", "packet=pts_time,flags", "-of", "csv=p=0"])
        .args(["-i", &sanitize_path(input_path)])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .map_err(|e| VadError::ExtractionFailed(format!("Falha ao executar ffprobe: {e}")))?;
    if !output.status.success() {
        return Err(VadError::ExtractionFailed(
            "ffprobe não conseguiu ler o ficheiro".to_string(),
        ));
    }
    Ok(parse_keyframe_lines(&String::from_utf8_lossy(&output.stdout)))
}

/// Answer of [`probe_keyframes_async`]: the probed path and the keyframes found in it.
pub type KeyframeProbe = (String, Result<Vec<f64>, VadError>);

/// Runs [`probe_keyframes`] on a worker thread; the receiver yields the path it was started for
/// together with the result, so the caller can drop answers for media it has since left.
pub fn probe_keyframes_async(input_path: String) -> Receiver<KeyframeProbe> {
    let (tx, rx) = unbounded();
    thread::spawn(move || {
        let result = probe_keyframes(&input_path);
        let _ = tx.send((input_path, result));
    });
    rx
}

/// Where a fast (`-c copy`) cut starting at `time` really starts: the last keyframe at or
/// before it (ffmpeg seeks back to a keyframe so the copied stream stays decodable).
pub fn keyframe_at_or_before(keyframes: &[f64], time: f64) -> Option<f64> {
    // Tolerance for pts rounding in the ffprobe output.
    let idx = keyframes.partition_point(|&k| k <= time + 0.001);
    idx.checked_sub(1).map(|i| keyframes[i])
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A private scratch directory under the system temp dir, removed on drop.
    struct TempDir(PathBuf);

    impl TempDir {
        fn new(tag: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("vad_clip_{tag}_{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }

        fn path(&self, name: &str) -> String {
            self.0.join(name).to_string_lossy().to_string()
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn ffmpeg(args: &[&str]) {
        let status = Command::new("ffmpeg")
            .args(["-nostdin", "-hide_banner", "-loglevel", "error", "-y"])
            .args(args)
            .status()
            .expect("FFmpeg must be available for test");
        assert!(status.success(), "ffmpeg {args:?} failed");
    }

    fn duration_of(path: &str) -> f64 {
        probe_duration(path).unwrap_or_else(|| panic!("ffprobe could not read {path}"))
    }

    fn has_stream(path: &str, kind: &str) -> bool {
        let out = Command::new("ffprobe")
            .args(["-v", "error", "-select_streams", kind, "-show_entries", "stream=codec_type", "-of", "csv=p=0", path])
            .output()
            .unwrap();
        !out.stdout.is_empty()
    }

    fn run_export(opts: ClipExportOptions) -> ClipExportStatus {
        let (rx, _handle) = export_clip_async(opts);
        while let Ok(status) = rx.recv() {
            match status {
                ClipExportStatus::Starting => continue,
                other => return other,
            }
        }
        panic!("export thread ended without a final status");
    }

    fn opts(input: &str, output: &str, start: f64, end: f64, exact: bool) -> ClipExportOptions {
        ClipExportOptions {
            input_path: input.to_string(),
            output_path: output.to_string(),
            start_seconds: start,
            end_seconds: end,
            exact_cut: exact,
        }
    }

    #[test]
    fn test_format_timestamp() {
        assert_eq!(format_timestamp(0.0), "00:00:00.000");
        assert_eq!(format_timestamp(5.25), "00:00:05.250");
        assert_eq!(format_timestamp(65.5), "00:01:05.500");
        assert_eq!(format_timestamp(3661.125), "01:01:01.125");
    }

    #[test]
    fn test_format_timestamp_rounding_carries_into_seconds() {
        // Used to print "00:00:05.1000" (ffmpeg reads that as 5.1 s).
        assert_eq!(format_timestamp(5.9996), "00:00:06.000");
        assert_eq!(format_timestamp(59.9999), "00:01:00.000");
        assert_eq!(format_timestamp(3599.9996), "01:00:00.000");
    }

    #[test]
    fn test_sanitize_path() {
        assert_eq!(sanitize_path("-foo.mp4"), "./-foo.mp4");
        assert_eq!(sanitize_path("./-foo.mp4"), "./-foo.mp4");
        assert_eq!(sanitize_path("/tmp/-foo.mp4"), "/tmp/-foo.mp4");
        assert_eq!(sanitize_path("video.mp4"), "video.mp4");
    }

    fn args_of(cmd: &Command) -> Vec<String> {
        cmd.get_args().map(|a| a.to_string_lossy().to_string()).collect()
    }

    #[test]
    fn test_build_ffmpeg_clip_command_copy_mode() {
        let cmd = build_ffmpeg_clip_command(&opts("-input.mp4", "-output.mp4", 10.0, 25.0, false));
        let args = args_of(&cmd);

        // Must pass -c copy for fast keyframe cut (§8)
        let c_idx = args.iter().position(|a| a == "-c").expect("Must contain -c");
        assert_eq!(args[c_idx + 1], "copy");
        assert!(args.contains(&"-avoid_negative_ts".to_string()));
        assert!(!args.contains(&"libx264".to_string()));
        assert!(args.contains(&"-n".to_string()) && !args.contains(&"-y".to_string()));

        // Must sanitize paths and end with -- before output (§4.26)
        let double_dash_idx = args.iter().position(|a| a == "--").expect("Must contain --");
        assert_eq!(args[double_dash_idx + 1], "./-output.mp4");
        assert_eq!(double_dash_idx + 2, args.len());

        let input_idx = args.iter().position(|a| a == "-i").expect("Must contain -i");
        assert_eq!(args[input_idx + 1], "./-input.mp4");
    }

    #[test]
    fn test_build_ffmpeg_clip_command_exact_mode() {
        let args = args_of(&build_ffmpeg_clip_command(&opts("input.mp4", "output.mp4", 5.0, 15.0, true)));

        // Must re-encode with libx264 and aac (§8)
        for expected in ["libx264", "-crf", "18", "aac"] {
            assert!(args.contains(&expected.to_string()), "missing {expected}: {args:?}");
        }
        assert!(!args.contains(&"copy".to_string()));
    }

    #[test]
    fn test_recode_flags_follow_the_destination_container() {
        let v = |p: &str| recode_flags(p).join(" ");
        assert_eq!(v("a.mp4"), "-c:v libx264 -crf 18 -c:a aac");
        assert_eq!(v("a.MKV"), "-c:v libx264 -crf 18 -c:a aac");
        assert_eq!(v("a"), "-c:v libx264 -crf 18 -c:a aac");
        assert_eq!(v("a.webm"), "-c:v libvpx-vp9 -crf 24 -b:v 0 -c:a libopus");
        assert_eq!(v("a.opus"), "-vn -c:a libopus");
        assert_eq!(v("a.mp3"), "-vn -c:a libmp3lame");
        assert_eq!(v("a.wav"), "-vn -c:a pcm_s16le");
        assert_eq!(v("a.flac"), "-vn -c:a flac");
        assert_eq!(v("a.ogg"), "-vn -c:a libvorbis");
        assert_eq!(v("a.m4a"), "-vn -c:a aac");
    }

    #[test]
    fn test_copy_to_audio_container_drops_video() {
        let args = args_of(&build_ffmpeg_clip_command(&opts("in.mp4", "out.mp3", 1.0, 2.0, false)));
        assert!(args.contains(&"-vn".to_string()));
        let args = args_of(&build_ffmpeg_clip_command(&opts("in.mp4", "out.mp4", 1.0, 2.0, false)));
        assert!(!args.contains(&"-vn".to_string()));
    }

    #[test]
    fn test_validate_options() {
        let dir = TempDir::new("validate");
        let valid_file = dir.path("existing.tmp");
        std::fs::File::create(&valid_file).unwrap();

        let ok_opts = opts(&valid_file, &dir.path("clip.mp4"), 2.0, 10.0, false);
        assert!(validate_options(&ok_opts).is_ok());

        assert!(validate_options(&opts(&valid_file, &dir.path("clip.mp4"), 10.0, 2.0, false)).is_err());
        assert!(validate_options(&opts(&valid_file, &dir.path("clip.mp4"), f64::NAN, 5.0, false)).is_err());
        assert!(validate_options(&opts(&valid_file, &dir.path("clip.mp4"), 1.0, f64::INFINITY, false)).is_err());
        assert!(validate_options(&opts(&dir.path("missing.mp4"), &dir.path("clip.mp4"), 0.0, 5.0, false)).is_err());
    }

    #[test]
    fn test_validate_rejects_output_that_exists_or_is_the_input() {
        let dir = TempDir::new("overwrite");
        let input = dir.path("in.mp4");
        std::fs::write(&input, b"original").unwrap();
        let other = dir.path("other.mp4");
        std::fs::write(&other, b"keep me").unwrap();

        let same = validate_options(&opts(&input, &input, 0.0, 1.0, false)).unwrap_err().to_string();
        assert!(same.contains("próprio ficheiro de entrada"), "{same}");
        let exists = validate_options(&opts(&input, &other, 0.0, 1.0, false)).unwrap_err().to_string();
        assert!(exists.contains("Já existe"), "{exists}");
    }

    #[test]
    fn test_failed_export_never_deletes_the_original_or_an_existing_file() {
        let dir = TempDir::new("nodelete");
        let input = dir.path("in.mp4");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=duration=2:size=160x120:rate=25", "-f", "lavfi", "-i", "sine=duration=2", "-c:v", "libx264", "-c:a", "aac", "--", &input]);
        let before = std::fs::read(&input).unwrap();
        let existing = dir.path("existing.mp4");
        std::fs::write(&existing, b"precious").unwrap();

        // output == input: rejected up front, the source stays byte-identical.
        assert!(matches!(run_export(opts(&input, &input, 0.5, 1.5, false)), ClipExportStatus::Failed(_)));
        assert_eq!(std::fs::read(&input).unwrap(), before);
        // output already exists: rejected, untouched.
        assert!(matches!(run_export(opts(&input, &existing, 0.5, 1.5, false)), ClipExportStatus::Failed(_)));
        assert_eq!(std::fs::read(&existing).unwrap(), b"precious");
    }

    #[test]
    fn test_resolve_output_path() {
        let dir = TempDir::new("resolve");
        let input = dir.path("talk.opus");
        assert_eq!(resolve_output_path(&input, "clip.opus"), PathBuf::from(dir.path("clip.opus")));
        // no extension: the input's is added so ffmpeg can choose the muxer
        assert_eq!(resolve_output_path(&input, "clip"), PathBuf::from(dir.path("clip.opus")));
        assert_eq!(resolve_output_path(&input, "/x/y.mp3"), PathBuf::from("/x/y.mp3"));
        // surrounding blanks are ignored
        assert_eq!(resolve_output_path(&input, "  a.mp3 "), PathBuf::from(dir.path("a.mp3")));
    }

    #[test]
    fn test_parse_keyframe_lines() {
        let text = "0.000000,K__\n0.040000,___\n2.000000,K__\nN/A,K__\n2.000000,K__\n1.000000,K_\n";
        assert_eq!(parse_keyframe_lines(text), vec![0.0, 1.0, 2.0]);
        assert!(parse_keyframe_lines("").is_empty());
    }

    #[test]
    fn test_keyframe_at_or_before() {
        let kf = [0.0, 2.0, 4.0, 6.0];
        assert_eq!(keyframe_at_or_before(&kf, 3.0), Some(2.0));
        assert_eq!(keyframe_at_or_before(&kf, 4.0), Some(4.0));
        assert_eq!(keyframe_at_or_before(&kf, 0.0), Some(0.0));
        assert_eq!(keyframe_at_or_before(&kf, 99.0), Some(6.0));
        assert_eq!(keyframe_at_or_before(&[], 1.0), None);
        assert_eq!(keyframe_at_or_before(&[5.0], 1.0), None);
    }

    /// Real keyframes (GOP = 2 s) and what a `-c copy` cut really does with them, so the hint the
    /// panel shows ("começa no keyframe anterior") is not a guess.
    #[test]
    fn test_real_keyframes_and_fast_cut_snap_to_previous_keyframe() {
        let dir = TempDir::new("keyframes");
        let src = dir.path("src.mp4");
        ffmpeg(&[
            "-f", "lavfi", "-i", "testsrc2=duration=8:size=160x120:rate=25", "-f", "lavfi", "-i", "sine=duration=8",
            "-c:v", "libx264", "-g", "50", "-keyint_min", "50", "-sc_threshold", "0", "-c:a", "aac", "--", &src,
        ]);

        let kf = probe_keyframes(&src).expect("ffprobe");
        assert_eq!(kf, vec![0.0, 2.0, 4.0, 6.0], "keyframes of the generated file");

        // fast cut of 3.0..7.0 starts at the keyframe 2.0, so it is ~5 s long, not 4 s
        let out = dir.path("fast.mp4");
        let reported = match run_export(opts(&src, &out, 3.0, 7.0, false)) {
            ClipExportStatus::Completed { duration_seconds, .. } => duration_seconds,
            other => panic!("fast export failed: {other:?}"),
        };
        let fast = duration_of(&out);
        assert!((fast - 5.0).abs() < 0.3, "fast cut duration {fast}");
        // the completion message must carry the real length, not the requested 4 s
        assert!((reported - fast).abs() < 0.01, "reported {reported} vs file {fast}");
        assert_eq!(keyframe_at_or_before(&kf, 3.0), Some(2.0));
    }

    /// Both cut modes into every container the panel can propose (same extension as the input),
    /// checking the duration and the streams of the result — not only that ffmpeg exited 0.
    #[test]
    fn test_export_matrix_all_containers_both_modes() {
        let dir = TempDir::new("matrix");
        let mp4 = dir.path("src.mp4");
        ffmpeg(&[
            "-f", "lavfi", "-i", "testsrc2=duration=6:size=160x120:rate=25", "-f", "lavfi", "-i", "sine=frequency=440:duration=6",
            "-c:v", "libx264", "-g", "25", "-c:a", "aac", "--", &mp4,
        ]);

        let mut sources = vec![("mp4".to_string(), mp4.clone())];
        for (ext, extra) in [
            ("mkv", vec!["-c:v", "libx264", "-g", "25", "-c:a", "aac"]),
            ("webm", vec!["-c:v", "libvpx-vp9", "-g", "25", "-b:v", "0", "-crf", "40", "-c:a", "libopus"]),
            ("mp3", vec!["-vn", "-c:a", "libmp3lame"]),
            ("flac", vec!["-vn", "-c:a", "flac"]),
            ("ogg", vec!["-vn", "-c:a", "libvorbis"]),
            ("opus", vec!["-vn", "-c:a", "libopus"]),
            ("wav", vec!["-vn", "-c:a", "pcm_s16le"]),
            ("m4a", vec!["-vn", "-c:a", "aac"]),
        ] {
            let path = dir.path(&format!("src.{ext}"));
            let mut args = vec!["-i", mp4.as_str()];
            args.extend(extra);
            args.push("--");
            args.push(&path);
            ffmpeg(&args);
            sources.push((ext.to_string(), path));
        }

        for (ext, src) in &sources {
            for exact in [false, true] {
                let out = dir.path(&format!("out_{}_{ext}.{ext}", if exact { "exact" } else { "fast" }));
                match run_export(opts(src, &out, 1.5, 4.5, exact)) {
                    ClipExportStatus::Completed { .. } => {}
                    other => panic!("{ext} exact={exact}: {other:?}"),
                }
                let dur = duration_of(&out);
                // fast = snaps to the previous keyframe / Ogg page (up to ~1 s with the 1 s GOP above)
                let tolerance = if exact || ext == "flac" { 0.35 } else { 1.1 };
                assert!(
                    (dur - 3.0).abs() < tolerance,
                    "{ext} exact={exact}: expected ~3.0 s, got {dur}"
                );
                assert!(has_stream(&out, "a:0"), "{ext} exact={exact}: no audio stream");
                assert_eq!(has_stream(&out, "v:0"), has_stream(src, "v:0"), "{ext} exact={exact}: video stream mismatch");
            }
        }
    }

    #[test]
    fn test_video_to_audio_container_drops_video_in_both_modes() {
        let dir = TempDir::new("audioout");
        let src = dir.path("src.mp4");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=duration=4:size=160x120:rate=25", "-f", "lavfi", "-i", "sine=duration=4", "-c:v", "libx264", "-c:a", "libmp3lame", "--", &src]);
        for exact in [false, true] {
            let out = dir.path(&format!("only_audio_{exact}.mp3"));
            match run_export(opts(&src, &out, 1.0, 3.0, exact)) {
                ClipExportStatus::Completed { .. } => {}
                other => panic!("exact={exact}: {other:?}"),
            }
            assert!(!has_stream(&out, "v:0"), "exact={exact}");
            assert!(has_stream(&out, "a:0"), "exact={exact}");
        }
    }

    #[test]
    fn test_cancel_kills_ffmpeg_and_removes_partial_output() {
        let dir = TempDir::new("cancel");
        let src = dir.path("big.mp4");
        // Long enough that the exact re-encode is still running when we cancel.
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=duration=120:size=1280x720:rate=30", "-c:v", "libx264", "-preset", "ultrafast", "--", &src]);
        let out = dir.path("cut.mp4");

        let (rx, handle) = export_clip_async(opts(&src, &out, 0.0, 110.0, true));
        assert_eq!(rx.recv().unwrap(), ClipExportStatus::Starting);
        thread::sleep(Duration::from_millis(400));
        handle.cancel();

        let started = std::time::Instant::now();
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), ClipExportStatus::Cancelled);
        assert!(started.elapsed() < Duration::from_secs(3), "cancel must not wait for the encode");
        assert!(!Path::new(&out).exists(), "partial output must be removed");
    }

    #[test]
    fn test_cancel_before_spawn_is_honoured() {
        let dir = TempDir::new("cancel_early");
        let src = dir.path("in.mp4");
        ffmpeg(&["-f", "lavfi", "-i", "testsrc2=duration=2:size=160x120:rate=25", "-c:v", "libx264", "--", &src]);
        let out = dir.path("cut.mp4");

        // Cancelled right after the call returns, i.e. possibly before the worker published the
        // pid: the worker itself must still kill the child and clean up.
        let (rx, handle) = export_clip_async(opts(&src, &out, 0.0, 1.5, true));
        handle.cancel();
        let last = loop {
            match rx.recv_timeout(Duration::from_secs(10)).unwrap() {
                ClipExportStatus::Starting => continue,
                other => break other,
            }
        };
        assert_eq!(last, ClipExportStatus::Cancelled);
        assert!(!Path::new(&out).exists());
    }
}
