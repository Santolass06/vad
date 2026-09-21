//! Meeting bookmarks and notes management module.
//!
//! Provides timestamped meeting notes exportable to Markdown in plain text format `[00:04:12]`
//! per PLANO_VAD.md §4.4 (v1 decision: plain text timestamps; URI scheme postponed to post-M6).
//!
//! Stores bookmarks atomically per media file in `~/.local/share/vad/bookmarks/`.

use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use tracing::{debug, info};

use crate::error::VadError;
use crate::util::{vad_bookmarks_dir, write_atomic};

/// A single timestamped meeting bookmark or note.
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq)]
pub struct Bookmark {
    /// Unique identifier within the store.
    pub id: u64,
    /// Timestamp in seconds from the beginning of the media.
    pub timestamp_secs: f64,
    /// Text content of the note.
    pub text: String,
    /// Creation time in Unix epoch seconds.
    pub created_at_unix: u64,
}

impl Bookmark {
    /// Formats this bookmark's timestamp into plain text `[HH:MM:SS]`.
    pub fn formatted_timestamp(&self) -> String {
        format_timestamp_secs(self.timestamp_secs)
    }
}

/// Formats seconds into plain text `[HH:MM:SS]`, always with the hours field (§4.4: `[00:04:12]`).
pub fn format_timestamp_secs(seconds: f64) -> String {
    let total_secs = seconds.max(0.0).floor() as u64;
    let s = total_secs % 60;
    let m = (total_secs / 60) % 60;
    let h = total_secs / 3600;
    format!("[{h:02}:{m:02}:{s:02}]")
}

/// Collection of bookmarks associated with a specific media file.
#[derive(Clone, Debug, Default, Serialize, Deserialize, PartialEq)]
pub struct BookmarkStore {
    /// Path or URL of the associated media file.
    pub media_path: String,
    /// Sorted list of bookmarks (ordered by `timestamp_secs`).
    pub bookmarks: Vec<Bookmark>,
    /// Monotonically increasing ID counter for newly created bookmarks.
    next_id: u64,
}

impl BookmarkStore {
    /// Creates a new empty store for the given media path.
    pub fn new(media_path: impl Into<String>) -> Self {
        Self {
            media_path: media_path.into(),
            bookmarks: Vec::new(),
            next_id: 1,
        }
    }

    /// Adds a new bookmark at `timestamp_secs` with `text`.
    /// Inserts the bookmark in sorted order by `timestamp_secs`.
    /// Returns the assigned ID.
    pub fn add(&mut self, timestamp_secs: f64, text: impl Into<String>) -> u64 {
        let id = self.next_id;
        self.next_id += 1;

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        let bookmark = Bookmark {
            id,
            timestamp_secs: timestamp_secs.max(0.0),
            text: text.into(),
            created_at_unix: now,
        };

        // Insert maintaining sorted order by timestamp
        let idx = self
            .bookmarks
            .partition_point(|b| b.timestamp_secs <= bookmark.timestamp_secs);
        self.bookmarks.insert(idx, bookmark);

        debug!(
            "Added bookmark #{} at {:.2}s for {}",
            id, timestamp_secs, self.media_path
        );
        id
    }

    /// Removes a bookmark by ID. Returns true if found and removed.
    pub fn remove(&mut self, id: u64) -> bool {
        if let Some(pos) = self.bookmarks.iter().position(|b| b.id == id) {
            self.bookmarks.remove(pos);
            debug!("Removed bookmark #{}", id);
            true
        } else {
            false
        }
    }

    /// Edits the text and/or timestamp of an existing bookmark.
    /// If timestamp is changed, re-sorts the list to maintain chronological order.
    /// Returns true if the bookmark was found and updated.
    pub fn edit(
        &mut self,
        id: u64,
        new_text: Option<String>,
        new_timestamp: Option<f64>,
    ) -> bool {
        let Some(pos) = self.bookmarks.iter().position(|b| b.id == id) else {
            return false;
        };

        let mut timestamp_changed = false;
        if let Some(txt) = new_text {
            self.bookmarks[pos].text = txt;
        }
        if let Some(ts) = new_timestamp {
            if (self.bookmarks[pos].timestamp_secs - ts).abs() > 1e-4 {
                self.bookmarks[pos].timestamp_secs = ts.max(0.0);
                timestamp_changed = true;
            }
        }

        if timestamp_changed {
            self.bookmarks.sort_by(|a, b| {
                a.timestamp_secs
                    .partial_cmp(&b.timestamp_secs)
                    .unwrap_or(std::cmp::Ordering::Equal)
            });
        }

        true
    }

    /// Finds a bookmark by ID.
    pub fn get(&self, id: u64) -> Option<&Bookmark> {
        self.bookmarks.iter().find(|b| b.id == id)
    }

    /// Exports bookmarks and optional transcription content into formatted Markdown.
    /// Timestamps are strictly plain text `[00:04:12]` per PLANO_VAD.md §4.4.
    pub fn export_to_markdown(&self, title: &str, transcription_md: Option<&str>) -> String {
        let mut md = String::new();
        md.push_str(&format!("# Notas da Reunião — {}\n\n", title));
        md.push_str("Gerado automaticamente pelo VAD.\n\n---\n\n");

        md.push_str("## Marcadores da Reunião\n\n");
        if self.bookmarks.is_empty() {
            md.push_str("*(Nenhum marcador registado)*\n\n");
        } else {
            for bm in &self.bookmarks {
                let ts = bm.formatted_timestamp();
                md.push_str(&format!("- {} {}\n", ts, bm.text));
            }
            md.push('\n');
        }

        if let Some(transcription) = transcription_md {
            md.push_str("---\n\n");
            md.push_str("## Transcrição (Whisper AI)\n\n");
            md.push_str(transcription);
            md.push('\n');
        }

        md
    }

    /// Generates a deterministic storage path for this media in `dir`.
    pub fn storage_path_in_dir(media_path: &str, dir: &Path) -> PathBuf {
        let stem = Path::new(media_path)
            .file_stem()
            .and_then(|s| s.to_str())
            .unwrap_or("media");

        // Hash the full media path to avoid collisions between different folders with the same
        // filename. FNV-1a, not `DefaultHasher`: its algorithm is unspecified and may change
        // between Rust releases, which would silently orphan every saved bookmark file.
        let hash_suffix = media_path
            .bytes()
            .fold(0xcbf2_9ce4_8422_2325_u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3));

        dir.join(format!("{stem}_{hash_suffix:016x}.json"))
    }

    /// Returns the standard file path where this store is persisted.
    pub fn standard_storage_path(&self) -> PathBuf {
        Self::storage_path_in_dir(&self.media_path, &vad_bookmarks_dir())
    }

    /// Persists this store to the standard bookmarks directory (see [`Self::save_to_dir`]).
    pub fn save_to_disk(&self) -> Result<(), VadError> {
        self.save_to_dir(&vad_bookmarks_dir())
    }

    /// Persists this store in `dir` atomically using `write_atomic` (§4.30).
    ///
    /// A store with no bookmarks (or no media) leaves nothing on disk — an empty file per opened
    /// media would only be a history of what the user played — and removes a file left over from
    /// bookmarks that were all deleted.
    pub fn save_to_dir(&self, dir: &Path) -> Result<(), VadError> {
        if self.media_path.is_empty() {
            return Ok(());
        }
        let path = Self::storage_path_in_dir(&self.media_path, dir);

        if self.bookmarks.is_empty() {
            return match fs::remove_file(&path) {
                Err(e) if e.kind() != std::io::ErrorKind::NotFound => Err(VadError::Io(e)),
                _ => Ok(()),
            };
        }

        let json = serde_json::to_vec_pretty(self)
            .map_err(|e| VadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

        write_atomic(&path, &json)?;
        info!("Saved {} bookmarks for {} to {:?}", self.bookmarks.len(), self.media_path, path);
        Ok(())
    }

    /// Loads the store for `media_path` from the standard bookmarks directory, or an empty one.
    pub fn load_from_disk(media_path: &str) -> Result<Self, VadError> {
        Self::load_from_dir(media_path, &vad_bookmarks_dir())
    }

    /// Loads the store for `media_path` from `dir` if it exists, or creates an empty one.
    pub fn load_from_dir(media_path: &str, dir: &Path) -> Result<Self, VadError> {
        let path = Self::storage_path_in_dir(media_path, dir);
        if !path.exists() {
            return Ok(Self::new(media_path));
        }

        let bytes = fs::read(&path).map_err(VadError::Io)?;
        let mut store: Self = serde_json::from_slice(&bytes)
            .map_err(|e| VadError::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, e)))?;

        // Ensure next_id is greater than any existing bookmark ID
        let max_id = store.bookmarks.iter().map(|b| b.id).max().unwrap_or(0);
        store.next_id = max_id + 1;

        // Ensure sorted
        store.bookmarks.sort_by(|a, b| {
            a.timestamp_secs
                .partial_cmp(&b.timestamp_secs)
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        info!("Loaded {} bookmarks for {} from {:?}", store.bookmarks.len(), media_path, path);
        Ok(store)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_format_timestamp_secs_plain_text() {
        assert_eq!(format_timestamp_secs(0.0), "[00:00:00]");
        assert_eq!(format_timestamp_secs(42.0), "[00:00:42]");
        assert_eq!(format_timestamp_secs(252.0), "[00:04:12]");
        assert_eq!(format_timestamp_secs(3665.0), "[01:01:05]");
        assert_eq!(format_timestamp_secs(-5.0), "[00:00:00]");
    }

    #[test]
    fn test_bookmark_store_add_sorted() {
        let mut store = BookmarkStore::new("/path/reuniao.opus");

        let id1 = store.add(252.0, "Introdução dos objetivos");
        let id2 = store.add(60.0, "Abertura");
        let id3 = store.add(1125.0, "Discussão do orçamento");

        assert_eq!(store.bookmarks.len(), 3);
        // Must be sorted by timestamp
        assert_eq!(store.bookmarks[0].id, id2);
        assert_eq!(store.bookmarks[0].timestamp_secs, 60.0);
        assert_eq!(store.bookmarks[1].id, id1);
        assert_eq!(store.bookmarks[1].timestamp_secs, 252.0);
        assert_eq!(store.bookmarks[2].id, id3);
        assert_eq!(store.bookmarks[2].timestamp_secs, 1125.0);
    }

    #[test]
    fn test_bookmark_store_edit_and_remove() {
        let mut store = BookmarkStore::new("audio.mp3");

        let id = store.add(100.0, "Nota original");
        assert_eq!(store.get(id).unwrap().text, "Nota original");

        // Edit text
        assert!(store.edit(id, Some("Nota atualizada".to_string()), None));
        assert_eq!(store.get(id).unwrap().text, "Nota atualizada");

        // Edit timestamp and verify re-sorting
        let id2 = store.add(50.0, "Nota anterior");
        assert_eq!(store.bookmarks[0].id, id2);

        // Move id to before id2
        assert!(store.edit(id, None, Some(10.0)));
        assert_eq!(store.bookmarks[0].id, id);
        assert_eq!(store.bookmarks[1].id, id2);

        // Remove
        assert!(store.remove(id));
        assert_eq!(store.bookmarks.len(), 1);
        assert_eq!(store.bookmarks[0].id, id2);
    }

    #[test]
    fn test_export_to_markdown_format_section_4_4() {
        let mut store = BookmarkStore::new("reuniao.mkv");
        store.add(252.0, "Introdução dos objetivos do projeto");
        store.add(1125.0, "Discussão do orçamento de TI");
        store.add(1930.0, "Aprovação da transição para Rust");

        let md = store.export_to_markdown("Reunião de Direção", None);

        // Confirm plain text format `[00:04:12]` per §4.4
        assert!(md.contains("# Notas da Reunião — Reunião de Direção"));
        assert!(md.contains("- [00:04:12] Introdução dos objetivos do projeto"));
        assert!(md.contains("- [00:18:45] Discussão do orçamento de TI"));
        assert!(md.contains("- [00:32:10] Aprovação da transição para Rust"));
    }

    fn scratch_dir(tag: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("vad_bm_{tag}_{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_save_and_load_roundtrip_through_real_store_api() {
        let dir = scratch_dir("roundtrip");
        let media = "/home/user/gravação.opus";

        let mut store = BookmarkStore::new(media);
        store.add(300.0, "Decisão final");
        store.add(120.0, "Ponto importante");
        store.save_to_dir(&dir).expect("save");

        let mut loaded = BookmarkStore::load_from_dir(media, &dir).expect("load");
        assert_eq!(loaded, store);
        assert_eq!(loaded.bookmarks[0].text, "Ponto importante");

        // ids keep increasing after a reload (no id reuse)
        let new_id = loaded.add(10.0, "Nova");
        assert!(loaded.bookmarks.iter().filter(|b| b.id == new_id).count() == 1);
        assert!(new_id > store.bookmarks.iter().map(|b| b.id).max().unwrap());

        // a different media path never sees these bookmarks
        assert!(BookmarkStore::load_from_dir("/other/gravação.opus", &dir).unwrap().bookmarks.is_empty());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_empty_store_leaves_no_file_and_removes_stale_one() {
        let dir = scratch_dir("empty");
        let media = "/media/a.mkv";
        let path = BookmarkStore::storage_path_in_dir(media, &dir);

        // Opening media and never adding a note must not write anything
        BookmarkStore::new(media).save_to_dir(&dir).unwrap();
        BookmarkStore::default().save_to_dir(&dir).unwrap();
        assert_eq!(fs::read_dir(&dir).unwrap().count(), 0, "empty stores must not create files");

        // Deleting the last bookmark removes the stale file
        let mut store = BookmarkStore::new(media);
        let id = store.add(5.0, "x");
        store.save_to_dir(&dir).unwrap();
        assert!(path.exists());
        store.remove(id);
        store.save_to_dir(&dir).unwrap();
        assert!(!path.exists());

        let _ = fs::remove_dir_all(&dir);
    }

    #[test]
    fn test_storage_path_is_stable_across_releases() {
        // The hash is part of the on-disk format: this literal must never change, or every
        // user's saved bookmarks are orphaned.
        let path = BookmarkStore::storage_path_in_dir("/home/user/reuniao.mkv", Path::new("/d"));
        assert_eq!(path, PathBuf::from("/d/reuniao_ad2c4c5aa7d1d0df.json"));
        // same file name in another folder gets another file
        assert_ne!(path, BookmarkStore::storage_path_in_dir("/other/reuniao.mkv", Path::new("/d")));
    }
}
