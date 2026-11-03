//! OpenAI-compatible chat-completions client (PLANO_VAD.md §4.2).
//!
//! One implementation covers OpenAI, Ollama, OpenRouter and any self-hosted server that
//! speaks `POST {base_url}/chat/completions`. It is synchronous (blocking `reqwest`) and
//! must run on a worker thread, never inside the egui `update()`.

use std::sync::atomic::AtomicBool;

use serde_json::json;
use url::Url;
use vad_core::VadError;

use crate::endpoint::{self, validate_base_url};
use crate::llm_provider::{summary_prompts, translate_prompts, AiPrivacyBadge, Summarizer};
use crate::secrets::Secret;

pub const DEFAULT_CLOUD_CONTEXT_WINDOW: usize = 16_000;
pub const DEFAULT_CLOUD_MAX_OUTPUT_TOKENS: usize = 1_024;

pub struct OpenAiCompatibleSummarizer {
    chat_url: String,
    model: String,
    api_key: Option<Secret>,
    badge: AiPrivacyBadge,
    context_window: usize,
    max_output_tokens: usize,
    client: reqwest::blocking::Client,
    retry: crate::http::RetryPolicy,
}

impl std::fmt::Debug for OpenAiCompatibleSummarizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiCompatibleSummarizer")
            .field("chat_url", &self.chat_url)
            .field("model", &self.model)
            .field("api_key", &self.api_key) // Secret redacts itself
            .finish()
    }
}

impl OpenAiCompatibleSummarizer {
    /// `base_url` is validated here again: the config on disk is not trusted. `api_key` is
    /// optional because Ollama and most self-hosted servers need none.
    pub fn new(base_url: &str, model: &str, api_key: Option<Secret>) -> Result<Self, VadError> {
        let url: Url = validate_base_url(base_url)?;
        if model.trim().is_empty() {
            return Err(VadError::Llm("modelo não definido nas Definições → IA".into()));
        }
        let client = crate::http::client(crate::http::REQUEST_TIMEOUT)?;
        Ok(Self {
            chat_url: format!("{}/chat/completions", url.as_str().trim_end_matches('/')),
            model: model.trim().to_string(),
            api_key,
            badge: if endpoint::is_loopback(&url) { AiPrivacyBadge::Local } else { AiPrivacyBadge::Cloud },
            context_window: DEFAULT_CLOUD_CONTEXT_WINDOW,
            max_output_tokens: DEFAULT_CLOUD_MAX_OUTPUT_TOKENS,
            client,
            retry: crate::http::RetryPolicy::DEFAULT,
        })
    }

    fn chat(&self, system: &str, user: &str, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let mut req = self.client.post(&self.chat_url).json(&json!({
            "model": self.model,
            "messages": [
                {"role": "system", "content": system},
                {"role": "user", "content": user},
            ],
            "max_tokens": self.max_output_tokens,
            "temperature": 0.2,
        }));
        if let Some(key) = &self.api_key {
            req = req.bearer_auth(key.expose());
        }
        let body = crate::http::execute(req, abort, self.retry)?;
        crate::http::non_empty(body["choices"][0]["message"]["content"].as_str(), "choices[0].message.content")
    }

    /// Cheapest possible real round-trip, for "Testar ligação" (§4.23).
    pub fn ping(&self) -> Result<(), VadError> {
        self.without_retry().chat("Responde apenas com a palavra: ok", "ping", None).map(|_| ())
    }
}

impl OpenAiCompatibleSummarizer {
    /// Test hook / "Testar ligação": single attempt, or a custom policy.
    #[allow(dead_code)]
    pub(crate) fn with_retry(mut self, retry: crate::http::RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    fn without_retry(&self) -> Self {
        Self {
            retry: crate::http::RetryPolicy::NONE,
            chat_url: self.chat_url.clone(), model: self.model.clone(), api_key: self.api_key.clone(), badge: self.badge, context_window: self.context_window, max_output_tokens: self.max_output_tokens, client: self.client.clone(),
        }
    }
}

impl Summarizer for OpenAiCompatibleSummarizer {
    fn context_window(&self) -> usize {
        self.context_window
    }
    fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }
    fn privacy_badge(&self) -> AiPrivacyBadge {
        self.badge
    }
    fn summarize_chunk(&self, chunk: &str, is_final_reduce: bool, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let (system, user) = summary_prompts(chunk, is_final_reduce);
        self.chat(system, &user, abort)
    }
    fn translate_text(&self, text: &str, source_lang: &str, target_lang: &str, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let (system, user) = translate_prompts(text, source_lang, target_lang);
        self.chat(&system, &user, abort)
    }
}

/// Builds the summarizer selected in `[ai]`. Blocking (keyring lookup): call from a worker.
///
/// A non-official endpoint must have been confirmed by the user: the Sprint_10 quality
/// gate validated `LocalQwen`, not whatever model sits behind a custom URL (§4.1).
pub fn build_from_config(
    ai: &vad_core::config::AiConfig,
    secrets: &crate::secrets::SecretStore,
) -> Result<OpenAiCompatibleSummarizer, VadError> {
    use crate::secrets::SecretSlot;
    let url = validate_base_url(&ai.openai_base_url)?;
    let official = endpoint::is_official_openai(&url);
    if !official && ai.custom_endpoint_acknowledged.as_deref() != Some(url.as_str()) {
        return Err(VadError::Llm(
            "endpoint personalizado por confirmar em Definições → IA (a qualidade não foi validada para este modelo)".into(),
        ));
    }
    let slot = if official { SecretSlot::OpenAi } else { SecretSlot::Custom };
    let key = secrets.resolve(slot).map(|(k, _)| k);
    // Official OpenAI always needs a key; custom/local servers may not.
    if official && key.is_none() {
        return Err(VadError::LlmAuthFailed("sem chave OpenAI configurada".into()));
    }
    OpenAiCompatibleSummarizer::new(url.as_str(), &ai.openai_model, key)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::serve;
    use std::sync::Arc;

    const OK_BODY: &str = r#"{"choices":[{"message":{"role":"assistant","content":" Good morning. "}}]}"#;

    #[test]
    fn translates_and_sends_key_model_and_prompts() {
        let (base, seen) = serve("200 OK", OK_BODY);
        let s = OpenAiCompatibleSummarizer::new(&base, "gpt-4o-mini", Some(Secret::new("sk-test-123"))).unwrap();
        assert_eq!(s.privacy_badge(), AiPrivacyBadge::Local, "loopback server");
        let out = s.translate_text("Bom dia.", "Português", "Inglês", None).unwrap();
        assert_eq!(out, "Good morning.");
        let req = seen.lock().unwrap()[0].clone();
        assert!(req.starts_with("POST /v1/chat/completions"), "{req}");
        assert!(req.to_lowercase().contains("authorization: bearer sk-test-123"));
        assert!(req.contains("\"model\":\"gpt-4o-mini\"") && req.contains("Bom dia."));
    }

    #[test]
    fn no_key_means_no_authorization_header() {
        let (base, seen) = serve("200 OK", OK_BODY);
        let s = OpenAiCompatibleSummarizer::new(&base, "llama3", None).unwrap();
        s.summarize_chunk("texto", false, None).unwrap();
        assert!(!seen.lock().unwrap()[0].to_lowercase().contains("authorization"));
    }

    #[test]
    fn maps_http_errors_without_leaking_the_key() {
        let (b401, _) = serve("401 Unauthorized", r#"{"error":{"message":"Incorrect API key provided: sk-test-123"}}"#);
        let s = OpenAiCompatibleSummarizer::new(&b401, "m", Some(Secret::new("sk-test-123"))).unwrap();
        let e = s.summarize_chunk("x", false, None).unwrap_err();
        assert!(matches!(e, VadError::LlmAuthFailed(_)), "{e:?}");
        assert!(!format!("{e:?}{e}").contains("sk-test"), "401 body must not be echoed");

        let (b429, _) = serve("429 Too Many Requests", "{}");
        let e = OpenAiCompatibleSummarizer::new(&b429, "m", None).unwrap().summarize_chunk("x", false, None).unwrap_err();
        assert!(matches!(e, VadError::LlmRateLimited(_)), "{e:?}");

        let (b500, _) = serve("500 Internal Server Error", "boom");
        let e = OpenAiCompatibleSummarizer::new(&b500, "m", None).unwrap().summarize_chunk("x", false, None).unwrap_err();
        assert!(matches!(e, VadError::Llm(_)), "{e:?}");
    }

    #[test]
    fn malformed_or_empty_responses_are_errors_not_empty_summaries() {
        for body in ["not json", r#"{"choices":[]}"#, r#"{"choices":[{"message":{"content":"  "}}]}"#] {
            let (base, _) = serve("200 OK", body);
            let r = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().summarize_chunk("x", false, None);
            assert!(matches!(r, Err(VadError::Llm(_))), "{body}: {r:?}");
        }
    }

    #[test]
    fn redirects_are_not_followed_so_the_key_cannot_hop_hosts() {
        let (base, _) = serve("302 Found", "");
        let s = OpenAiCompatibleSummarizer::new(&base, "m", Some(Secret::new("sk-x"))).unwrap();
        assert!(s.summarize_chunk("x", false, None).is_err());
    }

    #[test]
    fn connection_refused_is_a_clean_error_and_abort_is_honoured() {
        let s = OpenAiCompatibleSummarizer::new("http://127.0.0.1:1/v1", "m", Some(Secret::new("sk-x"))).unwrap();
        let e = s.summarize_chunk("x", false, None).unwrap_err();
        assert!(matches!(e, VadError::NoNetwork(_)) && !format!("{e}").contains("sk-x"), "{e:?}");
        let flag = AtomicBool::new(true);
        assert!(matches!(s.summarize_chunk("x", false, Some(&flag)), Err(VadError::LlmCancelled)));
    }

    #[test]
    fn constructor_rejects_bad_endpoint_and_missing_model_and_debug_hides_key() {
        assert!(OpenAiCompatibleSummarizer::new("http://169.254.169.254/v1", "m", None).is_err());
        assert!(OpenAiCompatibleSummarizer::new("file:///x", "m", None).is_err());
        assert!(OpenAiCompatibleSummarizer::new("https://api.openai.com/v1", "  ", None).is_err());
        let s = OpenAiCompatibleSummarizer::new("https://api.openai.com/v1", "m", Some(Secret::new("sk-hidden"))).unwrap();
        assert!(!format!("{s:?}").contains("sk-hidden"));
        assert_eq!(s.privacy_badge(), AiPrivacyBadge::Cloud);
    }

    #[test]
    fn build_from_config_enforces_key_and_custom_endpoint_confirmation() {
        use crate::secrets::{SecretBackend, SecretSlot, SecretStore};
        use vad_core::config::AiConfig;
        struct Dead;
        impl SecretBackend for Dead {
            fn get(&self, _: &str) -> Result<Option<String>, String> { Err("x".into()) }
            fn set(&self, _: &str, _: &str) -> Result<(), String> { Err("x".into()) }
            fn delete(&self, _: &str) -> Result<(), String> { Err("x".into()) }
        }
        let store = SecretStore::with(Box::new(Dead), Box::new(|_| None));
        let mut ai = AiConfig::default();
        // Official without key -> auth error, not a silent request.
        assert!(matches!(build_from_config(&ai, &store), Err(VadError::LlmAuthFailed(_))));
        store.save(SecretSlot::OpenAi, "sk-1");
        assert!(build_from_config(&ai, &store).is_ok());
        // Custom endpoint: refused until confirmed, and the confirmation is tied to the URL.
        ai.openai_base_url = "http://192.168.1.5:11434/v1".into();
        assert!(matches!(build_from_config(&ai, &store), Err(VadError::Llm(_))));
        ai.custom_endpoint_acknowledged = Some("http://192.168.1.5:11434/v1".into());
        let s = build_from_config(&ai, &store).expect("confirmed custom endpoint, no key needed");
        assert_eq!(s.privacy_badge(), AiPrivacyBadge::Cloud, "LAN address leaves the PC");
        ai.openai_base_url = "http://192.168.1.6:11434/v1".into();
        assert!(build_from_config(&ai, &store).is_err(), "changing the URL drops the confirmation");
        ai.openai_base_url = "http://169.254.169.254/v1".into();
        assert!(build_from_config(&ai, &store).is_err());
    }

    #[test]
    fn exported_summary_badge_follows_the_provider_not_a_constant() {
        use crate::summarizer::MapReduceSummarizer;
        use crate::whisper::TranscriptionSegment;
        let (base, _) = serve("200 OK", OK_BODY);
        let local = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap();
        let segs = [TranscriptionSegment { start_ms: 0, end_ms: 1000, text: "Bom dia.".into() }];
        let sum = MapReduceSummarizer::new(Arc::new(local)).summarize(&segs, None::<fn(crate::summarizer::SummarizeProgress)>, None).unwrap();
        assert_eq!(sum.privacy_badge, AiPrivacyBadge::Local, "loopback server");
        // Non-loopback endpoint: construction does no I/O, so no server is needed.
        let cloud = OpenAiCompatibleSummarizer::new("http://10.1.2.3:8080/v1", "m", None).unwrap();
        assert_eq!(cloud.privacy_badge(), AiPrivacyBadge::Cloud);
    }

    // ---- Sprint_13: retry with backoff+jitter ----
    use crate::http::RetryPolicy;
    use crate::test_http::serve_seq;
    use std::time::{Duration, Instant};

    const FAST: RetryPolicy = RetryPolicy { max_retries: 2, base: Duration::from_millis(20), cap: Duration::from_millis(200) };

    #[test]
    fn retries_429_and_5xx_then_succeeds() {
        let (base, seen) = serve_seq(&[("429 Too Many Requests", "{}", ""), ("503 Service Unavailable", "x", ""), ("200 OK", OK_BODY, "")]);
        let s = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(FAST);
        assert_eq!(s.summarize_chunk("x", false, None).unwrap(), "Good morning.");
        assert_eq!(seen.lock().unwrap().len(), 3, "1 attempt + 2 retries");
    }

    #[test]
    fn gives_up_after_max_retries_with_the_real_error() {
        let (base, seen) = serve_seq(&[("429 Too Many Requests", "{}", "")]);
        let s = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(FAST);
        assert!(matches!(s.summarize_chunk("x", false, None), Err(VadError::LlmRateLimited(_))));
        assert_eq!(seen.lock().unwrap().len(), 3);
    }

    #[test]
    fn auth_errors_and_other_4xx_and_offline_are_never_retried() {
        for (st, kind) in [("401 Unauthorized", 0), ("400 Bad Request", 1)] {
            let (base, seen) = serve_seq(&[(st, "{}", "")]);
            let r = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(FAST).summarize_chunk("x", false, None);
            assert!(r.is_err(), "{kind}");
            assert_eq!(seen.lock().unwrap().len(), 1, "{st} must not be retried");
        }
        let t = Instant::now();
        let r = OpenAiCompatibleSummarizer::new("http://127.0.0.1:1/v1", "m", None).unwrap().with_retry(FAST).summarize_chunk("x", false, None);
        assert!(matches!(r, Err(VadError::NoNetwork(_))));
        assert!(t.elapsed() < Duration::from_millis(500), "offline goes straight to fallback, no backoff");
    }

    #[test]
    fn retry_after_header_is_honoured_and_capped() {
        let (base, seen) = serve_seq(&[("429 Too Many Requests", "{}", "Retry-After: 1\r\n"), ("200 OK", OK_BODY, "")]);
        let policy = RetryPolicy { max_retries: 1, base: Duration::from_millis(10), cap: Duration::from_millis(400) };
        let t = Instant::now();
        OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(policy).summarize_chunk("x", false, None).unwrap();
        let waited = t.elapsed();
        assert_eq!(seen.lock().unwrap().len(), 2);
        assert!(waited >= Duration::from_millis(300) && waited < Duration::from_millis(900), "Retry-After 1s capped to 400ms, waited {waited:?}");
    }

    #[test]
    fn cancel_during_backoff_is_prompt() {
        let (base, _) = serve_seq(&[("503 Service Unavailable", "x", "Retry-After: 30\r\n")]);
        let policy = RetryPolicy { max_retries: 2, base: Duration::from_secs(5), cap: Duration::from_secs(30) };
        let flag = Arc::new(std::sync::atomic::AtomicBool::new(false));
        let f2 = Arc::clone(&flag);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(300));
            f2.store(true, std::sync::atomic::Ordering::Relaxed);
        });
        let t = Instant::now();
        let r = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(policy).summarize_chunk("x", false, Some(&flag));
        assert!(matches!(r, Err(VadError::LlmCancelled)), "{r:?}");
        assert!(t.elapsed() < Duration::from_secs(2));
    }

    #[test]
    fn ping_is_a_single_attempt_so_a_429_is_shown_not_hidden() {
        let (base, seen) = serve_seq(&[("429 Too Many Requests", "{}", "")]);
        let s = OpenAiCompatibleSummarizer::new(&base, "m", None).unwrap().with_retry(FAST);
        assert!(matches!(s.ping(), Err(VadError::LlmRateLimited(_))));
        assert_eq!(seen.lock().unwrap().len(), 1);
    }
}
