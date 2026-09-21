//! VAD AI crate (Whisper, Audio Extraction, Model Management).

pub mod extractor;
pub mod llm_provider;
pub mod model_manager;
pub mod summarizer;
pub mod translator;
pub mod vad_detector;
pub mod whisper;

pub use llm_provider::{
    AiPrivacyBadge, LlmProviderConfig, LocalQwenSummarizer, MockSummarizer, Summarizer,
    DEFAULT_QWEN_CONTEXT_WINDOW, DEFAULT_QWEN_MAX_OUTPUT_TOKENS, DEFAULT_QWEN_MODEL_FILENAME,
    DEFAULT_QWEN_TOKENIZER_FILENAME,
};
pub use summarizer::{
    MapReduceSummarizer, MeetingSummary, PartialSummary, SummarizeProgress, TranscriptionChunk,
};
pub use translator::{LlmTranslator, TargetLanguage};

pub use extractor::{
    AudioExtractionProgress, AudioExtractor, ExtractionHandle, ExtractionStatus, PcmAudio,
    CHANNELS, MAX_AUDIO_DURATION_SECS, MAX_AUDIO_SAMPLES, SAMPLE_RATE,
};
pub use model_manager::{
    find_llm_preset, find_preset, DiskModelInfo, ModelManager, ModelPreset, ModelSource,
    DISK_TOOLTIP, PRESET_LLM_MODELS, PRESET_MODELS, RAM_ONLY_TOOLTIP,
};
pub use vad_detector::{
    SilenceSegment, SpeechSegment, VadDetectionResult, VadDetector, VadParams,
};
pub use whisper::{TranscriptionSegment, WhisperEngine};
