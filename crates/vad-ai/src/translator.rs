use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tracing::{debug, info};
use vad_core::VadError;

use crate::llm_provider::Summarizer;
use crate::whisper::TranscriptionSegment;

/// Supported target languages for translation per PLANO_VAD.md §4.1.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TargetLanguage {
    English,
    Spanish,
    French,
    German,
    Italian,
    Portuguese,
}

impl TargetLanguage {
    pub const ALL: [TargetLanguage; 6] = [
        TargetLanguage::English,
        TargetLanguage::Spanish,
        TargetLanguage::French,
        TargetLanguage::German,
        TargetLanguage::Italian,
        TargetLanguage::Portuguese,
    ];

    pub fn display_name(&self) -> &'static str {
        match self {
            Self::English => "Inglês",
            Self::Spanish => "Espanhol",
            Self::French => "Francês",
            Self::German => "Alemão",
            Self::Italian => "Italiano",
            Self::Portuguese => "Português",
        }
    }

    pub fn iso_code(&self) -> &'static str {
        match self {
            Self::English => "en",
            Self::Spanish => "es",
            Self::French => "fr",
            Self::German => "de",
            Self::Italian => "it",
            Self::Portuguese => "pt",
        }
    }
}

/// Translator reusing the LLM provider for multilingual translation (§4.1).
pub struct LlmTranslator {
    summarizer: Arc<dyn Summarizer>,
}

impl LlmTranslator {
    pub fn new(summarizer: Arc<dyn Summarizer>) -> Self {
        Self { summarizer }
    }

    /// Translates a single text string (e.g. an executive summary or markdown document).
    pub fn translate_text(
        &self,
        text: &str,
        source_lang: &str,
        target_lang: TargetLanguage,
    ) -> Result<String, VadError> {
        self.summarizer
            .translate_text(text, source_lang, target_lang.display_name())
    }

    /// Translates a collection of transcription segments while preserving timestamp metadata.
    ///
    /// Segments are grouped into batches to reduce LLM overhead while respecting context limits.
    /// Emits progress `(completed_segments, total_segments)`.
    pub fn translate_segments<P>(
        &self,
        segments: &[TranscriptionSegment],
        source_lang: &str,
        target_lang: TargetLanguage,
        mut progress_cb: Option<P>,
        abort_flag: Option<Arc<AtomicBool>>,
    ) -> Result<Vec<TranscriptionSegment>, VadError>
    where
        P: FnMut(usize, usize),
    {
        if segments.is_empty() {
            return Ok(Vec::new());
        }

        let total_segments = segments.len();
        info!(
            "Starting translation of {} segments from {} to {}",
            total_segments,
            source_lang,
            target_lang.display_name()
        );

        let mut translated_segments = Vec::with_capacity(total_segments);
        // Batch size of 10 segments per prompt provides a good balance between prompt overhead and reliability
        let batch_size = 10;

        for (batch_idx, chunk) in segments.chunks(batch_size).enumerate() {
            if let Some(ref abort) = abort_flag {
                if abort.load(Ordering::Relaxed) {
                    debug!("Translation aborted by user request at batch {}", batch_idx);
                    return Err(VadError::LlmCancelled);
                }
            }

            // Build numbered prompt for the batch
            let mut prompt_lines = Vec::new();
            for (i, seg) in chunk.iter().enumerate() {
                prompt_lines.push(format!("[{}] {}", i + 1, seg.text.trim()));
            }
            let batch_input = prompt_lines.join("\n");

            let translation_result = self.summarizer.translate_text(
                &batch_input,
                source_lang,
                target_lang.display_name(),
            )?;

            // Parse translated lines back to segments
            let translated_lines = parse_numbered_translation(&translation_result, chunk.len());

            for (i, seg) in chunk.iter().enumerate() {
                let text = translated_lines
                    .get(i)
                    .cloned()
                    .unwrap_or_else(|| seg.text.clone());

                translated_segments.push(TranscriptionSegment {
                    start_ms: seg.start_ms,
                    end_ms: seg.end_ms,
                    text,
                });
            }

            if let Some(ref mut cb) = progress_cb {
                cb(translated_segments.len(), total_segments);
            }
        }

        Ok(translated_segments)
    }
}

/// Parses numbered lines from LLM translation output: `[1] Translation...`
fn parse_numbered_translation(output: &str, expected_count: usize) -> Vec<String> {
    let mut results = vec![String::new(); expected_count];
    let mut fallback_lines = Vec::new();

    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        // Try matching `[1]` or `1.`
        if let Some(after_bracket) = trimmed.strip_prefix('[') {
            if let Some((num_str, rest)) = after_bracket.split_once(']') {
                if let Ok(num) = num_str.trim().parse::<usize>() {
                    if num > 0 && num <= expected_count {
                        results[num - 1] = rest.trim().to_string();
                        continue;
                    }
                }
            }
        } else if let Some((num_str, rest)) = trimmed.split_once('.') {
            if let Ok(num) = num_str.trim().parse::<usize>() {
                if num > 0 && num <= expected_count {
                    results[num - 1] = rest.trim().to_string();
                    continue;
                }
            }
        }

        fallback_lines.push(trimmed.to_string());
    }

    // Fallback: fill any unpopulated slots with sequential lines if available
    let mut fallback_iter = fallback_lines.into_iter();
    for slot in results.iter_mut() {
        if slot.is_empty() {
            if let Some(line) = fallback_iter.next() {
                *slot = line;
            }
        }
    }

    results
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::llm_provider::{AiPrivacyBadge, MockSummarizer};

    #[test]
    fn test_target_language_properties() {
        assert_eq!(TargetLanguage::English.display_name(), "Inglês");
        assert_eq!(TargetLanguage::English.iso_code(), "en");
        assert_eq!(TargetLanguage::Spanish.display_name(), "Espanhol");
        assert_eq!(TargetLanguage::Spanish.iso_code(), "es");
        assert_eq!(TargetLanguage::ALL.len(), 6);
    }

    #[test]
    fn test_parse_numbered_translation() {
        let output = "[1] Good morning everyone.\n[2] Welcome to the meeting.\n[3] Let's begin.";
        let parsed = parse_numbered_translation(output, 3);
        assert_eq!(parsed.len(), 3);
        assert_eq!(parsed[0], "Good morning everyone.");
        assert_eq!(parsed[1], "Welcome to the meeting.");
        assert_eq!(parsed[2], "Let's begin.");
    }

    #[test]
    fn test_translate_segments_with_mock() {
        let summarizer = Arc::new(MockSummarizer::new(1024, 256, AiPrivacyBadge::Local));
        let translator = LlmTranslator::new(summarizer);

        let segments = vec![
            TranscriptionSegment {
                start_ms: 0,
                end_ms: 3000,
                text: "Bom dia a todos.".to_string(),
            },
            TranscriptionSegment {
                start_ms: 3000,
                end_ms: 6000,
                text: "Apresentamos o relatório trimestral.".to_string(),
            },
        ];

        let mut progress_count = 0;
        let res = translator.translate_segments(
            &segments,
            "Português",
            TargetLanguage::English,
            Some(|curr, total| {
                progress_count = curr;
                assert_eq!(total, 2);
            }),
            None,
        );

        assert!(res.is_ok());
        let translated = res.unwrap();
        assert_eq!(translated.len(), 2);
        assert_eq!(progress_count, 2);
        // Verify timestamps are preserved exactly
        assert_eq!(translated[0].start_ms, 0);
        assert_eq!(translated[0].end_ms, 3000);
        assert_eq!(translated[1].start_ms, 3000);
        assert_eq!(translated[1].end_ms, 6000);
    }

    #[test]
    fn test_translate_cancellation() {
        let summarizer = Arc::new(MockSummarizer::new(1024, 256, AiPrivacyBadge::Local));
        let translator = LlmTranslator::new(summarizer);

        let segments = vec![TranscriptionSegment {
            start_ms: 0,
            end_ms: 2000,
            text: "Teste".to_string(),
        }];

        let abort = Arc::new(AtomicBool::new(true));
        let res = translator.translate_segments(
            &segments,
            "Português",
            TargetLanguage::French,
            None::<fn(usize, usize)>,
            Some(abort),
        );

        assert!(res.is_err());
        match res.unwrap_err() {
            VadError::LlmCancelled => {}
            other => panic!("Expected LlmCancelled, got {:?}", other),
        }
    }
}
