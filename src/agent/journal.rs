//! Copy-on-write VFS journal (PLAN M4).
//!
//! A `Journal` is an owned, inspectable, cloneable value — not a side effect
//! buried in component state. Undo is `journal.revert(&mut fs)`. M7 can keep
//! several journals from forked runs and compare their `prior`/`after` pairs
//! without redesigning this type.

use crate::filesystem::{FileMetadata, FileSystem, FileType};

/// Recursive deletes that would remove this many entries (or more) are gated.
pub const RECURSIVE_DELETE_GATE_THRESHOLD: usize = 20;

/// Paths under `/system/config` (and the API key store, which is outside the
/// VFS) are gated — they escape casual undo into durable session/theme state.
pub fn is_protected_path(path: &str) -> bool {
    let path = FileSystem::normalize_path(path);
    FileSystem::is_inside("/system/config", &path)
}

/// Prior (and after) state of one path.
#[derive(Debug, Clone, PartialEq)]
pub enum PathState {
    /// Path did not exist.
    Absent,
    /// Directory — metadata only, no body.
    Directory { metadata: FileMetadata },
    /// Regular file with body.
    File {
        metadata: FileMetadata,
        content: String,
    },
}

impl PathState {
    pub fn capture(fs: &FileSystem, path: &str) -> Self {
        let path = FileSystem::normalize_path(path);
        match fs.get_metadata(&path) {
            None => PathState::Absent,
            Some(metadata) if matches!(metadata.file_type, FileType::Directory) => {
                PathState::Directory { metadata }
            }
            Some(metadata) => {
                let content = fs.read_file(&path).unwrap_or_default();
                PathState::File { metadata, content }
            }
        }
    }

    pub fn is_absent(&self) -> bool {
        matches!(self, PathState::Absent)
    }

    /// Human-readable body for transcript diffs.
    pub fn display_body(&self) -> String {
        match self {
            PathState::Absent => "(absent)".into(),
            PathState::Directory { .. } => "(directory)".into(),
            PathState::File { content, .. } => content.clone(),
        }
    }
}

/// One path the agent touched, with first-touch prior and latest after.
#[derive(Debug, Clone, PartialEq)]
pub struct FileDelta {
    pub path: String,
    pub prior: PathState,
    pub after: PathState,
}

/// Owned journal of a single agent run. O(files touched), not O(filesystem).
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Journal {
    entries: Vec<FileDelta>,
}

impl Journal {
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn entries(&self) -> &[FileDelta] {
        &self.entries
    }

    pub fn get(&self, path: &str) -> Option<&FileDelta> {
        let path = FileSystem::normalize_path(path);
        self.entries.iter().find(|e| e.path == path)
    }

    /// Record prior state on first touch only. Subsequent calls are no-ops for
    /// that path's `prior` (call `record_after` to refresh `after`).
    pub fn touch(&mut self, fs: &FileSystem, path: &str) {
        let path = FileSystem::normalize_path(path);
        if self.entries.iter().any(|e| e.path == path) {
            return;
        }
        let prior = PathState::capture(fs, &path);
        self.entries.push(FileDelta {
            path,
            prior: prior.clone(),
            after: prior,
        });
    }

    /// Refresh the `after` snapshot for a path that was already touched.
    pub fn record_after(&mut self, fs: &FileSystem, path: &str) {
        let path = FileSystem::normalize_path(path);
        if let Some(entry) = self.entries.iter_mut().find(|e| e.path == path) {
            entry.after = PathState::capture(fs, &path);
        }
    }

    /// Touch every path that a rename will affect (source subtree + destination
    /// subtree keys), then the caller runs `fs.rename` and `record_after`s.
    pub fn touch_rename(&mut self, fs: &FileSystem, old_path: &str, new_path: &str) {
        let old_path = FileSystem::normalize_path(old_path);
        let new_path = FileSystem::normalize_path(new_path);

        self.touch(fs, &old_path);
        for (child, relative) in fs.descendants_of(&old_path) {
            self.touch(fs, &child);
            let dest = format!("{new_path}/{relative}");
            self.touch(fs, &dest); // Absent before rename
        }
        self.touch(fs, &new_path); // Absent before rename
    }

    pub fn record_after_rename(&mut self, fs: &FileSystem, old_path: &str, new_path: &str) {
        let old_path = FileSystem::normalize_path(old_path);
        let new_path = FileSystem::normalize_path(new_path);

        // Old paths are now absent; new paths (and remapped children) present.
        self.record_after(fs, &old_path);
        self.record_after(fs, &new_path);

        // Children: we touched old children and new child keys before the rename.
        for entry_path in self
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
        {
            if entry_path != old_path
                && entry_path != new_path
                && (FileSystem::is_inside(&old_path, &entry_path)
                    || FileSystem::is_inside(&new_path, &entry_path))
            {
                self.record_after(fs, &entry_path);
            }
        }
    }

    /// Touch a path and, if it is a directory being deleted recursively, every
    /// descendant.
    pub fn touch_delete(&mut self, fs: &FileSystem, path: &str, recursive: bool) {
        let path = FileSystem::normalize_path(path);
        self.touch(fs, &path);
        if recursive && fs.is_directory(&path) {
            for (child, _) in fs.descendants_of(&path) {
                self.touch(fs, &child);
            }
        }
    }

    pub fn record_after_delete(&mut self, fs: &FileSystem, path: &str) {
        let path = FileSystem::normalize_path(path);
        // Refresh every entry under path (inclusive) that we touched.
        for entry_path in self
            .entries
            .iter()
            .map(|e| e.path.clone())
            .collect::<Vec<_>>()
        {
            if entry_path == path || FileSystem::is_inside(&path, &entry_path) {
                self.record_after(fs, &entry_path);
            }
        }
    }

    /// Revert every entry to its prior state.
    pub fn revert(&self, fs: &mut FileSystem) -> Result<(), String> {
        // Pass 1: remove anything that was Absent before (creations / rename
        // destinations). Deepest paths first so directories empty out.
        let mut created: Vec<&FileDelta> = self
            .entries
            .iter()
            .filter(|e| e.prior.is_absent())
            .collect();
        created.sort_by_key(|e| std::cmp::Reverse(e.path.len()));
        for entry in created {
            if fs.exists(&entry.path) {
                let recursive = fs.is_directory(&entry.path);
                fs.delete(&entry.path, recursive)?;
            }
        }

        // Pass 2: restore priors that were Present. Parents before children.
        let mut restore: Vec<&FileDelta> = self
            .entries
            .iter()
            .filter(|e| !e.prior.is_absent())
            .collect();
        restore.sort_by_key(|e| e.path.len());
        for entry in restore {
            restore_path(fs, &entry.path, &entry.prior)?;
        }

        Ok(())
    }

    /// Revert a single path. Other entries remain applicable.
    pub fn revert_path(&mut self, fs: &mut FileSystem, path: &str) -> Result<(), String> {
        let path = FileSystem::normalize_path(path);
        let entry = self
            .entries
            .iter()
            .find(|e| e.path == path)
            .cloned()
            .ok_or_else(|| format!("no journal entry for {path}"))?;

        match &entry.prior {
            PathState::Absent => {
                if fs.exists(&path) {
                    let recursive = fs.is_directory(&path);
                    fs.delete(&path, recursive)?;
                }
            }
            prior => {
                // If something else now occupies the path after a rename away,
                // delete it first only when prior wants this exact path back.
                restore_path(fs, &path, prior)?;
            }
        }

        if let Some(e) = self.entries.iter_mut().find(|e| e.path == path) {
            e.after = e.prior.clone();
        }
        Ok(())
    }

    /// Paths whose prior and after differ (something actually changed).
    pub fn changed_paths(&self) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|e| e.prior != e.after)
            .map(|e| e.path.as_str())
            .collect()
    }
}

fn restore_path(fs: &mut FileSystem, path: &str, prior: &PathState) -> Result<(), String> {
    match prior {
        PathState::Absent => {
            if fs.exists(path) {
                fs.delete(path, fs.is_directory(path))?;
            }
            Ok(())
        }
        PathState::Directory { metadata } => {
            if fs.exists(path) && !fs.is_directory(path) {
                fs.delete(path, false)?;
            }
            if !fs.exists(path) {
                // Ensure parent exists.
                if let Some(parent) = std::path::Path::new(path).parent() {
                    let parent = parent.to_string_lossy();
                    if parent != "/" && !parent.is_empty() && !fs.exists(parent.as_ref()) {
                        fs.create_directory(parent.as_ref(), true)?;
                    }
                }
                fs.create_directory(path, false)?;
            }
            // Best-effort metadata timestamps are not re-applied; tree shape
            // and content matter for undo. Silence unused.
            let _ = metadata;
            Ok(())
        }
        PathState::File { metadata, content } => {
            if fs.exists(path) && fs.is_directory(path) {
                fs.delete(path, true)?;
            }
            if let Some(parent) = std::path::Path::new(path).parent() {
                let parent = parent.to_string_lossy();
                if parent != "/" && !parent.is_empty() && !fs.exists(parent.as_ref()) {
                    fs.create_directory(parent.as_ref(), true)?;
                }
            }
            fs.write_file(path, content)?;
            let _ = metadata;
            Ok(())
        }
    }
}

/// Ask the user to confirm a gated operation. On the host (tests), always
/// declines — gated ops must be covered by explicit test setup, not prompts.
pub fn confirm_gate(message: &str) -> bool {
    #[cfg(target_arch = "wasm32")]
    {
        web_sys::window()
            .and_then(|w| w.confirm_with_message(message).ok())
            .unwrap_or(false)
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let _ = message;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seed() -> FileSystem {
        let mut fs = FileSystem::default();
        let _ = fs.write_file("/home/documents/a.txt", "alpha");
        let _ = fs.write_file("/home/documents/b.txt", "bravo");
        let _ = fs.create_directory("/home/documents/project", false);
        let _ = fs.write_file("/home/documents/project/c.txt", "charlie");
        fs
    }

    #[test]
    fn journal_records_prior_on_first_touch_only() {
        let mut fs = seed();
        let mut journal = Journal::new();

        journal.touch(&fs, "/home/documents/a.txt");
        fs.write_file("/home/documents/a.txt", "one").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        journal.touch(&fs, "/home/documents/a.txt"); // no-op for prior
        fs.write_file("/home/documents/a.txt", "two").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        fs.write_file("/home/documents/a.txt", "three").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        fs.write_file("/home/documents/a.txt", "four").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        fs.write_file("/home/documents/a.txt", "five").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        assert_eq!(journal.len(), 1);
        let e = journal.get("/home/documents/a.txt").unwrap();
        match &e.prior {
            PathState::File { content, .. } => assert_eq!(content, "alpha"),
            other => panic!("expected file prior, got {other:?}"),
        }
        match &e.after {
            PathState::File { content, .. } => assert_eq!(content, "five"),
            other => panic!("expected file after, got {other:?}"),
        }
    }

    #[test]
    fn revert_restores_overwritten_file() {
        let mut fs = seed();
        let mut journal = Journal::new();
        journal.touch(&fs, "/home/documents/a.txt");
        fs.write_file("/home/documents/a.txt", "CHANGED").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        journal.revert(&mut fs).unwrap();
        assert_eq!(fs.read_file("/home/documents/a.txt").unwrap(), "alpha");
    }

    #[test]
    fn revert_removes_created_file() {
        let mut fs = seed();
        let mut journal = Journal::new();
        journal.touch(&fs, "/home/documents/new.txt");
        fs.write_file("/home/documents/new.txt", "fresh").unwrap();
        journal.record_after(&fs, "/home/documents/new.txt");

        assert!(fs.exists("/home/documents/new.txt"));
        journal.revert(&mut fs).unwrap();
        assert!(!fs.exists("/home/documents/new.txt"));
    }

    #[test]
    fn revert_restores_deleted_file() {
        let mut fs = seed();
        let mut journal = Journal::new();
        journal.touch_delete(&fs, "/home/documents/b.txt", false);
        fs.delete("/home/documents/b.txt", false).unwrap();
        journal.record_after_delete(&fs, "/home/documents/b.txt");

        assert!(!fs.exists("/home/documents/b.txt"));
        journal.revert(&mut fs).unwrap();
        assert_eq!(fs.read_file("/home/documents/b.txt").unwrap(), "bravo");
    }

    #[test]
    fn revert_restores_renamed_directory_subtree() {
        let mut fs = seed();
        let mut journal = Journal::new();
        journal.touch_rename(
            &fs,
            "/home/documents/project",
            "/home/documents/renamed",
        );
        fs.rename("/home/documents/project", "/home/documents/renamed")
            .unwrap();
        journal.record_after_rename(
            &fs,
            "/home/documents/project",
            "/home/documents/renamed",
        );

        assert!(!fs.exists("/home/documents/project"));
        assert!(fs.exists("/home/documents/renamed/c.txt"));

        journal.revert(&mut fs).unwrap();

        assert!(fs.is_directory("/home/documents/project"));
        assert_eq!(
            fs.read_file("/home/documents/project/c.txt").unwrap(),
            "charlie"
        );
        assert!(!fs.exists("/home/documents/renamed"));
    }

    #[test]
    fn journal_size_is_o_files_touched() {
        let mut fs = seed();
        let mut journal = Journal::new();

        // Touch 3 files, write each twice — still 3 entries.
        for path in [
            "/home/documents/a.txt",
            "/home/documents/b.txt",
            "/home/documents/project/c.txt",
        ] {
            journal.touch(&fs, path);
            fs.write_file(path, "x").unwrap();
            journal.record_after(&fs, path);
            fs.write_file(path, "y").unwrap();
            journal.record_after(&fs, path);
        }

        assert_eq!(journal.len(), 3);
        // Default FS has many more files than 3.
        assert!(fs.list_directory("/home/documents").unwrap().len() >= 3);
    }

    #[test]
    fn per_file_revert_leaves_other_entries_applicable() {
        let mut fs = seed();
        let mut journal = Journal::new();

        journal.touch(&fs, "/home/documents/a.txt");
        fs.write_file("/home/documents/a.txt", "A1").unwrap();
        journal.record_after(&fs, "/home/documents/a.txt");

        journal.touch(&fs, "/home/documents/b.txt");
        fs.write_file("/home/documents/b.txt", "B1").unwrap();
        journal.record_after(&fs, "/home/documents/b.txt");

        journal.revert_path(&mut fs, "/home/documents/a.txt").unwrap();

        assert_eq!(fs.read_file("/home/documents/a.txt").unwrap(), "alpha");
        assert_eq!(fs.read_file("/home/documents/b.txt").unwrap(), "B1");

        // Remaining entry still reverts b.
        journal.revert(&mut fs).unwrap();
        assert_eq!(fs.read_file("/home/documents/b.txt").unwrap(), "bravo");
    }

    #[test]
    fn empty_run_produces_empty_journal() {
        let journal = Journal::new();
        assert!(journal.is_empty());
        assert!(journal.changed_paths().is_empty());
    }

    #[test]
    fn protected_path_detection() {
        assert!(is_protected_path("/system/config"));
        assert!(is_protected_path("/system/config/theme.json"));
        assert!(!is_protected_path("/system"));
        assert!(!is_protected_path("/home/documents"));
    }
}
