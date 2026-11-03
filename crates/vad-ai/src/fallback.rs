//! Disclosed cloud→local fallback (PLANO_VAD.md §4.21).
//!
//! When the cloud provider cannot be reached at all (`NoNetwork`), the whole operation is
//! redone with the local model. The result carries a notice so it is never presented as if it
//! had come from the provider that was asked for; and since the local summarizer produced
//! it, its privacy badge is 🔒 Local. Only `NoNetwork` triggers this: a bad key, a rate limit
//! or a timeout are shown as errors, because swapping a strong cloud model for a 0.5B local
//! one silently changes the answer quality in ways the user must choose, not discover.

use std::sync::Arc;

use vad_core::VadError;

use crate::llm_provider::Summarizer;

/// Text shown next to a result that came from the fallback. `provider_name` carries its own
/// preposition ("ao endpoint…", "à Anthropic") so the sentence reads naturally.
pub fn fallback_notice(provider_name: &str) -> String {
    format!(
        "Sem ligação {provider_name}: este resultado foi produzido localmente pelo modelo Qwen 0.5B (🔒 Local), com qualidade inferior à do provider pedido."
    )
}

/// Runs `op` with the cloud summarizer; on `NoNetwork` reruns it entirely with the local one.
/// Returns the value and, if the fallback was used, the notice to show.
///
/// If the local model is not available the original network error is returned, naming why
/// the fallback could not help.
pub fn run_with_local_fallback<T>(
    provider_name: &str,
    cloud: Arc<dyn Summarizer>,
    local: impl FnOnce() -> Result<Arc<dyn Summarizer>, VadError>,
    op: impl Fn(Arc<dyn Summarizer>) -> Result<T, VadError>,
) -> Result<(T, Option<String>), VadError> {
    match op(cloud) {
        Ok(v) => Ok((v, None)),
        Err(VadError::NoNetwork(why)) => match local() {
            Ok(local) => {
                tracing::warn!("fallback para o modelo local: {why}");
                op(local).map(|v| (v, Some(fallback_notice(provider_name))))
            }
            Err(e) => Err(VadError::NoNetwork(format!(
                "{why} — e o modelo local não está disponível para o substituir ({e})"
            ))),
        },
        Err(e) => Err(e),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_provider::{AiPrivacyBadge, MockSummarizer};
    use std::sync::atomic::AtomicBool;

    struct Failing(fn() -> VadError);
    impl Summarizer for Failing {
        fn context_window(&self) -> usize { 4096 }
        fn max_output_tokens(&self) -> usize { 256 }
        fn privacy_badge(&self) -> AiPrivacyBadge { AiPrivacyBadge::Cloud }
        fn summarize_chunk(&self, _: &str, _: bool, _: Option<&AtomicBool>) -> Result<String, VadError> { Err((self.0)()) }
        fn translate_text(&self, _: &str, _: &str, _: &str, _: Option<&AtomicBool>) -> Result<String, VadError> { Err((self.0)()) }
    }

    fn local() -> Result<Arc<dyn Summarizer>, VadError> {
        Ok(Arc::new(MockSummarizer::default()))
    }

    fn op(s: Arc<dyn Summarizer>) -> Result<(String, AiPrivacyBadge), VadError> {
        Ok((s.summarize_chunk("x", false, None)?, s.privacy_badge()))
    }

    #[test]
    fn offline_falls_back_with_a_notice_and_a_local_badge() {
        let cloud = Arc::new(Failing(|| VadError::NoNetwork("offline".into())));
        let ((_, badge), notice) = run_with_local_fallback("à OpenAI", cloud, local, op).unwrap();
        assert_eq!(badge, AiPrivacyBadge::Local, "the result is the local model's, and says so");
        let notice = notice.expect("fallback must never be silent");
        assert!(notice.contains("OpenAI") && notice.contains("localmente") && notice.contains("🔒"), "{notice}");
    }

    #[test]
    fn healthy_cloud_is_used_as_is_without_notice() {
        let cloud: Arc<dyn Summarizer> = Arc::new(MockSummarizer::new(4096, 256, AiPrivacyBadge::Cloud));
        let ((_, badge), notice) = run_with_local_fallback("OpenAI", cloud, || panic!("local must not load"), op).unwrap();
        assert_eq!((badge, notice), (AiPrivacyBadge::Cloud, None));
    }

    #[test]
    fn only_no_network_triggers_the_fallback() {
        for make in [
            (|| VadError::LlmAuthFailed("k".into())) as fn() -> VadError,
            || VadError::LlmRateLimited("429".into()),
            || VadError::LlmTimeout("t".into()),
            || VadError::LlmCancelled,
            || VadError::Llm("x".into()),
        ] {
            let r = run_with_local_fallback("P", Arc::new(Failing(make)), || panic!("local must not load for {:?}", make()), op);
            assert!(r.is_err());
        }
    }

    #[test]
    fn missing_local_model_keeps_the_error_visible() {
        let cloud = Arc::new(Failing(|| VadError::NoNetwork("offline".into())));
        let e = run_with_local_fallback("P", cloud, || Err(VadError::Llm("modelo em falta".into())), op).unwrap_err();
        let msg = e.to_string();
        assert!(matches!(e, VadError::NoNetwork(_)) && msg.contains("offline") && msg.contains("modelo em falta"), "{msg}");
    }
}
