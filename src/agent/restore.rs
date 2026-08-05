//! Named restore points (PLAN M7a).
//!
//! A restore point is a full trunk snapshot as `PathState` per path — O(filesystem)
//! at save time, but capped in count and serialized size so localStorage stays
//! one trunk plus a handful of points. Prefer this over persisting N live forks.
//!
//! Undo (M4 journal) remains the fast path for "revert the last agent run."
//! Restore points are for "go back to before I tried X."

use crate::agent::journal::{restore_path, PathState};
use crate::filesystem::FileSystem;
use serde::{Deserialize, Serialize};

/// localStorage key — outside the VFS, like the API key.
pub const RESTORE_POINTS_STORAGE_KEY: &str = "kernelosv2_restore_points";

/// Soft cap on how many points we keep. Oldest dropped first.
pub const MAX_RESTORE_POINTS: usize = 5;

/// Soft cap on serialized JSON size (UTF-16 code units ≈ chars for ASCII JSON).
/// Refuse a save that would push the store over this after eviction.
pub const MAX_STORE_CHARS: usize = 512_000;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RestorePoint {
    pub id: String,
    pub name: String,
    pub created_ms: u64,
    /// Complete trunk: every path → state. Sorted by path on capture.
    pub paths: Vec<(String, PathState)>,
}

#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct RestorePointStore {
    points: Vec<RestorePoint>,
}

impl RestorePointStore {
    pub fn new() -> Self {
        Self { points: Vec::new() }
    }

    pub fn load() -> Self {
        #[cfg(target_arch = "wasm32")]
        {
            if let Some(raw) = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
                .and_then(|s| s.get_item(RESTORE_POINTS_STORAGE_KEY).ok().flatten())
            {
                if let Ok(store) = serde_json::from_str::<RestorePointStore>(&raw) {
                    return store;
                }
            }
        }
        Self::new()
    }

    pub fn persist(&self) -> Result<(), String> {
        #[cfg(target_arch = "wasm32")]
        {
            let Some(storage) = web_sys::window()
                .and_then(|w| w.local_storage().ok().flatten())
            else {
                return Ok(());
            };
            let serialized = serde_json::to_string(self)
                .map_err(|e| format!("serialize restore points: {e}"))?;
            if serialized.chars().count() > MAX_STORE_CHARS {
                return Err(format!(
                    "restore point store exceeds {MAX_STORE_CHARS} character cap"
                ));
            }
            storage
                .set_item(RESTORE_POINTS_STORAGE_KEY, &serialized)
                .map_err(|e| format!("persist restore points: {e:?}"))?;
        }
        let _ = self;
        Ok(())
    }

    pub fn points(&self) -> &[RestorePoint] {
        &self.points
    }

    pub fn get(&self, id: &str) -> Option<&RestorePoint> {
        self.points.iter().find(|p| p.id == id)
    }

    /// Capture the current trunk as a named restore point.
    pub fn save_point(&mut self, fs: &FileSystem, name: &str) -> Result<&RestorePoint, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("restore point name must not be empty".into());
        }

        let point = RestorePoint {
            id: next_id(),
            name: name.to_string(),
            created_ms: now_ms(),
            paths: capture_trunk(fs),
        };

        // Drop oldest until we have room for one more under the count cap.
        while self.points.len() >= MAX_RESTORE_POINTS {
            self.points.remove(0);
        }
        self.points.push(point);

        // If serialized size blows the char cap, drop oldest until it fits or
        // only the newest remains — then fail if still too big.
        loop {
            let size = serde_json::to_string(self)
                .map_err(|e| format!("serialize restore points: {e}"))?
                .chars()
                .count();
            if size <= MAX_STORE_CHARS {
                break;
            }
            if self.points.len() <= 1 {
                self.points.clear();
                return Err(
                    "restore point is larger than the store cap; not saved".into(),
                );
            }
            self.points.remove(0);
        }

        self.persist()?;
        Ok(self.points.last().expect("just pushed"))
    }

    pub fn delete(&mut self, id: &str) -> Result<bool, String> {
        let before = self.points.len();
        self.points.retain(|p| p.id != id);
        if self.points.len() == before {
            return Ok(false);
        }
        self.persist()?;
        Ok(true)
    }

    /// Replace trunk with the named point. Caller should clear the run journal.
    pub fn restore(&self, fs: &mut FileSystem, id: &str) -> Result<(), String> {
        let point = self
            .get(id)
            .ok_or_else(|| format!("unknown restore point {id}"))?;
        apply_trunk(fs, &point.paths)
    }
}

/// Snapshot every path currently in the filesystem.
pub fn capture_trunk(fs: &FileSystem) -> Vec<(String, PathState)> {
    let mut paths = fs.all_paths();
    paths.sort();
    paths
        .into_iter()
        .map(|p| {
            let state = PathState::capture(fs, &p);
            (p, state)
        })
        .collect()
}

/// Make `fs` match `snapshot` exactly (plus keep `/` if missing from a corrupt
/// snapshot — we always ensure root exists via restore_path Directory).
pub fn apply_trunk(fs: &mut FileSystem, snapshot: &[(String, PathState)]) -> Result<(), String> {
    let target: std::collections::HashSet<&str> =
        snapshot.iter().map(|(p, _)| p.as_str()).collect();

    // Remove paths not in the snapshot, deepest first so directories empty out.
    let mut extras: Vec<String> = fs
        .all_paths()
        .into_iter()
        .filter(|p| p != "/" && !target.contains(p.as_str()))
        .collect();
    extras.sort_by_key(|p| std::cmp::Reverse(p.len()));
    for path in extras {
        if !fs.exists(&path) {
            continue;
        }
        let recursive = fs.is_directory(&path);
        fs.delete(&path, recursive)?;
    }

    // Restore snapshot paths, parents before children.
    let mut ordered: Vec<&(String, PathState)> = snapshot.iter().collect();
    ordered.sort_by_key(|(p, _)| p.len());
    for (path, state) in ordered {
        restore_path(fs, path, state)?;
    }

    Ok(())
}

fn next_id() -> String {
    format!("rp-{}", now_ms())
}

fn now_ms() -> u64 {
    #[cfg(target_arch = "wasm32")]
    {
        js_sys::Date::now() as u64
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::time::{SystemTime, UNIX_EPOCH};
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_millis() as u64)
            .unwrap_or(0)
    }
}

/// Prompt for a restore-point name in the browser; `None` if cancelled.
pub fn prompt_restore_name(default: &str) -> Option<String> {
    #[cfg(target_arch = "wasm32")]
    {
        let window = web_sys::window()?;
        let name = window
            .prompt_with_message_and_default("Name this restore point:", default)
            .ok()??;
        let name = name.trim().to_string();
        if name.is_empty() {
            None
        } else {
            Some(name)
        }
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        Some(default.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> FileSystem {
        let mut fs = FileSystem::default();
        let _ = fs.write_file("/home/documents/a.txt", "alpha");
        let _ = fs.write_file("/home/documents/b.txt", "bravo");
        fs
    }

    #[test]
    fn save_and_restore_round_trips_trunk() {
        let mut fs = seed();
        let mut store = RestorePointStore::new();
        store.save_point(&fs, "before").unwrap();

        fs.write_file("/home/documents/a.txt", "CHANGED").unwrap();
        fs.write_file("/home/documents/c.txt", "new").unwrap();
        fs.delete("/home/documents/b.txt", false).unwrap();

        let id = store.points()[0].id.clone();
        store.restore(&mut fs, &id).unwrap();

        assert_eq!(fs.read_file("/home/documents/a.txt").unwrap(), "alpha");
        assert_eq!(fs.read_file("/home/documents/b.txt").unwrap(), "bravo");
        assert!(!fs.exists("/home/documents/c.txt"));
    }

    #[test]
    fn empty_name_rejected() {
        let fs = seed();
        let mut store = RestorePointStore::new();
        assert!(store.save_point(&fs, "  ").is_err());
    }

    #[test]
    fn max_points_evicts_oldest() {
        let fs = seed();
        let mut store = RestorePointStore::new();
        for i in 0..(MAX_RESTORE_POINTS + 2) {
            // Unique ids via now_ms — sleep not available; force distinct by
            // mutating store ids after save if clocks collide.
            let name = format!("p{i}");
            store.save_point(&fs, &name).unwrap();
            // Ensure uniqueness even if now_ms is sticky in tests.
            if let Some(last) = store.points.last_mut() {
                last.id = format!("rp-test-{i}");
            }
        }
        assert_eq!(store.points().len(), MAX_RESTORE_POINTS);
        assert_eq!(store.points()[0].name, "p2");
        assert_eq!(
            store.points().last().unwrap().name,
            format!("p{}", MAX_RESTORE_POINTS + 1)
        );
    }

    #[test]
    fn delete_removes_point() {
        let fs = seed();
        let mut store = RestorePointStore::new();
        store.save_point(&fs, "x").unwrap();
        let id = store.points()[0].id.clone();
        assert!(store.delete(&id).unwrap());
        assert!(store.points().is_empty());
    }

    #[test]
    fn restore_unknown_id_errors() {
        let mut fs = seed();
        let store = RestorePointStore::new();
        assert!(store.restore(&mut fs, "nope").is_err());
    }
}
