use std::path::{Path, PathBuf};

/// Individual media item in the playlist.
/// Accepts both local filesystem files and online streaming URLs (Sprint 04).
/// Actual streaming playback resolution (via yt-dlp) is handled in Sprint 05.
#[derive(Debug, Clone, PartialEq)]
pub enum PlaylistItem {
    File {
        path: PathBuf,
        title: Option<String>,
        duration: Option<f64>,
    },
    Url {
        url: String,
        title: Option<String>,
        duration: Option<f64>,
    },
}

impl PlaylistItem {
    /// Creates a new `PlaylistItem::File` from a path.
    pub fn from_file(path: impl Into<PathBuf>) -> Self {
        let path = path.into();
        let title = path
            .file_name()
            .map(|f| f.to_string_lossy().to_string());
        Self::File {
            path,
            title,
            duration: None,
        }
    }

    /// Creates a new `PlaylistItem::Url` from a URL string.
    pub fn from_url(url: impl Into<String>) -> Self {
        let url = url.into();
        let title = url
            .split('/')
            .next_back()
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string());
        Self::Url {
            url,
            title,
            duration: None,
        }
    }

    /// Returns the user-facing title or a fallback derived from the path/URL.
    pub fn title(&self) -> &str {
        match self {
            Self::File { title, path, .. } => {
                title.as_deref().unwrap_or_else(|| path.to_str().unwrap_or("Ficheiro"))
            }
            Self::Url { title, url, .. } => {
                title.as_deref().unwrap_or(url.as_str())
            }
        }
    }

    /// Returns the target location (filesystem path or URL string).
    pub fn location(&self) -> String {
        match self {
            Self::File { path, .. } => path.to_string_lossy().to_string(),
            Self::Url { url, .. } => url.clone(),
        }
    }

    /// Returns whether this item is an online stream/URL.
    pub fn is_url(&self) -> bool {
        matches!(self, Self::Url { .. })
    }

    /// Returns the duration if known.
    pub fn duration(&self) -> Option<f64> {
        match self {
            Self::File { duration, .. } | Self::Url { duration, .. } => *duration,
        }
    }

    /// Sets or updates the known duration.
    pub fn set_duration(&mut self, dur: Option<f64>) {
        match self {
            Self::File { duration, .. } | Self::Url { duration, .. } => *duration = dur,
        }
    }

    /// Sets or updates the user-facing title.
    pub fn set_title(&mut self, t: impl Into<String>) {
        let t = Some(t.into());
        match self {
            Self::File { title, .. } | Self::Url { title, .. } => *title = t,
        }
    }
}

/// Playlist repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    Single,
}

impl RepeatMode {
    /// Cycles through repeat modes: Off -> All -> Single -> Off.
    pub fn cycle(self) -> Self {
        match self {
            Self::Off => Self::All,
            Self::All => Self::Single,
            Self::Single => Self::Off,
        }
    }

    /// MPRIS string representation ("None", "Playlist", "Track").
    pub fn as_mpris_str(self) -> &'static str {
        match self {
            Self::Off => "None",
            Self::All => "Playlist",
            Self::Single => "Track",
        }
    }
}

/// Lightweight pseudorandom number generator (XorShift64) to avoid external `rand` crate (§5).
fn xorshift64(state: &mut u64) -> u64 {
    let mut x = *state;
    if x == 0 {
        x = 0xdeadbeefcafebabe;
    }
    x ^= x << 13;
    x ^= x >> 7;
    x ^= x << 17;
    *state = x;
    x
}

/// Complete playlist data structure.
#[derive(Debug, Clone, Default)]
pub struct Playlist {
    items: Vec<PlaylistItem>,
    current_index: Option<usize>,
    shuffle: bool,
    repeat: RepeatMode,
    shuffled_order: Vec<usize>,
    shuffled_pos: Option<usize>,
    rng_seed: u64,
}

impl Playlist {
    /// Creates a new empty playlist.
    pub fn new() -> Self {
        let seed = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x123456789abcdef0);

        Self {
            items: Vec::new(),
            current_index: None,
            shuffle: false,
            repeat: RepeatMode::Off,
            shuffled_order: Vec::new(),
            shuffled_pos: None,
            rng_seed: seed,
        }
    }

    /// Returns the number of items in the playlist.
    pub fn len(&self) -> usize {
        self.items.len()
    }

    /// Returns whether the playlist is empty.
    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    /// Returns an immutable slice of all playlist items in original order.
    pub fn items(&self) -> &[PlaylistItem] {
        &self.items
    }

    /// Returns a mutable slice of all playlist items.
    pub fn items_mut(&mut self) -> &mut [PlaylistItem] {
        &mut self.items
    }

    /// Adds a new item to the end of the playlist. Returns the index of the added item.
    pub fn add(&mut self, item: PlaylistItem) -> usize {
        let idx = self.items.len();
        self.items.push(item);
        if self.current_index.is_none() {
            self.current_index = Some(0);
        }
        self.rebuild_shuffle();
        idx
    }

    /// Adds a file item to the playlist.
    pub fn add_file(&mut self, path: impl AsRef<Path>) -> usize {
        self.add(PlaylistItem::from_file(path.as_ref()))
    }

    /// Adds a URL item to the playlist.
    pub fn add_url(&mut self, url: impl Into<String>) -> usize {
        self.add(PlaylistItem::from_url(url))
    }

    /// Removes an item at the specified index.
    pub fn remove(&mut self, index: usize) -> Option<PlaylistItem> {
        if index >= self.items.len() {
            return None;
        }

        let removed = self.items.remove(index);

        if self.items.is_empty() {
            self.current_index = None;
            self.shuffled_pos = None;
            self.shuffled_order.clear();
        } else if let Some(curr) = self.current_index {
            if curr == index {
                self.current_index = Some(curr.min(self.items.len() - 1));
            } else if curr > index {
                self.current_index = Some(curr - 1);
            }
            self.rebuild_shuffle();
        }

        Some(removed)
    }

    /// Clears all items from the playlist.
    pub fn clear(&mut self) {
        self.items.clear();
        self.current_index = None;
        self.shuffled_pos = None;
        self.shuffled_order.clear();
    }

    /// Moves an item from `from_idx` to `to_idx`.
    pub fn move_item(&mut self, from_idx: usize, to_idx: usize) -> bool {
        let len = self.items.len();
        if from_idx >= len || to_idx >= len || from_idx == to_idx {
            return false;
        }

        let item = self.items.remove(from_idx);
        self.items.insert(to_idx, item);

        // Adjust current_index
        if let Some(curr) = self.current_index {
            if curr == from_idx {
                self.current_index = Some(to_idx);
            } else if from_idx < curr && to_idx >= curr {
                self.current_index = Some(curr - 1);
            } else if from_idx > curr && to_idx <= curr {
                self.current_index = Some(curr + 1);
            }
        }

        self.rebuild_shuffle();
        true
    }

    /// Returns the currently active playlist item, if any.
    pub fn current(&self) -> Option<&PlaylistItem> {
        self.current_index.and_then(|idx| self.items.get(idx))
    }

    /// Returns the index of the currently active item.
    pub fn current_index(&self) -> Option<usize> {
        self.current_index
    }

    /// Sets the active item to the specified index.
    pub fn set_current(&mut self, index: usize) -> Option<&PlaylistItem> {
        if index < self.items.len() {
            self.current_index = Some(index);
            if self.shuffle {
                self.shuffled_pos = self.shuffled_order.iter().position(|&i| i == index);
            }
            self.current()
        } else {
            None
        }
    }

    /// Queries whether there is a next track available under the current repeat/shuffle mode.
    pub fn has_next(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }

        match self.repeat {
            RepeatMode::Single | RepeatMode::All => true,
            RepeatMode::Off => {
                if self.shuffle {
                    self.shuffled_pos.is_some_and(|pos| pos + 1 < self.shuffled_order.len())
                } else {
                    self.current_index.is_some_and(|curr| curr + 1 < self.items.len())
                }
            }
        }
    }

    /// Queries whether there is a previous track available.
    pub fn has_previous(&self) -> bool {
        if self.items.is_empty() {
            return false;
        }

        match self.repeat {
            RepeatMode::Single | RepeatMode::All => true,
            RepeatMode::Off => {
                if self.shuffle {
                    self.shuffled_pos.is_some_and(|pos| pos > 0)
                } else {
                    self.current_index.is_some_and(|curr| curr > 0)
                }
            }
        }
    }

    /// Advances to the next item according to shuffle and repeat modes.
    /// Returns the newly active item, or `None` if the end of the playlist was reached.
    #[allow(clippy::should_implement_trait)]
    pub fn next(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }

        if self.repeat == RepeatMode::Single {
            return self.current();
        }

        if self.shuffle {
            let next_pos = match self.shuffled_pos {
                Some(pos) => {
                    if pos + 1 < self.shuffled_order.len() {
                        Some(pos + 1)
                    } else if self.repeat == RepeatMode::All {
                        Some(0)
                    } else {
                        None
                    }
                }
                None => Some(0),
            };

            if let Some(pos) = next_pos {
                self.shuffled_pos = Some(pos);
                let original_idx = self.shuffled_order[pos];
                self.current_index = Some(original_idx);
                return self.current();
            }
            None
        } else {
            let next_idx = match self.current_index {
                Some(curr) => {
                    if curr + 1 < self.items.len() {
                        Some(curr + 1)
                    } else if self.repeat == RepeatMode::All {
                        Some(0)
                    } else {
                        None
                    }
                }
                None => Some(0),
            };

            if let Some(idx) = next_idx {
                self.current_index = Some(idx);
                return self.current();
            }
            None
        }
    }

    /// Moves to the previous item according to shuffle and repeat modes.
    pub fn previous(&mut self) -> Option<&PlaylistItem> {
        if self.items.is_empty() {
            return None;
        }

        if self.repeat == RepeatMode::Single {
            return self.current();
        }

        if self.shuffle {
            let prev_pos = match self.shuffled_pos {
                Some(pos) => {
                    if pos > 0 {
                        Some(pos - 1)
                    } else if self.repeat == RepeatMode::All {
                        Some(self.shuffled_order.len() - 1)
                    } else {
                        None
                    }
                }
                None => Some(0),
            };

            if let Some(pos) = prev_pos {
                self.shuffled_pos = Some(pos);
                let original_idx = self.shuffled_order[pos];
                self.current_index = Some(original_idx);
                return self.current();
            }
            None
        } else {
            let prev_idx = match self.current_index {
                Some(curr) => {
                    if curr > 0 {
                        Some(curr - 1)
                    } else if self.repeat == RepeatMode::All {
                        Some(self.items.len() - 1)
                    } else {
                        None
                    }
                }
                None => Some(0),
            };

            if let Some(idx) = prev_idx {
                self.current_index = Some(idx);
                return self.current();
            }
            None
        }
    }

    /// Returns the current shuffle state.
    pub fn shuffle(&self) -> bool {
        self.shuffle
    }

    /// Sets or toggles shuffle. Rebuilds the shuffled permutation non-destructively.
    pub fn set_shuffle(&mut self, enabled: bool) {
        if self.shuffle == enabled {
            return;
        }
        self.shuffle = enabled;
        self.rebuild_shuffle();
    }

    /// Toggles shuffle mode and returns the new state.
    pub fn toggle_shuffle(&mut self) -> bool {
        self.set_shuffle(!self.shuffle);
        self.shuffle
    }

    /// Returns the current repeat mode.
    pub fn repeat(&self) -> RepeatMode {
        self.repeat
    }

    /// Sets the repeat mode.
    pub fn set_repeat(&mut self, mode: RepeatMode) {
        self.repeat = mode;
    }

    /// Cycles the repeat mode (Off -> All -> Single -> Off) and returns the new mode.
    pub fn cycle_repeat(&mut self) -> RepeatMode {
        self.repeat = self.repeat.cycle();
        self.repeat
    }

    /// Rebuilds the shuffled permutation using Fisher-Yates shuffle.
    fn rebuild_shuffle(&mut self) {
        let n = self.items.len();
        self.shuffled_order = (0..n).collect();

        if self.shuffle && n > 1 {
            for i in (1..n).rev() {
                let r = (xorshift64(&mut self.rng_seed) as usize) % (i + 1);
                self.shuffled_order.swap(i, r);
            }
        }

        // Synchronize shuffled_pos with current_index
        if let Some(curr) = self.current_index {
            self.shuffled_pos = self.shuffled_order.iter().position(|&idx| idx == curr);
        } else {
            self.shuffled_pos = None;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_playlist_basic_add_remove() {
        let mut pl = Playlist::new();
        assert!(pl.is_empty());
        assert_eq!(pl.len(), 0);

        let idx0 = pl.add_file("/path/to/video1.mp4");
        let idx1 = pl.add_url("https://example.com/stream.m3u8");
        assert_eq!(idx0, 0);
        assert_eq!(idx1, 1);
        assert_eq!(pl.len(), 2);
        assert_eq!(pl.current_index(), Some(0));

        let cur = pl.current().expect("Expected current item");
        assert_eq!(cur.title(), "video1.mp4");
        assert!(!cur.is_url());

        // Remove first item
        let removed = pl.remove(0).expect("Failed to remove item 0");
        assert_eq!(removed.title(), "video1.mp4");
        assert_eq!(pl.len(), 1);
        assert_eq!(pl.current_index(), Some(0));

        let cur = pl.current().expect("Expected current item");
        assert_eq!(cur.location(), "https://example.com/stream.m3u8");
        assert!(cur.is_url());
    }

    #[test]
    fn test_playlist_move_item() {
        let mut pl = Playlist::new();
        pl.add_file("/a.mp4");
        pl.add_file("/b.mp4");
        pl.add_file("/c.mp4");

        pl.set_current(0);
        assert_eq!(pl.current().map(|i| i.title()), Some("a.mp4"));

        // Move 0 to 2: order becomes b, c, a
        assert!(pl.move_item(0, 2));
        assert_eq!(pl.items()[0].title(), "b.mp4");
        assert_eq!(pl.items()[1].title(), "c.mp4");
        assert_eq!(pl.items()[2].title(), "a.mp4");
        assert_eq!(pl.current_index(), Some(2));
    }

    #[test]
    fn test_playlist_repeat_modes() {
        let mut pl = Playlist::new();
        pl.add_file("/1.mp4");
        pl.add_file("/2.mp4");

        // RepeatMode::Off
        pl.set_current(0);
        assert_eq!(pl.next().map(|i| i.title()), Some("2.mp4"));
        assert_eq!(pl.next(), None); // End of list
        assert!(!pl.has_next());

        // RepeatMode::All
        pl.set_repeat(RepeatMode::All);
        assert!(pl.has_next());
        assert_eq!(pl.next().map(|i| i.title()), Some("1.mp4")); // Wraps to start
        assert_eq!(pl.previous().map(|i| i.title()), Some("2.mp4")); // Wraps to end

        // RepeatMode::Single
        pl.set_repeat(RepeatMode::Single);
        assert_eq!(pl.next().map(|i| i.title()), Some("2.mp4"));
        assert_eq!(pl.next().map(|i| i.title()), Some("2.mp4"));
        assert_eq!(pl.previous().map(|i| i.title()), Some("2.mp4"));
    }

    #[test]
    fn test_playlist_shuffle_traversal_non_destructive() {
        let mut pl = Playlist::new();
        for i in 0..10 {
            pl.add_file(format!("/video_{i}.mp4"));
        }

        pl.set_shuffle(true);
        assert!(pl.shuffle());

        let mut visited = Vec::new();
        pl.set_current(pl.shuffled_order[0]);
        visited.push(pl.current().unwrap().title().to_string());

        while let Some(next) = pl.next() {
            visited.push(next.title().to_string());
        }

        assert_eq!(visited.len(), 10);
        // All items should be visited exactly once
        let mut sorted = visited.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), 10);

        // Turn shuffle off: original order must remain intact
        pl.set_shuffle(false);
        assert_eq!(pl.items()[0].title(), "video_0.mp4");
        assert_eq!(pl.items()[9].title(), "video_9.mp4");
    }
}
