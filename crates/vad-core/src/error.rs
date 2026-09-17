use thiserror::Error;

/// Main error type for `vad-core` operations.
#[derive(Debug, Error)]
pub enum VadError {
    #[error("libmpv error: {0}")]
    Mpv(#[from] libmpv2::Error),

    #[error("Render callback panic caught: {0}")]
    RenderCallbackPanic(String),

    #[error("Property error: {0}")]
    Property(String),

    #[error("Playback error: {0}")]
    Playback(String),

    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
}
