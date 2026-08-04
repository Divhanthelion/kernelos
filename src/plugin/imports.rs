//! Capability-gated host import functions for guest WASM modules.

use js_sys::{Function, Object, Reflect, WebAssembly};
use std::cell::RefCell;
use std::rc::Rc;
use wasm_bindgen::prelude::*;
use yew::Callback;

use crate::filesystem::FileSystem;
use crate::plugin::abi::{Capability, Grant};
use crate::plugin::memory::{call_i32, GuestMemory};

/// Shared state filled immediately after instantiation, before guest code runs.
pub struct InstanceShared {
    pub memory: RefCell<Option<WebAssembly::Memory>>,
    pub alloc: RefCell<Option<Function>>,
    pub plugin_id: String,
    pub grant: Grant,
    pub fs: Rc<RefCell<FileSystem>>,
    pub on_notify: Callback<(String, String)>,
}

impl InstanceShared {
    pub fn guest(&self) -> Result<(GuestMemory, Function), String> {
        let memory = self
            .memory
            .borrow()
            .clone()
            .ok_or_else(|| "instance not ready".to_string())?;
        let alloc = self
            .alloc
            .borrow()
            .clone()
            .ok_or_else(|| "instance not ready".to_string())?;
        Ok((GuestMemory::new(memory), alloc))
    }

    fn write_result(&self, data: &[u8]) -> Result<i32, String> {
        let (guest, alloc) = self.guest()?;
        // Guest `alloc(0)` is defined as failure, but an empty file is a valid
        // result. Reserve one byte while keeping the returned payload length 0.
        let allocation_len = data.len().max(1);
        let ptr = call_i32(&alloc, &[&JsValue::from_f64(allocation_len as f64)])?;
        if ptr <= 0 {
            return Err("guest alloc failed".into());
        }
        if !data.is_empty() {
            guest
                .write(ptr as u32, data)
                .map_err(|e| e.to_string())?;
        }
        let hdr_ptr = call_i32(&alloc, &[&JsValue::from_f64(8.0)])?;
        if hdr_ptr <= 0 {
            return Err("guest alloc for header failed".into());
        }
        guest
            .write_header(hdr_ptr as u32, ptr as u32, data.len() as u32)
            .map_err(|e| e.to_string())?;
        Ok(hdr_ptr)
    }
}

pub fn build_imports(shared: Rc<InstanceShared>) -> Object {
    let env = Object::new();

    if shared.grant.has(&Capability::Notify) {
        let s = Rc::clone(&shared);
        let closure = Closure::wrap(Box::new(move |ptr: i32, len: i32| {
            if let Ok((guest, _)) = s.guest() {
                if let Ok(bytes) = guest.read(ptr as u32, len as u32) {
                    if let Ok(val) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                        let title = val
                            .get("title")
                            .and_then(|v| v.as_str())
                            .unwrap_or("Notification")
                            .to_string();
                        let body = val
                            .get("body")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        s.on_notify.emit((title, body));
                    }
                }
            }
        }) as Box<dyn Fn(i32, i32)>);
        let _ = Reflect::set(&env, &"host_notify".into(), closure.as_ref().unchecked_ref());
        closure.forget();
    }

    if shared.grant.has(&Capability::Persist) {
        let s = Rc::clone(&shared);
        let read_closure = Closure::wrap(Box::new(move || -> i32 {
            let path = format!("/system/config/plugin_{}.json", s.plugin_id);
            let data = s.fs.borrow().read_file(&path).unwrap_or_default();
            s.write_result(data.as_bytes()).unwrap_or(0)
        }) as Box<dyn Fn() -> i32>);
        let _ = Reflect::set(
            &env,
            &"host_persist_read".into(),
            read_closure.as_ref().unchecked_ref(),
        );
        read_closure.forget();

        let s = Rc::clone(&shared);
        let write_closure = Closure::wrap(Box::new(move |ptr: i32, len: i32| -> i32 {
            let Ok((guest, _)) = s.guest() else {
                return 0;
            };
            let Ok(bytes) = guest.read(ptr as u32, len as u32) else {
                return 0;
            };
            let Ok(text) = String::from_utf8(bytes) else {
                return 0;
            };
            let path = format!("/system/config/plugin_{}.json", s.plugin_id);
            match s.fs.borrow_mut().write_file(&path, &text) {
                Ok(_) => 1,
                Err(_) => 0,
            }
        }) as Box<dyn Fn(i32, i32) -> i32>);
        let _ = Reflect::set(
            &env,
            &"host_persist_write".into(),
            write_closure.as_ref().unchecked_ref(),
        );
        write_closure.forget();
    }

    if let Some(prefix) = shared.grant.vfs_read_prefix().map(str::to_string) {
        let s = Rc::clone(&shared);
        let vfs_read = Closure::wrap(Box::new(move |ptr: i32, len: i32| -> i32 {
            let Ok((guest, _)) = s.guest() else {
                return 0;
            };
            let Ok(bytes) = guest.read(ptr as u32, len as u32) else {
                return 0;
            };
            let Ok(path) = serde_json::from_slice::<String>(&bytes) else {
                return 0;
            };
            // Normalize *before* the grant check — raw `starts_with` misses `..`
            // traversal and has no path-boundary (see HANDOFF P0-1).
            let Some(path) = allow_vfs_path(&prefix, &path) else {
                return 0;
            };
            let Ok(content) = s.fs.borrow().read_file(&path) else {
                return 0;
            };
            s.write_result(content.as_bytes()).unwrap_or(0)
        }) as Box<dyn Fn(i32, i32) -> i32>);
        let _ = Reflect::set(
            &env,
            &"host_vfs_read".into(),
            vfs_read.as_ref().unchecked_ref(),
        );
        vfs_read.forget();
    }

    // Capability::VfsWrite / Grant::vfs_write_prefix exist in the ABI, but no
    // `host_vfs_write` import is wired here (and the PDK does not declare one).
    // Leave it unimplemented rather than ship a write path with a raw
    // `starts_with` gate — when VfsWrite lands it must use `allow_vfs_path`.

    let imports = Object::new();
    let _ = Reflect::set(&imports, &"env".into(), &env);
    imports
}

/// Normalize grant prefix and guest path, then enforce boundary-aware
/// containment. Returns the normalized path if allowed.
pub(crate) fn allow_vfs_path(prefix: &str, guest_path: &str) -> Option<String> {
    let prefix = FileSystem::normalize_path(prefix);
    let path = FileSystem::normalize_path(guest_path);
    if FileSystem::is_inside(&prefix, &path) {
        Some(path)
    } else {
        None
    }
}

pub fn fill_shared(shared: &InstanceShared, instance: &WebAssembly::Instance) -> Result<(), String> {
    let exports = instance.exports();
    let memory = Reflect::get(&exports, &"memory".into())
        .map_err(|_| "missing memory export")?
        .dyn_into::<WebAssembly::Memory>()
        .map_err(|_| "memory export wrong type")?;
    let alloc = Reflect::get(&exports, &"alloc".into())
        .map_err(|_| "missing alloc export")?
        .dyn_into::<Function>()
        .map_err(|_| "alloc export wrong type")?;
    *shared.memory.borrow_mut() = Some(memory);
    *shared.alloc.borrow_mut() = Some(alloc);
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::allow_vfs_path;

    #[test]
    fn vfs_read_rejects_dotdot_traversal() {
        // Proven P0-1 exploit: raw starts_with("/home/documents") passes this
        // string, but after normalize it escapes to /system/config/theme.json.
        assert_eq!(
            allow_vfs_path(
                "/home/documents",
                "/home/documents/../../system/config/theme.json"
            ),
            None
        );
    }

    #[test]
    fn vfs_read_rejects_prefix_without_boundary() {
        // "/home/doc" must not grant "/home/documents-private/...".
        assert_eq!(
            allow_vfs_path("/home/doc", "/home/documents-private/secrets"),
            None
        );
    }

    #[test]
    fn vfs_read_allows_paths_inside_grant() {
        assert_eq!(
            allow_vfs_path("/home/documents", "/home/documents/readme.txt"),
            Some("/home/documents/readme.txt".into())
        );
        // Sloppy trailing slash on the grant still works after normalize.
        assert_eq!(
            allow_vfs_path("/home/documents/", "/home/documents/notes/a.txt"),
            Some("/home/documents/notes/a.txt".into())
        );
        // Exact grant root is allowed.
        assert_eq!(
            allow_vfs_path("/home/documents", "/home/documents"),
            Some("/home/documents".into())
        );
    }
}
