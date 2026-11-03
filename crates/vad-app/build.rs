//! Exposes the git commit to the About dialog (`VAD_GIT_COMMIT`). Falls back to "desconhecido"
//! outside a git checkout (e.g. a source tarball), so the build never fails because of it.
fn main() {
    let commit = std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| "desconhecido".to_string());
    println!("cargo:rustc-env=VAD_GIT_COMMIT={commit}");
    println!("cargo:rerun-if-changed=../../.git/HEAD");
}
