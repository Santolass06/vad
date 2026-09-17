use std::path::Path;
use vad_core::VadError;

#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

/// Checks if a named binary exists in any directory listed in `$PATH` and is executable.
/// Searches directly without invoking any shell (`sh -c`) to prevent command injection (§4.26).
pub fn is_executable_in_path(binary: &str) -> bool {
    let Some(paths) = std::env::var_os("PATH") else {
        return false;
    };

    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(binary);
        if is_file_executable(&candidate) {
            return true;
        }
    }
    false
}

fn is_file_executable(path: &Path) -> bool {
    let Ok(metadata) = path.metadata() else {
        return false;
    };

    if !metadata.is_file() {
        return false;
    }

    #[cfg(unix)]
    {
        metadata.permissions().mode() & 0o111 != 0
    }

    #[cfg(not(unix))]
    {
        true
    }
}

/// Probes the host system for external runtime dependencies (`ffmpeg` and `yt-dlp`).
/// Returns a list of `VadError` for any missing dependencies so that the application
/// can degrade graciosamente via the error table in `vad-core/src/error.rs` (§4.14).
pub fn probe_dependencies() -> Vec<VadError> {
    let mut missing = Vec::new();

    if !is_executable_in_path("ffmpeg") {
        missing.push(VadError::FfmpegNotFound);
    }

    if !is_executable_in_path("yt-dlp") {
        missing.push(VadError::YtDlpNotFound);
    }

    missing
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_executable_in_path_common_bins() {
        // sh or ls should always be in PATH on Unix systems
        #[cfg(unix)]
        assert!(is_executable_in_path("sh"));

        // A non-existent binary must return false
        assert!(!is_executable_in_path("non_existent_binary_xyz_12345"));
    }

    #[test]
    fn test_probe_dependencies_missing_detection() {
        let prev_path = std::env::var_os("PATH");
        std::env::set_var("PATH", "/tmp/non_existent_vad_test_path_9999");
        let missing = probe_dependencies();
        if let Some(ref p) = prev_path {
            std::env::set_var("PATH", p);
        }
        assert!(missing.iter().any(|e| matches!(e, VadError::FfmpegNotFound)));
        assert!(missing.iter().any(|e| matches!(e, VadError::YtDlpNotFound)));
    }
}
