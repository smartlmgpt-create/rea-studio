//! Real undo/redo with cross-session persistence.
//!
//! Model: the stack holds EVERY state (including the current one); `pos`
//! points at the current state. Undo walks back, redo walks forward.
//! Keystroke bursts are coalesced by time (COALESCE_MS) so one Ctrl+Z undoes
//! a burst, not a single character. Programmatic edits (paste, replace-all,
//! suggestion accept) call `force()` to break the burst.
//!
//! The newest entries are persisted to `backups/<fnv-of-path>/` so undo
//! history AND unsaved buffers survive app restarts (crash recovery).

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

const COALESCE_MS: u64 = 350;
const MAX_ENTRIES: usize = 200;
const MAX_TOTAL_BYTES: usize = 16 * 1024 * 1024;
const MAX_PERSISTED: usize = 10;
const PERSIST_MAX_FILE_BYTES: usize = 2 * 1024 * 1024;

#[derive(Clone)]
struct Entry {
    text: String,
    cursor: Option<usize>,
}

#[derive(Default)]
pub struct UndoStack {
    entries: Vec<Entry>,
    pos: usize, // index of the current state
    last_push: Option<Instant>,
    persist_needed: bool,
}

impl UndoStack {
    pub fn with_initial(text: &str) -> Self {
        let mut s = Self::default();
        s.entries.push(Entry { text: text.to_string(), cursor: None });
        s.pos = 0;
        s
    }

    /// Record a new state. Call after EVERY edit (typing, paste, programmatic).
    pub fn after_edit(&mut self, new_text: &str, new_cursor: Option<usize>) {
        let now = Instant::now();
        let coalesce = self
            .last_push
            .map(|t| t.elapsed() < Duration::from_millis(COALESCE_MS))
            .unwrap_or(false);
        self.last_push = Some(now);
        self.push_state(new_text, new_cursor, coalesce);
    }

    /// Record a state breaking the coalescing burst (paste, replace-all…).
    pub fn force(&mut self, new_text: &str, new_cursor: Option<usize>) {
        self.last_push = Some(Instant::now());
        self.push_state(new_text, new_cursor, false);
    }

    fn push_state(&mut self, new_text: &str, new_cursor: Option<usize>, coalesce: bool) {
        // identical state? nothing to record
        if let Some(cur) = self.entries.get(self.pos) {
            if cur.text == new_text {
                return;
            }
        }
        // typing burst: REPLACE the current head instead of stacking a step
        if coalesce && !self.entries.is_empty() && self.pos + 1 == self.entries.len() {
            self.entries[self.pos] = Entry { text: new_text.to_string(), cursor: new_cursor };
        } else {
            self.entries.truncate(self.pos + 1);
            self.entries.push(Entry { text: new_text.to_string(), cursor: new_cursor });
            self.pos = self.entries.len() - 1;
        }
        // budget
        while self.entries.len() > MAX_ENTRIES || self.total_bytes() > MAX_TOTAL_BYTES {
            if self.entries.len() <= 2 {
                break;
            }
            self.entries.remove(0);
            self.pos = self.pos.saturating_sub(1);
        }
        self.persist_needed = true;
    }

    fn total_bytes(&self) -> usize {
        self.entries.iter().map(|e| e.text.len()).sum()
    }

    pub fn undo(&mut self) -> Option<(String, Option<usize>)> {
        if self.pos == 0 {
            return None;
        }
        self.pos -= 1;
        self.entries.get(self.pos).map(|e| (e.text.clone(), e.cursor))
    }

    pub fn redo(&mut self) -> Option<(String, Option<usize>)> {
        if self.pos + 1 >= self.entries.len() {
            return None;
        }
        self.pos += 1;
        self.entries.get(self.pos).map(|e| (e.text.clone(), e.cursor))
    }

    pub fn can_undo(&self) -> bool {
        self.pos > 0
    }
    pub fn can_redo(&self) -> bool {
        self.pos + 1 < self.entries.len()
    }

    /// The current (newest) text — used for crash recovery.
    pub fn newest_text(&self) -> Option<&str> {
        self.entries.last().map(|e| e.text.as_str())
    }

    // ------------------------------------------------- persistence

    fn backup_dir(base: &Path, key: &str) -> PathBuf {
        base.join(format!("{:016x}", crate::util::fnv(key)))
    }

    /// Persist the newest entries so history survives app restarts.
    pub fn persist(&mut self, base: &Path, key: &str) {
        if !self.persist_needed && !dir_has_meta(&Self::backup_dir(base, key)) {
            return;
        }
        self.persist_needed = false;
        let dir = Self::backup_dir(base, key);
        let _ = std::fs::create_dir_all(&dir);
        if let Ok(rd) = std::fs::read_dir(&dir) {
            for e in rd.flatten() {
                let name = e.file_name().to_string_lossy().to_string();
                if name.starts_with("snap_") {
                    let _ = std::fs::remove_file(e.path());
                }
            }
        }
        let start = self.entries.len().saturating_sub(MAX_PERSISTED);
        let mut metas: Vec<serde_json::Value> = Vec::new();
        for (i, e) in self.entries[start..].iter().enumerate() {
            if e.text.len() > PERSIST_MAX_FILE_BYTES {
                continue;
            }
            let fname = format!("snap_{:03}.txt", i);
            if std::fs::write(dir.join(&fname), &e.text).is_ok() {
                metas.push(serde_json::json!({
                    "file": fname,
                    "cursor": e.cursor,
                }));
            }
        }
        let pos_rel = self.pos.saturating_sub(start);
        let meta = serde_json::json!({ "entries": metas, "pos": pos_rel });
        if let Ok(j) = serde_json::to_string(&meta) {
            let _ = std::fs::write(dir.join("meta.json"), j);
        }
    }

    /// Restore a persisted stack (cross-session undo).
    pub fn load(base: &Path, key: &str) -> Option<UndoStack> {
        let dir = Self::backup_dir(base, key);
        let meta_s = std::fs::read_to_string(dir.join("meta.json")).ok()?;
        let meta: serde_json::Value = serde_json::from_str(&meta_s).ok()?;
        let mut entries = Vec::new();
        for e in meta.get("entries")?.as_array()? {
            let fname = e.get("file")?.as_str()?;
            let text = std::fs::read_to_string(dir.join(fname)).ok()?;
            let cursor = e.get("cursor").and_then(|c| c.as_u64()).map(|c| c as usize);
            entries.push(Entry { text, cursor });
        }
        if entries.is_empty() {
            return None;
        }
        let pos = (meta.get("pos").and_then(|t| t.as_u64()).unwrap_or(entries.len() as u64 - 1) as usize)
            .min(entries.len() - 1);
        Some(UndoStack { entries, pos, last_push: None, persist_needed: false })
    }

    /// The latest persisted buffer (crash recovery of UNSAVED changes).
    pub fn load_last_text(base: &Path, key: &str) -> Option<String> {
        let st = Self::load(base, key)?;
        st.entries.get(st.pos).map(|e| e.text.clone())
    }

    pub fn clear_persisted(base: &Path, key: &str) {
        let dir = Self::backup_dir(base, key);
        if dir.exists() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

fn dir_has_meta(dir: &Path) -> bool {
    dir.join("meta.json").exists()
}
