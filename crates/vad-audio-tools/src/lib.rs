//! VAD Audio Tools crate (Waveform pyramid, Clip export).

pub mod clip_export;
pub mod waveform_pyramid;

pub use clip_export::{
    build_ffmpeg_clip_command, export_clip_async, format_timestamp, sanitize_path,
    validate_options, ClipExportHandle, ClipExportOptions, ClipExportStatus,
};
pub use waveform_pyramid::{MinMaxPoint, WaveformPyramid, ZoomLevel, TARGET_VISIBLE_POINTS};

