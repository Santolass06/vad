//! VAD AI crate (Whisper, Audio Extraction, Model Management).

pub mod extractor;
pub mod model_manager;
pub mod vad_detector;
pub mod whisper;

pub use extractor::{
    AudioExtractionProgress, AudioExtractor, ExtractionHandle, ExtractionStatus, PcmAudio,
    CHANNELS, MAX_AUDIO_DURATION_SECS, MAX_AUDIO_SAMPLES, SAMPLE_RATE,
};
pub use model_manager::{
    find_preset, DiskModelInfo, ModelManager, ModelPreset, ModelSource, DISK_TOOLTIP,
    PRESET_MODELS, RAM_ONLY_TOOLTIP,
};
pub use vad_detector::{
    SilenceSegment, SpeechSegment, VadDetectionResult, VadDetector, VadParams,
};
pub use whisper::{TranscriptionSegment, WhisperEngine};
