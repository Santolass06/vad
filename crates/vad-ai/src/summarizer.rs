use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use vad_core::VadError;

use crate::llm_provider::{AiPrivacyBadge, Summarizer};
use crate::whisper::TranscriptionSegment;

/// Progress stage reported during map-reduce summarization per PLANO_VAD.md §4.20.
///
/// Progress is emitted chunk-by-chunk ("a resumir bloco 3 de 7") rather than token-by-token
/// streaming SSE, ensuring clean decoupling and actionable progress feedback.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SummarizeProgress {
    /// Initial chunking of transcription segments.
    Chunking { total_chunks: usize },
    /// Generating partial summary for a chunk (1-indexed: `current` of `total`).
    SummarizingChunk { current: usize, total: usize },
    /// Intermediate reduction of partial summaries that still do not fit one prompt
    /// (batch `current` of `total` in reduction `pass`).
    ReducingBatch { pass: usize, current: usize, total: usize },
    /// Synthesizing partial summaries into the final consolidated summary.
    Synthesizing { pass: usize, total_passes: usize },
    /// Summarization completed.
    Done,
}

impl SummarizeProgress {
    /// Formatted status string in Portuguese for UI labels.
    pub fn display_message(&self) -> String {
        match self {
            Self::Chunking { total_chunks } => {
                format!("A preparar {} blocos de transcrição...", total_chunks)
            }
            Self::SummarizingChunk { current, total } => {
                format!("A resumir bloco {} de {}...", current, total)
            }
            Self::ReducingBatch { pass, current, total } => {
                format!("A condensar resumos (passo {pass}, parte {current} de {total})...")
            }
            Self::Synthesizing { .. } => "A sintetizar resumo final da reunião...".to_string(),
            Self::Done => "Resumo concluído com sucesso.".to_string(),
        }
    }
}

/// Partial summary produced for a single temporal block during the Map phase.
#[derive(Debug, Clone, PartialEq)]
pub struct PartialSummary {
    pub chunk_index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub summary_text: String,
}

/// Consolidated meeting summary produced by Map-Reduce.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MeetingSummary {
    /// Full structured Markdown content.
    pub markdown: String,
    /// Total number of raw transcription chunks processed.
    pub total_chunks: usize,
    /// Duration of the covered audio in seconds.
    pub duration_seconds: f64,
    /// Privacy indicator of the model used (§4.1).
    pub privacy_badge: AiPrivacyBadge,
}

/// Helper struct defining the boundaries and text of a raw chunk.
#[derive(Debug, Clone)]
pub struct TranscriptionChunk {
    pub chunk_index: usize,
    pub start_ms: i64,
    pub end_ms: i64,
    pub text: String,
}

/// Map-Reduce Summarizer orchestrator (§4.19).
pub struct MapReduceSummarizer {
    summarizer: Arc<dyn Summarizer>,
}

impl MapReduceSummarizer {
    /// Creates a new summarizer with the given LLM provider.
    pub fn new(summarizer: Arc<dyn Summarizer>) -> Self {
        Self { summarizer }
    }

    /// Calculates safe token budget for chunk contents.
    /// Reserves ~25% (or minimum 350 tokens) for system prompt, framing, and instructions.
    pub fn max_chunk_tokens(&self) -> usize {
        let ctx = self.summarizer.context_window();
        let overhead = (ctx / 4).max(350);
        ctx.saturating_sub(overhead).max(100)
    }

    /// Partitions segments into discrete temporal chunks respecting the context window limit.
    ///
    /// Preserves timestamps [HH:MM:SS] on each line to maintain context for the LLM.
    pub fn partition_segments(&self, segments: &[TranscriptionSegment]) -> Vec<TranscriptionChunk> {
        if segments.is_empty() {
            return Vec::new();
        }

        // Budget in the provider's own tokens. Counting line by line slightly over-counts
        // (no merges across lines), which only errs on the safe side of the window.
        let max_tokens = self.max_chunk_tokens();

        let mut chunks = Vec::new();
        let mut current_lines = Vec::new();
        let mut current_tokens = 0;
        let mut chunk_start_ms = segments[0].start_ms;
        let mut chunk_end_ms = segments[0].end_ms;
        let mut chunk_index = 1;

        for seg in segments {
            let line = format!(
                "[{}] {}",
                TranscriptionSegment::format_timestamp(seg.start_ms),
                seg.text.trim()
            );
            let line_tokens = self.summarizer.count_tokens(&line) + 1; // + newline

            if !current_lines.is_empty() && (current_tokens + line_tokens > max_tokens) {
                // Emit current chunk
                chunks.push(TranscriptionChunk {
                    chunk_index,
                    start_ms: chunk_start_ms,
                    end_ms: chunk_end_ms,
                    text: current_lines.join("\n"),
                });
                chunk_index += 1;
                current_lines.clear();
                current_tokens = 0;
                chunk_start_ms = seg.start_ms;
            }

            chunk_end_ms = seg.end_ms;
            current_tokens += line_tokens;
            current_lines.push(line);
        }

        if !current_lines.is_empty() {
            chunks.push(TranscriptionChunk {
                chunk_index,
                start_ms: chunk_start_ms,
                end_ms: chunk_end_ms,
                text: current_lines.join("\n"),
            });
        }

        chunks
    }

    /// Summarizes a transcription using Map-Reduce with eager raw-block disposal (§4.19).
    ///
    /// # Memory Guarantee (§4.19)
    /// Each raw transcription block is discarded (`drop`) immediately after its partial
    /// summary is computed. The heap retains only the compact partial summaries, preventing
    /// the memory footprint from scaling with the raw transcript size.
    pub fn summarize<P>(
        &self,
        segments: &[TranscriptionSegment],
        mut progress_cb: Option<P>,
        abort_flag: Option<Arc<AtomicBool>>,
    ) -> Result<MeetingSummary, VadError>
    where
        P: FnMut(SummarizeProgress),
    {
        if segments.is_empty() {
            return Ok(MeetingSummary {
                markdown: "# Resumo da Reunião\n\nNenhuma transcrição disponível para resumir.".to_string(),
                total_chunks: 0,
                duration_seconds: 0.0,
                privacy_badge: self.summarizer.privacy_badge(),
            });
        }

        let total_duration_secs = (segments.last().map(|s| s.end_ms).unwrap_or(0)
            - segments.first().map(|s| s.start_ms).unwrap_or(0))
            .max(0) as f64
            / 1000.0;

        let chunks = self.partition_segments(segments);
        let total_chunks = chunks.len();

        if let Some(ref mut cb) = progress_cb {
            cb(SummarizeProgress::Chunking { total_chunks });
        }

        info!(
            "Starting Map-Reduce summarization: {} segments into {} chunks ({:.1}s audio)",
            segments.len(),
            total_chunks,
            total_duration_secs
        );

        // --- MAP PHASE with Eager Drop (§4.19) ---
        let mut partial_summaries = Vec::with_capacity(total_chunks);

        for chunk in chunks {
            // Check cancellation (§4.20)
            if let Some(ref abort) = abort_flag {
                if abort.load(Ordering::Relaxed) {
                    debug!("Summarization aborted by user request before chunk {}", chunk.chunk_index);
                    return Err(VadError::LlmCancelled);
                }
            }

            if let Some(ref mut cb) = progress_cb {
                cb(SummarizeProgress::SummarizingChunk {
                    current: chunk.chunk_index,
                    total: total_chunks,
                });
            }

            let start_ts = TranscriptionSegment::format_timestamp(chunk.start_ms);
            let end_ts = TranscriptionSegment::format_timestamp(chunk.end_ms);
            let chunk_idx = chunk.chunk_index;
            let start_ms = chunk.start_ms;
            let end_ms = chunk.end_ms;
            let raw_text = chunk.text;

            debug!(
                "Processing chunk {}/{} ({} chars, span {} -> {})",
                chunk_idx, total_chunks, raw_text.len(), start_ts, end_ts
            );

            // Compute partial summary
            let summary_text =
                self.summarizer
                    .summarize_chunk(&raw_text, false, abort_flag.as_deref())?;

            // CRITICAL: Drop raw chunk text immediately! (§4.19)
            drop(raw_text);

            partial_summaries.push(PartialSummary {
                chunk_index: chunk_idx,
                start_ms,
                end_ms,
                summary_text,
            });
        }

        // --- REDUCE PHASE ---
        if let Some(ref abort) = abort_flag {
            if abort.load(Ordering::Relaxed) {
                return Err(VadError::LlmCancelled);
            }
        }

        let final_markdown = self.reduce_partial_summaries(
            &partial_summaries,
            total_duration_secs,
            &mut progress_cb,
            &abort_flag,
        )?;

        if let Some(ref mut cb) = progress_cb {
            cb(SummarizeProgress::Done);
        }

        Ok(MeetingSummary {
            markdown: final_markdown,
            total_chunks,
            duration_seconds: total_duration_secs,
            privacy_badge: self.summarizer.privacy_badge(),
        })
    }

    /// Reduces partial summaries into a final coherent document, recursively if necessary.
    fn reduce_partial_summaries<P>(
        &self,
        partials: &[PartialSummary],
        duration_secs: f64,
        progress_cb: &mut Option<P>,
        abort_flag: &Option<Arc<AtomicBool>>,
    ) -> Result<String, VadError>
    where
        P: FnMut(SummarizeProgress),
    {
        if partials.is_empty() {
            return Ok("Nenhum resumo parcial gerado.".to_string());
        }

        if partials.len() == 1 {
            return Ok(with_title(&partials[0].summary_text, duration_secs));
        }

        let max_tokens = self.max_chunk_tokens();

        let mut current_level: Vec<String> = partials
            .iter()
            .map(|p| {
                format!(
                    "[{}-{}] {}",
                    TranscriptionSegment::format_timestamp(p.start_ms),
                    TranscriptionSegment::format_timestamp(p.end_ms),
                    p.summary_text.trim()
                )
            })
            .collect();

        let mut pass = 1;
        let abort = abort_flag.as_deref();

        while current_level.len() > 1 {
            if abort.is_some_and(|a| a.load(Ordering::Relaxed)) {
                return Err(VadError::LlmCancelled);
            }

            let joined = current_level.join("\n\n");
            // If all fit comfortably in one reduction prompt, execute final synthesis
            if self.summarizer.count_tokens(&joined) <= max_tokens {
                if let Some(ref mut cb) = progress_cb {
                    cb(SummarizeProgress::Synthesizing {
                        pass,
                        total_passes: pass,
                    });
                }
                let body = self.summarizer.summarize_chunk(&joined, true, abort)?;
                return Ok(with_title(&body, duration_secs));
            }

            // Hierarchical reduction: combine into batches, each within the token budget
            let mut batches: Vec<String> = Vec::new();
            let mut batch: Vec<String> = Vec::new();
            let mut batch_tokens = 0;
            for item in current_level {
                let item_tokens = self.summarizer.count_tokens(&item) + 2;
                if !batch.is_empty() && batch_tokens + item_tokens > max_tokens {
                    batches.push(batch.join("\n\n"));
                    batch.clear();
                    batch_tokens = 0;
                }
                batch_tokens += item_tokens;
                batch.push(item);
            }
            if !batch.is_empty() {
                batches.push(batch.join("\n\n"));
            }

            let total_batches = batches.len();
            let mut next_level = Vec::with_capacity(total_batches);
            for (i, batch_text) in batches.into_iter().enumerate() {
                if let Some(ref mut cb) = progress_cb {
                    cb(SummarizeProgress::ReducingBatch {
                        pass,
                        current: i + 1,
                        total: total_batches,
                    });
                }
                next_level.push(self.summarizer.summarize_chunk(&batch_text, false, abort)?);
            }

            current_level = next_level;
            pass += 1;
        }

        // A single item is left: it is the summary itself (no further synthesis to do).
        let final_text = current_level.into_iter().next().unwrap_or_default();
        Ok(with_title(&final_text, duration_secs))
    }
}

/// Prepends the document title unless the model already produced a top-level heading.
fn with_title(body: &str, duration_secs: f64) -> String {
    let body = body.trim();
    if body.starts_with("# ") {
        return format!("{body}\n");
    }
    format!(
        "# Resumo da Reunião (Duração: {:.0} min)\n\n{body}\n",
        duration_secs / 60.0
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_provider::MockSummarizer;

    /// Generates a synthetic meeting transcription of specified duration in minutes.
    fn make_synthetic_transcription(minutes: u32, words_per_segment: usize) -> Vec<TranscriptionSegment> {
        let mut segments = Vec::new();
        let total_seconds = minutes * 60;
        let mut current_sec = 0;
        let step = 4; // 1 segment every 4 seconds

        let sample_phrases = [
            "Revimos o estado atual da migração para Rust e mpv.",
            "O orçamento do departamento de engenharia foi aprovado pela direção.",
            "A latência da extração de PCM em WAV diminuiu cerca de trinta por cento.",
            "Precisamos de definir as datas para a entrega do milestone M5b.",
            "Concluímos a integração do equalizador de dez bandas e o painel de cor.",
            "Ficou decidido manter o formato GGUF como padrão para modelos locais.",
        ];

        let mut idx = 0;
        while current_sec < total_seconds {
            let start_ms = (current_sec as i64) * 1000;
            let end_ms = start_ms + (step as i64) * 1000;

            let phrase = sample_phrases[idx % sample_phrases.len()];
            idx += 1;

            let text = if words_per_segment > 10 {
                format!("{phrase} {phrase}")
            } else {
                phrase.to_string()
            };

            segments.push(TranscriptionSegment {
                start_ms,
                end_ms,
                text,
            });

            current_sec += step;
        }

        segments
    }

    #[test]
    fn test_partition_segments_respects_token_budget() {
        let summarizer = Arc::new(MockSummarizer::new(500, 100, AiPrivacyBadge::Local));
        let map_reduce = MapReduceSummarizer::new(summarizer);

        // 10 minutes synthetic meeting
        let segments = make_synthetic_transcription(10, 8);
        assert!(!segments.is_empty());

        let chunks = map_reduce.partition_segments(&segments);
        assert!(chunks.len() > 1, "Should create multiple chunks");

        let max_chars = map_reduce.max_chunk_tokens() * 38 / 10;
        for chunk in &chunks {
            assert!(
                chunk.text.len() <= max_chars * 11 / 10, // minor margin for last line
                "Chunk exceeded character budget: len={}, max={}",
                chunk.text.len(),
                max_chars
            );
        }
    }

    #[test]
    fn test_summarize_90_minute_transcription_exit_criterion() {
        // --- EXIT CRITERION TEST (Sprint_Planning_09 §Critério de saída) ---
        // Context window set to 1024 tokens (moderate test window).
        // 90 minutes transcription has ~1,350 segments and ~90,000 characters.
        let summarizer = Arc::new(MockSummarizer::new(1024, 256, AiPrivacyBadge::Local));
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&summarizer) as Arc<dyn Summarizer>);

        let ninety_min_segments = make_synthetic_transcription(90, 8);
        assert_eq!(ninety_min_segments.len(), 1350);

        let mut progress_events = Vec::new();
        let summary_res = map_reduce.summarize(
            &ninety_min_segments,
            Some(|p| progress_events.push(p)),
            None,
        );

        assert!(summary_res.is_ok(), "Map-reduce failed: {:?}", summary_res.err());
        let summary = summary_res.unwrap();

        // 1. Verify chunking occurred and multiple chunks were processed
        assert!(summary.total_chunks >= 4, "Expected at least 4 chunks, got {}", summary.total_chunks);
        assert_eq!(summary.privacy_badge, AiPrivacyBadge::Local);
        assert!(summary.duration_seconds >= 5400.0);

        // 2. Verify progress events were emitted per block (§4.20)
        assert!(progress_events.iter().any(|e| matches!(e, SummarizeProgress::Chunking { .. })));
        assert!(progress_events.iter().any(|e| matches!(e, SummarizeProgress::SummarizingChunk { current: 1, .. })));
        assert!(progress_events.iter().any(|e| matches!(e, SummarizeProgress::Synthesizing { .. })));
        assert_eq!(progress_events.last(), Some(&SummarizeProgress::Done));

        // 3. Verify that all chunks received by MockSummarizer strictly stayed within context window
        let received = summarizer.chunks_received();
        assert!(!received.is_empty());
        for (i, c) in received.iter().enumerate() {
            let tokens = c.len() / 4;
            assert!(
                tokens <= summarizer.context_window,
                "Chunk {} had {} tokens, exceeding limit {}",
                i,
                tokens,
                summarizer.context_window
            );
        }

        // 4. Verify markdown structure
        assert!(summary.markdown.contains("# Resumo da Reunião"));
        assert!(summary.markdown.contains("Sumário Executivo"));
    }

    #[test]
    fn test_summarize_cancellation_aborts_immediately() {
        let summarizer = Arc::new(MockSummarizer::new(512, 128, AiPrivacyBadge::Local));
        let map_reduce = MapReduceSummarizer::new(summarizer);

        let segments = make_synthetic_transcription(20, 8);
        let abort_flag = Arc::new(AtomicBool::new(true)); // Pre-aborted

        let res = map_reduce.summarize(
            &segments,
            None::<fn(SummarizeProgress)>,
            Some(abort_flag),
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            VadError::LlmCancelled => {}
            other => panic!("Expected LlmCancelled, got {:?}", other),
        }
    }

    /// Provider whose tokens are single characters: the worst case for a chars-per-token guess.
    struct CharTokens(MockSummarizer);

    impl Summarizer for CharTokens {
        fn context_window(&self) -> usize {
            self.0.context_window()
        }
        fn max_output_tokens(&self) -> usize {
            self.0.max_output_tokens()
        }
        fn privacy_badge(&self) -> AiPrivacyBadge {
            AiPrivacyBadge::Local
        }
        fn count_tokens(&self, text: &str) -> usize {
            text.chars().count()
        }
        fn summarize_chunk(&self, c: &str, f: bool, a: Option<&AtomicBool>) -> Result<String, VadError> {
            self.0.summarize_chunk(c, f, a)
        }
        fn translate_text(&self, t: &str, s: &str, d: &str, a: Option<&AtomicBool>) -> Result<String, VadError> {
            self.0.translate_text(t, s, d, a)
        }
    }

    #[test]
    fn test_partition_budgets_with_the_providers_own_token_count() {
        let provider = Arc::new(CharTokens(MockSummarizer::new(1024, 256, AiPrivacyBadge::Local)));
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&provider) as Arc<dyn Summarizer>);
        let chunks = map_reduce.partition_segments(&make_synthetic_transcription(20, 8));
        assert!(chunks.len() > 1);
        for c in &chunks {
            let tokens = provider.count_tokens(&c.text);
            assert!(
                tokens <= map_reduce.max_chunk_tokens(),
                "chunk {} has {tokens} tokens, budget {}",
                c.chunk_index,
                map_reduce.max_chunk_tokens()
            );
        }
    }

    #[test]
    fn test_hierarchical_reduction_reports_progress_and_stays_in_window() {
        // Tiny window: the partial summaries do not fit one prompt, so extra passes are needed.
        let provider = Arc::new(MockSummarizer::new(256, 64, AiPrivacyBadge::Local));
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&provider) as Arc<dyn Summarizer>);
        let mut events = Vec::new();
        let summary = map_reduce
            .summarize(&make_synthetic_transcription(30, 8), Some(|p| events.push(p)), None)
            .expect("summary");
        assert!(events.iter().any(|e| matches!(e, SummarizeProgress::ReducingBatch { .. })));
        assert!(provider.chunks_received().iter().all(|c| c.len() / 4 <= 256));
        assert!(summary.markdown.starts_with("# Resumo da Reunião"));
    }

    #[test]
    fn test_cancel_during_map_stops_before_the_next_chunk() {
        let provider = Arc::new(MockSummarizer::new(512, 128, AiPrivacyBadge::Local));
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&provider) as Arc<dyn Summarizer>);
        let abort = Arc::new(AtomicBool::new(false));
        let flag = Arc::clone(&abort);
        let res = map_reduce.summarize(
            &make_synthetic_transcription(20, 8),
            Some(move |p: SummarizeProgress| {
                if matches!(p, SummarizeProgress::SummarizingChunk { current: 2, .. }) {
                    flag.store(true, Ordering::Relaxed);
                }
            }),
            Some(abort),
        );
        assert!(matches!(res, Err(VadError::LlmCancelled)));
        assert_eq!(provider.chunks_received().len(), 1, "only chunk 1 was summarized");
    }

    /// The chunker must respect the window measured with the real tokenizer (not a guess).
    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER (~490 MB)"]
    fn test_real_qwen_90_minute_chunks_fit_the_window() {
        use crate::llm_provider::{tests::real_qwen, LocalQwenSummarizer};
        let qwen = Arc::new(real_qwen(64).expect("env vars not set"));
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&qwen) as Arc<dyn Summarizer>);
        let chunks = map_reduce.partition_segments(&make_synthetic_transcription(90, 8));
        let mut worst = 0;
        for c in &chunks {
            // Both prompts (map and reduce) wrap the chunk in a few hundred tokens at most.
            let prompt = LocalQwenSummarizer::format_prompt(
                "És um assistente de síntese em língua portuguesa. Resume o bloco de transcrição fornecido de forma concisa, capturando os pontos discutidos, ideias centrais e quaisquer decisões ou números mencionados. Responde apenas com o resumo dos pontos relevantes.",
                &format!("Resume objetivamente o seguinte bloco de transcrição:\n\n{}", c.text),
            );
            worst = worst.max(qwen.count_tokens(&prompt).unwrap());
        }
        println!("{} chunks; worst full prompt = {worst} tokens (window {})", chunks.len(), qwen.context_window());
        assert!(worst <= qwen.context_window());
    }

    /// Real 90-minute run on the real model with the app's own output limit (exit criterion).
    /// Needs `VAD_TEST_QWEN_MODEL` and `VAD_TEST_QWEN_TOKENIZER`; `VAD_TEST_MINUTES` overrides 90.
    /// Prints elapsed time and RSS (baseline, model loaded, peak during the run).
    #[test]
    #[ignore = "needs VAD_TEST_QWEN_MODEL and VAD_TEST_QWEN_TOKENIZER (~490 MB); slow"]
    fn test_real_qwen_map_reduce_exit_criterion() {
        use crate::llm_provider::{tests::real_qwen, DEFAULT_QWEN_MAX_OUTPUT_TOKENS};
        use std::sync::atomic::AtomicU64;
        use std::time::Instant;
        let mib = |b: u64| b as f64 / 1_048_576.0;

        let rss_base = vad_core::get_process_rss_bytes().unwrap_or(0);
        let qwen = Arc::new(real_qwen(DEFAULT_QWEN_MAX_OUTPUT_TOKENS).expect("env vars not set"));
        let rss_loaded = vad_core::get_process_rss_bytes().unwrap_or(0);
        let minutes: u32 = std::env::var("VAD_TEST_MINUTES").ok().and_then(|v| v.parse().ok()).unwrap_or(90);
        let segments = make_synthetic_transcription(minutes, 8);
        let map_reduce = MapReduceSummarizer::new(Arc::clone(&qwen) as Arc<dyn Summarizer>);

        // RSS is sampled from a side thread: the peak is reached inside a forward pass.
        let peak = Arc::new(AtomicU64::new(0));
        let stop = Arc::new(AtomicBool::new(false));
        let sampler = {
            let (peak, stop) = (Arc::clone(&peak), Arc::clone(&stop));
            std::thread::spawn(move || {
                while !stop.load(Ordering::Relaxed) {
                    peak.fetch_max(vad_core::get_process_rss_bytes().unwrap_or(0), Ordering::Relaxed);
                    std::thread::sleep(std::time::Duration::from_millis(200));
                }
            })
        };

        let t0 = Instant::now();
        let mut events = Vec::new();
        let result = map_reduce.summarize(
            &segments,
            Some(|p: SummarizeProgress| {
                println!("[{:>6.0}s] {}", t0.elapsed().as_secs_f64(), p.display_message());
                events.push(p)
            }),
            None,
        );
        stop.store(true, Ordering::Relaxed);
        sampler.join().unwrap();
        println!(
            "elapsed {:.0}s | RSS baseline {:.0} MiB, model loaded {:.0} MiB, peak {:.0} MiB",
            t0.elapsed().as_secs_f64(), mib(rss_base), mib(rss_loaded), mib(peak.load(Ordering::Relaxed))
        );
        let summary = result.expect("real map-reduce (a ContextExceeded here means the criterion fails)");
        println!("chunks: {}\n---\n{}", summary.total_chunks, summary.markdown);
        assert!(summary.total_chunks >= 2 || minutes < 20);
        assert_eq!(events.last(), Some(&SummarizeProgress::Done));
        assert!(summary.markdown.trim().len() > 200, "summary is implausibly short");
    }
}
