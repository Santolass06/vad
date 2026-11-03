//! Cloud provider selection and the "Testar ligação" probe (PLANO_VAD.md §4.2, §4.23).

use std::sync::Arc;
use std::time::Instant;

use vad_core::config::{AiConfig, AiProviderKind};
use vad_core::VadError;

use crate::anthropic::AnthropicSummarizer;
use crate::gemini::GeminiSummarizer;
use crate::llm_provider::Summarizer;
use crate::openai_compatible::{build_from_config, OpenAiCompatibleSummarizer};
use crate::secrets::{Secret, SecretSlot, SecretStore};

/// Builds the selected cloud summarizer. Blocking (keyring): call from a worker.
/// `Local` is built by the app, which owns the model files.
pub fn build_cloud(ai: &AiConfig, secrets: &SecretStore) -> Result<Arc<dyn Summarizer>, VadError> {
    let key = |slot: SecretSlot, what: &str| {
        secrets
            .resolve(slot)
            .map(|(k, _)| k)
            .ok_or_else(|| VadError::LlmAuthFailed(format!("sem chave {what} configurada")))
    };
    match ai.provider {
        AiProviderKind::Local => Err(VadError::Llm("provider local não é cloud".into())),
        AiProviderKind::OpenaiCompatible => Ok(Arc::new(build_from_config(ai, secrets)?)),
        AiProviderKind::Anthropic => {
            Ok(Arc::new(AnthropicSummarizer::new(&ai.anthropic_model, key(SecretSlot::Anthropic, "Anthropic")?)?))
        }
        AiProviderKind::Gemini => Ok(Arc::new(GeminiSummarizer::new(&ai.gemini_model, key(SecretSlot::Gemini, "Gemini")?)?)),
    }
}

/// What the user has typed in a provider card, not yet saved.
#[derive(Debug, Clone)]
pub enum Draft {
    OpenAi { base_url: String, model: String },
    Anthropic { model: String },
    Gemini { model: String },
}

/// Probes the provider with a minimal real request using the **draft** values, so a wrong
/// `base_url`, key or model shows up here and not on the first summary. Nothing is
/// persisted. Returns the measured latency in ms. Blocking: run it on a worker thread.
///
/// A bad URL fails before any network traffic (`validate_base_url`).
pub fn test_connection(draft: &Draft, key: Option<Secret>) -> Result<u64, VadError> {
    let started = Instant::now();
    match draft {
        Draft::OpenAi { base_url, model } => {
            let url = crate::endpoint::validate_base_url(base_url)?;
            if crate::endpoint::is_official_openai(&url) && key.is_none() {
                return Err(VadError::LlmAuthFailed("falta a chave OpenAI".into()));
            }
            OpenAiCompatibleSummarizer::new(url.as_str(), model, key)?.ping()?
        }
        Draft::Anthropic { model } => {
            let key = key.ok_or_else(|| VadError::LlmAuthFailed("falta a chave Anthropic".into()))?;
            AnthropicSummarizer::new(model, key)?.ping()?
        }
        Draft::Gemini { model } => {
            let key = key.ok_or_else(|| VadError::LlmAuthFailed("falta a chave Gemini".into()))?;
            GeminiSummarizer::new(model, key)?.ping()?
        }
    }
    Ok(started.elapsed().as_millis() as u64)
}

/// One-line, user-facing diagnosis that tells the failure kinds apart (§4.23): never a
/// bare "falhou".
pub fn diagnose(e: &VadError) -> String {
    match e {
        VadError::LlmAuthFailed(_) => "Chave inválida ou sem permissão (HTTP 401/403).".into(),
        VadError::LlmRateLimited(_) => "O provider limitou os pedidos (HTTP 429): a chave funciona, tenta mais tarde.".into(),
        VadError::LlmTimeout(_) => "O provider não respondeu a tempo.".into(),
        VadError::NoNetwork(_) => "Sem ligação ao provider: verifica a rede e o endereço (sem resposta).".into(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_http::serve;

    const OK: &str = r#"{"choices":[{"message":{"content":"ok"}}]}"#;

    #[test]
    fn success_reports_latency_and_sends_the_draft_key_without_saving_anything() {
        let (base, seen) = serve("200 OK", OK);
        let ms = test_connection(&Draft::OpenAi { base_url: base, model: "m".into() }, Some(Secret::new("sk-draft"))).unwrap();
        assert!(ms < 5_000);
        assert!(seen.lock().unwrap()[0].to_lowercase().contains("bearer sk-draft"));
    }

    #[test]
    fn bad_url_is_caught_before_any_network_traffic() {
        for bad in ["http://169.254.169.254/v1", "file:///x", "nonsense"] {
            let e = test_connection(&Draft::OpenAi { base_url: bad.into(), model: "m".into() }, None).unwrap_err();
            assert!(matches!(e, VadError::Llm(_)), "{bad}: {e:?}");
        }
    }

    #[test]
    fn each_failure_has_a_distinct_diagnosis() {
        let (b401, _) = serve("401 Unauthorized", "{}");
        let (b404, _) = serve("404 Not Found", "{}");
        let (b429, _) = serve("429 Too Many", "{}");
        let d = |b: String| diagnose(&test_connection(&Draft::OpenAi { base_url: b, model: "m".into() }, Some(Secret::new("k"))).unwrap_err());
        let msgs = [d(b401), d(b404), d(b429), diagnose(&test_connection(&Draft::OpenAi { base_url: "http://127.0.0.1:1/v1".into(), model: "m".into() }, None).unwrap_err())];
        assert!(msgs[0].contains("Chave inválida"), "{msgs:?}");
        assert!(msgs[1].contains("404") && msgs[1].contains("modelo"), "{msgs:?}");
        assert!(msgs[2].contains("429"), "{msgs:?}");
        assert!(msgs[3].contains("Sem ligação"), "{msgs:?}");
        let unique: std::collections::HashSet<_> = msgs.iter().collect();
        assert_eq!(unique.len(), 4, "four different problems, four different messages");
    }

    #[test]
    fn cloud_providers_require_a_key() {
        for d in [Draft::Anthropic { model: "m".into() }, Draft::Gemini { model: "m".into() }, Draft::OpenAi { base_url: "https://api.openai.com/v1".into(), model: "m".into() }] {
            assert!(matches!(test_connection(&d, None), Err(VadError::LlmAuthFailed(_))), "{d:?}");
        }
    }

    #[test]
    fn abort_interrupts_a_hung_request() {
        use std::sync::atomic::{AtomicBool, Ordering};
        // Server accepts and never answers.
        let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = l.local_addr().unwrap().port();
        std::thread::spawn(move || {
            let _held: Vec<_> = l.incoming().take(1).flatten().collect();
            std::thread::sleep(std::time::Duration::from_secs(10));
        });
        let s = OpenAiCompatibleSummarizer::new(&format!("http://127.0.0.1:{port}/v1"), "m", None).unwrap();
        let flag = Arc::new(AtomicBool::new(false));
        let f2 = Arc::clone(&flag);
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(300));
            f2.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let r = s.summarize_chunk("x", false, Some(&flag));
        assert!(matches!(r, Err(VadError::LlmCancelled)), "{r:?}");
        assert!(t.elapsed().as_secs() < 2, "cancel took {:?}", t.elapsed());
    }
}
