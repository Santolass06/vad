//! Anthropic Messages API client (PLANO_VAD.md §4.2): one thin POST, no SDK.

use std::sync::atomic::AtomicBool;
use std::time::Duration;

use serde_json::json;
use vad_core::VadError;

use crate::llm_provider::{summary_prompts, translate_prompts, AiPrivacyBadge, Summarizer};
use crate::secrets::Secret;

pub const ANTHROPIC_BASE_URL: &str = "https://api.anthropic.com";
const API_VERSION: &str = "2023-06-01";
const CONTEXT_WINDOW: usize = 150_000;
const MAX_OUTPUT_TOKENS: usize = 1_024;

pub struct AnthropicSummarizer {
    messages_url: String,
    model: String,
    api_key: Secret,
    client: reqwest::blocking::Client,
    retry: crate::http::RetryPolicy,
}

impl std::fmt::Debug for AnthropicSummarizer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnthropicSummarizer").field("model", &self.model).field("api_key", &self.api_key).finish()
    }
}

impl AnthropicSummarizer {
    pub fn new(model: &str, api_key: Secret) -> Result<Self, VadError> {
        Self::with_base_url(ANTHROPIC_BASE_URL, model, api_key, crate::http::REQUEST_TIMEOUT)
    }

    /// The host is fixed in production; tests point it at a local server.
    pub(crate) fn with_base_url(base: &str, model: &str, api_key: Secret, timeout: Duration) -> Result<Self, VadError> {
        if model.trim().is_empty() {
            return Err(VadError::Llm("modelo Anthropic não definido nas Definições → IA".into()));
        }
        if api_key.expose().trim().is_empty() {
            return Err(VadError::LlmAuthFailed("sem chave Anthropic configurada".into()));
        }
        Ok(Self {
            messages_url: format!("{}/v1/messages", base.trim_end_matches('/')),
            model: model.trim().to_string(),
            api_key,
            client: crate::http::client(timeout)?,
            retry: crate::http::RetryPolicy::DEFAULT,
        })
    }

    fn call(&self, system: &str, user: &str, max_tokens: usize, abort: Option<&AtomicBool>) -> Result<String, VadError> {
        let req = self
            .client
            .post(&self.messages_url)
            .header("x-api-key", self.api_key.expose())
            .header("anthropic-version", API_VERSION)
            .json(&json!({
                "model": self.model,
                "max_tokens": max_tokens,
                "system": system,
                "messages": [{"role": "user", "content": user}],
            }));
        let body = crate::http::execute(req, abort, self.retry)?;
        // content is a list of blocks; join the text ones.
        let text: String = body["content"]
            .as_array()
            .map(|blocks| blocks.iter().filter_map(|b| b["text"].as_str()).collect::<Vec<_>>().join(""))
            .unwrap_or_default();
        crate::http::non_empty(Some(&text).filter(|t| !t.is_empty()).map(|t| t.as_str()), "content[].text")
    }

    /// Smallest real round-trip, for "Testar ligação" (§4.23).
    pub fn ping(&self) -> Result<(), VadError> {
        self.without_retry().call("Responde apenas com a palavra: ok", "ping", 8, None).map(|_| ())
    }
}

impl AnthropicSummarizer {
    /// Test hook / "Testar ligação": single attempt, or a custom policy.
    #[allow(dead_code)]
    pub(crate) fn with_retry(mut self, retry: crate::http::RetryPolicy) -> Self {
        self.retry = retry;
        self
    }

    fn without_retry(&self) -> Self {
        Self {
            retry: crate::http::RetryPolicy::NONE,
            messages_url: self.messages_url.clone(), model: self.model.clone(), api_key: self.api_key.clone(), client: self.client.clone(),
        }
    }
}

impl Summarizer for AnthropicSummarizer {
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
pub(crate) mod tests {
    use super::*;
    use crate::test_http::serve;
    fn serve_root(status: &'static str, body: &'static str) -> (String, std::sync::Arc<std::sync::Mutex<Vec<String>>>) {
        let (b, seen) = serve(status, body);
        (b.trim_end_matches("/v1").to_string(), seen)
    }

    #[test]
    fn sends_key_in_header_and_joins_text_blocks() {
        let (base, seen) = serve_root("200 OK", r#"{"content":[{"type":"text","text":"Bom "},{"type":"text","text":"dia."}]}"#);
        let s = AnthropicSummarizer::with_base_url(&base, "claude-x", Secret::new("sk-ant-1"), Duration::from_secs(5)).unwrap();
        assert_eq!(s.translate_text("Hello", "Inglês", "Português", None).unwrap(), "Bom dia.");
        let req = seen.lock().unwrap()[0].to_lowercase();
        assert!(req.starts_with("post /v1/messages"), "{req}");
        assert!(req.contains("x-api-key: sk-ant-1") && req.contains("anthropic-version: 2023-06-01"));
        assert!(!req.contains("authorization"), "Anthropic uses x-api-key, not bearer");
        assert_eq!(s.privacy_badge(), AiPrivacyBadge::Cloud);
    }

    #[test]
    fn errors_map_and_never_leak_the_key() {
        let (b, _) = serve_root("401 Unauthorized", r#"{"error":{"message":"invalid x-api-key sk-ant-1"}}"#);
        let e = AnthropicSummarizer::with_base_url(&b, "m", Secret::new("sk-ant-1"), Duration::from_secs(5)).unwrap().ping().unwrap_err();
        assert!(matches!(e, VadError::LlmAuthFailed(_)) && !format!("{e:?}").contains("sk-ant"), "{e:?}");
        let (b, _) = serve_root("429 Too Many Requests", "{}");
        assert!(matches!(AnthropicSummarizer::with_base_url(&b, "m", Secret::new("k"), Duration::from_secs(5)).unwrap().ping(), Err(VadError::LlmRateLimited(_))));
        let (b, _) = serve_root("200 OK", r#"{"content":[]}"#);
        assert!(matches!(AnthropicSummarizer::with_base_url(&b, "m", Secret::new("k"), Duration::from_secs(5)).unwrap().ping(), Err(VadError::Llm(_))));
    }

    #[test]
    fn rejects_missing_model_or_key_up_front() {
        assert!(AnthropicSummarizer::new(" ", Secret::new("k")).is_err());
        assert!(matches!(AnthropicSummarizer::new("m", Secret::new("  ")), Err(VadError::LlmAuthFailed(_))));
        assert!(!format!("{:?}", AnthropicSummarizer::new("m", Secret::new("sk-hidden")).unwrap()).contains("sk-hidden"));
    }
}
