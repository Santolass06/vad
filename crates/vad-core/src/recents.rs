use std::fs;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, warn};

use crate::error::VadError;
use crate::util::{vad_recentes_path, write_atomic};

/// Default capacity limit for recent media items per PLANO_VAD.md §4.6.
pub const DEFAULT_MAX_RECENTS: usize = 20;

fn default_max_entries() -> usize {
    DEFAULT_MAX_RECENTS
}

/// A media entry recorded in the recent playback history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentEntry {
    /// Filesystem absolute path or online streaming URL.
    pub location: String,
    /// User-facing media title.
    pub title: String,
    /// Last playback position in seconds.
    pub timestamp: f64,
    /// Total media duration in seconds (if known).
    pub duration: Option<f64>,
    /// Whether this entry is an online stream/URL.
    pub is_url: bool,
    /// Unix timestamp in seconds when this entry was last played or updated.
    pub updated_at: u64,
}

impl RecentEntry {
    /// Formats seconds into HH:MM:SS format (e.g. "00:34:12").
    pub fn format_seconds(seconds: f64) -> String {
        let s = seconds.max(0.0) as u64;
        let m = s / 60;
        let s = s % 60;
        let h = m / 60;
        let m = m % 60;
        format!("{h:02}:{m:02}:{s:02}")
    }

    /// Formatted current timestamp (e.g. "00:34:12").
    pub fn formatted_timestamp(&self) -> String {
        Self::format_seconds(self.timestamp)
    }

    /// Formatted total duration (e.g. "01:30:00"), or empty if unknown.
    pub fn formatted_duration(&self) -> Option<String> {
        self.duration.map(Self::format_seconds)
    }

    /// Combined summary matching design/Dialogs.dc.html (e.g. "00:34:12 de 01:30:00").
    pub fn formatted_summary(&self) -> String {
        let cur = self.formatted_timestamp();
        if let Some(dur) = self.formatted_duration() {
            format!("{cur} de {dur}")
        } else {
            cur
        }
    }
}

/// Thread-safe in-memory and on-disk store for recent playback history.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RecentsStore {
    #[serde(default = "default_max_entries")]
    pub max_entries: usize,
    #[serde(default)]
    pub entries: Vec<RecentEntry>,
}

impl Default for RecentsStore {
    fn default() -> Self {
        Self::new()
    }
}

impl RecentsStore {
    /// Creates an empty recents store with the default cap of 20 items.
    pub fn new() -> Self {
        Self {
            max_entries: DEFAULT_MAX_RECENTS,
            entries: Vec::new(),
        }
    }

    /// Creates an empty recents store with a custom capacity cap.
    pub fn with_max_entries(max_entries: usize) -> Self {
        Self {
            max_entries: max_entries.max(1),
            entries: Vec::new(),
        }
    }

    /// Adds or updates a recent media entry.
    /// If the location is already present, it is updated and moved to the front (index 0).
    /// If the list exceeds `max_entries`, older entries are pruned (§4.6).
    pub fn add_or_update(
        &mut self,
        location: &str,
        title: &str,
        timestamp: f64,
        duration: Option<f64>,
        is_url: bool,
    ) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Remove previous instance if existing
        self.entries.retain(|e| e.location != location);

        let entry = RecentEntry {
            location: location.to_string(),
            title: title.to_string(),
            timestamp: timestamp.max(0.0),
            duration,
            is_url,
            updated_at: now,
        };

        self.entries.insert(0, entry);

        // Prune to maximum entries
        if self.entries.len() > self.max_entries {
            self.entries.truncate(self.max_entries);
        }
    }

    /// Returns the entry for a given location, if present.
    pub fn get(&self, location: &str) -> Option<&RecentEntry> {
        self.entries.iter().find(|e| e.location == location)
    }

    /// Returns the most recent entry (head of the list).
    pub fn most_recent(&self) -> Option<&RecentEntry> {
        self.entries.first()
    }

    /// Returns a slice of all recent entries in chronological order (most recent first).
    pub fn entries(&self) -> &[RecentEntry] {
        &self.entries
    }

    /// Removes an entry by location. Returns true if an entry was removed.
    pub fn remove(&mut self, location: &str) -> bool {
        let prev_len = self.entries.len();
        self.entries.retain(|e| e.location != location);
        self.entries.len() < prev_len
    }

    /// Clears all entries from the store.
    pub fn clear(&mut self) {
        self.entries.clear();
    }

    /// Returns the count of stored entries.
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if there are no stored entries.
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Saves the store to a JSON file using atomic write (.tmp + rename, §4.30).
    pub fn save_to_path(&self, path: &Path) -> Result<(), VadError> {
        let json_bytes = serde_json::to_vec_pretty(self)
            .map_err(|e| VadError::Config(format!("Failed to serialize recentes.json: {e}")))?;
        write_atomic(path, &json_bytes)?;
        debug!("Saved {} recents entries to {:?}", self.entries.len(), path);
        Ok(())
    }

    /// Loads the store from a JSON file.
    /// Returns default store if file does not exist.
    /// Logs a warning and returns default store if corrupted, avoiding startup crash (§5).
    pub fn load_from_path(path: &Path) -> Result<Self, VadError> {
        if !path.exists() {
            debug!("Recents file {:?} does not exist, starting with empty store", path);
            return Ok(Self::new());
        }

        let content = match fs::read_to_string(path) {
            Ok(c) => c,
            Err(e) => {
                warn!("Failed to read {:?}: {:?}, falling back to empty recents", path, e);
                return Ok(Self::new());
            }
        };

        match serde_json::from_str::<Self>(&content) {
            Ok(store) => {
                debug!("Loaded {} recents entries from {:?}", store.entries.len(), path);
                Ok(store)
            }
            Err(e) => {
                warn!(
                    "Corrupted recentes.json at {:?}: {:?}, falling back to default store",
                    path, e
                );
                Ok(Self::new())
            }
        }
    }

    /// Saves to the default path `~/.config/vad/recentes.json` (§4.6).
    pub fn save_default(&self) -> Result<(), VadError> {
        self.save_to_path(&vad_recentes_path())
    }

    /// Loads from the default path `~/.config/vad/recentes.json`.
    pub fn load_default() -> Self {
        Self::load_from_path(&vad_recentes_path()).unwrap_or_default()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU64;

    static TEST_COUNTER: AtomicU64 = AtomicU64::new(100);

    fn temp_test_dir(prefix: &str) -> std::path::PathBuf {
        let pid = std::process::id();
        let cnt = TEST_COUNTER.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("vad_recents_test_{prefix}_{pid}_{cnt}"));
        let _ = fs::create_dir_all(&dir);
        dir
    }

    #[test]
    fn test_recents_add_order_and_truncation() {
        let mut store = RecentsStore::with_max_entries(3);

        store.add_or_update("/video1.mp4", "Video 1", 10.0, Some(100.0), false);
        store.add_or_update("/video2.mp4", "Video 2", 20.0, Some(200.0), false);
        store.add_or_update("/video3.mp4", "Video 3", 30.0, Some(300.0), false);

        assert_eq!(store.len(), 3);
        assert_eq!(store.most_recent().unwrap().location, "/video3.mp4");

        // Adding 4th item prunes oldest (/video1.mp4)
        store.add_or_update("https://youtube.com/v4", "Video 4", 40.0, None, true);
        assert_eq!(store.len(), 3);
        assert_eq!(store.most_recent().unwrap().location, "https://youtube.com/v4");
        assert!(store.get("/video1.mp4").is_none());
        assert!(store.get("/video2.mp4").is_some());
    }

    #[test]
    fn test_recents_update_existing_moves_to_front() {
        let mut store = RecentsStore::with_max_entries(20);

        store.add_or_update("/a.mp4", "A", 10.0, Some(60.0), false);
        store.add_or_update("/b.mp4", "B", 20.0, Some(60.0), false);
        assert_eq!(store.most_recent().unwrap().location, "/b.mp4");

        // Re-adding /a.mp4 updates position and moves to front
        store.add_or_update("/a.mp4", "A (novo)", 45.0, Some(60.0), false);
        assert_eq!(store.len(), 2);
        assert_eq!(store.most_recent().unwrap().location, "/a.mp4");
        assert_eq!(store.most_recent().unwrap().timestamp, 45.0);
        assert_eq!(store.most_recent().unwrap().title, "A (novo)");
    }

    #[test]
    fn test_recents_atomic_save_and_load() {
        let dir = temp_test_dir("save_load");
        let path = dir.join("recentes.json");

        let mut store = RecentsStore::new();
        store.add_or_update(
            "/movie.mkv",
            "Movie",
            2052.0,
            Some(5400.0),
            false,
        );
        store.save_to_path(&path).expect("Failed to save recents");

        let loaded = RecentsStore::load_from_path(&path).expect("Failed to load recents");
        assert_eq!(loaded.len(), 1);
        let entry = loaded.most_recent().unwrap();
        assert_eq!(entry.location, "/movie.mkv");
        assert_eq!(entry.timestamp, 2052.0);
        assert_eq!(entry.formatted_summary(), "00:34:12 de 01:30:00");

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_recents_corrupted_json_fallback() {
        let dir = temp_test_dir("corrupt");
        let path = dir.join("recentes.json");

        fs::write(&path, b"INVALID_NON_JSON_DATA{{").unwrap();
        let loaded = RecentsStore::load_from_path(&path).expect("Should degrade gracefully");
        assert_eq!(loaded.len(), 0);

        let _ = fs::remove_dir_all(&dir);
    }
}
