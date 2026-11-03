//! Exposes the git commit to the About dialog (`VAD_GIT_COMMIT`), with a `-dirty` suffix when the
//! tracked files differ from HEAD, so the "source" pointer shown to users is never a lie. Falls
//! back to "desconhecido" outside a git checkout (e.g. a source tarball); the build never fails
//! because of it.
use std::path::PathBuf;
use std::process::Command;

fn git(args: &[&str]) -> Option<String> {
    let out = Command::new("git").args(args).output().ok().filter(|o| o.status.success())?;
    Some(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

fn main() {
    let commit = git(&["rev-parse", "--short=12", "HEAD"]).filter(|s| !s.is_empty());
    let value = match commit {
        Some(c) => {
            let dirty = git(&["status", "--porcelain", "--untracked-files=no"]).is_some_and(|s| !s.is_empty());
            if dirty { format!("{c}-dirty") } else { c }
        }
        None => "desconhecido".to_string(),
    };
    println!("cargo:rustc-env=VAD_GIT_COMMIT={value}");

    // HEAD alone does not change when committing on a branch: watch what it points to, the
    // packed refs and the index too.
    if let Some(git_dir) = git(&["rev-parse", "--git-dir"]).map(PathBuf::from) {
        let root = std::env::var("CARGO_MANIFEST_DIR").map(PathBuf::from).unwrap_or_default();
        let abs = |p: PathBuf| if p.is_absolute() { p } else { root.join(p) };
        let git_dir = abs(git_dir);
        println!("cargo:rerun-if-changed={}", git_dir.join("HEAD").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("packed-refs").display());
        println!("cargo:rerun-if-changed={}", git_dir.join("index").display());
        if let Some(r) = git(&["symbolic-ref", "-q", "HEAD"]) {
            println!("cargo:rerun-if-changed={}", git_dir.join(r).display());
        }
    }
}
