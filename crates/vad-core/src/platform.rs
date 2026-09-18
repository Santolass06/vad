use crate::error::VadError;
use crate::state::{PlaybackState, PlayerEvent};

/// Common trait defining the interface for OS-level platform integrations
/// (e.g. MPRIS media keys, screensaver inhibition, system tray).
///
/// Implemented per PLANO_VAD.md §4.25 to decouple system-specific APIs
/// from the core playback engine and UI panels.
pub trait PlatformIntegration: Send {
    /// Human-readable identifier for diagnostics and logging (e.g. "mpris", "screensaver").
    fn name(&self) -> &'static str;

    /// Dispatches discrete player events to this platform integration.
    ///
    /// By default, routes to specific handler methods (`on_playback_state`, `on_file_loaded`, etc.).
    fn on_event(&mut self, event: &PlayerEvent) -> Result<(), VadError> {
        match event {
            PlayerEvent::PlaybackStateChanged(state) => self.on_playback_state(*state),
            PlayerEvent::FileLoaded {
                path,
                title,
                duration,
            } => self.on_file_loaded(path, title.as_deref(), *duration),
            PlayerEvent::SeekOccurred(pos) => self.on_seek(*pos),
            PlayerEvent::VolumeChanged(vol) => self.on_volume_changed(*vol),
            PlayerEvent::MutedChanged(muted) => self.on_mute_changed(*muted),
            _ => Ok(()),
        }
    }

    /// Invoked when playback state transitions (Playing, Paused, Idle).
    fn on_playback_state(&mut self, state: PlaybackState) -> Result<(), VadError> {
        let _ = state;
        Ok(())
    }

    /// Invoked when a new media file is loaded and metadata is available.
    fn on_file_loaded(
        &mut self,
        path: &str,
        title: Option<&str>,
        duration: Option<f64>,
    ) -> Result<(), VadError> {
        let _ = (path, title, duration);
        Ok(())
    }

    /// Invoked when playback position changes via seek.
    fn on_seek(&mut self, position_secs: f64) -> Result<(), VadError> {
        let _ = position_secs;
        Ok(())
    }

    /// Invoked when playback volume changes (0.0 .. 100.0).
    fn on_volume_changed(&mut self, volume: f64) -> Result<(), VadError> {
        let _ = volume;
        Ok(())
    }

    /// Invoked when mute state changes.
    fn on_mute_changed(&mut self, muted: bool) -> Result<(), VadError> {
        let _ = muted;
        Ok(())
    }

    /// Periodic hook called from main event loop if integration needs ticking.
    fn update(&mut self) -> Result<(), VadError> {
        Ok(())
    }

    /// Explicit cleanup hook called on application shutdown.
    fn shutdown(&mut self) -> Result<(), VadError> {
        Ok(())
    }
}
