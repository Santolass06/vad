use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use candle_core::quantized::gguf_file;
use candle_core::{Device, Tensor};
use candle_transformers::models::quantized_qwen2::ModelWeights;
use serde::{Deserialize, Serialize};
use tokenizers::Tokenizer;
use tracing::info;
use vad_core::VadError;

/// Default Qwen 2.5 0.5B Instruct GGUF model filename (§4.2, 491,400,032 bytes on Hugging Face).
pub const DEFAULT_QWEN_MODEL_FILENAME: &str = "qwen2.5-0.5b-instruct-q4_k_m.gguf";

/// Default tokenizer filename matching Qwen 2.5.
pub const DEFAULT_QWEN_TOKENIZER_FILENAME: &str = "qwen2.5-tokenizer.json";

/// Default input context window in characters/approx tokens for LocalQwen.
/// 0.5B model supports up to 32k tokens, but for fast and memory-efficient local execution (§5)
/// we default to 4096 tokens (~12,000 to 16,000 Portuguese characters).
pub const DEFAULT_QWEN_CONTEXT_WINDOW: usize = 4096;

/// Default generation limit for a single summary chunk.
pub const DEFAULT_QWEN_MAX_OUTPUT_TOKENS: usize = 768;

fn default_qwen_model() -> String {
    DEFAULT_QWEN_MODEL_FILENAME.to_string()
}

fn default_qwen_context_window() -> usize {
    DEFAULT_QWEN_CONTEXT_WINDOW
}

/// Privacy badge indicator for AI operations per PLANO_VAD.md §4.1.
///
/// Displayed in the UI before and during summarization/translation to ensure
/// no operation that leaves the machine is silent (§4.1, §4.21).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AiPrivacyBadge {
    /// Fully local inference; zero audio, text, or metadata leaves the computer.
    Local,
    /// Transcripts leave the machine to an external cloud API.
    Cloud,
}

impl AiPrivacyBadge {
    /// User-facing short badge label with icon.
    pub fn label(&self) -> &'static str {
        match self {
            Self::Local => "🔒 Local",
            Self::Cloud => "☁️ Sai do PC",
        }
    }

    /// User-facing explanatory tooltip per §4.1 and §4.13.
    pub fn tooltip(&self) -> &'static str {
        match self {
            Self::Local => {
                "Processado localmente neste computador. Nenhum dado sai da máquina (§4.1)."
            }
            Self::Cloud => {
                "O texto da transcrição é enviado ao provider cloud escolhido (§4.1, §4.2)."
            }
        }
    }

    /// Returns true if this is purely local processing.
    pub fn is_local(&self) -> bool {
        matches!(self, Self::Local)
    }
}

/// Provider configuration enum per PLANO_VAD.md §4.2.
///
/// Designed forward-looking to accommodate cloud providers (`OpenAiCompatible`,
/// `Anthropic`, `Gemini`) arriving in Sprint 11+ without requiring future refactors.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum LlmProviderConfig {
    /// Local offline inference via GGUF and Candle (Milestone M5a).
    LocalQwen {
        #[serde(default = "default_qwen_model")]
        model_filename: String,
        #[serde(default = "default_qwen_context_window")]
        context_window: usize,
    },
    /// OpenAI-compatible HTTP endpoint (OpenAI, Ollama, OpenRouter, self-hosted) (§4.2).
    OpenAiCompatible {
        base_url: String,
        model: String,
    },
    /// Anthropic Messages API (§4.2).
    Anthropic {
        model: String,
    },
    /// Google Gemini API (§4.2).
    Gemini {
        model: String,
    },
}

impl Default for LlmProviderConfig {
    fn default() -> Self {
        Self::LocalQwen {
            model_filename: default_qwen_model(),
            context_window: default_qwen_context_window(),
        }
    }
}

impl LlmProviderConfig {
    /// Returns the appropriate privacy badge for this provider (§4.1).
    pub fn privacy_badge(&self) -> AiPrivacyBadge {
        match self {
            Self::LocalQwen { .. } => AiPrivacyBadge::Local,
            Self::OpenAiCompatible { base_url, .. } => {
                let lower = base_url.to_lowercase();
                if lower.contains("localhost") || lower.contains("127.0.0.1") || lower.contains("0.0.0.0") {
                    AiPrivacyBadge::Local
                } else {
                    AiPrivacyBadge::Cloud
                }
            }
            Self::Anthropic { .. } | Self::Gemini { .. } => AiPrivacyBadge::Cloud,
        }
    }

    /// Display name of the provider.
    pub fn display_name(&self) -> &'static str {
        match self {
            Self::LocalQwen { .. } => "Local (Qwen2.5 0.5B)",
            Self::OpenAiCompatible { .. } => "OpenAI-compatible",
            Self::Anthropic { .. } => "Anthropic Claude",
            Self::Gemini { .. } => "Google Gemini",
        }
    }
}

/// Core interface for LLM text summarization and translation.
///
/// Designed per PLANO_VAD.md §4.19 where context window and token limits are properties
/// of the specific provider instance, not a global constant.
pub trait Summarizer: Send + Sync {
    /// Maximum context window (in tokens) supported by this provider for input.
    fn context_window(&self) -> usize;

    /// Maximum number of tokens to generate per inference call.
    fn max_output_tokens(&self) -> usize;

    /// Returns the privacy badge associated with this summarizer (§4.1).
    fn privacy_badge(&self) -> AiPrivacyBadge;

    /// Number of tokens this provider's tokenizer produces for `text`.
    ///
    /// The map-reduce chunker budgets by this, not by a fixed characters-per-token guess: the
    /// real ratio depends on the tokenizer and on the text (timestamps and accents cost more).
    /// The default is a conservative 3 characters per token.
    fn count_tokens(&self, text: &str) -> usize {
        text.len().div_ceil(3)
    }

    /// Summarizes a chunk of transcription text.
    ///
    /// - `chunk`: The raw transcription block (or joined partial summaries).
    /// - `is_final_reduce`: True if this call synthesizes multiple partial summaries into
    ///   the final meeting minutes, false if it summarizes an intermediate chunk.
    /// - `abort`: checked inside the call, so cancelling does not wait for a whole chunk (§4.20).
    fn summarize_chunk(
        &self,
        chunk: &str,
        is_final_reduce: bool,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError>;

    /// Translates text from `source_lang` to `target_lang` using LLM prompt guidance (§4.1).
    fn translate_text(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError>;
}

fn is_aborted(abort: Option<&AtomicBool>) -> bool {
    abort.is_some_and(|a| a.load(Ordering::Relaxed))
}

/// Prompt tokens fed per forward pass while prefilling, so an abort waits for one slice
/// (a few seconds on CPU) and not for the whole prompt.
const PREFILL_SLICE_TOKENS: usize = 128;

/// Local LLM summarizer powered by Candle and Qwen 2.5 Instruct GGUF.
pub struct LocalQwenSummarizer {
    weights: Arc<Mutex<ModelWeights>>,
    tokenizer: Arc<Tokenizer>,
    context_window: usize,
    max_output_tokens: usize,
    device: Device,
}

impl LocalQwenSummarizer {
    /// Loads a local Qwen GGUF model and tokenizer from disk paths.
    pub fn load_from_paths(
        model_path: &Path,
        tokenizer_path: &Path,
        context_window: usize,
        max_output_tokens: usize,
    ) -> Result<Self, VadError> {
        if !model_path.exists() {
            return Err(VadError::Llm(format!(
                "Ficheiro de modelo GGUF não encontrado: {:?}",
                model_path
            )));
        }
        if !tokenizer_path.exists() {
            return Err(VadError::Llm(format!(
                "Ficheiro de tokenizer não encontrado: {:?}",
                tokenizer_path
            )));
        }

        info!("Loading Qwen GGUF model from {:?}...", model_path);
        let mut file = File::open(model_path).map_err(|e| {
            VadError::Llm(format!("Falha ao abrir ficheiro de modelo: {:?}", e))
        })?;

        let device = Device::Cpu;
        let content = gguf_file::Content::read(&mut file).map_err(|e| {
            VadError::Llm(format!("Falha ao ler cabeçalho GGUF: {:?}", e))
        })?;

        let weights = ModelWeights::from_gguf(content, &mut file, &device).map_err(|e| {
            VadError::Llm(format!("Falha ao inicializar pesos Qwen2 GGUF: {:?}", e))
        })?;

        info!("Loading tokenizer from {:?}...", tokenizer_path);
        let tokenizer = Tokenizer::from_file(tokenizer_path).map_err(|e| {
            VadError::Llm(format!("Falha ao carregar tokenizer: {:?}", e))
        })?;

        Ok(Self {
            weights: Arc::new(Mutex::new(weights)),
            tokenizer: Arc::new(tokenizer),
            context_window,
            max_output_tokens,
            device,
        })
    }

    /// Number of tokens the real tokenizer produces for `text`.
    pub fn count_tokens(&self, text: &str) -> Result<usize, VadError> {
        self.tokenizer
            .encode(text, true)
            .map(|e| e.get_ids().len())
            .map_err(|e| VadError::Llm(format!("Falha ao tokenizar texto: {:?}", e)))
    }

    /// Formats a chat prompt using the Qwen 2.5 Instruct chat template.
    pub fn format_prompt(system_prompt: &str, user_prompt: &str) -> String {
        format!(
            "<|im_start|>system\n{}<|im_end|>\n<|im_start|>user\n{}<|im_end|>\n<|im_start|>assistant\n",
            system_prompt.trim(),
            user_prompt.trim()
        )
    }

    /// Runs text generation on the loaded model.
    pub fn generate(
        &self,
        prompt: &str,
        max_tokens: usize,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError> {
        let encoding = self.tokenizer.encode(prompt, true).map_err(|e| {
            VadError::Llm(format!("Falha ao tokenizar prompt: {:?}", e))
        })?;

        let tokens = encoding.get_ids();
        if tokens.len() > self.context_window {
            return Err(VadError::LlmContextExceeded(format!(
                "Prompt tem {} tokens, mas a janela de contexto é {}",
                tokens.len(),
                self.context_window
            )));
        }

        let mut weights = self.weights.lock().map_err(|_| {
            VadError::Llm("Falha ao adquirir lock do modelo LLM".to_string())
        })?;

        // Reset KV cache between separate calls (§4.19 / Candle)
        weights.clear_kv_cache();

        let mut generated_tokens = Vec::new();

        // End-of-sequence token IDs for Qwen2.5 (<|im_end|>, <|endoftext|>)
        let eos_token_id = self.tokenizer.token_to_id("<|im_end|>").unwrap_or(151645);
        let eot_token_id = self.tokenizer.token_to_id("<|endoftext|>").unwrap_or(151643);

        // Prefill in slices; the logits of the last slice predict the first generated token.
        let mut logits = None;
        for (i, slice) in tokens.chunks(PREFILL_SLICE_TOKENS).enumerate() {
            if is_aborted(abort) {
                weights.clear_kv_cache();
                return Err(VadError::LlmCancelled);
            }
            let input = Tensor::new(slice, &self.device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| VadError::Llm(format!("Falha ao criar tensor de entrada: {:?}", e)))?;
            logits = Some(
                weights
                    .forward(&input, i * PREFILL_SLICE_TOKENS)
                    .map_err(|e| VadError::Llm(format!("Erro no forward do prompt: {:?}", e)))?,
            );
        }
        let Some(mut logits) = logits else {
            return Err(VadError::Llm("Prompt vazio".to_string()));
        };

        for index in 0..max_tokens {
            if is_aborted(abort) {
                weights.clear_kv_cache();
                return Err(VadError::LlmCancelled);
            }

            // `forward` already narrows to the last position: logits are `(1, vocab)`.
            let logits_s = logits
                .squeeze(0)
                .map_err(|e| VadError::Llm(format!("Erro ao extrair logits: {:?}", e)))?;

            // Greedy argmax sampling for deterministic summary/translation
            let next_token = logits_s
                .argmax(0)
                .and_then(|t| t.to_scalar::<u32>())
                .map_err(|e| VadError::Llm(format!("Erro no argmax: {:?}", e)))?;

            if next_token == eos_token_id || next_token == eot_token_id {
                break;
            }

            generated_tokens.push(next_token);

            // Forward next single token with updated position index
            let next_input = Tensor::new(&[next_token], &self.device)
                .and_then(|t| t.unsqueeze(0))
                .map_err(|e| VadError::Llm(format!("Erro ao criar tensor para próximo token: {:?}", e)))?;

            logits = weights
                .forward(&next_input, tokens.len() + index)
                .map_err(|e| VadError::Llm(format!("Erro no forward do token {}: {:?}", index, e)))?;
        }

        // Clean up KV cache again after generation
        weights.clear_kv_cache();

        let text = self.tokenizer.decode(&generated_tokens, true).map_err(|e| {
            VadError::Llm(format!("Falha ao descodificar tokens gerados: {:?}", e))
        })?;

        Ok(text.trim().to_string())
    }
}

impl Summarizer for LocalQwenSummarizer {
    fn context_window(&self) -> usize {
        self.context_window
    }

    fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    fn privacy_badge(&self) -> AiPrivacyBadge {
        AiPrivacyBadge::Local
    }

    fn count_tokens(&self, text: &str) -> usize {
        // A tokenizer failure must not make the budget optimistic: fall back to the default ratio.
        LocalQwenSummarizer::count_tokens(self, text).unwrap_or_else(|_| text.len().div_ceil(3))
    }

    fn summarize_chunk(
        &self,
        chunk: &str,
        is_final_reduce: bool,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError> {
        let (system_prompt, user_prompt) = if is_final_reduce {
            (
                "És um assistente executivo de reuniões em língua portuguesa. A tua função é criar um resumo estruturado e profissional com base nos resumos parciais fornecidos. Organiza a resposta com: 1. Sumário Executivo; 2. Principais Discussões; 3. Decisões Tomadas; 4. Próximos Passos e Tarefas. Não inventes factos.",
                format!("Aqui estão os resumos parciais de diferentes secções da reunião:\n\n{}\n\nPor favor sintetiza o resumo final estruturado da reunião:", chunk),
            )
        } else {
            (
                "És um assistente de síntese em língua portuguesa. Resume o bloco de transcrição fornecido de forma concisa, capturando os pontos discutidos, ideias centrais e quaisquer decisões ou números mencionados. Responde apenas com o resumo dos pontos relevantes.",
                format!("Resume objetivamente o seguinte bloco de transcrição:\n\n{}", chunk),
            )
        };

        let prompt = Self::format_prompt(system_prompt, &user_prompt);
        self.generate(&prompt, self.max_output_tokens, abort)
    }

    fn translate_text(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError> {
        let system_prompt = format!(
            "És um tradutor profissional e rigoroso de {} para {}. Traduz o texto fornecido com máxima fidelidade e naturalidade, sem alucinações, sem comentários adicionais e sem explicações.",
            source_lang, target_lang
        );
        let user_prompt = format!("Texto a traduzir:\n{}", text);
        let prompt = Self::format_prompt(&system_prompt, &user_prompt);
        self.generate(&prompt, self.max_output_tokens, abort)
    }
}

/// Deterministic mock summarizer for unit testing and offline development.
///
/// Validates chunking boundaries, token limits, and map-reduce flows in milliseconds
/// without needing a multi-hundred MB model file.
pub struct MockSummarizer {
    pub context_window: usize,
    pub max_output_tokens: usize,
    pub privacy_badge: AiPrivacyBadge,
    /// Records chunks received to verify memory/chunking patterns in tests.
    pub recorded_chunks: Arc<Mutex<Vec<String>>>,
    /// Optional prefix to add to outputs.
    pub output_prefix: String,
}

impl Default for MockSummarizer {
    fn default() -> Self {
        Self::new(2048, 512, AiPrivacyBadge::Local)
    }
}

impl MockSummarizer {
    pub fn new(context_window: usize, max_output_tokens: usize, badge: AiPrivacyBadge) -> Self {
        Self {
            context_window,
            max_output_tokens,
            privacy_badge: badge,
            recorded_chunks: Arc::new(Mutex::new(Vec::new())),
            output_prefix: String::new(),
        }
    }

    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.output_prefix = prefix.to_string();
        self
    }

    pub fn chunks_received(&self) -> Vec<String> {
        self.recorded_chunks.lock().unwrap().clone()
    }
}

impl Summarizer for MockSummarizer {
    fn context_window(&self) -> usize {
        self.context_window
    }

    fn max_output_tokens(&self) -> usize {
        self.max_output_tokens
    }

    fn privacy_badge(&self) -> AiPrivacyBadge {
        self.privacy_badge
    }

    fn count_tokens(&self, text: &str) -> usize {
        text.len() / 4
    }

    fn summarize_chunk(
        &self,
        chunk: &str,
        is_final_reduce: bool,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError> {
        if is_aborted(abort) {
            return Err(VadError::LlmCancelled);
        }
        // Enforce context window constraint (§4.19)
        // Approximate 1 token ~= 4 characters
        let approx_tokens = self.count_tokens(chunk);
        if approx_tokens > self.context_window {
            return Err(VadError::LlmContextExceeded(format!(
                "Bloco com ~{} tokens excedeu a janela de contexto de {}",
                approx_tokens, self.context_window
            )));
        }

        if let Ok(mut chunks) = self.recorded_chunks.lock() {
            chunks.push(chunk.to_string());
        }

        if is_final_reduce {
            Ok(format!(
                "{}# Resumo da Reunião\n\n## Sumário Executivo\nSíntese consolidada.\n\n## Decisões\nDecisões extraídas.\n\n{}",
                self.output_prefix,
                chunk
                    .lines()
                    .take(5)
                    .map(|l| format!("- {}", l))
                    .collect::<Vec<_>>()
                    .join("\n")
            ))
        } else {
            // Extract leading sentence or key excerpt
            let preview = chunk
                .lines()
                .find(|l| !l.trim().is_empty())
                .unwrap_or("Ponto discutido");
            Ok(format!(
                "{}Resumo parcial: {}",
                self.output_prefix,
                if preview.len() > 80 {
                    &preview[..80]
                } else {
                    preview
                }
            ))
        }
    }

    fn translate_text(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: &str,
        abort: Option<&AtomicBool>,
    ) -> Result<String, VadError> {
        if is_aborted(abort) {
            return Err(VadError::LlmCancelled);
        }
        let approx_tokens = self.count_tokens(text);
        if approx_tokens > self.context_window {
            return Err(VadError::LlmContextExceeded(format!(
                "Texto para tradução com ~{} tokens excedeu a janela de contexto de {}",
                approx_tokens, self.context_window
            )));
        }

        Ok(format!(
            "[{target_lang} from {source_lang}]: {text}"
        ))
    }
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    #[test]
    fn test_privacy_badge_values_and_tooltips() {
        let local_badge = AiPrivacyBadge::Local;
        assert_eq!(local_badge.label(), "🔒 Local");
        assert!(local_badge.tooltip().contains("Processado localmente"));
        assert!(local_badge.is_local());

        let cloud_badge = AiPrivacyBadge::Cloud;
        assert_eq!(cloud_badge.label(), "☁️ Sai do PC");
        assert!(cloud_badge.tooltip().contains("provider cloud"));
        assert!(!cloud_badge.is_local());
    }

    #[test]
    fn test_llm_provider_config_serialization_and_badges() {
        let qwen_cfg = LlmProviderConfig::LocalQwen {
            model_filename: "qwen.gguf".to_string(),
            context_window: 2048,
        };
        assert_eq!(qwen_cfg.privacy_badge(), AiPrivacyBadge::Local);

        let local_openai = LlmProviderConfig::OpenAiCompatible {
            base_url: "http://localhost:11434/v1".to_string(),
            model: "llama3".to_string(),
        };
        assert_eq!(local_openai.privacy_badge(), AiPrivacyBadge::Local);

        let remote_openai = LlmProviderConfig::OpenAiCompatible {
            base_url: "https://api.openai.com/v1".to_string(),
            model: "gpt-4o".to_string(),
        };
        assert_eq!(remote_openai.privacy_badge(), AiPrivacyBadge::Cloud);

        let anthropic = LlmProviderConfig::Anthropic {
            model: "claude-3-5-sonnet".to_string(),
        };
        assert_eq!(anthropic.privacy_badge(), AiPrivacyBadge::Cloud);

        // Verify JSON roundtrip
        let json = serde_json::to_string(&qwen_cfg).unwrap();
        let deserialized: LlmProviderConfig = serde_json::from_str(&json).unwrap();
        assert_eq!(deserialized, qwen_cfg);
    }

    #[test]
    fn test_mock_summarizer_respects_context_window() {
        let summarizer = MockSummarizer::new(50, 100, AiPrivacyBadge::Local);
        assert_eq!(summarizer.context_window(), 50);

        // Chunk within limit (50 tokens ~= 200 chars)
        let small_chunk = "Resumo curto que cabe perfeitamente na janela.";
        let res = summarizer.summarize_chunk(small_chunk, false, None);
        assert!(res.is_ok());

        // Chunk exceeding limit (> 50 tokens)
        let huge_chunk = "A".repeat(400);
        let err_res = summarizer.summarize_chunk(&huge_chunk, false, None);
        assert!(err_res.is_err());
        match err_res.unwrap_err() {
            VadError::LlmContextExceeded(msg) => {
                assert!(msg.contains("excedeu a janela de contexto"));
            }
            other => panic!("Expected LlmContextExceeded, got {:?}", other),
        }
    }

    #[test]
    fn test_qwen_chat_prompt_formatting() {
        let formatted = LocalQwenSummarizer::format_prompt("Tu és um resumo.", "Resume isto.");
        assert_eq!(
            formatted,
            "<|im_start|>system\nTu és um resumo.<|im_end|>\n<|im_start|>user\nResume isto.<|im_end|>\n<|im_start|>assistant\n"
        );
    }

    /// Runs the real model. Needs `VAD_TEST_QWEN_MODEL` and `VAD_TEST_QWEN_TOKENIZER`:
    /// `cargo test -p vad-ai -- --ignored --nocapture real_qwen`.
    pub(crate) fn real_qwen(max_output_tokens: usize) -> Option<LocalQwenSummarizer> {
        let model = std::env::var("VAD_TEST_QWEN_MODEL").ok()?;
        let tokenizer = std::env::var("VAD_TEST_QWEN_TOKENIZER").ok()?;
        Some(
            LocalQwenSummarizer::load_from_paths(
                Path::new(&model),
                Path::new(&tokenizer),
                DEFAULT_QWEN_CONTEXT_WINDOW,
                max_output_tokens,
            )
            .expect("load real Qwen"),
        )
    }

    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER (~490 MB)"]
    fn test_real_qwen_generates_coherent_text() {
        let Some(q) = real_qwen(64) else { panic!("env vars not set") };
        let prompt = LocalQwenSummarizer::format_prompt(
            "Responde apenas com a capital pedida.",
            "Qual é a capital de França?",
        );
        let out = q.generate(&prompt, 16, None).expect("generate");
        println!("generate -> {out:?}");
        assert!(out.contains("Paris"), "unexpected output: {out:?}");
    }

    /// Prints prefill and decode throughput (no assertion on time): the numbers go in the diary.
    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER (~490 MB)"]
    fn test_real_qwen_throughput() {
        use std::time::Instant;
        let q = real_qwen(64).expect("env vars not set");
        let text = "[00:01:04] Revimos o estado atual da migração para Rust e mpv.\n".repeat(40);
        let prompt = LocalQwenSummarizer::format_prompt("Resume.", &text);
        let n = q.count_tokens(&prompt).unwrap();
        let t = Instant::now();
        q.generate(&prompt, 1, None).unwrap();
        let prefill = t.elapsed().as_secs_f64();
        let t = Instant::now();
        q.generate(&prompt, 33, None).unwrap();
        let both = t.elapsed().as_secs_f64();
        println!("prompt {n} tokens: prefill {:.2}s ({:.0} tok/s); decode {:.1} tok/s",
            prefill, n as f64 / prefill, 32.0 / (both - prefill));
    }

    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER (~490 MB)"]
    fn test_real_qwen_abort_does_not_wait_for_the_whole_prompt() {
        use std::time::{Duration, Instant};
        let q = Arc::new(real_qwen(64).expect("env vars not set"));
        let text = "[00:01:04] Revimos o estado atual da migração para Rust e mpv.\n".repeat(150);
        let prompt = LocalQwenSummarizer::format_prompt("Resume.", &text);
        assert!(q.count_tokens(&prompt).unwrap() > 1500, "prompt must take a long prefill");

        let abort = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&abort);
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_millis(500));
            flag.store(true, Ordering::Relaxed);
        });
        let t = Instant::now();
        let res = q.generate(&prompt, 64, Some(&abort));
        let waited = t.elapsed();
        println!("aborted after {waited:?}");
        assert!(matches!(res, Err(VadError::LlmCancelled)), "got {res:?}");
        assert!(waited < Duration::from_secs(20), "abort took {waited:?}");
    }
}
