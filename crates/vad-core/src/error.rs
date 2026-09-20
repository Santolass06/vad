use thiserror::Error;

/// Severity level for error actions and degradation states.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ErrorSeverity {
    Fatal,
    Degraded,
    Warning,
    Info,
}

/// Actionable metadata mapped from a `VadError` per PLANO_VAD.md §4.14.
/// Defines how the UI should react (severity, user message, suggested install command,
/// disabled features, and whether the dialog/action can be ignored).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ErrorAction {
    pub severity: ErrorSeverity,
    pub title: &'static str,
    pub description: &'static str,
    pub install_command: Option<&'static str>,
    pub disabled_features: &'static [&'static str],
    pub can_ignore: bool,
}

/// Main error type for `vad-core` operations and subsystem degradation.
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

    #[error("FFmpeg not found in PATH")]
    FfmpegNotFound,

    #[error("yt-dlp not found in PATH")]
    YtDlpNotFound,

    #[error("OpenGL context unavailable: {0}")]
    GlContextUnavailable(String),

    #[error("Player initialization failed: {0}")]
    PlayerInitFailed(String),

    #[error("Hardware decoding unavailable: {0}")]
    HwDecUnavailable(String),

    #[error("Audio extraction failed: {0}")]
    ExtractionFailed(String),

    #[error("Model download failed: {0}")]
    ModelDownloadFailed(String),

    #[error("Platform integration error: {0}")]
    Platform(String),

    #[error("Configuration error: {0}")]
    Config(String),

    #[error("Invalid URL scheme: {0}")]
    InvalidUrlScheme(String),

    #[error("Whisper error: {0}")]
    Whisper(String),

    #[error("Whisper callback panic caught: {0}")]
    WhisperCallbackPanic(String),
}

impl VadError {
    /// Returns the UI action and degradation policy for this error per PLANO_VAD.md §4.14.
    pub fn action(&self) -> ErrorAction {
        match self {
            VadError::FfmpegNotFound => ErrorAction {
                severity: ErrorSeverity::Degraded,
                title: "Dependência em falta: FFmpeg",
                description: "Necessário para waveform, transcrição e corte de clips.",
                install_command: Some("sudo apt install ffmpeg"),
                disabled_features: &["waveform", "whisper", "clip_export"],
                can_ignore: true,
            },
            VadError::YtDlpNotFound => ErrorAction {
                severity: ErrorSeverity::Degraded,
                title: "Dependência em falta: yt-dlp",
                description: "Necessário para reprodução por URL (YouTube, Twitch).",
                install_command: Some("sudo apt install yt-dlp"),
                disabled_features: &["url_playback"],
                can_ignore: true,
            },
            VadError::GlContextUnavailable(_) => ErrorAction {
                severity: ErrorSeverity::Fatal,
                title: "Contexto Gráfico OpenGL Indisponível",
                description: "O contexto OpenGL (glow) não pôde ser inicializado pelo eframe.",
                install_command: None,
                disabled_features: &["video_rendering"],
                can_ignore: false,
            },
            VadError::PlayerInitFailed(_) => ErrorAction {
                severity: ErrorSeverity::Fatal,
                title: "Falha na Inicialização do Motor mpv",
                description: "Não foi possível inicializar a biblioteca libmpv.",
                install_command: Some("sudo apt install libmpv-dev"),
                disabled_features: &["playback"],
                can_ignore: false,
            },
            VadError::HwDecUnavailable(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Aceleração por Hardware Indisponível",
                description: "A usar descodificação por software (CPU). Drivers VA-API/NVDEC não detetados.",
                install_command: Some("sudo apt install intel-media-va-driver"),
                disabled_features: &["hardware_decoding"],
                can_ignore: true,
            },
            VadError::ExtractionFailed(_) => ErrorAction {
                severity: ErrorSeverity::Degraded,
                title: "Falha na Extração de Áudio",
                description: "Ocorreu um erro durante a extração de PCM com FFmpeg.",
                install_command: None,
                disabled_features: &["waveform", "whisper"],
                can_ignore: true,
            },
            VadError::ModelDownloadFailed(_) => ErrorAction {
                severity: ErrorSeverity::Degraded,
                title: "Falha no Download do Modelo",
                description: "Não foi possível descarregar o modelo do Whisper.",
                install_command: None,
                disabled_features: &["whisper"],
                can_ignore: true,
            },
            VadError::RenderCallbackPanic(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Pânico no Callback de Renderização",
                description: "Um pânico no callback FFI foi capturado com segurança.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Mpv(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro do Motor mpv",
                description: "O libmpv reportou um erro ao executar um comando ou aceder a uma propriedade.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Property(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro de Propriedade",
                description: "Uma propriedade do mpv não pôde ser lida ou escrita.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Playback(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro de Reprodução",
                description: "Ocorreu um erro durante a reprodução do ficheiro atual.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Io(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro de E/S",
                description: "Ocorreu um erro de entrada/saída ao aceder a um ficheiro ou recurso.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Platform(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro de Integração com o Sistema",
                description: "Falha na comunicação com serviços de sistema operativo (D-Bus/MPRIS/Screensaver).",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Config(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro de Configuração",
                description: "Falha ao ler, escrever ou serializar ficheiro de configuração/histórico.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::InvalidUrlScheme(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "URL Inválido",
                description: "Esquema de URL não permitido. Apenas http://, https:// e rtsp:// são suportados.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
            VadError::Whisper(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Erro no Whisper AI",
                description: "Ocorreu um erro durante o processamento ou transcrição com o Whisper.",
                install_command: None,
                disabled_features: &["whisper"],
                can_ignore: true,
            },
            VadError::WhisperCallbackPanic(_) => ErrorAction {
                severity: ErrorSeverity::Warning,
                title: "Pânico no Callback do Whisper",
                description: "Um pânico no callback FFI do Whisper foi capturado com segurança.",
                install_command: None,
                disabled_features: &[],
                can_ignore: true,
            },
        }
    }
}
