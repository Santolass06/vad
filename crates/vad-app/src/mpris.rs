//! MPRIS (Media Player Remote Interfacing Specification) v2 implementation for Linux.
//!
//! Exposes `org.mpris.MediaPlayer2` and `org.mpris.MediaPlayer2.Player` via `zbus`
//! per PLANO_VAD.md §4.25 and Sprint_Planning_03.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::{sync_channel, Receiver, SyncSender};
use std::sync::{Arc, Mutex, RwLock};

use eframe::egui;
use tracing::{debug, info, warn};
use vad_core::platform::PlatformIntegration;
use vad_core::{PlaybackState, Player, Playlist, RepeatMode, SharedPlayerState, VadError};

use zbus::interface;
use zbus::zvariant::{ObjectPath, Value};

/// Validates a URI's scheme against exactly what `MprisRoot::supported_uri_schemes`
/// advertises, before it is ever handed to mpv — an external MPRIS controller must
/// not be able to reach schemes (`smb://`, `javascript:`, ...) that this player does
/// not claim to support.
fn is_allowed_mpris_uri(uri: &str) -> bool {
    const ALLOWED_SCHEMES: [&str; 4] = ["file://", "http://", "https://", "rtsp://"];
    ALLOWED_SCHEMES.iter().any(|scheme| uri.starts_with(scheme))
}

/// Internal metadata and status state shared between MPRIS D-Bus interface and player events.
#[derive(Debug, Clone)]
struct MprisState {
    playback_status: String,
    title: String,
    url: String,
    duration_micros: i64,
    /// Underlying player volume (0.0..1.0), tracked independently of `muted`
    /// so it can be restored verbatim when the player is unmuted.
    volume: f64,
    muted: bool,
}

impl MprisState {
    /// Volume as reported to MPRIS clients: 0.0 while muted, actual volume otherwise.
    fn effective_volume(&self) -> f64 {
        if self.muted {
            0.0
        } else {
            self.volume
        }
    }
}

impl Default for MprisState {
    fn default() -> Self {
        Self {
            playback_status: "Stopped".to_string(),
            title: String::new(),
            url: String::new(),
            duration_micros: 0,
            volume: 1.0,
            muted: false,
        }
    }
}

/// Root MPRIS interface `org.mpris.MediaPlayer2`.
struct MprisRoot {
    quit_requested: Arc<AtomicBool>,
    egui_ctx: egui::Context,
}

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
        "VAD"
    }

    #[zbus(property)]
    fn desktop_entry(&self) -> &str {
        "vad"
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
        info!("MPRIS Root.Quit invoked, requesting clean application shutdown");
        self.quit_requested.store(true, Ordering::Relaxed);
        self.egui_ctx.request_repaint();
    }
}

/// Player MPRIS interface `org.mpris.MediaPlayer2.Player`.
struct MprisPlayer {
    player: Player,
    shared_state: Arc<SharedPlayerState>,
    state: Arc<RwLock<MprisState>>,
    playlist: Arc<Mutex<Playlist>>,
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
    fn loop_status(&self) -> String {
        self.playlist
            .lock()
            .map(|p| p.repeat().as_mpris_str().to_string())
            .unwrap_or_else(|_| "None".to_string())
    }

    #[zbus(property)]
    fn set_loop_status(&self, status: &str) {
        if let Ok(mut p) = self.playlist.lock() {
            let mode = match status {
                "Track" => RepeatMode::Single,
                "Playlist" => RepeatMode::All,
                _ => RepeatMode::Off,
            };
            p.set_repeat(mode);
        }
    }

    #[zbus(property)]
    fn rate(&self) -> f64 {
        self.player.speed().unwrap_or(1.0)
    }

    #[zbus(property)]
    fn shuffle(&self) -> bool {
        self.playlist
            .lock()
            .map(|p| p.shuffle())
            .unwrap_or(false)
    }

    #[zbus(property)]
    fn set_shuffle(&self, shuffle: bool) {
        if let Ok(mut p) = self.playlist.lock() {
            p.set_shuffle(shuffle);
        }
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
            .map(|s| s.effective_volume())
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
        self.playlist
            .lock()
            .map(|p| p.has_next())
            .unwrap_or(false)
    }

    #[zbus(property)]
    fn can_go_previous(&self) -> bool {
        let has_prev = self
            .playlist
            .lock()
            .map(|p| p.has_previous())
            .unwrap_or(false);
        has_prev || (self.shared_state.get_time_pos() > 3.0)
    }

    fn next(&self) {
        debug!("MPRIS Player.Next invoked");
        if let Ok(mut p) = self.playlist.lock() {
            if let Some(item) = p.next() {
                let loc = item.location();
                let _ = self.player.load_file(&loc);
            }
        }
    }

    fn previous(&self) {
        debug!("MPRIS Player.Previous invoked");
        if self.shared_state.get_time_pos() > 3.0 {
            let _ = self.player.seek_absolute(0.0);
        } else if let Ok(mut p) = self.playlist.lock() {
            if let Some(item) = p.previous() {
                let loc = item.location();
                let _ = self.player.load_file(&loc);
            }
        }
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
        if !is_allowed_mpris_uri(&uri) {
            warn!("MPRIS Player.OpenUri rejected (unsupported scheme): {}", uri);
            return;
        }
        debug!("MPRIS Player.OpenUri invoked: {}", uri);
        let _ = self.player.load_file(&uri);
    }

    #[zbus(signal)]
    async fn seeked(emitter: &zbus::object_server::SignalEmitter<'_>, position: i64) -> zbus::Result<()>;
}

/// A pending D-Bus notification for the single background notifier thread.
enum MprisNotification {
    PropertyChanged(&'static str),
    Seeked(i64),
}

/// Linux MPRIS integration managing D-Bus connection and event dispatch.
pub struct MprisServer {
    state: Arc<RwLock<MprisState>>,
    connection: Option<zbus::connection::Connection>,
    bus_name: String,
    quit_requested: Arc<AtomicBool>,
    /// Bounded, non-blocking handoff to the notifier thread (see
    /// `spawn_notifier_thread`). `None` when D-Bus is unavailable.
    notify_tx: Option<SyncSender<MprisNotification>>,
}

impl MprisServer {
    pub fn new(
        player: Player,
        shared_state: Arc<SharedPlayerState>,
        playlist: Arc<Mutex<Playlist>>,
        egui_ctx: egui::Context,
    ) -> Result<Self, VadError> {
        let state = Arc::new(RwLock::new(MprisState::default()));
        let quit_requested = Arc::new(AtomicBool::new(false));
        let quit_requested_for_root = Arc::clone(&quit_requested);

        // Connect to session bus
        let (conn, bus_name) = match zbus::block_on(async {
            let primary_name = "org.mpris.MediaPlayer2.vad";
            let conn_builder = zbus::connection::Builder::session()
                .map_err(|e| VadError::Platform(format!("Failed to connect to D-Bus session: {e}")))?
                .serve_at(
                    "/org/mpris/MediaPlayer2",
                    MprisRoot {
                        quit_requested: quit_requested_for_root,
                        egui_ctx,
                    },
                )
                .map_err(|e| VadError::Platform(format!("Failed to register MPRIS root: {e}")))?
                .serve_at(
                    "/org/mpris/MediaPlayer2",
                    MprisPlayer {
                        player,
                        shared_state,
                        state: Arc::clone(&state),
                        playlist,
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

        let notify_tx = conn
            .as_ref()
            .map(|c| Self::spawn_notifier_thread(c.clone()));

        Ok(Self {
            state,
            connection: conn,
            bus_name,
            quit_requested,
            notify_tx,
        })
    }

    /// Starts the single background thread that performs D-Bus signal emission,
    /// fed by a bounded channel. All `notify_*` calls come from `on_event`, which
    /// runs on the egui UI thread every frame; one persistent thread (instead of
    /// spawning one per event) keeps a stalled session bus from piling up
    /// unbounded OS threads under sustained events like scrubbing.
    fn spawn_notifier_thread(conn: zbus::connection::Connection) -> SyncSender<MprisNotification> {
        let (tx, rx): (_, Receiver<MprisNotification>) = sync_channel(16);
        std::thread::spawn(move || {
            while let Ok(notification) = rx.recv() {
                zbus::block_on(async {
                    let Ok(iface_ref) = conn
                        .object_server()
                        .interface::<_, MprisPlayer>("/org/mpris/MediaPlayer2")
                        .await
                    else {
                        return;
                    };
                    match notification {
                        MprisNotification::PropertyChanged(prop) => {
                            let iface = iface_ref.get().await;
                            let emitter = iface_ref.signal_emitter();
                            match prop {
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
                        MprisNotification::Seeked(pos_micros) => {
                            let _ = MprisPlayer::seeked(iface_ref.signal_emitter(), pos_micros).await;
                        }
                    }
                });
            }
        });
        tx
    }

    /// Queues a property-changed signal for the notifier thread. Non-blocking:
    /// if the channel is full (notifier stuck on a stalled bus), the update is
    /// dropped rather than blocking the UI thread or piling up more work.
    fn notify_player_property_changed(&self, property_name: &'static str) {
        if let Some(ref tx) = self.notify_tx {
            let _ = tx.try_send(MprisNotification::PropertyChanged(property_name));
        }
    }

    /// See `notify_player_property_changed` for the non-blocking/dropping rationale.
    fn notify_seeked(&self, position_secs: f64) {
        if let Some(ref tx) = self.notify_tx {
            let pos_micros = (position_secs * 1_000_000.0) as i64;
            let _ = tx.try_send(MprisNotification::Seeked(pos_micros));
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
                let muted = guard.muted;
                drop(guard);
                // Effective (reported) volume only moves if we're not currently muted.
                if !muted {
                    self.notify_player_property_changed("Volume");
                }
            }
        }
        Ok(())
    }

    fn on_mute_changed(&mut self, muted: bool) -> Result<(), VadError> {
        if let Ok(mut guard) = self.state.write() {
            if guard.muted != muted {
                guard.muted = muted;
                drop(guard);
                self.notify_player_property_changed("Volume");
            }
        }
        Ok(())
    }

    fn shutdown(&mut self) -> Result<(), VadError> {
        if let Some(ref conn) = self.connection {
            if !self.bus_name.is_empty() {
                let bus_name = self.bus_name.clone();
                let conn_clone = conn.clone();
                zbus::block_on(async move {
                    let _ = conn_clone.release_name(bus_name).await;
                });
            }
        }
        Ok(())
    }

    fn quit_requested(&self) -> bool {
        self.quit_requested.load(Ordering::Relaxed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpris_server_lifecycle_and_event_handling() {
        let player = Player::new().expect("Failed to initialize player");
        let shared_state = Arc::new(SharedPlayerState::new());
        let playlist = Arc::new(Mutex::new(Playlist::new()));

        let mut server = MprisServer::new(player, shared_state, playlist, egui::Context::default())
            .expect("Failed to create MprisServer");
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

        // 3. Test volume updates: muting reports 0.0 externally but preserves the
        // real volume internally, and unmuting restores the exact prior value.
        server
            .on_volume_changed(75.0)
            .expect("on_volume_changed failed");
        assert!((server.state.read().unwrap().volume - 0.75).abs() < 0.01);
        assert!((server.state.read().unwrap().effective_volume() - 0.75).abs() < 0.01);

        server.on_mute_changed(true).expect("on_mute_changed failed");
        assert_eq!(server.state.read().unwrap().effective_volume(), 0.0);
        assert!((server.state.read().unwrap().volume - 0.75).abs() < 0.01);

        server.on_mute_changed(false).expect("on_mute_changed failed");
        assert!((server.state.read().unwrap().effective_volume() - 0.75).abs() < 0.01);

        // 4. Test seek
        server.on_seek(42.0).expect("on_seek failed");

        // 5. Test quit request via MPRIS Root.Quit
        assert!(!server.quit_requested());
        server.quit_requested.store(true, Ordering::Relaxed);
        assert!(server.quit_requested());

        // 6. Test shutdown
        server.shutdown().expect("shutdown failed");
    }

    #[test]
    fn test_open_uri_scheme_allowlist() {
        assert!(is_allowed_mpris_uri("file:///home/user/movie.mp4"));
        assert!(is_allowed_mpris_uri("http://example.com/video.mp4"));
        assert!(is_allowed_mpris_uri("https://example.com/stream.m3u8"));
        assert!(is_allowed_mpris_uri("rtsp://192.168.1.100:554/live"));

        assert!(!is_allowed_mpris_uri("smb://nas/share/video.mp4"));
        assert!(!is_allowed_mpris_uri("ftp://ftp.example.com/file"));
        assert!(!is_allowed_mpris_uri("javascript:alert(1)"));
    }

    #[test]
    fn test_mpris_playlist_sync_and_navigation() {
        let player = Player::new().expect("Failed to initialize player");
        let shared_state = Arc::new(SharedPlayerState::new());
        let playlist = Arc::new(Mutex::new(Playlist::new()));
        let state = Arc::new(RwLock::new(MprisState::default()));

        let mpris_player = MprisPlayer {
            player,
            shared_state: Arc::clone(&shared_state),
            state: Arc::clone(&state),
            playlist: Arc::clone(&playlist),
        };

        // Empty playlist
        assert!(!mpris_player.can_go_next());
        assert!(!mpris_player.can_go_previous());
        assert_eq!(mpris_player.loop_status(), "None");
        assert!(!mpris_player.shuffle());

        // Set loop status via MPRIS
        mpris_player.set_loop_status("Track");
        assert_eq!(mpris_player.loop_status(), "Track");
        mpris_player.set_loop_status("Playlist");
        assert_eq!(mpris_player.loop_status(), "Playlist");
        mpris_player.set_loop_status("None");
        assert_eq!(mpris_player.loop_status(), "None");

        // Set shuffle
        mpris_player.set_shuffle(true);
        assert!(mpris_player.shuffle());
        mpris_player.set_shuffle(false);
        assert!(!mpris_player.shuffle());

        // Populate playlist
        {
            let mut p = playlist.lock().unwrap();
            p.add(vad_core::PlaylistItem::from_file("/tmp/track1.mp3"));
            p.add(vad_core::PlaylistItem::from_file("/tmp/track2.mp3"));
            p.set_current(0);
        }

        assert!(mpris_player.can_go_next());
        assert!(!mpris_player.can_go_previous());

        // Navigation
        mpris_player.next();
        {
            let p = playlist.lock().unwrap();
            assert_eq!(p.current_index(), Some(1));
        }
        assert!(!mpris_player.can_go_next());
        assert!(mpris_player.can_go_previous());
    }
}
