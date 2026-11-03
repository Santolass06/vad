//! Sprint_14 — M5b acceptance tests (PLANO_VAD.md §10.6 and §10.7).
//!
//! Plan wording says `wiremock`; these use the in-repo `test_http` server instead (same idea:
//! a scripted local HTTP server, no extra dependency).

use std::io::Write;
use std::sync::atomic::AtomicBool;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

use vad_core::VadError;

use crate::anthropic::AnthropicSummarizer;
use crate::fallback::run_with_local_fallback;
use crate::gemini::GeminiSummarizer;
use crate::http::RetryPolicy;
use crate::llm_provider::{AiPrivacyBadge, MockSummarizer, Summarizer};
use crate::openai_compatible::OpenAiCompatibleSummarizer;
use crate::secrets::Secret;
use crate::test_http::serve_seq;

const FAST: RetryPolicy = RetryPolicy { max_retries: 2, base: Duration::from_millis(10), cap: Duration::from_millis(50) };
const KEY: &str = "sk-LEAKCANARY-9f3a71c2d4e8";
const OK_OPENAI: &str = r#"{"choices":[{"message":{"content":"ok"}}]}"#;
const OK_ANTHROPIC: &str = r#"{"content":[{"type":"text","text":"ok"}]}"#;
const OK_GEMINI: &str = r#"{"candidates":[{"content":{"parts":[{"text":"ok"}]}}]}"#;

type Script = Vec<(&'static str, &'static str, &'static str)>;

/// Builds each provider against `base` (the test server) with fast retries and `timeout`.
fn providers(base: &str, timeout: Duration) -> Vec<(&'static str, Arc<dyn Summarizer>)> {
    let root = base.trim_end_matches("/v1").to_string();
    vec![
        (
            "openai",
            Arc::new(OpenAiCompatibleSummarizer::new(base, "m", Some(Secret::new(KEY))).unwrap().with_timeout(timeout).with_retry(FAST)),
        ),
        (
            "anthropic",
            Arc::new(AnthropicSummarizer::with_base_url(&root, "m", Secret::new(KEY), timeout).unwrap().with_retry(FAST)),
        ),
        (
            "gemini",
            Arc::new(GeminiSummarizer::with_base_url(&root, "m", Secret::new(KEY), timeout).unwrap().with_retry(FAST)),
        ),
    ]
}

fn local() -> Result<Arc<dyn Summarizer>, VadError> {
    Ok(Arc::new(MockSummarizer::default()))
}

fn op(s: Arc<dyn Summarizer>) -> Result<(String, AiPrivacyBadge), VadError> {
    Ok((s.summarize_chunk("texto", false, None)?, s.privacy_badge()))
}

#[test]
fn http_statuses_map_to_the_right_error_without_infinite_retry_or_silent_fallback() {
    // (script, expected attempts, error predicate)
    #[allow(clippy::type_complexity)]
    let cases: Vec<(&str, Script, usize, fn(&VadError) -> bool)> = vec![
        ("401", vec![("401 Unauthorized", "{}", "")], 1, |e| matches!(e, VadError::LlmAuthFailed(_))),
        ("403", vec![("403 Forbidden", "{}", "")], 1, |e| matches!(e, VadError::LlmAuthFailed(_))),
        ("404", vec![("404 Not Found", "{}", "")], 1, |e| matches!(e, VadError::Llm(m) if m.contains("404"))),
        ("429 forever", vec![("429 Too Many Requests", "{}", "")], 3, |e| matches!(e, VadError::LlmRateLimited(_))),
        ("500 forever", vec![("500 Internal Server Error", "boom", "")], 3, |e| matches!(e, VadError::Llm(_))),
        ("400 not retried", vec![("400 Bad Request", "bad", "")], 1, |e| matches!(e, VadError::Llm(_))),
    ];
    for (name, script, attempts, is_expected) in cases {
        for provider in ["openai", "anthropic", "gemini"] {
            let (base, seen) = serve_seq(&script);
            let (_, s) = providers(&base, Duration::from_secs(5)).into_iter().find(|(n, _)| *n == provider).unwrap();
            let err = run_with_local_fallback("à X", s, || panic!("{provider}/{name}: must not fall back"), op)
                .expect_err(name);
            assert!(is_expected(&err), "{provider}/{name}: {err:?}");
            assert_eq!(seen.lock().unwrap().len(), attempts, "{provider}/{name}: attempts are bounded");
        }
    }
}

#[test]
fn a_transient_429_then_success_is_invisible_to_the_user() {
    for (provider, ok) in [("openai", OK_OPENAI), ("anthropic", OK_ANTHROPIC), ("gemini", OK_GEMINI)] {
        let (base, seen) = serve_seq(&[("429 Too Many Requests", "{}", ""), ("200 OK", ok, "")]);
        let (_, s) = providers(&base, Duration::from_secs(5)).into_iter().find(|(n, _)| *n == provider).unwrap();
        let ((text, badge), note) = run_with_local_fallback("à X", s, || panic!("no fallback"), op).unwrap();
        // The OpenAI-compatible client pointed at loopback is honestly 🔒; the fixed-host
        // providers are always ☁️.
        let expected = if provider == "openai" { AiPrivacyBadge::Local } else { AiPrivacyBadge::Cloud };
        assert_eq!((text.as_str(), badge, note), ("ok", expected, None), "{provider}");
        assert_eq!(seen.lock().unwrap().len(), 2);
    }
}

#[test]
fn timeout_is_a_timeout_error_not_retried_and_not_a_silent_fallback() {
    for idx in 0..3 {
        // Accepts and never answers.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let held: Vec<_> = l.incoming().take(3).flatten().collect();
            std::thread::sleep(Duration::from_secs(5));
            drop(held);
        });
        let base = format!("http://127.0.0.1:{port}/v1");
        let (name, s) = providers(&base, Duration::from_millis(300)).remove(idx);
        let t = std::time::Instant::now();
        let err = run_with_local_fallback("à X", s, || panic!("{name}: timeout must not fall back"), op).unwrap_err();
        assert!(matches!(err, VadError::LlmTimeout(_)), "{name}: {err:?}");
        assert!(t.elapsed() < Duration::from_secs(3), "{name}: timeouts are not retried");
    }
}

#[test]
fn offline_falls_back_with_disclosure_for_every_provider() {
    for (name, s) in providers("http://127.0.0.1:1/v1", Duration::from_secs(2)) {
        let ((_, badge), note) = run_with_local_fallback("à X", s, local, op).unwrap();
        assert_eq!(badge, AiPrivacyBadge::Local, "{name}: result is the local model's");
        assert!(note.is_some_and(|n| n.contains("localmente") && n.contains("🔒")), "{name}: fallback must be disclosed");
    }
}

// ---- §10.7: anti-leak ----

#[derive(Clone, Default)]
struct Sink(Arc<Mutex<Vec<u8>>>);
impl Write for Sink {
    fn write(&mut self, b: &[u8]) -> std::io::Result<usize> {
        self.0.lock().unwrap().extend_from_slice(b);
        Ok(b.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for Sink {
    type Writer = Sink;
    fn make_writer(&'a self) -> Sink {
        self.clone()
    }
}

/// Process-wide capture of everything `tracing` emits (worker threads included, which a
/// thread-local subscriber would miss). The canary key is unique to this module.
fn log_capture() -> &'static Sink {
    static SINK: OnceLock<Sink> = OnceLock::new();
    SINK.get_or_init(|| {
        let sink = Sink::default();
        tracing::subscriber::set_global_default(
            tracing_subscriber::fmt().with_writer(sink.clone()).with_max_level(tracing::Level::TRACE).with_ansi(false).finish(),
        )
        .expect("no other test may install a global tracing subscriber");
        sink
    })
}

/// After authenticated calls through every provider — success, auth failure, retried 429, a
/// 500 whose body *echoes the key*, offline — the key appears in no log line, no error text
/// and not in the serialised config.
#[test]
fn the_api_key_never_reaches_logs_errors_or_config() {
    let sink = log_capture();
    let echo_500: &'static str = Box::leak(format!("upstream error for key {KEY} please retry").into_boxed_str());
    let echo_401: &'static str = Box::leak(format!(r#"{{"error":"Incorrect API key provided: {KEY}"}}"#).into_boxed_str());
    let mut surfaced = String::new();

    let scenarios: Vec<Script> = vec![
        vec![("200 OK", OK_OPENAI, "")],
        vec![("401 Unauthorized", echo_401, "")],
        vec![("429 Too Many Requests", echo_500, ""), ("200 OK", OK_OPENAI, "")],
        vec![("500 Internal Server Error", echo_500, "")],
    ];
    for script in scenarios {
        let (base, _) = serve_seq(&script);
        for (_, s) in providers(&base, Duration::from_secs(5)) {
            // Both entry points: summary and translation.
            let abort = AtomicBool::new(false);
            for r in [s.summarize_chunk("t", false, Some(&abort)), s.translate_text("t", "Português", "Inglês", Some(&abort))] {
                if let Err(e) = r {
                    surfaced.push_str(&format!("{e} | {e:?}\n"));
                }
            }
        }
    }
    for (_, s) in providers("http://127.0.0.1:1/v1", Duration::from_secs(1)) {
        if let Err(e) = s.summarize_chunk("t", false, None) {
            surfaced.push_str(&format!("{e} | {e:?}\n"));
        }
    }
    // Debug formatting of the clients themselves.
    for (_, s) in providers("http://127.0.0.1:1/v1", Duration::from_secs(1)) {
        let _ = s.context_window();
    }
    surfaced.push_str(&format!("{:?}", OpenAiCompatibleSummarizer::new("http://127.0.0.1:1/v1", "m", Some(Secret::new(KEY))).unwrap()));

    let log = String::from_utf8_lossy(&sink.0.lock().unwrap()).to_string();
    assert!(!log.is_empty(), "the capture must actually have seen log lines (retry warnings), or this test proves nothing");
    assert!(log.contains("nova tentativa"), "retry warnings were exercised");
    assert!(!log.contains(KEY) && !log.contains("LEAKCANARY"), "KEY LEAKED INTO LOGS:\n{log}");
    assert!(!surfaced.contains("LEAKCANARY"), "KEY LEAKED INTO AN ERROR MESSAGE:\n{surfaced}");

    // The config written to disk, with a cloud provider selected.
    let cfg = vad_core::VadConfig {
        ai: vad_core::config::AiConfig { provider: vad_core::config::AiProviderKind::Anthropic, ..Default::default() },
        ..Default::default()
    };
    let dir = std::env::temp_dir().join(format!("vad-leak-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    cfg.save_to_path(&dir.join("config.toml")).unwrap();
    let toml = std::fs::read_to_string(dir.join("config.toml")).unwrap();
    let _ = std::fs::remove_dir_all(dir);
    assert!(!toml.contains("LEAKCANARY") && !toml.contains("sk-") && !toml.contains("AIza"));
}

/// The grep of §10.7 as a reusable check: scans files on disk for key-shaped strings.
#[test]
fn key_shaped_strings_are_detected_by_the_scanner_used_on_real_files() {
    fn leaks(text: &str) -> bool {
        text.contains("sk-") || text.contains("sk-ant-") || text.contains("AIza") || text.to_lowercase().contains("bearer ")
    }
    assert!(leaks("api_key = \"sk-abc\"") && leaks("key AIzaSyXYZ") && leaks("Authorization: Bearer x"));
    assert!(!leaks("provider = \"anthropic\"\nanthropic_model = \"claude-haiku-5-5\""));
    // Whatever real files exist on this machine must be clean too.
    for path in [vad_core::vad_config_path()] {
        if let Ok(t) = std::fs::read_to_string(&path) {
            assert!(!leaks(&t), "{} contains a key-shaped string", path.display());
        }
    }
}
