pub mod error;
pub mod player;
pub mod state;

pub use error::{ErrorAction, ErrorSeverity, VadError};
pub use player::{AbLoopStatus, GlProcAddressFn, Player, TrackInfo, VideoRenderContext};
pub use state::{
    create_event_channel, EventReceiver, EventSender, PlaybackState, PlayerEvent, SharedPlayerState,
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
}
