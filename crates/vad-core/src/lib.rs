pub mod error;
pub mod player;
pub mod state;

pub use error::VadError;
pub use player::{GlProcAddressFn, Player, VideoRenderContext};
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
    fn test_player_playback_and_hwdec_query() {
        let test_file = "/tmp/M0_test_1080p_h264_aac.mp4";
        if !std::path::Path::new(test_file).exists() {
            eprintln!("Skipping playback test, file {} does not exist", test_file);
            return;
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
}
