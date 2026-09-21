//! VAD Audio Tools crate (Waveform pyramid, Clip export).

pub mod clip_export;
pub mod waveform_pyramid;

pub use clip_export::{
    build_ffmpeg_clip_command, export_clip_async, format_timestamp, keyframe_at_or_before,
    probe_duration, probe_keyframes, probe_keyframes_async, recode_flags, resolve_output_path, sanitize_path,
    validate_options, ClipExportHandle, ClipExportOptions, ClipExportStatus, KeyframeProbe,
};
pub use waveform_pyramid::{MinMaxPoint, WaveformPyramid, ZoomLevel, TARGET_VISIBLE_POINTS};
