use serde::{Serialize, Deserialize};
use web_sys::Storage;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const FS_STORAGE_KEY: &str = "kernelosv2_fs";
const FILE_CONTENT_PREFIX: &str = "kernelosv2_file:";

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
}

impl Default for FileSystem {
    fn default() -> Self {
        Self::new().unwrap_or_else(|_| FileSystem {
            files: HashMap::new(),
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
            r##"{"theme": "dark", "accent": "#4a9eff", "wallpaper": "gradient"}"##,
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

        // Store content
        if let Some(storage) = Self::get_storage() {
            let key = format!("{}{}", FILE_CONTENT_PREFIX, path);
            storage.set_item(&key, content)
                .map_err(|e| format!("Failed to write file content: {:?}", e))?;
        }

        Ok(())
    }

    fn get_storage() -> Option<Storage> {
        web_sys::window()
            .and_then(|w| w.local_storage().ok())
            .flatten()
    }

    fn current_timestamp() -> u64 {
        js_sys::Date::now() as u64
    }

    pub fn save(&self) -> Result<(), String> {
        let storage = Self::get_storage()
            .ok_or_else(|| "Local storage not available".to_string())?;
        
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

        let storage = Self::get_storage()
            .ok_or_else(|| "Local storage not available".to_string())?;
        
        let key = format!("{}{}", FILE_CONTENT_PREFIX, path);
        storage.get_item(&key)
            .map_err(|e| format!("Failed to read file: {:?}", e))?
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
                                let key = format!("{}{}", FILE_CONTENT_PREFIX, child_path);
                                let _ = storage.remove_item(&key);
                            }
                        }
                    }
                }

                for child_path in paths_to_delete {
                    self.files.remove(&child_path);
                }
            }
        } else {
            // Delete file content
            if let Some(storage) = Self::get_storage() {
                let key = format!("{}{}", FILE_CONTENT_PREFIX, path);
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

        let metadata = self.files.remove(&old_path).unwrap();
        let new_name = Path::new(&new_path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        let now = Self::current_timestamp();
        
        self.files.insert(new_path.clone(), FileMetadata {
            name: new_name,
            modified: now,
            ..metadata
        });

        // If it's a file, move the content
        if let Some(storage) = Self::get_storage() {
            let old_key = format!("{}{}", FILE_CONTENT_PREFIX, old_path);
            let new_key = format!("{}{}", FILE_CONTENT_PREFIX, new_path);
            
            if let Ok(Some(content)) = storage.get_item(&old_key) {
                let _ = storage.set_item(&new_key, &content);
                let _ = storage.remove_item(&old_key);
            }
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

        let now = Self::current_timestamp();
        let new_name = Path::new(&dest_path)
            .file_name()
            .ok_or_else(|| "Invalid path".to_string())?
            .to_string_lossy()
            .to_string();

        self.files.insert(dest_path.clone(), FileMetadata {
            name: new_name,
            created: now,
            modified: now,
            ..src_metadata
        });

        // Copy file content if it's a file
        if let Some(storage) = Self::get_storage() {
            let src_key = format!("{}{}", FILE_CONTENT_PREFIX, src_path);
            let dest_key = format!("{}{}", FILE_CONTENT_PREFIX, dest_path);
            
            if let Ok(Some(content)) = storage.get_item(&src_key) {
                let _ = storage.set_item(&dest_key, &content);
            }
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

    fn normalize_path(path: &str) -> String {
        let path = path.trim();
        
        if path.is_empty() || path == "." {
            return "/".to_string();
        }

        let mut normalized = PathBuf::from(path);
        
        // Handle relative paths and normalize
        let result = normalized.to_string_lossy().to_string();
        
        // Remove trailing slash unless it's root
        if result.len() > 1 && result.ends_with('/') {
            result[..result.len()-1].to_string()
        } else if result.is_empty() {
            "/".to_string()
        } else {
            result
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_normalize_path() {
        assert_eq!(FileSystem::normalize_path(""), "/");
        assert_eq!(FileSystem::normalize_path("."), "/");
        assert_eq!(FileSystem::normalize_path("/home"), "/home");
        assert_eq!(FileSystem::normalize_path("/home/"), "/home");
    }
}
