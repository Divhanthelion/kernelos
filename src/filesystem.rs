use serde::{Serialize, Deserialize};
use web_sys::Storage;
use std::collections::HashMap;
use std::path::Path;

const FS_STORAGE_KEY: &str = "kernelosv2_fs";
const FILE_CONTENT_PREFIX: &str = "kernelosv2_file:";

/// localStorage key for a VFS file body. Single source of truth — callers must
/// not concatenate the prefix themselves.
pub fn content_key(path: &str) -> String {
    format!("{FILE_CONTENT_PREFIX}{path}")
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FileType {
    File,
    Directory,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileMetadata {
    pub name: String,
    pub file_type: FileType,
    pub size: usize,
    pub created: u64,
    pub modified: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FileSystem {
    files: HashMap<String, FileMetadata>,
    /// In-memory file bodies. Always updated on write so the VFS remains a
    /// complete value when localStorage is unavailable (host tests). Mirrored
    /// to `content_key()` in localStorage when storage is present. Skipped in
    /// the metadata JSON blob — bodies live under their own keys.
    #[serde(skip)]
    contents: HashMap<String, String>,
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| FileSystem {
            files: HashMap::new(),
            contents: HashMap::new(),
        })
    }
}

impl FileSystem {
    pub fn new() -> Result<Self, String> {
        // Try to load existing file system from local storage
        if let Some(storage) = Self::get_storage() {
            if let Ok(Some(data)) = storage.get_item(FS_STORAGE_KEY) {
                if !data.is_empty() {
                    if let Ok(fs) = serde_json::from_str::<FileSystem>(&data) {
                        return Ok(fs);
                    }
                    log::warn!("Failed to parse saved filesystem, creating new one");
                }
            }
        }

        // Create new file system with initial structure
        let mut fs = FileSystem {
            files: HashMap::new(),
            contents: HashMap::new(),
        };
        
        fs.initialize_default_structure()?;
        fs.save()?;
        
        Ok(fs)
    }

    fn initialize_default_structure(&mut self) -> Result<(), String> {
        let now = Self::current_timestamp();
        
        // Create root directory
        self.files.insert("/".to_string(), FileMetadata {
            name: "/".to_string(),
            file_type: FileType::Directory,
            size: 0,
            created: now,
            modified: now,
        });

        // Create directory structure
        let directories = [
            "/home",
            "/home/documents",
            "/home/pictures",
            "/home/music",
            "/home/downloads",
            "/applications",
            "/system",
            "/system/config",
        ];

        for dir in &directories {
            self.create_directory_internal(dir, now)?;
        }

        // Create some welcome files
        self.write_file_internal(
            "/home/documents/welcome.txt",
            "Welcome to KernelOS v2!\n\nThis is a WebAssembly-based desktop environment.\n\nFeatures:\n- File Explorer\n- Terminal\n- Text Editor\n- Calculator\n- Paint\n- Settings\n- Games\n\nEnjoy exploring!",
            now,
        )?;

        self.write_file_internal(
            "/home/documents/readme.md",
            "# KernelOS v2\n\nA modern desktop environment running entirely in your browser.\n\n## Getting Started\n\n1. Use the taskbar at the bottom to launch applications\n2. Right-click on the desktop for a context menu\n3. Drag windows by their title bar\n4. Resize windows by dragging their edges\n\n## Keyboard Shortcuts\n\n- `Ctrl+S` - Save in text editor\n- `Arrow keys` - Navigate in terminal history\n- `Tab` - Auto-complete in terminal",
            now,
        )?;

        self.write_file_internal(
            "/system/config/theme.json",
            r##"{"theme": "dark", "accent": "#4a9eff", "wallpaper": "gradient1"}"##,
            now,
        )?;

        Ok(())
    }

    fn create_directory_internal(&mut self, path: &str, timestamp: u64) -> Result<(), String> {
        let path = Self::normalize_path(path);
        
        if self.files.contains_key(&path) {
            return Ok(()); // Already exists
        }

        let name = Path::new(&path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        self.files.insert(path, FileMetadata {
            name,
            file_type: FileType::Directory,
            size: 0,
            created: timestamp,
            modified: timestamp,
        });

        Ok(())
    }

    fn write_file_internal(&mut self, path: &str, content: &str, timestamp: u64) -> Result<(), String> {
        let path = Self::normalize_path(path);
        
        let name = Path::new(&path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        self.files.insert(path.clone(), FileMetadata {
            name,
            file_type: FileType::File,
            size: content.len(),
            created: timestamp,
            modified: timestamp,
        });

        // Store content in memory (always) and localStorage (when available).
        self.contents.insert(path.clone(), content.to_string());
        if let Some(storage) = Self::get_storage() {
            let key = content_key(&path);
            storage.set_item(&key, content)
                .map_err(|e| format!("Failed to write file content: {:?}", e))?;
        }

        Ok(())
    }

    // Both of these reach through wasm-bindgen, which panics when called off a
    // wasm target. Gating them keeps the tree logic exercisable under `cargo test`.

    #[cfg(target_arch = "wasm32")]
    fn get_storage() -> Option<Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn get_storage() -> Option<Storage> {
        None
    }

    #[cfg(target_arch = "wasm32")]
    fn current_timestamp() -> u64 {
        js_sys::Date::now() as u64
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn current_timestamp() -> u64 {
        0
    }

    /// Persist metadata to local storage. When storage is unavailable — private
    /// browsing, or a non-browser host such as the test runner — the filesystem
    /// degrades to in-memory rather than failing every operation.
    pub fn save(&self) -> Result<(), String> {
        let Some(storage) = Self::get_storage() else {
            return Ok(());
        };

        let serialized = serde_json::to_string(self)
            .map_err(|e| format!("Failed to serialize filesystem: {}", e))?;

        storage.set_item(FS_STORAGE_KEY, &serialized)
            .map_err(|e| format!("Failed to save filesystem: {:?}", e))?;

        Ok(())
    }

    pub fn list_directory(&self, path: &str) -> Result<Vec<FileMetadata>, String> {
        let path = Self::normalize_path(path);
        
        // Check if path exists and is a directory
        match self.files.get(&path) {
            Some(metadata) if matches!(metadata.file_type, FileType::Directory) => {},
            Some(_) => return Err(format!("'{}' is not a directory", path)),
            None => return Err(format!("Directory '{}' does not exist", path)),
        }

        let path_prefix = if path == "/" { "/".to_string() } else { format!("{}/", path) };
        
        let mut files: Vec<FileMetadata> = self.files
            .iter()
            .filter(|(file_path, _)| {
                if *file_path == &path {
                    return false;
                }
                if !file_path.starts_with(&path_prefix) {
                    return false;
                }
                // Only direct children
                let remaining = &file_path[path_prefix.len()..];
                !remaining.contains('/')
            })
            .map(|(_, metadata)| metadata.clone())
            .collect();

        // Sort: directories first, then alphabetically
        files.sort_by(|a, b| {
            match (&a.file_type, &b.file_type) {
                (FileType::Directory, FileType::File) => std::cmp::Ordering::Less,
                (FileType::File, FileType::Directory) => std::cmp::Ordering::Greater,
                _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
            }
        });

        Ok(files)
    }

    pub fn create_directory(&mut self, path: &str, create_parents: bool) -> Result<(), String> {
        let path = Self::normalize_path(path);
        
        if self.files.contains_key(&path) {
            return Err(format!("'{}' already exists", path));
        }

        // Check/create parent directories
        if let Some(parent) = Path::new(&path).parent() {
            let parent_path = parent.to_string_lossy().to_string();
            if !parent_path.is_empty() && parent_path != "/" {
                if !self.files.contains_key(&parent_path) {
                    if create_parents {
                        self.create_directory(&parent_path, true)?;
                    } else {
                        return Err(format!("Parent directory '{}' does not exist", parent_path));
                    }
                }
            }
        }

        let now = Self::current_timestamp();
        self.create_directory_internal(&path, now)?;
        self.save()?;
        
        Ok(())
    }

    pub fn write_file(&mut self, path: &str, contents: &str) -> Result<(), String> {
        let path = Self::normalize_path(path);
        
        // Check parent directory exists
        if let Some(parent) = Path::new(&path).parent() {
            let parent_path = parent.to_string_lossy().to_string();
            if !parent_path.is_empty() && parent_path != "/" && !self.files.contains_key(&parent_path) {
                return Err(format!("Parent directory '{}' does not exist", parent_path));
            }
        }

        let now = Self::current_timestamp();
        let created = self.files.get(&path).map(|m| m.created).unwrap_or(now);
        
        self.write_file_internal(&path, contents, created)?;
        
        // Update modified time
        if let Some(metadata) = self.files.get_mut(&path) {
            metadata.modified = now;
            metadata.size = contents.len();
        }

        self.save()?;
        Ok(())
    }

    pub fn read_file(&self, path: &str) -> Result<String, String> {
        let path = Self::normalize_path(path);
        
        match self.files.get(&path) {
            Some(metadata) if matches!(metadata.file_type, FileType::File) => {},
            Some(_) => return Err(format!("'{}' is a directory", path)),
            None => return Err(format!("File '{}' does not exist", path)),
        }

        if let Some(storage) = Self::get_storage() {
            let key = content_key(&path);
            if let Ok(Some(content)) = storage.get_item(&key) {
                return Ok(content);
            }
        }

        self.contents
            .get(&path)
            .cloned()
            .ok_or_else(|| format!("File content not found for '{}'", path))
    }

    pub fn delete(&mut self, path: &str, recursive: bool) -> Result<(), String> {
        let path = Self::normalize_path(path);
        
        if path == "/" {
            return Err("Cannot delete root directory".to_string());
        }

        let metadata = self.files.get(&path)
            .ok_or_else(|| format!("'{}' does not exist", path))?
            .clone();

        if matches!(metadata.file_type, FileType::Directory) {
            let children = self.list_directory(&path)?;
            
            if !children.is_empty() {
                if !recursive {
                    return Err(format!("Directory '{}' is not empty", path));
                }
                
                // Delete children recursively
                let path_prefix = format!("{}/", path);
                let paths_to_delete: Vec<String> = self.files
                    .keys()
                    .filter(|p| p.starts_with(&path_prefix))
                    .cloned()
                    .collect();

                if let Some(storage) = Self::get_storage() {
                    for child_path in &paths_to_delete {
                        if let Some(child_meta) = self.files.get(child_path) {
                            if matches!(child_meta.file_type, FileType::File) {
                                let key = content_key(child_path);
                                let _ = storage.remove_item(&key);
                            }
                        }
                    }
                }

                for child_path in paths_to_delete {
                    self.contents.remove(&child_path);
                    self.files.remove(&child_path);
                }
            }
        } else {
            // Delete file content
            self.contents.remove(&path);
            if let Some(storage) = Self::get_storage() {
                let key = content_key(&path);
                let _ = storage.remove_item(&key);
            }
        }

        self.files.remove(&path);
        self.save()?;
        
        Ok(())
    }

    pub fn rename(&mut self, old_path: &str, new_path: &str) -> Result<(), String> {
        let old_path = Self::normalize_path(old_path);
        let new_path = Self::normalize_path(new_path);

        if !self.files.contains_key(&old_path) {
            return Err(format!("'{}' does not exist", old_path));
        }

        if self.files.contains_key(&new_path) {
            return Err(format!("'{}' already exists", new_path));
        }

        if Self::is_inside(&old_path, &new_path) {
            return Err(format!("Cannot move '{}' into itself", old_path));
        }

        let metadata = self.files.remove(&old_path).unwrap();
        let new_name = Path::new(&new_path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        let now = Self::current_timestamp();
        let is_directory = matches!(metadata.file_type, FileType::Directory);

        self.files.insert(new_path.clone(), FileMetadata {
            name: new_name,
            modified: now,
            ..metadata
        });

        // A directory carries its whole subtree with it; without this the
        // children keep their old keys and become unreachable orphans.
        if is_directory {
            let children: Vec<_> = self.descendants_of(&old_path);
            for (child_path, relative) in children {
                let child_new_path = format!("{}/{}", new_path, relative);
                let child = self.files.remove(&child_path).unwrap();
                let is_file = matches!(child.file_type, FileType::File);

                self.files.insert(child_new_path.clone(), child);

                if is_file {
                    Self::move_content(self, &child_path, &child_new_path, false);
                }
            }
        } else {
            Self::move_content(self, &old_path, &new_path, false);
        }

        self.save()?;
        Ok(())
    }

    pub fn copy(&mut self, src_path: &str, dest_path: &str) -> Result<(), String> {
        let src_path = Self::normalize_path(src_path);
        let dest_path = Self::normalize_path(dest_path);

        let src_metadata = self.files.get(&src_path)
            .ok_or_else(|| format!("'{}' does not exist", src_path))?
            .clone();

        if self.files.contains_key(&dest_path) {
            return Err(format!("'{}' already exists", dest_path));
        }

        if Self::is_inside(&src_path, &dest_path) {
            return Err(format!("Cannot copy '{}' into itself", src_path));
        }

        let now = Self::current_timestamp();
        let new_name = Path::new(&dest_path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        let is_directory = matches!(src_metadata.file_type, FileType::Directory);

        self.files.insert(dest_path.clone(), FileMetadata {
            name: new_name,
            created: now,
            modified: now,
            ..src_metadata
        });

        // Copying a directory has to duplicate the whole subtree, otherwise the
        // destination is an empty shell.
        if is_directory {
            let children: Vec<_> = self.descendants_of(&src_path);
            for (child_path, relative) in children {
                let child_dest_path = format!("{}/{}", dest_path, relative);
                let child = self.files.get(&child_path).unwrap().clone();
                let is_file = matches!(child.file_type, FileType::File);

                self.files.insert(child_dest_path.clone(), FileMetadata {
                    created: now,
                    modified: now,
                    ..child
                });

                if is_file {
                    Self::move_content(self, &child_path, &child_dest_path, true);
                }
            }
        } else {
            Self::move_content(self, &src_path, &dest_path, true);
        }

        self.save()?;
        Ok(())
    }

    pub fn exists(&self, path: &str) -> bool {
        let path = Self::normalize_path(path);
        self.files.contains_key(&path)
    }

    pub fn is_directory(&self, path: &str) -> bool {
        let path = Self::normalize_path(path);
        self.files.get(&path)
            .map(|m| matches!(m.file_type, FileType::Directory))
            .unwrap_or(false)
    }

    pub fn get_metadata(&self, path: &str) -> Option<FileMetadata> {
        let path = Self::normalize_path(path);
        self.files.get(&path).cloned()
    }

    /// Resolve a path to its canonical absolute form, collapsing `.` and `..`
    /// segments and any redundant or trailing slashes. Paths are always rooted:
    /// `..` at the root is a no-op rather than an escape.
    pub fn normalize_path(path: &str) -> String {
        let mut stack: Vec<&str> = Vec::new();

        for segment in path.trim().split('/') {
            match segment {
                "" | "." => {}
                ".." => {
                    stack.pop();
                }
                name => stack.push(name),
            }
        }

        if stack.is_empty() {
            "/".to_string()
        } else {
            format!("/{}", stack.join("/"))
        }
    }

    /// Public for journal / agent tooling: every descendant of `path`.
    pub fn descendants_of(&self, path: &str) -> Vec<(String, String)> {
        let path = Self::normalize_path(path);
        let prefix = format!("{}/", path);
        self.files
            .keys()
            .filter_map(|p| {
                p.strip_prefix(&prefix)
                    .map(|rest| (p.clone(), rest.to_string()))
            })
            .collect()
    }

    /// True if `dest` sits inside `src` (equal, or a path under `src/` with a
    /// directory boundary). Used by move/copy cycle detection and by plugin
    /// VFS capability checks — do not replace with raw `starts_with`.
    pub fn is_inside(src: &str, dest: &str) -> bool {
        dest == src || dest.starts_with(&format!("{}/", src))
    }

    fn move_content(fs: &mut FileSystem, from: &str, to: &str, keep_source: bool) {
        if let Some(content) = fs.contents.get(from).cloned() {
            fs.contents.insert(to.to_string(), content);
            if !keep_source {
                fs.contents.remove(from);
            }
        }

        if let Some(storage) = Self::get_storage() {
            let from_key = content_key(from);
            let to_key = content_key(to);
            if let Ok(Some(content)) = storage.get_item(&from_key) {
                let _ = storage.set_item(&to_key, &content);
                if !keep_source {
                    let _ = storage.remove_item(&from_key);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A filesystem with no storage backing, for exercising tree logic.
    fn tree(paths: &[(&str, FileType)]) -> FileSystem {
        let mut files = HashMap::new();
        files.insert("/".to_string(), FileMetadata {
            name: "/".to_string(),
            file_type: FileType::Directory,
            size: 0,
            created: 0,
            modified: 0,
        });

        for (path, file_type) in paths {
            files.insert(path.to_string(), FileMetadata {
                name: path.rsplit('/').next().unwrap().to_string(),
                file_type: file_type.clone(),
                size: 0,
                created: 0,
                modified: 0,
            });
        }

        FileSystem {
            files,
            contents: HashMap::new(),
        }
    }

    fn sorted_paths(fs: &FileSystem) -> Vec<String> {
        let mut paths: Vec<String> = fs.files.keys().cloned().collect();
        paths.sort();
        paths
    }

    #[test]
    fn normalize_path_handles_edge_cases() {
        assert_eq!(FileSystem::normalize_path(""), "/");
        assert_eq!(FileSystem::normalize_path("."), "/");
        assert_eq!(FileSystem::normalize_path("/"), "/");
        assert_eq!(FileSystem::normalize_path("/home"), "/home");
        assert_eq!(FileSystem::normalize_path("/home/"), "/home");
        assert_eq!(FileSystem::normalize_path("/home//documents"), "/home/documents");
        assert_eq!(FileSystem::normalize_path("  /home/documents  "), "/home/documents");
    }

    #[test]
    fn normalize_path_resolves_dot_segments() {
        assert_eq!(FileSystem::normalize_path("/home/documents/.."), "/home");
        assert_eq!(FileSystem::normalize_path("/home/documents/../readme.md"), "/home/readme.md");
        assert_eq!(FileSystem::normalize_path("/home/./documents"), "/home/documents");
        assert_eq!(FileSystem::normalize_path("/home/a/b/../../c"), "/home/c");
    }

    #[test]
    fn normalize_path_cannot_escape_root() {
        assert_eq!(FileSystem::normalize_path("/.."), "/");
        assert_eq!(FileSystem::normalize_path("/../../.."), "/");
        assert_eq!(FileSystem::normalize_path("/home/../../etc"), "/etc");
    }

    #[test]
    fn is_inside_detects_containment() {
        assert!(FileSystem::is_inside("/home", "/home"));
        assert!(FileSystem::is_inside("/home", "/home/documents"));
        assert!(!FileSystem::is_inside("/home", "/homework"));
        assert!(!FileSystem::is_inside("/home/documents", "/home"));
    }

    #[test]
    fn renaming_a_directory_carries_its_children() {
        let mut fs = tree(&[
            ("/home", FileType::Directory),
            ("/home/documents", FileType::Directory),
            ("/home/documents/welcome.txt", FileType::File),
            ("/home/documents/nested", FileType::Directory),
            ("/home/documents/nested/deep.txt", FileType::File),
        ]);

        fs.rename("/home/documents", "/home/archive").unwrap();

        assert_eq!(sorted_paths(&fs), vec![
            "/",
            "/home",
            "/home/archive",
            "/home/archive/nested",
            "/home/archive/nested/deep.txt",
            "/home/archive/welcome.txt",
        ]);
        assert_eq!(fs.get_metadata("/home/archive").unwrap().name, "archive");
    }

    #[test]
    fn copying_a_directory_duplicates_its_subtree() {
        let mut fs = tree(&[
            ("/home", FileType::Directory),
            ("/home/documents", FileType::Directory),
            ("/home/documents/welcome.txt", FileType::File),
            ("/home/documents/nested", FileType::Directory),
            ("/home/documents/nested/deep.txt", FileType::File),
        ]);

        fs.copy("/home/documents", "/home/backup").unwrap();

        assert_eq!(sorted_paths(&fs), vec![
            "/",
            "/home",
            "/home/backup",
            "/home/backup/nested",
            "/home/backup/nested/deep.txt",
            "/home/backup/welcome.txt",
            "/home/documents",
            "/home/documents/nested",
            "/home/documents/nested/deep.txt",
            "/home/documents/welcome.txt",
        ]);
    }

    #[test]
    fn sibling_directories_with_shared_prefixes_are_untouched() {
        let mut fs = tree(&[
            ("/home", FileType::Directory),
            ("/home/doc", FileType::Directory),
            ("/home/doc/a.txt", FileType::File),
            ("/home/documents", FileType::Directory),
            ("/home/documents/b.txt", FileType::File),
        ]);

        fs.rename("/home/doc", "/home/moved").unwrap();

        assert!(fs.exists("/home/moved/a.txt"));
        assert!(fs.exists("/home/documents/b.txt"));
        assert!(!fs.exists("/home/documents".replace("documents", "moveduments").as_str()));
    }

    #[test]
    fn a_directory_cannot_be_moved_or_copied_into_itself() {
        let mut fs = tree(&[
            ("/home", FileType::Directory),
            ("/home/documents", FileType::Directory),
        ]);

        assert!(fs.rename("/home/documents", "/home/documents/inner").is_err());
        assert!(fs.copy("/home/documents", "/home/documents/inner").is_err());
        assert!(fs.exists("/home/documents"));
    }

    #[test]
    fn renaming_a_file_leaves_the_tree_intact() {
        let mut fs = tree(&[
            ("/home", FileType::Directory),
            ("/home/a.txt", FileType::File),
        ]);

        fs.rename("/home/a.txt", "/home/b.txt").unwrap();

        assert!(!fs.exists("/home/a.txt"));
        assert!(fs.exists("/home/b.txt"));
        assert_eq!(fs.get_metadata("/home/b.txt").unwrap().name, "b.txt");
    }
}
