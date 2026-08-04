//! Plugin host: load, instantiate and drive guest WASM modules.
//!
//! Two channels, kept separate (see PLUGIN_PLAN.md §2):
//! - **Render channel** — the host pulls a frame: `render(event) -> Vec<UiOp>`.
//! - **Command channel** — the guest pushes side-effecting requests through
//!   host imports it calls during `update`.
//!
//! Loading is asynchronous only where it has to be (fetching bytes over HTTP).
//! Instantiation is synchronous (`WebAssembly.Module` / `WebAssembly.Instance`
//! constructors are sync in js_sys), so opening a window for an installed
//! plugin, and every `update`/`render` call, happen on the Yew event loop with
//! no futures involved.

pub mod abi;
pub mod imports;
pub mod memory;
pub mod render;

use std::cell::RefCell;
use std::rc::Rc;

use js_sys::{Function, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use yew::Callback;

use base64::Engine as _;

use crate::filesystem::{FileSystem, FileType};
use crate::plugin::abi::{
    decode_ui_ops, encode_event, Capability, Event, Grant, PermissionsStore, PluginManifest, UiOp,
    ABI_VERSION,
};
use crate::plugin::imports::{build_imports, fill_shared, InstanceShared};
use crate::plugin::memory::{call_i32, call_void, GuestMemory};

pub const PERMISSIONS_PATH: &str = "/system/config/permissions.json";
pub const APPLICATIONS_DIR: &str = "/applications";

/// Plugins shipped with the OS, fetched from the static server on first run
/// and then persisted into the VFS like any installed plugin.
const BUNDLED_PLUGINS: &[&str] = &["hello"];

/// WASM page size in bytes. Guest memory is always a multiple of this.
pub(crate) const WASM_PAGE_SIZE: u32 = 65536;

/// Return `Ok(())` if `byte_length` fits in `max_pages`, else an error naming
/// both the observed and capped page counts (for the crash-card message).
pub(crate) fn check_pages(byte_length: u32, max_pages: u32) -> Result<(), String> {
    let pages = byte_length / WASM_PAGE_SIZE;
    if pages > max_pages {
        Err(format!(
            "memory limit exceeded ({pages} > {max_pages} pages)"
        ))
    } else {
        Ok(())
    }
}

// ── Per-window instance ──────────────────────────────────────────────────────

/// One WASM instance, owned by exactly one window. Two windows of the same
/// plugin get two independent handles (and therefore independent state).
pub struct PluginHandle {
    id: String,
    shared: Rc<InstanceShared>,
    reset_fn: Function,
    update_fn: Function,
    render_fn: Function,
    max_pages: u32,
    crashed: bool,
    crash_message: Option<String>,
    ops: Vec<UiOp>,
}

impl std::fmt::Debug for PluginHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // Deliberately omits the WASM instance and guest memory; only the
        // identity and health of the handle are useful in a debug print.
        f.debug_struct("PluginHandle")
            .field("id", &self.id)
            .field("crashed", &self.crashed)
            .field("crash_message", &self.crash_message)
            .field("ops", &self.ops.len())
            .finish()
    }
}

impl PluginHandle {
    fn new(
        id: String,
        shared: Rc<InstanceShared>,
        reset_fn: Function,
        update_fn: Function,
        render_fn: Function,
        max_pages: u32,
    ) -> Self {
        Self {
            id,
            shared,
            reset_fn,
            update_fn,
            render_fn,
            max_pages,
            crashed: false,
            crash_message: None,
            ops: Vec::new(),
        }
    }
    pub fn id(&self) -> &str {
        &self.id
    }

    /// The last frame the plugin produced, for synchronous rendering in `view()`.
    pub fn ops(&self) -> &[UiOp] {
        &self.ops
    }

    pub fn is_crashed(&self) -> bool {
        self.crashed
    }

    pub fn crash_message(&self) -> Option<&str> {
        self.crash_message.as_deref()
    }

    /// Mark the plugin dead with an explicit message (used when a re-instantiation
    /// attempt itself fails, so the crash screen can explain why).
    pub fn set_crash(&mut self, message: String) {
        self.crashed = true;
        self.crash_message = Some(message);
    }

    /// Deliver one event. On any failure — trap, bounds violation, decode
    /// error — the plugin is marked dead and the crash is surfaced to the UI.
    /// A crashed handle refuses further calls until re-instantiated.
    pub fn send(&mut self, event: &Event) -> Result<(), String> {
        if self.crashed {
            return Err(self
                .crash_message
                .clone()
                .unwrap_or_else(|| "plugin crashed".to_string()));
        }
        // Cap guest linear memory before every call so a runaway alloc loop
        // trips the crash card instead of OOMing the tab (HANDOFF P0-2).
        if let Err(e) = self.check_memory_cap() {
            self.set_crash(e.clone());
            return Err(e);
        }
        match self.send_inner(event) {
            Ok(()) => {
                // Growth during the call is visible here; trip before the next frame.
                if let Err(e) = self.check_memory_cap() {
                    self.set_crash(e.clone());
                    return Err(e);
                }
                Ok(())
            }
            Err(e) => {
                self.crashed = true;
                self.crash_message = Some(e.clone());
                Err(e)
            }
        }
    }

    /// Fail if the guest's current memory exceeds `max_pages`.
    fn check_memory_cap(&self) -> Result<(), String> {
        let (guest, _) = self.shared.guest()?;
        check_pages(guest.byte_length(), self.max_pages)
    }

    fn send_inner(&mut self, event: &Event) -> Result<(), String> {
        let bytes = encode_event(event)?;

        // Reset the bump arena at the start of every call (plan §3).
        call_void(&self.reset_fn)?;

        let (guest, alloc) = self.shared.guest()?;
        let ptr = call_i32(&alloc, &[&JsValue::from_f64(bytes.len() as f64)])?;
        if ptr <= 0 {
            return Err("guest alloc failed".into());
        }
        guest.write(ptr as u32, &bytes)?;

        let dirty = call_i32(
            &self.update_fn,
            &[
                &JsValue::from_f64(ptr as f64),
                &JsValue::from_f64(bytes.len() as f64),
            ],
        )?;
        if dirty != 0 {
            self.pull_frame()?;
        }
        Ok(())
    }

    /// Call `render()` and decode the frame the guest left in its arena.
    /// The host does the read, so the guest's `(ptr, len)` is bounds-checked.
    fn pull_frame(&mut self) -> Result<(), String> {
        let (guest, _) = self.shared.guest()?;
        let hdr_ptr = call_i32(&self.render_fn, &[])?;
        if hdr_ptr <= 0 {
            return Err("guest render returned a null header".into());
        }
        let (out_ptr, out_len) = guest.read_header(hdr_ptr as u32)?;
        let raw = guest.read(out_ptr, out_len)?;
        let ops = decode_ui_ops(&raw)?;
        self.ops = ops;
        Ok(())
    }
}

// ── Registry ─────────────────────────────────────────────────────────────────

pub struct PluginApp {
    pub manifest: PluginManifest,
    pub bytes: Vec<u8>,
    pub grant: Grant,
}

/// Lightweight view of an installed plugin, for the app registry.
pub struct PluginAppInfo {
    pub id: String,
    pub name: String,
    pub icon: String,
    pub category: String,
    pub width: i32,
    pub height: i32,
    pub min_width: i32,
    pub min_height: i32,
    pub on_desktop: bool,
    pub in_quick_launch: bool,
}

struct PluginRegistry {
    fs: Option<Rc<RefCell<FileSystem>>>,
    on_notify: Option<Callback<(String, String)>>,
    apps: Vec<PluginApp>,
}

impl PluginRegistry {
    fn empty() -> Self {
        Self {
            fs: None,
            on_notify: None,
            apps: Vec::new(),
        }
    }

    /// Synchronously load plugins persisted in the VFS (`/applications`).
    /// Runs at desktop startup, before any window is created, so restored
    /// plugin windows can re-instantiate without waiting on the network.
    fn load_installed(&mut self) {
        let Some(fs) = self.fs.clone() else {
            return;
        };
        let Ok(entries) = fs.borrow().list_directory(APPLICATIONS_DIR) else {
            return;
        };
        for entry in entries {
            if entry.file_type != FileType::File || !entry.name.ends_with(".json") {
                continue;
            }
            let id = entry.name.trim_end_matches(".json").to_string();
            let manifest_path = format!("{APPLICATIONS_DIR}/{id}.json");
            let Ok(raw) = fs.borrow().read_file(&manifest_path) else {
                continue;
            };
            let Ok(manifest) = serde_json::from_str::<PluginManifest>(&raw) else {
                log::warn!("plugin {id}: unparseable manifest, skipping");
                continue;
            };
            if manifest.abi_version != ABI_VERSION {
                log::warn!(
                    "plugin {id}: ABI version {} unsupported (host is {ABI_VERSION}), skipping",
                    manifest.abi_version
                );
                continue;
            }
            let wasm_path = format!("{APPLICATIONS_DIR}/{id}.wasm.b64");
            let Ok(b64) = fs.borrow().read_file(&wasm_path) else {
                log::warn!("plugin {id}: missing wasm payload, skipping");
                continue;
            };
            let Ok(bytes) = base64::engine::general_purpose::STANDARD.decode(b64.trim()) else {
                log::warn!("plugin {id}: corrupt wasm payload, skipping");
                continue;
            };
            let grant = grant_for(&fs, &id, &manifest.requests);
            self.apps.push(PluginApp {
                manifest,
                bytes,
                grant,
            });
        }
    }
}

thread_local! {
    static REGISTRY: RefCell<PluginRegistry> = RefCell::new(PluginRegistry::empty());
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Initialise the registry: load installed plugins synchronously, then fetch
/// bundled plugins in the background. Idempotent per page load.
pub fn init(
    fs: Rc<RefCell<FileSystem>>,
    on_notify: Callback<(String, String)>,
    on_changed: Callback<()>,
) {
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if reg.fs.is_none() {
            reg.fs = Some(fs.clone());
            reg.on_notify = Some(on_notify.clone());
            reg.load_installed();
        }
    });

    for id in BUNDLED_PLUGINS {
        let fs = Rc::clone(&fs);
        let notify = on_notify.clone();
        let changed = on_changed.clone();
        let id = id.to_string();
        spawn_local(async move {
            if is_installed(&id) {
                return;
            }
            let manifest_url = format!("/plugins/{id}.json");
            let wasm_url = format!("/plugins/{id}.wasm");
            match install_from_url(&fs, &manifest_url, &wasm_url, notify).await {
                Ok(()) => changed.emit(()),
                Err(e) => log::warn!("bundled plugin '{id}' not loaded: {e}"),
            }
        });
    }
}

pub fn apps() -> Vec<PluginAppInfo> {
    REGISTRY.with(|r| {
        r.borrow()
            .apps
            .iter()
            .map(|a| PluginAppInfo {
                id: a.manifest.id.clone(),
                name: a.manifest.name.clone(),
                icon: a.manifest.icon.clone(),
                category: a.manifest.category.clone(),
                width: a.manifest.width,
                height: a.manifest.height,
                min_width: a.manifest.min_width,
                min_height: a.manifest.min_height,
                on_desktop: a.manifest.on_desktop,
                in_quick_launch: a.manifest.in_quick_launch,
            })
            .collect()
    })
}

pub fn is_installed(id: &str) -> bool {
    REGISTRY.with(|r| r.borrow().apps.iter().any(|a| a.manifest.id == id))
}

pub fn manifest(id: &str) -> Option<PluginManifest> {
    REGISTRY.with(|r| {
        r.borrow()
            .apps
            .iter()
            .find(|a| a.manifest.id == id)
            .map(|a| a.manifest.clone())
    })
}

/// Create a fresh per-window instance of an installed plugin. Synchronous.
pub fn instantiate(
    id: &str,
    fs: &Rc<RefCell<FileSystem>>,
    on_notify: Callback<(String, String)>,
) -> Result<PluginHandle, String> {
    let app = REGISTRY.with(|r| {
        r.borrow()
            .apps
            .iter()
            .find(|a| a.manifest.id == id)
            .map(|a| PluginApp {
                manifest: a.manifest.clone(),
                bytes: a.bytes.clone(),
                grant: a.grant.clone(),
            })
    })
    .ok_or_else(|| format!("plugin '{id}' is not installed"))?;

    instantiate_bytes(&app.manifest, &app.grant, &app.bytes, fs, on_notify)
}

/// Fetch a manifest + wasm pair, verify it, persist it to the VFS and register
/// it. This is the M8 install path (`pkg install` and bundled-plugin seeding).
pub async fn install_from_url(
    fs: &Rc<RefCell<FileSystem>>,
    manifest_url: &str,
    wasm_url: &str,
    on_notify: Callback<(String, String)>,
) -> Result<(), String> {
    let manifest_json = fetch_text(manifest_url).await?;
    let manifest: PluginManifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("bad manifest: {e}"))?;
    if manifest.abi_version != ABI_VERSION {
        return Err(format!(
            "unsupported ABI version {} (host supports {ABI_VERSION})",
            manifest.abi_version
        ));
    }

    let bytes = fetch_bytes(wasm_url).await?;

    // Content-hash pinning when the manifest names the exact authorised bytes.
    if let Some(expected) = &manifest.wasm_hash {
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("wasm hash mismatch: expected {expected}, got {actual}"));
        }
    }

    // Verify the module actually compiles and links before persisting it, so a
    // bad plugin fails the install instead of failing at launch.
    let probe = instantiate_bytes(&manifest, &Grant(manifest.requests.clone()), &bytes, fs, on_notify.clone())
        .map_err(|e| format!("load check failed: {e}"))?;
    drop(probe);

    register(fs, manifest, bytes, on_notify)?;
    Ok(())
}

pub fn uninstall(id: &str, fs: &Rc<RefCell<FileSystem>>) -> Result<(), String> {
    REGISTRY.with(|r| -> Result<(), String> {
        let mut reg = r.borrow_mut();
        let pos = reg
            .apps
            .iter()
            .position(|a| a.manifest.id == id)
            .ok_or_else(|| format!("plugin '{id}' is not installed"))?;
        reg.apps.remove(pos);
        Ok(())
    })?;

    {
        let mut fs = fs.borrow_mut();
        let _ = fs.delete(&format!("{APPLICATIONS_DIR}/{id}.json"), false);
        let _ = fs.delete(&format!("{APPLICATIONS_DIR}/{id}.wasm.b64"), false);
    }

    // Drop the stored grant too, so a reinstall re-prompts (all-or-nothing).
    let mut store = load_permissions(fs);
    store.grants.remove(id);
    save_permissions(fs, &store);

    Ok(())
}

// ── Internals ────────────────────────────────────────────────────────────────

fn instantiate_bytes(
    manifest: &PluginManifest,
    grant: &Grant,
    bytes: &[u8],
    fs: &Rc<RefCell<FileSystem>>,
    on_notify: Callback<(String, String)>,
) -> Result<PluginHandle, String> {
    let shared = Rc::new(InstanceShared {
        memory: RefCell::new(None),
        alloc: RefCell::new(None),
        plugin_id: manifest.id.clone(),
        grant: grant.clone(),
        fs: Rc::clone(fs),
        on_notify,
    });

    // The imports object contains exactly the granted capabilities. A plugin
    // denied a capability it imports fails to *link* here, immediately.
    let imports = build_imports(Rc::clone(&shared));

    let module = WebAssembly::Module::new(&Uint8Array::from(bytes).into())
        .map_err(|e| format!("module compile failed: {e:?}"))?;
    let instance = WebAssembly::Instance::new(&module, &imports)
        .map_err(|e| format!("instantiate failed (missing capability import?): {e:?}"))?;
    fill_shared(&shared, &instance)?;

    let exports = instance.exports();

    let abi_fn: Function = Reflect::get(&exports, &"abi_version".into())
        .map_err(|_| "missing abi_version export".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "abi_version export wrong type".to_string())?;
    let abi = call_i32(&abi_fn, &[])?;
    if abi != ABI_VERSION as i32 {
        return Err(format!("unsupported ABI version {abi} (host supports {ABI_VERSION})"));
    }

    let reset_fn: Function = Reflect::get(&exports, &"reset".into())
        .map_err(|_| "missing reset export".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "reset export wrong type".to_string())?;
    let update_fn: Function = Reflect::get(&exports, &"update".into())
        .map_err(|_| "missing update export".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "update export wrong type".to_string())?;
    let render_fn: Function = Reflect::get(&exports, &"render".into())
        .map_err(|_| "missing render export".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "render export wrong type".to_string())?;

    let mut handle = PluginHandle::new(
        manifest.id.clone(),
        Rc::clone(&shared),
        reset_fn,
        update_fn,
        render_fn,
        manifest.max_pages,
    );

    // Pull the initial frame so the window has something to render immediately.
    handle.send(&Event::Init)?;
    Ok(handle)
}

fn register(
    fs: &Rc<RefCell<FileSystem>>,
    manifest: PluginManifest,
    bytes: Vec<u8>,
    on_notify: Callback<(String, String)>,
) -> Result<(), String> {
    let id = manifest.id.clone();

    REGISTRY.with(|r| {
        let reg = r.borrow();
        if reg.apps.iter().any(|a| a.manifest.id == id) {
            return Err(format!("plugin '{id}' is already installed"));
        }
        Ok(())
    })?;

    // Persist to the VFS: manifest as JSON, wasm as base64 (localStorage is a
    // string store — see plan §11).
    let manifest_path = format!("{APPLICATIONS_DIR}/{id}.json");
    let wasm_path = format!("{APPLICATIONS_DIR}/{id}.wasm.b64");
    {
        let mut fs = fs.borrow_mut();
        if !fs.is_directory(APPLICATIONS_DIR) {
            fs.create_directory(APPLICATIONS_DIR, true)?;
        }
        let raw = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        fs.write_file(&manifest_path, &raw)?;
        let b64 = base64::engine::general_purpose::STANDARD.encode(&bytes);
        fs.write_file(&wasm_path, &b64)?;
    }

    // Record the grant (all-or-nothing for v1 — see plan §5).
    let grant = grant_for(fs, &id, &manifest.requests);

    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if reg.apps.iter().any(|a| a.manifest.id == id) {
            return Err(format!("plugin '{id}' is already installed"));
        }
        let _ = on_notify;
        reg.apps.push(PluginApp {
            manifest,
            bytes,
            grant,
        });
        Ok(())
    })
}

fn grant_for(fs: &Rc<RefCell<FileSystem>>, id: &str, requests: &[Capability]) -> Grant {
    let mut store = load_permissions(fs);
    if let Some(grant) = store.grants.get(id) {
        return grant.clone();
    }
    let grant = Grant(requests.to_vec());
    store.grants.insert(id.to_string(), grant.clone());
    save_permissions(fs, &store);
    grant
}

fn load_permissions(fs: &Rc<RefCell<FileSystem>>) -> PermissionsStore {
    fs.borrow()
        .read_file(PERMISSIONS_PATH)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_permissions(fs: &Rc<RefCell<FileSystem>>, store: &PermissionsStore) {
    if let Ok(raw) = serde_json::to_string_pretty(store) {
        let mut fs = fs.borrow_mut();
        if !fs.is_directory("/system/config") {
            let _ = fs.create_directory("/system/config", true);
        }
        let _ = fs.write_file(PERMISSIONS_PATH, &raw);
    }
}

async fn fetch_text(url: &str) -> Result<String, String> {
    let bytes = fetch_bytes(url).await?;
    String::from_utf8(bytes).map_err(|e| format!("{url} is not UTF-8: {e}"))
}

async fn fetch_bytes(url: &str) -> Result<Vec<u8>, String> {
    let window = web_sys::window().ok_or_else(|| "no window".to_string())?;
    let promise = window.fetch_with_str(url);
    let resp = JsFuture::from(promise)
        .await
        .map_err(|e| format!("fetch {url} failed: {e:?}"))?;

    let ok = Reflect::get(&resp, &"ok".into())
        .map(|v| v.as_bool().unwrap_or(false))
        .unwrap_or(false);
    if !ok {
        return Err(format!("fetch {url} failed: HTTP error"));
    }

    let array_buffer: Function = Reflect::get(&resp, &"arrayBuffer".into())
        .map_err(|_| "response has no arrayBuffer".to_string())?
        .dyn_into::<Function>()
        .map_err(|_| "arrayBuffer is not a function".to_string())?;
    let buf_promise: js_sys::Promise = array_buffer
        .call0(&resp)
        .map_err(|e| format!("arrayBuffer call failed: {e:?}"))?
        .dyn_into::<js_sys::Promise>()
        .map_err(|_| "arrayBuffer did not return a promise".to_string())?;

    let buf = JsFuture::from(buf_promise)
        .await
        .map_err(|e| format!("arrayBuffer failed: {e:?}"))?;
    Ok(Uint8Array::new(&buf).to_vec())
}

fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let digest = Sha256::digest(bytes);
    digest.iter().map(|b| format!("{b:02x}")).collect()
}

#[cfg(test)]
mod tests {
    use super::{check_pages, WASM_PAGE_SIZE};

    #[test]
    fn memory_cap_allows_at_limit() {
        assert!(check_pages(256 * WASM_PAGE_SIZE, 256).is_ok());
        assert!(check_pages(0, 256).is_ok());
    }

    #[test]
    fn memory_cap_rejects_over_limit() {
        let err = check_pages(257 * WASM_PAGE_SIZE, 256).unwrap_err();
        assert_eq!(err, "memory limit exceeded (257 > 256 pages)");
    }
}
