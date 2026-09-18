//! MPRIS (Media Player Remote Interfacing Specification) v2 implementation for Linux.
//!
//! Exposes `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` via `zbus`
//! per PLANO_VAD.md §4.25 and Sprint_Planning_03.

use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use tracing::{debug, info, warn};
use vad_core::platform::PlatformIntegration;
use vad_core::{PlaybackState, Player, SharedPlayerState, VadError};

use zbus::interface;
use zbus::zvariant::{ObjectPath, Value};

/// Internal metadata and status state shared between MPRIS D-Bus interface and player events.
#[derive(Debug, Clone)]
struct MprisState {
    playback_status: String,
    title: String,
    url: String,
    duration_micros: i64,
    volume: f64,
}

impl Default for MprisState {
    fn default() -> Self {
        Self {
            playback_status: "Stopped".to_string(),
            title: String::new(),
            url: String::new(),
            duration_micros: 0,
            volume: 1.0,
        }
    }
}

/// Root MPRIS interface `org.mpris.MediaPlayer2`.
struct MprisRoot;

#[interface(name = "org.mpris.MediaPlayer2")]
impl MprisRoot {
    #[zbus(property)]
    fn can_quit(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_raise(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn has_track_list(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn identity(&self) -> &str {
        "VAD Video Player"
    }

    #[zbus(property)]
    fn supported_uri_schemes(&self) -> Vec<String> {
        vec![
            "file".to_string(),
            "http".to_string(),
            "https".to_string(),
            "rtsp".to_string(),
        ]
    }

    #[zbus(property)]
    fn supported_mime_types(&self) -> Vec<String> {
        vec![
            "video/mp4".to_string(),
            "video/x-matroska".to_string(),
            "video/quicktime".to_string(),
            "video/webm".to_string(),
            "audio/mpeg".to_string(),
            "audio/aac".to_string(),
            "audio/flac".to_string(),
            "audio/wav".to_string(),
            "audio/ogg".to_string(),
        ]
    }

    fn raise(&self) {
        debug!("MPRIS Root.Raise invoked");
    }

    fn quit(&self) {
        info!("MPRIS Root.Quit invoked, scheduling clean application exit");
        std::thread::spawn(|| {
            std::thread::sleep(std::time::Duration::from_millis(50));
            std::process::exit(0);
        });
    }
}

/// Player MPRIS interface `org.mpris.MediaPlayer2.Player`.
struct MprisPlayer {
    player: Player,
    shared_state: Arc<SharedPlayerState>,
    state: Arc<RwLock<MprisState>>,
}

#[interface(name = "org.mpris.MediaPlayer2.Player")]
impl MprisPlayer {
    #[zbus(property)]
    fn playback_status(&self) -> String {
        self.state
            .read()
            .map(|s| s.playback_status.clone())
            .unwrap_or_else(|_| "Stopped".to_string())
    }

    #[zbus(property)]
    fn loop_status(&self) -> &str {
        "None"
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.player.speed().unwrap_or(1.0)
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn metadata(&self) -> HashMap<String, Value<'_>> {
        let mut map = HashMap::new();
        let guard = self.state.read().ok();

        let track_id = ObjectPath::try_from("/org/mpris/MediaPlayer2/TrackList/NoTrack")
            .unwrap_or_else(|_| ObjectPath::from_static_str_unchecked("/"));
        map.insert("mpris:trackid".to_string(), Value::from(track_id));

        if let Some(ref s) = guard {
            if !s.title.is_empty() {
                map.insert("xesam:title".to_string(), Value::from(s.title.clone()));
            }
            if !s.url.is_empty() {
                map.insert("xesam:url".to_string(), Value::from(s.url.clone()));
            }
            if s.duration_micros > 0 {
                map.insert("mpris:length".to_string(), Value::from(s.duration_micros));
            }
        }

        map
    }

    #[zbus(property)]
    fn volume(&self) -> f64 {
        self.state
            .read()
            .map(|s| s.volume)
            .unwrap_or_else(|_| self.player.volume().unwrap_or(100.0) / 100.0)
    }

    #[zbus(property)]
    fn set_volume(&mut self, volume: f64) {
        let vol = volume.clamp(0.0, 1.0) * 100.0;
        let _ = self.player.set_volume(vol);
        if let Ok(mut guard) = self.state.write() {
            guard.volume = volume.clamp(0.0, 1.0);
        }
    }

    #[zbus(property)]
    fn position(&self) -> i64 {
        (self.shared_state.get_time_pos() * 1_000_000.0) as i64
    }

    #[zbus(property)]
    fn minimum_rate(&self) -> f64 {
        0.25
    }

    #[zbus(property)]
    fn maximum_rate(&self) -> f64 {
        4.0
    }

    #[zbus(property)]
    fn can_control(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_play(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_pause(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_seek(&self) -> bool {
        true
    }

    #[zbus(property)]
    fn can_go_next(&self) -> bool {
        false
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        false
    }

    fn next(&self) {
        debug!("MPRIS Player.Next invoked (single-file scope: no-op)");
    }

    fn previous(&self) {
        debug!("MPRIS Player.Previous invoked (seeking to start)");
        let _ = self.player.seek_absolute(0.0);
    }

    fn pause(&self) {
        debug!("MPRIS Player.Pause invoked");
        let _ = self.player.pause();
    }

    fn play_pause(&self) {
        debug!("MPRIS Player.PlayPause invoked");
        let _ = self.player.toggle_pause();
    }

    fn stop(&self) {
        debug!("MPRIS Player.Stop invoked");
        let _ = self.player.stop();
    }

    fn play(&self) {
        debug!("MPRIS Player.Play invoked");
        let _ = self.player.play();
    }

    fn seek(&self, offset_micros: i64) {
        let delta_secs = offset_micros as f64 / 1_000_000.0;
        debug!("MPRIS Player.Seek invoked: {} secs", delta_secs);
        let _ = self.player.seek_relative(delta_secs);
    }

    fn set_position(&self, _track_id: ObjectPath<'_>, position_micros: i64) {
        let pos_secs = position_micros as f64 / 1_000_000.0;
        debug!("MPRIS Player.SetPosition invoked: {} secs", pos_secs);
        let _ = self.player.seek_absolute(pos_secs);
    }

    fn open_uri(&self, uri: String) {
        debug!("MPRIS Player.OpenUri invoked: {}", uri);
        let _ = self.player.load_file(&uri);
    }

    #[zbus(signal)]
    async fn seeked(emitter: &zbus::object_server::SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

/// Linux MPRIS integration managing D-Bus connection and event dispatch.
pub struct MprisServer {
    state: Arc<RwLock<MprisState>>,
    connection: Option<zbus::connection::Connection>,
    bus_name: String,
}

impl MprisServer {
    pub fn new(player: Player, shared_state: Arc<SharedPlayerState>) -> Result<Self, VadError> {
        let state = Arc::new(RwLock::new(MprisState::default()));

        // Connect to session bus
        let (conn, bus_name) = match zbus::block_on(async {
            let primary_name = "org.mpris.MediaPlayer2.vad";
            let conn_builder = zbus::connection::Builder::session()
                .map_err(|e| VadError::Platform(format!("Failed to connect to D-Bus session: {e}")))?
                .serve_at("/org/mpris/MediaPlayer2", MprisRoot)
                .map_err(|e| VadError::Platform(format!("Failed to register MPRIS root: {e}")))?
                .serve_at(
                    "/org/mpris/MediaPlayer2",
                    MprisPlayer {
                        player,
                        shared_state,
                        state: Arc::clone(&state),
                    },
                )
                .map_err(|e| VadError::Platform(format!("Failed to register MPRIS player: {e}")))?;

            let built_conn = conn_builder
                .build()
                .await
                .map_err(|e| VadError::Platform(format!("Failed to build D-Bus connection: {e}")))?;

            // Try requesting primary name; if not primary owner (multiple instances), request .instance<pid>
            let chosen_name = if built_conn.request_name(primary_name).await.is_ok() {
                primary_name.to_string()
            } else {
                let inst_name = format!("org.mpris.MediaPlayer2.vad.instance{}", std::process::id());
                let _ = built_conn.request_name(inst_name.as_str()).await;
                inst_name
            };

            info!("MPRIS service active on bus name: {}", chosen_name);
            Ok::<_, VadError>((built_conn, chosen_name))
        }) {
            Ok(res) => (Some(res.0), res.1),
            Err(e) => {
                warn!("MPRIS initialization failed (degraded mode, D-Bus unavailable): {:?}", e);
                (None, String::new())
            }
        };

        Ok(Self {
            state,
            connection: conn,
            bus_name,
        })
    }

    /// Helper to emit property changed signal for MPRIS player interface
    fn notify_player_property_changed(&self, property_name: &str) {
        if let Some(ref conn) = self.connection {
            let conn_clone = conn.clone();
            let prop = property_name.to_string();
            // Emit properties changed via zbus object server in async task
            zbus::block_on(async move {
                if let Ok(iface_ref) = conn_clone
                    .object_server()
                    .interface::<_, MprisPlayer>("/org/mpris/MediaPlayer2")
                    .await
                {
                    let iface = iface_ref.get().await;
                    let emitter = iface_ref.signal_emitter();
                    match prop.as_str() {
                        "PlaybackStatus" => {
                            let _ = iface.playback_status_changed(emitter).await;
                        }
                        "Metadata" => {
                            let _ = iface.metadata_changed(emitter).await;
                        }
                        "Volume" => {
                            let _ = iface.volume_changed(emitter).await;
                        }
                        _ => {}
                    }
                }
            });
        }
    }

    fn notify_seeked(&self, position_secs: f64) {
        if let Some(ref conn) = self.connection {
            let conn_clone = conn.clone();
            let pos_micros = (position_secs * 1_000_000.0) as i64;
            zbus::block_on(async move {
                if let Ok(iface_ref) = conn_clone
                    .object_server()
                    .interface::<_, MprisPlayer>("/org/mpris/MediaPlayer2")
                    .await
                {
                    let _ = MprisPlayer::seeked(iface_ref.signal_emitter(), pos_micros).await;
                }
            });
        }
    }
}

impl PlatformIntegration for MprisServer {
    fn name(&self) -> &'static str {
        "mpris"
    }

    fn on_playback_state(&mut self, state: PlaybackState) -> Result<(), VadError> {
        let status = match state {
            PlaybackState::Playing => "Playing",
            PlaybackState::Paused => "Paused",
            PlaybackState::Idle => "Stopped",
        };

        if let Ok(mut guard) = self.state.write() {
            if guard.playback_status != status {
                guard.playback_status = status.to_string();
                drop(guard);
                self.notify_player_property_changed("PlaybackStatus");
            }
        }
        Ok(())
    }

    fn on_file_loaded(
        &mut self,
        path: &str,
        title: Option<&str>,
        duration: Option<f64>,
    ) -> Result<(), VadError> {
        let title_str = title
            .map(|t| t.to_string())
            .unwrap_or_else(|| {
                std::path::Path::new(path)
                    .file_name()
                    .map(|f| f.to_string_lossy().to_string())
                    .unwrap_or_else(|| path.to_string())
            });

        let url_str = if path.starts_with("http://")
            || path.starts_with("https://")
            || path.starts_with("rtsp://")
            || path.starts_with("file://")
        {
            path.to_string()
        } else {
            format!("file://{}", path)
        };

        let dur_micros = duration.map(|d| (d * 1_000_000.0) as i64).unwrap_or(0);

        if let Ok(mut guard) = self.state.write() {
            guard.title = title_str;
            guard.url = url_str;
            guard.duration_micros = dur_micros;
            drop(guard);
            self.notify_player_property_changed("Metadata");
        }
        Ok(())
    }

    fn on_seek(&mut self, position_secs: f64) -> Result<(), VadError> {
        self.notify_seeked(position_secs);
        Ok(())
    }

    fn on_volume_changed(&mut self, volume: f64) -> Result<(), VadError> {
        let vol_normalized = (volume / 100.0).clamp(0.0, 1.0);
        if let Ok(mut guard) = self.state.write() {
            if (guard.volume - vol_normalized).abs() > 0.001 {
                guard.volume = vol_normalized;
                drop(guard);
                self.notify_player_property_changed("Volume");
            }
        }
        Ok(())
    }

    fn on_mute_changed(&mut self, muted: bool) -> Result<(), VadError> {
        if let Ok(mut guard) = self.state.write() {
            if muted {
                guard.volume = 0.0;
            }
            drop(guard);
            self.notify_player_property_changed("Volume");
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), VadError> {
        if let Some(ref conn) = self.connection {
            if !self.bus_name.is_empty() {
                let bus_name = self.bus_name.clone();
                let conn_clone = conn.clone();
                let _ = zbus::block_on(async move {
                    let _ = conn_clone.release_name(bus_name).await;
                });
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpris_server_lifecycle_and_event_handling() {
        let player = Player::new().expect("Failed to initialize player");
        let shared_state = Arc::new(SharedPlayerState::new());

        let mut server = MprisServer::new(player, shared_state).expect("Failed to create MprisServer");
        assert_eq!(server.name(), "mpris");

        // 1. Test playback state update
        server
            .on_playback_state(PlaybackState::Playing)
            .expect("on_playback_state failed");
        assert_eq!(server.state.read().unwrap().playback_status, "Playing");

        server
            .on_playback_state(PlaybackState::Paused)
            .expect("on_playback_state failed");
        assert_eq!(server.state.read().unwrap().playback_status, "Paused");

        server
            .on_playback_state(PlaybackState::Idle)
            .expect("on_playback_state failed");
        assert_eq!(server.state.read().unwrap().playback_status, "Stopped");

        // 2. Test file loaded metadata update
        server
            .on_file_loaded("/path/to/sample_movie.mp4", Some("Sample Movie"), Some(150.0))
            .expect("on_file_loaded failed");
        {
            let guard = server.state.read().unwrap();
            assert_eq!(guard.title, "Sample Movie");
            assert_eq!(guard.url, "file:///path/to/sample_movie.mp4");
            assert_eq!(guard.duration_micros, 150_000_000);
        }

        // 3. Test volume updates
        server
            .on_volume_changed(75.0)
            .expect("on_volume_changed failed");
        assert!((server.state.read().unwrap().volume - 0.75).abs() < 0.01);

        server.on_mute_changed(true).expect("on_mute_changed failed");
        assert_eq!(server.state.read().unwrap().volume, 0.0);

        // 4. Test seek
        server.on_seek(42.0).expect("on_seek failed");

        // 5. Test shutdown
        server.shutdown().expect("shutdown failed");
    }
}
