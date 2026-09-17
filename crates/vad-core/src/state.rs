use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use crossbeam_channel::{Receiver, Sender};

/// High-level playback status.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PlaybackState {
    Idle,
    Playing,
    Paused,
}

/// Discrete playback events sent from mpv background event loop to the UI.
/// High-frequency `time-pos` updates are deliberately excluded to avoid
/// flooding the UI event loop at ~60 Hz and breaking reactive sleep (§5).
#[derive(Debug, Clone)]
pub enum PlayerEvent {
    FileLoaded {
        path: String,
        title: Option<String>,
        duration: Option<f64>,
    },
    PlaybackStateChanged(PlaybackState),
    HwdecChanged(Option<String>),
    VolumeChanged(f64),
    MutedChanged(bool),
    SeekOccurred(f64),
    EndOfFile,
    Error(String),
}

/// Shared lock-free state read on-demand by UI threads without channel overhead.
#[derive(Debug)]
pub struct SharedPlayerState {
    time_pos_bits: AtomicU64,
    duration_bits: AtomicU64,
    paused: AtomicBool,
}

impl Default for SharedPlayerState {
    fn default() -> Self {
        Self::new()
    }
}

impl SharedPlayerState {
    pub fn new() -> Self {
        Self {
            time_pos_bits: AtomicU64::new(0.0_f64.to_bits()),
            duration_bits: AtomicU64::new(0.0_f64.to_bits()),
            paused: AtomicBool::new(true),
        }
    }

    #[inline]
    pub fn set_time_pos(&self, pos: f64) {
        self.time_pos_bits.store(pos.to_bits(), Ordering::Release);
    }

    #[inline]
    pub fn get_time_pos(&self) -> f64 {
        f64::from_bits(self.time_pos_bits.load(Ordering::Acquire))
    }

    #[inline]
    pub fn set_duration(&self, duration: f64) {
        self.duration_bits.store(duration.to_bits(), Ordering::Release);
    }

    #[inline]
    pub fn get_duration(&self) -> f64 {
        f64::from_bits(self.duration_bits.load(Ordering::Acquire))
    }

    #[inline]
    pub fn set_paused(&self, paused: bool) {
        self.paused.store(paused, Ordering::Release);
    }

    #[inline]
    pub fn is_paused(&self) -> bool {
        self.paused.load(Ordering::Acquire)
    }
}

pub type EventSender = Sender<PlayerEvent>;
pub type EventReceiver = Receiver<PlayerEvent>;

/// Creates an unbounded event channel for discrete player events.
pub fn create_event_channel() -> (EventSender, EventReceiver) {
    crossbeam_channel::unbounded()
}
