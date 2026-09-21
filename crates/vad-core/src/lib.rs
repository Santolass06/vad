pub mod bookmarks;
pub mod config;
pub mod error;
pub mod platform;
pub mod player;
pub mod playlist;
pub mod recents;
pub mod state;
pub mod util;

pub use bookmarks::{format_timestamp_secs, Bookmark, BookmarkStore};
pub use config::{
    EqualizerConfig, ModelStorageMode, PlayerConfig, RecentsConfig, ShortcutsConfig, VadConfig,
    WhisperConfig,
};
pub use error::{ErrorAction, ErrorSeverity, VadError};
pub use platform::PlatformIntegration;
pub use player::{
    AbLoopStatus, AudioDevice, GlProcAddressFn, Player, TrackInfo, VideoRenderContext,
};
pub use playlist::{PlaylistItem, Playlist, RepeatMode};
pub use recents::{RecentEntry, RecentsStore, DEFAULT_MAX_RECENTS};
pub use state::{
    create_event_channel, EventReceiver, EventSender, PlaybackState, PlayerEvent, SharedPlayerState,
};
pub use util::{
    get_process_rss_bytes, is_allowed_url_scheme, vad_bookmarks_dir, vad_config_dir,
    vad_config_path, vad_data_dir, vad_models_dir, vad_mpv_config_dir, vad_recentes_path, write_atomic,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_shared_player_state_atomics() {
        let state = SharedPlayerState::new();
        assert_eq!(state.get_time_pos(), 0.0);
        assert_eq!(state.get_duration(), 0.0);
        assert!(state.is_paused());

        state.set_time_pos(42.5);
        state.set_duration(120.0);
        state.set_paused(false);

        assert_eq!(state.get_time_pos(), 42.5);
        assert_eq!(state.get_duration(), 120.0);
        assert!(!state.is_paused());
    }

    #[test]
    fn test_player_init_and_defensive_properties() {
        let player = Player::new().expect("Failed to create player");
        // Check that querying a non-existent property does not panic and returns Ok(None)
        let non_existent = player
            .get_property_optional::<String>("non-existent-property-12345")
            .expect("Defensive getter failed");
        assert_eq!(non_existent, None);

        // Check that initial hwdec is auto-safe
        let hwdec = player
            .get_property_optional::<String>("hwdec")
            .expect("hwdec query failed");
        assert_eq!(hwdec, Some("auto-safe".to_string()));

        // Test set_hwdec
        player.set_hwdec("no").expect("Failed to set hwdec to no");
        let hwdec_no = player
            .get_property_optional::<String>("hwdec")
            .expect("hwdec query failed");
        assert_eq!(hwdec_no, Some("no".to_string()));
    }

    #[test]
    fn test_panic_safety_catch_unwind() {
        let (tx, rx) = create_event_channel();
        let callback_that_panics = move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                panic!("Intentional test panic in FFI callback");
            }));
            if let Err(payload) = result {
                let msg = if let Some(s) = payload.downcast_ref::<&str>() {
                    s.to_string()
                } else if let Some(s) = payload.downcast_ref::<String>() {
                    s.clone()
                } else {
                    "unknown panic".to_string()
                };
                let _ = tx.send(PlayerEvent::Error(format!("Callback panic: {msg}")));
            }
        };

        // Execute the callback: must not abort the process, must send error event
        callback_that_panics();

        let event = rx.try_recv().expect("Expected error event to be sent");
        match event {
            PlayerEvent::Error(msg) => {
                assert!(msg.contains("Intentional test panic"));
            }
            other => panic!("Expected PlayerEvent::Error, got {:?}", other),
        }
    }

    #[test]
    fn test_vad_error_taxonomy() {
        // FfmpegNotFound must map to Degraded with suggested install command and disabled features
        let err_ffmpeg = VadError::FfmpegNotFound;
        let action_ffmpeg = err_ffmpeg.action();
        assert_eq!(action_ffmpeg.severity, ErrorSeverity::Degraded);
        assert_eq!(action_ffmpeg.install_command, Some("sudo apt install ffmpeg"));
        assert!(action_ffmpeg.disabled_features.contains(&"waveform"));
        assert!(action_ffmpeg.disabled_features.contains(&"whisper"));
        assert!(action_ffmpeg.can_ignore);

        // YtDlpNotFound must map to Degraded with yt-dlp install command
        let err_ytdlp = VadError::YtDlpNotFound;
        let action_ytdlp = err_ytdlp.action();
        assert_eq!(action_ytdlp.severity, ErrorSeverity::Degraded);
        assert_eq!(action_ytdlp.install_command, Some("sudo apt install yt-dlp"));
        assert!(action_ytdlp.disabled_features.contains(&"url_playback"));

        // PlayerInitFailed must be Fatal
        let err_init = VadError::PlayerInitFailed("libmpv init failed".to_string());
        let action_init = err_init.action();
        assert_eq!(action_init.severity, ErrorSeverity::Fatal);
        assert!(!action_init.can_ignore);

        // GlContextUnavailable must be Fatal
        let err_gl = VadError::GlContextUnavailable("No GL".to_string());
        assert_eq!(err_gl.action().severity, ErrorSeverity::Fatal);
    }

    #[test]
    fn test_player_playback_and_hwdec_query() {
        // Self-contained fixture (not the Sprint_00 /tmp file, which is
        // deliberately volatile and may not exist on this run) — regenerated
        // with the same ffmpeg synthetic-source approach as Sprint_00, just
        // smaller/shorter so the test stays fast.
        let test_file = "/tmp/vad_test_core_playback.mp4";
        if !std::path::Path::new(test_file).exists() {
            let generated = std::process::Command::new("ffmpeg")
                .args([
                    "-y", "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=30:duration=3",
                    "-f", "lavfi", "-i", "sine=frequency=440:duration=3",
                    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "30",
                    "-c:a", "aac", "-shortest", test_file,
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
            if !generated {
                eprintln!(
                    "Skipping playback test: ffmpeg unavailable or failed to generate fixture at {}",
                    test_file
                );
                return;
            }
        }

        let player = Player::new().expect("Failed to create player");
        player.load_file(test_file).expect("Failed to load file");
        
        // Wait up to 1 second for file demuxing to initialize
        let start = std::time::Instant::now();
        while start.elapsed() < std::time::Duration::from_millis(1500) {
            if let Ok(Some(dur)) = player.duration() {
                if dur > 0.0 {
                    break;
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        // Test volume
        player.set_volume(85.0).expect("Failed to set volume");
        let vol = player.volume().expect("Failed to get volume");
        assert!((vol - 85.0).abs() < 1.0);

        // Test play/pause toggle
        player.pause().expect("Failed to pause");
        assert!(player.is_paused().expect("Failed to query paused"));
        player.play().expect("Failed to play");

        // Test seek (now that media is demuxed)
        let _ = player.seek_relative(5.0);
        let _ = player.seek_absolute(10.0);

        // Query hwdec_current: should return either Some(driver) or None (SW fallback)
        let hwdec = player.hwdec_current().expect("Failed to query hwdec_current");
        println!("hwdec_current reported: {:?}", hwdec);

        // Force SW mode
        player.set_hwdec("no").expect("Failed to set hwdec to no");
        let forced_sw = player.hwdec_current().expect("Failed to query hwdec after setting no");
        assert_eq!(forced_sw, None, "Forced hwdec=no must return None representing SW (CPU)");
    }

    #[test]
    fn test_player_speed_tracks_and_ab_loop() {
        let player = Player::new().expect("Failed to create player");

        // Test default speed and speed mutation
        let speed = player.speed().expect("Failed to query speed");
        assert_eq!(speed, 1.0);
        player.set_speed(1.5).expect("Failed to set speed");
        let new_speed = player.speed().expect("Failed to query updated speed");
        assert!((new_speed - 1.5).abs() < 0.01);

        // Test A-B loop status initially Off
        let ab = player.ab_loop_status().expect("Failed to query ab loop status");
        assert_eq!(ab, AbLoopStatus::Off);

        // Test track querying: with no media loaded, track-list/count is 0, so both must be empty
        let audio_tracks = player.audio_tracks().expect("Failed to query audio tracks");
        let sub_tracks = player.subtitle_tracks().expect("Failed to query subtitle tracks");
        assert!(audio_tracks.is_empty());
        assert!(sub_tracks.is_empty());
    }

    #[test]
    fn test_platform_integration_trait_dispatch() {
        struct MockIntegration {
            state: PlaybackState,
            title: Option<String>,
            last_seek: Option<f64>,
            volume: f64,
            muted: bool,
            shutdown_called: bool,
        }

        impl PlatformIntegration for MockIntegration {
            fn name(&self) -> &'static str {
                "mock"
            }

            fn on_playback_state(&mut self, state: PlaybackState) -> Result<(), VadError> {
                self.state = state;
                Ok(())
            }

            fn on_file_loaded(
                &mut self,
                _path: &str,
                title: Option<&str>,
                _duration: Option<f64>,
            ) -> Result<(), VadError> {
                self.title = title.map(|s| s.to_string());
                Ok(())
            }

            fn on_seek(&mut self, position_secs: f64) -> Result<(), VadError> {
                self.last_seek = Some(position_secs);
                Ok(())
            }

            fn on_volume_changed(&mut self, volume: f64) -> Result<(), VadError> {
                self.volume = volume;
                Ok(())
            }

            fn on_mute_changed(&mut self, muted: bool) -> Result<(), VadError> {
                self.muted = muted;
                Ok(())
            }

            fn shutdown(&mut self) -> Result<(), VadError> {
                self.shutdown_called = true;
                Ok(())
            }
        }

        let mut mock = MockIntegration {
            state: PlaybackState::Idle,
            title: None,
            last_seek: None,
            volume: 100.0,
            muted: false,
            shutdown_called: false,
        };

        assert_eq!(mock.name(), "mock");

        // Dispatch FileLoaded
        mock.on_event(&PlayerEvent::FileLoaded {
            path: "/path/video.mkv".to_string(),
            title: Some("Sample Video".to_string()),
            duration: Some(120.0),
        })
        .expect("on_event FileLoaded failed");
        assert_eq!(mock.title.as_deref(), Some("Sample Video"));

        // Dispatch PlaybackStateChanged
        mock.on_event(&PlayerEvent::PlaybackStateChanged(PlaybackState::Playing))
            .expect("on_event PlaybackStateChanged failed");
        assert_eq!(mock.state, PlaybackState::Playing);

        // Dispatch SeekOccurred
        mock.on_event(&PlayerEvent::SeekOccurred(45.5))
            .expect("on_event SeekOccurred failed");
        assert_eq!(mock.last_seek, Some(45.5));

        // Dispatch VolumeChanged
        mock.on_event(&PlayerEvent::VolumeChanged(75.0))
            .expect("on_event VolumeChanged failed");
        assert_eq!(mock.volume, 75.0);

        // Dispatch MutedChanged
        mock.on_event(&PlayerEvent::MutedChanged(true))
            .expect("on_event MutedChanged failed");
        assert!(mock.muted);

        // Shutdown
        mock.shutdown().expect("shutdown failed");
        assert!(mock.shutdown_called);
    }

    #[test]
    fn test_concurrent_player_commands() {
        let player = Player::new().expect("Failed to create player");
        let player_ui = player.clone();
        let player_mpris = player.clone();

        let t1 = std::thread::spawn(move || {
            for i in 0..50 {
                let _ = player_ui.toggle_pause();
                let _ = player_ui.seek_relative((i % 5) as f64 - 2.0);
                let _ = player_ui.set_speed(1.0 + (i % 3) as f64 * 0.25);
                let _ = player_ui.volume();
                std::thread::yield_now();
            }
        });

        let t2 = std::thread::spawn(move || {
            for i in 0..50 {
                let _ = player_mpris.pause();
                let _ = player_mpris.seek_absolute((i % 10) as f64);
                let _ = player_mpris.set_volume(50.0 + (i % 20) as f64);
                let _ = player_mpris.is_paused();
                std::thread::yield_now();
            }
        });

        t1.join().expect("UI player thread panicked");
        t2.join().expect("MPRIS player thread panicked");
    }

    #[test]
    fn test_player_video_and_audio_properties() {
        let player = Player::new().expect("Failed to create player");

        // Aspect ratio
        player.set_video_aspect_override("16:9").expect("Failed to set aspect");
        let aspect = player.video_aspect_override().expect("Failed to get aspect");
        assert!((aspect - 16.0 / 9.0).abs() < 0.01);

        player.set_video_aspect_override("-1").expect("Failed to reset aspect");
        let aspect_auto = player.video_aspect_override().expect("Failed to get aspect");
        assert!(aspect_auto <= 0.0);

        // Rotation
        player.set_video_rotate(90).expect("Failed to set rotate");
        assert_eq!(player.video_rotate().unwrap(), 90);
        player.set_video_rotate(0).expect("Failed to reset rotate");
        assert_eq!(player.video_rotate().unwrap(), 0);

        // Crop & Panscan
        player.set_panscan(0.5).expect("Failed to set panscan");
        assert!((player.panscan().unwrap() - 0.5).abs() < 0.01);
        player.set_panscan(0.0).expect("Failed to reset panscan");

        // Delay A/V
        player.set_audio_delay(0.12).expect("Failed to set audio delay");
        assert!((player.audio_delay().unwrap() - 0.12).abs() < 0.01);
        player.set_audio_delay(0.0).expect("Failed to reset audio delay");

        player.set_sub_delay(-0.08).expect("Failed to set sub delay");
        assert!((player.sub_delay().unwrap() - (-0.08)).abs() < 0.01);
        player.set_sub_delay(0.0).expect("Failed to reset sub delay");

        // Subtitle visibility
        player.set_sub_visibility(false).expect("Failed to set sub visibility");
        assert!(!player.sub_visibility().unwrap());
        player.set_sub_visibility(true).expect("Failed to enable sub visibility");
        assert!(player.sub_visibility().unwrap());

        // Color adjustments
        player.set_color_adjustments(10, -5, 15, 0).expect("Failed to set color");
        let (b, c, s, g) = player.color_adjustments().expect("Failed to get color");
        assert_eq!(b, 10);
        assert_eq!(c, -5);
        assert_eq!(s, 15);
        assert_eq!(g, 0);
        player.reset_color_adjustments().expect("Failed to reset color");
        let (b, c, s, g) = player.color_adjustments().expect("Failed to get color");
        assert_eq!((b, c, s, g), (0, 0, 0, 0));

        // Audio devices
        let devices = player.audio_devices().expect("Failed to query audio devices");
        assert!(!devices.is_empty(), "Audio devices list must have at least auto/default device");
        let current_dev = player.audio_device().expect("Failed to get audio device");
        assert!(!current_dev.is_empty());

        // Audio filters (10-band EQ + RNNoise)
        let gains = [-2.0, 1.0, 3.0, 4.0, 2.0, 0.0, -1.0, 2.0, 3.0, 1.0];
        player.set_audio_filters(&gains, true).expect("Failed to set audio filters");
        player.set_audio_filters(&[0.0; 10], false).expect("Failed to clear audio filters");

        // Volume boost up to 200%
        player.set_volume(150.0).expect("Failed to set volume boost");
        let vol = player.volume().unwrap_or(100.0);
        assert!((vol - 150.0).abs() < 1.0);
    }

    /// Plants a sentinel `~/.config/mpv/mpv.conf` for the lifetime of the guard and restores the
    /// user's original file (byte-for-byte, raw bytes so a non-UTF-8 config survives) on drop —
    /// also when an assertion panics. Serialised: both tests below share the same file.
    struct SentinelMpvConf {
        conf_file: std::path::PathBuf,
        dir: std::path::PathBuf,
        original: Option<Vec<u8>>,
        dir_existed: bool,
        _lock: std::sync::MutexGuard<'static, ()>,
    }

    impl SentinelMpvConf {
        fn create(content: &str) -> Option<Self> {
            static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
            let _lock = LOCK.lock().unwrap_or_else(|e| e.into_inner());
            let home = std::env::var("HOME").ok().filter(|h| !h.trim().is_empty())?;
            let dir = std::path::PathBuf::from(home).join(".config").join("mpv");
            let conf_file = dir.join("mpv.conf");
            let original = std::fs::read(&conf_file).ok();
            let dir_existed = dir.exists();
            std::fs::create_dir_all(&dir).expect("Failed to create ~/.config/mpv");
            std::fs::write(&conf_file, content).expect("Failed to write sentinel mpv.conf");
            Some(Self { conf_file, dir, original, dir_existed, _lock })
        }
    }

    impl Drop for SentinelMpvConf {
        fn drop(&mut self) {
            match self.original.take() {
                Some(content) => {
                    let _ = std::fs::write(&self.conf_file, content);
                }
                None => {
                    let _ = std::fs::remove_file(&self.conf_file);
                    if !self.dir_existed {
                        let _ = std::fs::remove_dir(&self.dir);
                    }
                }
            }
        }
    }

    #[test]
    fn test_mpv_config_isolation() {
        // Exit criterion verification (§9, Sprint_Planning_05):
        // Ensure VAD's libmpv instance does NOT inherit user's ~/.config/mpv.
        let Some(_sentinel) = SentinelMpvConf::create("speed=2.5\nvolume=42\n") else {
            return;
        };

        // Initialize Player using Player::new() with isolated config-dir and --no-config
        let player = Player::new().expect("Failed to initialize Player");
        let speed = player.speed().expect("Failed to read speed");
        let vol = player.volume().expect("Failed to read volume");

        // Speed must be 1.0 (default), NOT 2.5
        assert_eq!(
            speed, 1.0,
            "mpv must not inherit speed=2.5 from user ~/.config/mpv/mpv.conf"
        );
        // Volume must be 100.0 (default), NOT 42
        assert_eq!(
            vol, 100.0,
            "mpv must not inherit volume=42 from user ~/.config/mpv/mpv.conf"
        );
    }

    /// M2 exit criterion (§9): a YouTube URL plays through yt-dlp while a personal
    /// `~/.config/mpv/mpv.conf` exists. The sentinel `ytdl-format` matches no format, so if it
    /// were inherited yt-dlp would fail to resolve the stream.
    /// Network-dependent, hence ignored by default: `cargo test -p vad-core -- --ignored`.
    #[test]
    #[ignore = "needs network access and yt-dlp"]
    fn test_youtube_url_playback_ignores_user_mpv_config() {
        let Some(_sentinel) = SentinelMpvConf::create("ytdl-format=vad-nonexistent-format\n")
        else {
            return;
        };

        let player = Player::new().expect("Failed to initialize Player");
        // "Me at the zoo", the first YouTube upload (19s)
        player
            .load_url("https://www.youtube.com/watch?v=jNQXAC9IVRw")
            .expect("Failed to load URL");

        let start = std::time::Instant::now();
        let mut duration = None;
        while start.elapsed() < std::time::Duration::from_secs(60) {
            if let Ok(Some(d)) = player.duration() {
                duration = Some(d);
                break;
            }
            std::thread::sleep(std::time::Duration::from_millis(250));
        }
        let duration = duration.expect("yt-dlp did not resolve the stream within 60s");
        assert!(
            (18.0..=20.0).contains(&duration),
            "unexpected duration {duration}"
        );
    }

    /// Premise behind `VadApp::load_media_at`: `loadfile` is asynchronous, so a seek issued right
    /// after it is not applied — the seek has to wait until the file is loaded.
    #[test]
    fn test_seek_applies_only_after_file_loaded() {
        let test_file = "/tmp/vad_test_seek_after_load.mp4";
        let generated = std::path::Path::new(test_file).exists()
            || std::process::Command::new("ffmpeg")
                .args([
                    "-y", "-f", "lavfi", "-i", "testsrc2=size=320x240:rate=30:duration=6",
                    "-f", "lavfi", "-i", "sine=frequency=440:duration=6",
                    "-c:v", "libx264", "-pix_fmt", "yuv420p", "-crf", "30",
                    "-c:a", "aac", "-shortest", test_file,
                ])
                .status()
                .map(|s| s.success())
                .unwrap_or(false);
        if !generated {
            eprintln!("Skipping seek test: ffmpeg unavailable or failed to generate fixture");
            return;
        }

        let player = Player::new().expect("Failed to create player");
        player.load_file(test_file).expect("Failed to load file");

        // Right after `loadfile` nothing is open yet: the seek must not take effect.
        let early = player.seek_absolute(4.0);
        let early_pos = player.time_pos().ok().flatten().unwrap_or(0.0);
        assert!(
            early.is_err() || early_pos < 1.0,
            "seek right after loadfile unexpectedly applied (pos {early_pos})"
        );

        // Once the duration is known the file is loaded and the seek works.
        let start = std::time::Instant::now();
        while player.duration().ok().flatten().is_none() {
            assert!(start.elapsed() < std::time::Duration::from_secs(5), "file never loaded");
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        player.seek_absolute(4.0).expect("seek after load failed");
        let start = std::time::Instant::now();
        loop {
            let pos = player.time_pos().ok().flatten().unwrap_or(0.0);
            if pos >= 3.9 {
                break;
            }
            assert!(
                start.elapsed() < std::time::Duration::from_secs(3),
                "seek after load not applied (pos {pos})"
            );
            std::thread::sleep(std::time::Duration::from_millis(20));
        }

        let _ = std::fs::remove_file(test_file);
    }

    #[test]
    fn test_player_url_scheme_enforcement() {
        let player = Player::new().expect("Failed to create player");

        // Prohibited schemes must return Err(VadError::InvalidUrlScheme(_)) (§4.27)
        let res_file = player.load_file("file:///etc/shadow");
        assert!(matches!(res_file, Err(VadError::InvalidUrlScheme(_))));

        let res_smb = player.load_file("smb://nas/share/video.mp4");
        assert!(matches!(res_smb, Err(VadError::InvalidUrlScheme(_))));

        let res_ftp = player.load_file("ftp://example.com/stream");
        assert!(matches!(res_ftp, Err(VadError::InvalidUrlScheme(_))));
    }
}
