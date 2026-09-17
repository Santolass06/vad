mod app;
mod render;

use app::VadApp;
use eframe::egui;
use tracing::info;

fn main() -> eframe::Result<()> {
    tracing_subscriber::fmt::init();
    info!("Starting VAD Video Player (M0.5 Prototype)");

    let initial_file = std::env::args().nth(1);

    let native_options = eframe::NativeOptions {
        renderer: eframe::Renderer::Glow,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 720.0])
            .with_min_inner_size([640.0, 360.0])
            .with_title("VAD - Video Audio Decoder (M0.5 Gate Prototype)"),
        ..Default::default()
    };

    eframe::run_native(
        "VAD Video Player",
        native_options,
        Box::new(move |cc| Ok(Box::new(VadApp::new(cc, initial_file)))),
    )
}
