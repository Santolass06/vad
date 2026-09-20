use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use crate::error::VadError;

static ATOMIC_WRITE_COUNTER: AtomicU64 = AtomicU64::new(1);

/// Returns the primary configuration directory for VAD (`~/.config/vad` or `$XDG_CONFIG_HOME/vad`).
pub fn vad_config_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_CONFIG_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("vad");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home).join(".config").join("vad");
        }
    }

    PathBuf::from(".config").join("vad")
}

/// Returns the dedicated mpv isolated configuration directory (`~/.config/vad/mpv`).
/// This folder isolates mpv from the user's personal `~/.config/mpv` (§3).
pub fn vad_mpv_config_dir() -> PathBuf {
    vad_config_dir().join("mpv")
}

/// Returns the path to `recentes.json` (`~/.config/vad/recentes.json`, §4.6).
pub fn vad_recentes_path() -> PathBuf {
    vad_config_dir().join("recentes.json")
}

/// Returns the path to `config.toml` (`~/.config/vad/config.toml`, §5).
pub fn vad_config_path() -> PathBuf {
    vad_config_dir().join("config.toml")
}

/// Returns the models directory for VAD (`~/.local/share/vad/models` or `$XDG_DATA_HOME/vad/models`, §4.13).
pub fn vad_models_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        if !xdg.trim().is_empty() {
            return PathBuf::from(xdg).join("vad").join("models");
        }
    }

    if let Ok(home) = std::env::var("HOME") {
        if !home.trim().is_empty() {
            return PathBuf::from(home)
                .join(".local")
                .join("share")
                .join("vad")
                .join("models");
        }
    }

    PathBuf::from(".local")
        .join("share")
        .join("vad")
        .join("models")
}

/// Validates a URL against the allowlisted schemes per PLANO_VAD.md §4.27.
/// Only `http://`, `https://`, and `rtsp://` are permitted.
/// Rejects `file://`, `smb://`, `ftp://`, and other arbitrary schemes.
pub fn is_allowed_url_scheme(input: &str) -> bool {
    let trimmed = input.trim().as_bytes();
    const ALLOWED_SCHEMES: [&str; 3] = ["http://", "https://", "rtsp://"];
    // Compare bytes: slicing the `str` would panic when the cut lands inside a multi-byte char.
    ALLOWED_SCHEMES.iter().any(|scheme| {
        trimmed
            .get(..scheme.len())
            .is_some_and(|prefix| prefix.eq_ignore_ascii_case(scheme.as_bytes()))
    })
}

/// Atomic file writer adhering to PLANO_VAD.md §4.30.
/// Writes to a temporary file (`.tmp`) in the *same* directory as `target_path`,
/// forces synchronization to disk via `sync_all()`, and applies `std::fs::rename`
/// which is atomic on POSIX filesystems.
pub fn write_atomic(target_path: &Path, data: &[u8]) -> Result<(), VadError> {
    let parent = target_path
        .parent()
        .unwrap_or_else(|| Path::new("."));

    // Ensure directory exists
    fs::create_dir_all(parent).map_err(VadError::Io)?;

    let filename = target_path
        .file_name()
        .and_then(|f| f.to_str())
        .unwrap_or("file");

    let pid = std::process::id();
    let counter = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
    let now_nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);

    let temp_name = format!(".{filename}.{pid}.{now_nanos}.{counter}.tmp");
    let temp_path = parent.join(temp_name);

    // Write file in temp location
    let write_res = (|| -> Result<(), std::io::Error> {
        let mut file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)?;

        file.write_all(data)?;
        file.sync_all()?;
        drop(file);

        fs::rename(&temp_path, target_path)?;
        Ok(())
    })();

    if let Err(e) = write_res {
        let _ = fs::remove_file(&temp_path);
        return Err(VadError::Io(e));
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_url_scheme_allowlist() {
        assert!(is_allowed_url_scheme("http://example.com/video.mp4"));
        assert!(is_allowed_url_scheme("https://www.youtube.com/watch?v=12345"));
        assert!(is_allowed_url_scheme("HTTP://EXAMPLE.COM/STREAM"));
        assert!(is_allowed_url_scheme("rtsp://192.168.1.100:554/live"));
        assert!(is_allowed_url_scheme("  https://twitch.tv/stream  "));

        // Prohibited schemes (§4.27)
        assert!(!is_allowed_url_scheme("file:///etc/shadow"));
        assert!(!is_allowed_url_scheme("smb://nas.local/share/movie.mkv"));
        assert!(!is_allowed_url_scheme("ftp://ftp.example.com/file.mp4"));
        assert!(!is_allowed_url_scheme("javascript:alert(1)"));
        assert!(!is_allowed_url_scheme("/local/path/to/movie.mp4"));
        assert!(!is_allowed_url_scheme(""));

        // Multi-byte input must be rejected, not panic on a non-char-boundary slice
        assert!(!is_allowed_url_scheme("éééé.mp4"));
        assert!(!is_allowed_url_scheme("日本語日本語://x"));
    }

    #[test]
    fn test_write_atomic_creates_and_overwrites() {
        let dir = tempfile_compat_dir("test_write_atomic");
        let target = dir.join("test_file.txt");

        write_atomic(&target, b"initial content").expect("Atomic write failed");
        assert_eq!(fs::read_to_string(&target).unwrap(), "initial content");

        write_atomic(&target, b"updated content").expect("Atomic overwrite failed");
        assert_eq!(fs::read_to_string(&target).unwrap(), "updated content");

        // Clean up
        let _ = fs::remove_dir_all(&dir);
    }

    fn tempfile_compat_dir(prefix: &str) -> PathBuf {
        let pid = std::process::id();
        let counter = ATOMIC_WRITE_COUNTER.fetch_add(1, Ordering::Relaxed);
        let path = std::env::temp_dir().join(format!("vad_unit_{prefix}_{pid}_{counter}"));
        let _ = fs::create_dir_all(&path);
        path
    }
}
