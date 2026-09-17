use std::ffi::OsStr;
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
    is_executable_in_path_list(binary, &paths)
}

/// Core search logic, parameterized on the path list so it can be tested without
/// mutating the process-global `$PATH` (which would race with other tests in this
/// binary running on other threads).
fn is_executable_in_path_list(binary: &str, paths: &OsStr) -> bool {
    for dir in std::env::split_paths(paths) {
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
        // Exercises the same lookup `probe_dependencies` relies on, but against an
        // explicit bogus path list instead of mutating the process-global `PATH`
        // (which would race with other tests reading it on other threads — Sprint_02 review).
        let bogus_path = OsStr::new("/tmp/non_existent_vad_test_path_9999");
        assert!(!is_executable_in_path_list("ffmpeg", bogus_path));
        assert!(!is_executable_in_path_list("yt-dlp", bogus_path));
    }
}
