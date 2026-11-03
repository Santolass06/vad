//! Sprint_17 — `PRIVACY.md` is the single source of the allowed outbound hosts (§10.7).
//!
//! The list lives in a machine-readable block at the end of `PRIVACY.md`. These tests fail if
//! any non-test source file names a network host that is not in it, and if a listed host is no
//! longer used anywhere (so the document cannot silently drift from the code).

use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

fn root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn allowlist() -> BTreeSet<String> {
    let doc = std::fs::read_to_string(root().join("PRIVACY.md")).expect("PRIVACY.md");
    let block = doc
        .split("egress-allowlist-begin")
        .nth(1)
        .and_then(|r| r.split("egress-allowlist-end").next())
        .expect("allowlist block in PRIVACY.md");
    block.lines().map(str::trim).filter(|l| !l.is_empty()).map(String::from).collect()
}

fn sources(dir: &Path, out: &mut Vec<PathBuf>) {
    for e in std::fs::read_dir(dir).unwrap().flatten() {
        let p = e.path();
        if p.is_dir() {
            sources(&p, out);
        } else if p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// Hosts named by `http(s)://` literals in production code (everything before `#[cfg(test)]`,
/// comments excluded, loopback/private placeholders excluded).
fn hosts_in_code() -> BTreeSet<String> {
    let mut files = Vec::new();
    sources(&root().join("crates"), &mut files);
    let mut hosts = BTreeSet::new();
    for f in files {
        let src = std::fs::read_to_string(&f).unwrap();
        let code = src.split("#[cfg(test)]").next().unwrap_or(&src);
        for line in code.lines() {
            let t = line.trim_start();
            if t.starts_with("//") {
                continue;
            }
            let mut rest = line;
            while let Some(i) = rest.find("http") {
                rest = &rest[i + 4..];
                let after = rest.strip_prefix("s://").or_else(|| rest.strip_prefix("://"));
                let Some(after) = after else { continue };
                let host: String = after.chars().take_while(|c| c.is_ascii_alphanumeric() || *c == '.' || *c == '-').collect();
                if host.is_empty() || host == "..." || host.starts_with("127.") || host == "localhost" || host.ends_with(".invalid") {
                    continue;
                }
                hosts.insert(host);
            }
        }
    }
    hosts
}

#[test]
fn every_host_in_the_code_is_declared_in_privacy_md() {
    let allowed = allowlist();
    let undeclared: Vec<_> = hosts_in_code().into_iter().filter(|h| !allowed.contains(h)).collect();
    assert!(
        undeclared.is_empty(),
        "hosts used in code but not declared in PRIVACY.md (add them to the table AND the list): {undeclared:?}"
    );
}

#[test]
fn every_declared_host_is_actually_used() {
    let used = hosts_in_code();
    let stale: Vec<_> = allowlist().into_iter().filter(|h| !used.contains(h)).collect();
    assert!(stale.is_empty(), "declared in PRIVACY.md but unused in code: {stale:?}");
}

#[test]
fn there_is_no_telemetry_dependency() {
    let lock = std::fs::read_to_string(root().join("Cargo.lock")).unwrap();
    for bad in ["sentry", "opentelemetry", "segment", "posthog", "mixpanel", "google-analytics", "datadog", "bugsnag", "rollbar"] {
        assert!(!lock.contains(&format!("name = \"{bad}")), "telemetry-like crate in Cargo.lock: {bad}");
    }
}

#[test]
fn privacy_md_documents_the_cloud_opt_in_and_the_keys_rule() {
    let doc = std::fs::read_to_string(root().join("PRIVACY.md")).unwrap();
    for must in ["telemetria", "Nunca chaves de API", "Só se escolheres", "🔒 Local", "☁️ Sai do PC"] {
        assert!(doc.contains(must), "PRIVACY.md lost: {must}");
    }
}

/// Sprint_17 criterion 2: `THIRD_PARTY_NOTICES.md` must match the current `Cargo.lock`. This is
/// the cheap half of `scripts/gen_third_party_notices.py --check` (which also regenerates the
/// text): if someone changes dependencies and forgets to regenerate, `cargo test` fails.
#[test]
fn third_party_notices_header_matches_the_current_cargo_lock() {
    use sha2::{Digest, Sha256};
    let lock = std::fs::read(root().join("Cargo.lock")).unwrap();
    let want = format!("{:x}", Sha256::digest(&lock));
    let notices = std::fs::read_to_string(root().join("THIRD_PARTY_NOTICES.md")).expect("THIRD_PARTY_NOTICES.md");
    assert!(
        notices.contains(&format!("Cargo.lock sha256: `{want}`")),
        "THIRD_PARTY_NOTICES.md is stale vs Cargo.lock — run scripts/gen_third_party_notices.py"
    );
    assert!(notices.contains("epaint_default_fonts"), "must cover the fonts egui embeds");
}
