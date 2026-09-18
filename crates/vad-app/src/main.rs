mod app;
mod panels;
mod probe;
mod render;

#[cfg(target_os = "linux")]
mod mpris;
#[cfg(target_os = "linux")]
mod screensaver;

use app::VadApp;
use clap::Parser;
use eframe::egui;
use probe::probe_dependencies;
use tracing::{info, warn};

/// VAD - Video Audio Decoder desktop media player.
#[derive(Parser, Debug)]
#[command(
    name = "vad",
    version,
    about = "VAD - Video Audio Decoder (M1 Prototype)"
)]
pub struct CliArgs {
    /// Media file or URL to open on launch
    #[arg(value_name = "FILE")]
    pub file: Option<String>,

    /// Start application in fullscreen mode
    #[arg(short, long)]
    pub fullscreen: bool,
}

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting VAD Video Player (M1)");

    let args = CliArgs::parse();

    // Probe runtime dependencies (ffmpeg and yt-dlp per §4.14 / Task 4)
    let missing_deps = probe_dependencies();
    if !missing_deps.is_empty() {
        for err in &missing_deps {
            let action = err.action();
            warn!(
                "Dependency missing: {} - {}. Suggested fix: {:?}",
                action.title, action.description, action.install_command
            );
        }
    }

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([640.0, 360.0])
            .with_title("VAD - Video Audio Decoder")
            .with_fullscreen(args.fullscreen),
        ..Default::default()
    };

    let initial_file = args.file;

    eframe::run_native(
        "VAD Video Player",
        native_options,
        Box::new(move |cc| Ok(Box::new(VadApp::new(cc, initial_file, missing_deps)))),
    )
}
