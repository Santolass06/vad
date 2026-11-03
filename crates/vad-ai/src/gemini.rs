//! Google Gemini `generateContent` client (PLANO_VAD.md §4.2): one thin POST, no SDK.
//!
//! The key goes in the `x-goog-api-key` header, **never** in `?key=`: a URL ends up in
//! error messages and logs, a header does not.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;
use vad_core::VadError;

use crate::llm_provider::{summary_prompts, translate_prompts, AiPrivacyBadge, Summarizer};
use crate::secrets::Secret;

pub const GEMINI_BASE_URL: &str = "https://generativelanguage.googleapis.com";
const CONTEXT_WINDOW: usize = 200_000;
const MAX_OUTPUT_TOKENS: usize = 1_024;

pub struct GeminiSummarizer {
    url: String,
    model: String,
    api_key: Secret,
    client: reqwest::blocking::Client,
    retry: crate::http::RetryPolicy,
}

impl std::fmt::Debug for GeminiSummarizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GeminiSummarizer").field("model", &self.model).field("api_key", &self.api_key).finish()
    }
}

/// Model ids go into the URL path: allow only what real ids contain.
fn valid_model_id(m: &str) -> bool {
    !m.is_empty() && m.len() <= 80 && m.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_'))
}

impl GeminiSummarizer {
    pub fn new(model: &str, api_key: Secret) -> Result<Self, VadError> {
        Self::with_base_url(GEMINI_BASE_URL, model, api_key, crate::http::REQUEST_TIMEOUT)
    }

    pub(crate) fn with_base_url(base: &str, model: &str, api_key: Secret, timeout: Duration) -> Result<Self, VadError> {
        let model = model.trim();
        if !valid_model_id(model) {
            return Err(VadError::Llm("modelo Gemini inválido nas Definições → IA".into()));
        }
        if api_key.expose().trim().is_empty() {
            return Err(VadError::LlmAuthFailed("sem chave Gemini configurada".into()));
        }
        Ok(Self {
            url: format!("{}/v1beta/models/{model}:generateContent", base.trim_end_matches('/')),
            model: model.to_string(),
            api_key,
            client: crate::http::client(timeout)?,
            retry: crate::http::RetryPolicy::DEFAULT,
        })
    }

    fn call(&self, system: &str, user: &str, max_tokens: usize, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let req = self
            .client
            .post(&self.url)
            .header("x-goog-api-key", self.api_key.expose())
            .json(&json!({
                "systemInstruction": {"parts": [{"text": system}]},
                "contents": [{"role": "user", "parts": [{"text": user}]}],
                "generationConfig": {"maxOutputTokens": max_tokens, "temperature": 0.2},
            }));
        let body = crate::http::execute(req, abort, self.retry)?;
        let text: String = body["candidates"][0]["content"]["parts"]
            .as_array()
            .map(|p| p.iter().filter_map(|x| x["text"].as_str()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        // A blocked prompt returns candidates without content: surface it, not an empty summary.
        crate::http::non_empty(Some(&text).filter(|t| !t.is_empty()).map(|t| t.as_str()), "candidates[0].content.parts[].text")
    }

    pub fn ping(&self) -> Result<(), VadError> {
        self.without_retry().call("Responde apenas com a palavra: ok", "ping", 8, None).map(|_| ())
    }
}

impl GeminiSummarizer {
    /// Test hook / "Testar ligação": single attempt, or a custom policy.
    #[allow(dead_code)]
    pub(crate) fn with_retry(mut self, retry: crate::http::RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    fn without_retry(&self) -> Self {
        Self {
            retry: crate::http::RetryPolicy::NONE,
            url: self.url.clone(), model: self.model.clone(), api_key: self.api_key.clone(), client: self.client.clone(),
        }
    }
}

impl Summarizer for GeminiSummarizer {
    fn context_window(&self) -> usize {
        CONTEXT_WINDOW
    }
    fn max_output_tokens(&self) -> usize {
        MAX_OUTPUT_TOKENS
    }
    fn privacy_badge(&self) -> AiPrivacyBadge {
        AiPrivacyBadge::Cloud
    }
    fn summarize_chunk(&self, chunk: &str, is_final_reduce: bool, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let (system, user) = summary_prompts(chunk, is_final_reduce);
        self.call(system, &user, MAX_OUTPUT_TOKENS, abort)
    }
    fn translate_text(&self, text: &str, source_lang: &str, target_lang: &str, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let (system, user) = translate_prompts(text, source_lang, target_lang);
        self.call(&system, &user, MAX_OUTPUT_TOKENS, abort)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::serve;
    fn serve_root(status: &'static str, body: &'static str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let (b, seen) = serve(status, body);
        (b.trim_end_matches("/v1").to_string(), seen)
    }

    #[test]
    fn key_goes_in_header_not_in_the_url() {
        let (base, seen) = serve_root("200 OK", r#"{"candidates":[{"content":{"parts":[{"text":"Good "},{"text":"morning."}]}}]}"#);
        let s = GeminiSummarizer::with_base_url(&base, "gemini-2.0-flash", Secret::new("AIza-secret"), Duration::from_secs(5)).unwrap();
        assert_eq!(s.translate_text("Bom dia.", "Português", "Inglês", None).unwrap(), "Good morning.");
        let req = seen.lock().unwrap()[0].clone();
        let first_line = req.lines().next().unwrap();
        assert!(first_line.starts_with("POST /v1beta/models/gemini-2.0-flash:generateContent"), "{first_line}");
        assert!(!first_line.contains("AIza") && !first_line.contains("key="), "key must not be in the URL: {first_line}");
        assert!(req.to_lowercase().contains("x-goog-api-key: aiza-secret"));
    }

    #[test]
    fn blocked_prompt_and_http_errors_are_errors() {
        let (b, _) = serve_root("200 OK", r#"{"promptFeedback":{"blockReason":"SAFETY"},"candidates":[]}"#);
        assert!(matches!(GeminiSummarizer::with_base_url(&b, "m", Secret::new("k"), Duration::from_secs(5)).unwrap().ping(), Err(VadError::Llm(_))));
        let (b, _) = serve_root("403 Forbidden", r#"{"error":{"message":"API key not valid AIza-secret"}}"#);
        let e = GeminiSummarizer::with_base_url(&b, "m", Secret::new("AIza-secret"), Duration::from_secs(5)).unwrap().ping().unwrap_err();
        assert!(matches!(e, VadError::LlmAuthFailed(_)) && !format!("{e:?}").contains("AIza"), "{e:?}");
        let (b, _) = serve_root("404 Not Found", "{}");
        let e = GeminiSummarizer::with_base_url(&b, "nope", Secret::new("k"), Duration::from_secs(5)).unwrap().ping().unwrap_err();
        assert!(e.to_string().contains("404"), "{e}");
    }

    #[test]
    fn model_id_cannot_inject_into_the_url_path() {
        for bad in ["", "a/b", "m?x=1", "m:generateContent#", "../v1", "a b"] {
            assert!(GeminiSummarizer::new(bad, Secret::new("k")).is_err(), "{bad}");
        }
        assert!(GeminiSummarizer::new("gemini-2.0-flash", Secret::new("k")).is_ok());
        assert!(matches!(GeminiSummarizer::new("m", Secret::new(" ")), Err(VadError::LlmAuthFailed(_))));
    }
}
