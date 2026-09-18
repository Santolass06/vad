//! Screensaver inhibition implementation for Linux.
//!
//! Implements `org.freedesktop.ScreenSaver.Inhibit` via `zbus` per PLANO_VAD.md §4.7
//! and Sprint_Planning_03. Inhibit is active exclusively during video/audio playback,
//! and released when paused or stopped.

use tracing::{debug, info, warn};
use vad_core::platform::PlatformIntegration;
use vad_core::{PlaybackState, VadError};
use zbus::proxy;

#[proxy(
    default_service = "org.freedesktop.ScreenSaver",
    default_path = "/org/freedesktop/ScreenSaver",
    interface = "org.freedesktop.ScreenSaver"
)]
trait ScreenSaverDbus {
    /// Inhibits screensaver/display power saving.
    fn inhibit(&self, application_name: &str, reason_for_inhibit: &str) -> zbus::Result<u32>;

    /// Releases a previously requested screensaver inhibition.
    fn un_inhibit(&self, cookie: u32) -> zbus::Result<()>;
}

/// Linux platform integration for screensaver inhibition.
pub struct ScreenSaverInhibitor {
    connection: Option<zbus::connection::Connection>,
    cookie: Option<u32>,
}

impl ScreenSaverInhibitor {
    pub fn new() -> Result<Self, VadError> {
        let connection = match zbus::block_on(async { zbus::connection::Connection::session().await }) {
            Ok(conn) => Some(conn),
            Err(e) => {
                warn!(
                    "D-Bus session unavailable for screensaver inhibitor: {:?} (degraded mode)",
                    e
                );
                None
            }
        };

        Ok(Self {
            connection,
            cookie: None,
        })
    }

    fn do_inhibit(&mut self) {
        if self.cookie.is_some() {
            return;
        }

        if let Some(ref conn) = self.connection {
            let conn_clone = conn.clone();
            let result = zbus::block_on(async move {
                let proxy = ScreenSaverDbusProxy::new(&conn_clone).await?;
                proxy.inhibit("vad", "Reprodução de multimédia").await
            });

            match result {
                Ok(cookie) => {
                    info!("Screensaver inhibition active (cookie: {})", cookie);
                    self.cookie = Some(cookie);
                }
                Err(e) => {
                    debug!("Could not inhibit screensaver via D-Bus: {:?}", e);
                }
            }
        }
    }

    fn do_un_inhibit(&mut self) {
        let Some(cookie) = self.cookie else {
            return;
        };

        if let Some(ref conn) = self.connection {
            let conn_clone = conn.clone();
            let result = zbus::block_on(async move {
                let proxy = ScreenSaverDbusProxy::new(&conn_clone).await?;
                proxy.un_inhibit(cookie).await
            });

            // Only clear the cookie once release actually succeeds — on a
            // transient D-Bus failure we keep it so a later pause/stop retries
            // instead of leaking the inhibition for the rest of the session.
            match result {
                Ok(()) => {
                    info!("Screensaver inhibition released (cookie: {})", cookie);
                    self.cookie = None;
                }
                Err(e) => {
                    debug!("Could not un-inhibit screensaver (cookie {}): {:?}", cookie, e);
                }
            }
        }
    }
}

impl PlatformIntegration for ScreenSaverInhibitor {
    fn name(&self) -> &'static str {
        "screensaver"
    }

    fn on_playback_state(&mut self, state: PlaybackState) -> Result<(), VadError> {
        match state {
            PlaybackState::Playing => {
                self.do_inhibit();
            }
            PlaybackState::Paused | PlaybackState::Idle => {
                self.do_un_inhibit();
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), VadError> {
        self.do_un_inhibit();
        Ok(())
    }
}

impl Drop for ScreenSaverInhibitor {
    fn drop(&mut self) {
        self.do_un_inhibit();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_screensaver_inhibitor_state_transitions() {
        let mut inhibitor = ScreenSaverInhibitor::new().expect("Failed to create inhibitor");
        assert_eq!(inhibitor.name(), "screensaver");

        // When starting or idle, cookie is None
        assert_eq!(inhibitor.cookie, None);

        // Transition to Playing -> triggers inhibit
        inhibitor
            .on_playback_state(PlaybackState::Playing)
            .expect("on_playback_state Playing failed");

        // Transition to Paused -> triggers un-inhibit
        inhibitor
            .on_playback_state(PlaybackState::Paused)
            .expect("on_playback_state Paused failed");
        assert_eq!(inhibitor.cookie, None);

        // Transition to Idle -> triggers un-inhibit
        inhibitor
            .on_playback_state(PlaybackState::Idle)
            .expect("on_playback_state Idle failed");
        assert_eq!(inhibitor.cookie, None);

        // Shutdown releases cookie
        inhibitor.shutdown().expect("shutdown failed");
        assert_eq!(inhibitor.cookie, None);
    }
}
