//! System tray via `StatusNotifierItem` (`ksni`), PLANO_VAD.md §4.8 — **best effort**.
//!
//! KDE and most other desktops show it. **GNOME does not show tray icons unless the user
//! installs the "AppIndicator and KStatusNotifierItem Support" extension**; without it
//! this integration runs but nothing is visible. The app says so (see [`GNOME_NOTE`]) rather
//! than leaving the user to think it is broken.
//!
//! Deliberately *not* done: closing the window does not hide it to the tray. With the icon
//! invisible (GNOME) that would leave a running app with no way back.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use eframe::egui;
use ksni::blocking::TrayMethods;
use ksni::menu::{MenuItem, StandardItem};
use tracing::{info, warn};
use vad_core::platform::PlatformIntegration;
use vad_core::{PlaybackState, Player, VadError};

/// Shown in the app (tray tooltip and the help text next to the tray status).
pub const GNOME_NOTE: &str = "A bandeja do sistema não aparece no GNOME sem a extensão «AppIndicator and KStatusNotifierItem Support». No KDE funciona de origem.";

struct VadTray {
    player: Player,
    ctx: egui::Context,
    quit: Arc<AtomicBool>,
    playing: bool,
    window_visible: bool,
}

impl ksni::Tray for VadTray {
    fn id(&self) -> String {
        "vad".into()
    }

    fn title(&self) -> String {
        "VAD".into()
    }

    fn icon_name(&self) -> String {
        "multimedia-player".into()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: if self.playing { "VAD — a reproduzir".into() } else { "VAD".into() },
            description: String::new(),
            icon_name: String::new(),
            icon_pixmap: Vec::new(),
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        self.toggle_window();
    }

    fn menu(&self) -> Vec<MenuItem<Self>> {
        vec![
            StandardItem {
                label: if self.window_visible { "Esconder janela" } else { "Mostrar janela" }.into(),
                activate: Box::new(|t: &mut Self| t.toggle_window()),
                ..Default::default()
            }
            .into(),
            StandardItem {
                label: if self.playing { "Pausar" } else { "Reproduzir" }.into(),
                activate: Box::new(|t: &mut Self| {
                    if let Err(e) = t.player.toggle_pause() {
                        warn!("tray: play/pause failed: {e}");
                    }
                }),
                ..Default::default()
            }
            .into(),
            MenuItem::Separator,
            StandardItem {
                label: "Sair".into(),
                activate: Box::new(|t: &mut Self| {
                    t.quit.store(true, Ordering::Relaxed);
                    // Wake the UI thread so it notices `quit_requested()` even if idle.
                    t.ctx.request_repaint();
                }),
                ..Default::default()
            }
            .into(),
        ]
    }
}

impl VadTray {
    fn toggle_window(&mut self) {
        self.window_visible = !self.window_visible;
        self.ctx.send_viewport_cmd(egui::ViewportCommand::Visible(self.window_visible));
        if self.window_visible {
            self.ctx.send_viewport_cmd(egui::ViewportCommand::Focus);
        }
        self.ctx.request_repaint();
    }
}

/// Linux tray integration. Creating it never fails the app: with no D-Bus session or no
/// StatusNotifierWatcher it returns an error that the caller logs, and the app goes on.
pub struct TrayIntegration {
    handle: ksni::blocking::Handle<VadTray>,
    quit: Arc<AtomicBool>,
}

impl TrayIntegration {
    pub fn new(player: Player, ctx: egui::Context) -> Result<Self, VadError> {
        let quit = Arc::new(AtomicBool::new(false));
        let tray = VadTray { player, ctx, quit: Arc::clone(&quit), playing: false, window_visible: true };
        let handle = tray
            .spawn()
            .map_err(|e| VadError::Platform(format!("bandeja indisponível ({e}); {GNOME_NOTE}")))?;
        info!("system tray registered (visible only if the desktop shows StatusNotifierItems)");
        Ok(Self { handle, quit })
    }
}

impl PlatformIntegration for TrayIntegration {
    fn name(&self) -> &'static str {
        "tray"
    }

    fn on_playback_state(&mut self, state: PlaybackState) -> Result<(), VadError> {
        let playing = state == PlaybackState::Playing;
        let _ = self.handle.update(|t| t.playing = playing);
        Ok(())
    }

    fn quit_requested(&self) -> bool {
        self.quit.load(Ordering::Relaxed)
    }

    fn shutdown(&mut self) -> Result<(), VadError> {
        self.handle.shutdown();
        Ok(())
    }
}
