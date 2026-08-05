//! Plugin host: load, instantiate and drive guest WASM modules.
//!
//! Two channels, kept separate (see PLUGIN_PLAN.md §2):
//! - **Render channel** — the host pulls a frame: `render(event) -> Vec<UiOp>`.
//! - **Command channel** — the guest pushes side-effecting requests through
//!   host imports it calls during `update`.
//!
//! Loading is asynchronous where persistence or network access requires it.
//! Instantiation is synchronous (`WebAssembly.Module` / `WebAssembly.Instance`
//! constructors are sync in js_sys), so opening a window for an installed
//! plugin, and every `update`/`render` call, happen on the Yew event loop.

pub mod abi;
pub mod imports;
pub mod memory;
pub mod render;
pub mod wasm_store;

use std::cell::RefCell;
use std::collections::{BTreeSet, HashSet};
use std::rc::Rc;

use js_sys::{Function, Reflect, Uint8Array, WebAssembly};
use wasm_bindgen::JsCast;
use wasm_bindgen::JsValue;
use wasm_bindgen_futures::{spawn_local, JsFuture};
use yew::Callback;

use base64::Engine as _;

use crate::filesystem::{FileSystem, FileType};
use crate::plugin::abi::{
    decode_ui_ops, encode_event, Event, Grant, PermissionsStore, PluginManifest, UiOp, ABI_VERSION,
};
use crate::plugin::imports::{build_imports, fill_shared, InstanceShared};
use crate::plugin::memory::{call_i32, call_void};

pub const PERMISSIONS_PATH: &str = "/system/config/permissions.json";
pub const APPLICATIONS_DIR: &str = "/applications";
const DISABLED_BUNDLED_PLUGINS_PATH: &str = "/system/config/disabled_bundled_plugins.json";

/// Plugins shipped with the OS, fetched from the static server when absent
/// unless the user has explicitly removed them.
const BUNDLED_PLUGINS: &[&str] = &["hello"];

/// WASM page size in bytes. Guest memory is always a multiple of this.
pub(crate) const WASM_PAGE_SIZE: u32 = 65536;
/// Host-wide ceiling; plugin manifests may request less, never more.
pub(crate) const MAX_PLUGIN_PAGES: u32 = 256;

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

fn validate_plugin_id(id: &str) -> Result<(), String> {
    let id = id.as_bytes();
    let valid_id = !id.is_empty()
        && id.len() <= 64
        && id.first().is_some_and(u8::is_ascii_alphanumeric)
        && id.last().is_some_and(u8::is_ascii_alphanumeric)
        && id
            .iter()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || *byte == b'-');
    if !valid_id {
        return Err(
            "plugin id must be a lowercase alphanumeric slug with optional hyphens".to_string(),
        );
    }
    Ok(())
}

fn validate_manifest_limits(manifest: &PluginManifest) -> Result<(), String> {
    validate_plugin_id(&manifest.id)?;
    if manifest.max_pages == 0 || manifest.max_pages > MAX_PLUGIN_PAGES {
        return Err(format!(
            "manifest max_pages {} is outside the host range 1..={MAX_PLUGIN_PAGES}",
            manifest.max_pages
        ));
    }
    Ok(())
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
    installing: HashSet<String>,
    hydrated: bool,
}

impl PluginRegistry {
    fn empty() -> Self {
        Self {
            fs: None,
            on_notify: None,
            apps: Vec::new(),
            installing: HashSet::new(),
            hydrated: false,
        }
    }
}

/// Load installed manifests from the VFS and their raw modules from IndexedDB.
/// The registry is populated only after all asynchronous reads complete.
async fn load_installed(fs: &Rc<RefCell<FileSystem>>) -> Vec<PluginApp> {
    let Ok(entries) = fs.borrow().list_directory(APPLICATIONS_DIR) else {
        return Vec::new();
    };

    let mut candidates = Vec::new();
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
        if let Err(e) = validate_manifest_limits(&manifest) {
            log::warn!("plugin {id}: {e}, skipping");
            continue;
        }

        // Consent is required: never fabricate a grant here. Bundled plugins
        // write their grant at first-boot install; everything else must have
        // been confirmed via `pkg install`.
        let Some(grant) = stored_grant(fs, &id) else {
            log::warn!("plugin {id}: no stored grant, skipping (reinstall to consent)");
            continue;
        };
        candidates.push((id, manifest, grant));
    }

    let mut apps = Vec::new();
    for (id, manifest, grant) in candidates {
        match load_wasm_bytes(fs, &id, manifest.max_pages).await {
            Ok(bytes) => apps.push(PluginApp {
                manifest,
                bytes,
                grant,
            }),
            Err(e) => log::warn!("plugin {id}: {e}, skipping"),
        }
    }
    apps
}

/// Read raw bytes from IndexedDB, migrating one legacy base64 VFS payload on
/// first use. The old VFS file is removed only after the IndexedDB write lands.
async fn load_wasm_bytes(
    fs: &Rc<RefCell<FileSystem>>,
    id: &str,
    max_pages: u32,
) -> Result<Vec<u8>, String> {
    if let Some(bytes) = wasm_store::get(id).await? {
        validate_memory_limit(&bytes, max_pages)?;
        return Ok(bytes);
    }

    let legacy_path = format!("{APPLICATIONS_DIR}/{id}.wasm.b64");
    let encoded = fs
        .borrow()
        .read_file(&legacy_path)
        .map_err(|_| "missing wasm payload".to_string())?;
    let bytes = decode_legacy_wasm(&encoded)?;
    validate_memory_limit(&bytes, max_pages)?;
    wasm_store::put(id, &bytes).await?;

    if let Err(e) = fs.borrow_mut().delete(&legacy_path, false) {
        log::warn!("plugin {id}: migrated bytes but could not remove legacy payload: {e}");
    }
    Ok(bytes)
}

fn decode_legacy_wasm(encoded: &str) -> Result<Vec<u8>, String> {
    base64::engine::general_purpose::STANDARD
        .decode(encoded.trim())
        .map_err(|_| "corrupt legacy wasm payload".to_string())
}

/// Require one defined 32-bit memory with an explicit maximum no greater than
/// the manifest cap. This closes the gap where a guest can grow without bound
/// inside one call before the host's post-call soft check runs.
fn validate_memory_limit(bytes: &[u8], max_pages: u32) -> Result<(), String> {
    const WASM_HEADER: &[u8; 8] = b"\0asm\x01\0\0\0";
    if bytes.get(..WASM_HEADER.len()) != Some(WASM_HEADER) {
        return Err("invalid WebAssembly header".to_string());
    }

    let mut cursor = WASM_HEADER.len();
    let mut memory_count = 0_u32;
    while cursor < bytes.len() {
        let section_id = *bytes
            .get(cursor)
            .ok_or_else(|| "truncated WebAssembly section".to_string())?;
        cursor += 1;
        let section_size = read_uleb(bytes, &mut cursor)?;
        let section_size = usize::try_from(section_size)
            .map_err(|_| "WebAssembly section is too large".to_string())?;
        let section_end = cursor
            .checked_add(section_size)
            .filter(|end| *end <= bytes.len())
            .ok_or_else(|| "truncated WebAssembly section".to_string())?;

        if section_id == 5 {
            let mut memory_cursor = cursor;
            let count = read_uleb(bytes, &mut memory_cursor)?;
            for _ in 0..count {
                memory_count = memory_count
                    .checked_add(1)
                    .ok_or_else(|| "too many WebAssembly memories".to_string())?;
                let flags = read_uleb(bytes, &mut memory_cursor)?;
                if flags & !0x03 != 0 {
                    return Err("64-bit or unsupported WebAssembly memory".to_string());
                }
                let initial = read_uleb(bytes, &mut memory_cursor)?;
                if flags & 0x01 == 0 {
                    return Err("plugin memory has no hard maximum".to_string());
                }
                let maximum = read_uleb(bytes, &mut memory_cursor)?;
                if initial > maximum {
                    return Err("plugin memory minimum exceeds its maximum".to_string());
                }
                if maximum > u64::from(max_pages) {
                    return Err(format!(
                        "plugin memory maximum {maximum} exceeds manifest cap {max_pages}"
                    ));
                }
            }
            if memory_cursor != section_end {
                return Err("malformed WebAssembly memory section".to_string());
            }
        }
        cursor = section_end;
    }

    match memory_count {
        1 => Ok(()),
        0 => Err("plugin must define one bounded linear memory".to_string()),
        count => Err(format!("plugin defines {count} linear memories; expected one")),
    }
}

fn read_uleb(bytes: &[u8], cursor: &mut usize) -> Result<u64, String> {
    let mut value = 0_u64;
    for shift in (0..70).step_by(7) {
        let byte = *bytes
            .get(*cursor)
            .ok_or_else(|| "truncated WebAssembly integer".to_string())?;
        *cursor += 1;
        if shift == 63 && byte > 1 {
            return Err("WebAssembly integer overflow".to_string());
        }
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Ok(value);
        }
    }
    Err("WebAssembly integer overflow".to_string())
}

thread_local! {
    static REGISTRY: RefCell<PluginRegistry> = RefCell::new(PluginRegistry::empty());
}

struct InstallReservation {
    id: String,
}

impl Drop for InstallReservation {
    fn drop(&mut self) {
        REGISTRY.with(|r| {
            r.borrow_mut().installing.remove(&self.id);
        });
    }
}

fn reserve_install(id: &str) -> Result<InstallReservation, String> {
    REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if reg.apps.iter().any(|app| app.manifest.id == id) {
            return Err(format!("plugin '{id}' is already installed"));
        }
        if !reg.installing.insert(id.to_string()) {
            return Err(format!("plugin '{id}' is already installing"));
        }
        Ok(InstallReservation { id: id.to_string() })
    })
}

// ── Public API ───────────────────────────────────────────────────────────────

/// Initialise the registry from IndexedDB, then fetch missing bundled plugins.
/// `on_ready` fires after persisted plugins are available for session restore.
/// Idempotent per page load.
pub fn init(
    fs: Rc<RefCell<FileSystem>>,
    on_notify: Callback<(String, String)>,
    on_changed: Callback<()>,
    on_ready: Callback<()>,
) {
    let should_init = REGISTRY.with(|r| {
        let mut reg = r.borrow_mut();
        if reg.fs.is_none() {
            reg.fs = Some(fs.clone());
            reg.on_notify = Some(on_notify.clone());
            true
        } else {
            false
        }
    });

    if !should_init {
        on_ready.emit(());
        return;
    }

    spawn_local(async move {
        let installed = load_installed(&fs).await;
        REGISTRY.with(|r| {
            let mut reg = r.borrow_mut();
            for app in installed {
                let id = &app.manifest.id;
                if !reg.installing.contains(id)
                    && !reg
                        .apps
                        .iter()
                        .any(|current| current.manifest.id == id.as_str())
                {
                    reg.apps.push(app);
                }
            }
            reg.hydrated = true;
        });

        let disabled_bundled = load_disabled_bundled_plugins(&fs);
        for id in BUNDLED_PLUGINS {
            let id = id.to_string();
            if is_installed(&id) || disabled_bundled.contains(&id) {
                continue;
            }
            let manifest_url = format!("/plugins/{id}.json");
            let wasm_url = format!("/plugins/{id}.wasm");
            // Bundled plugins ship with the OS. plugin::init runs before any
            // Terminal exists, so there is no user to prompt — auto-grant the
            // manifest's requested capabilities. This carve-out is deliberate;
            // interactive installs must go through consent in `pkg install`.
            match fetch_plugin_manifest(&manifest_url).await {
                Ok(manifest) => {
                    let grant = Grant(manifest.requests.clone());
                    match complete_install(
                        &fs,
                        manifest,
                        &wasm_url,
                        grant,
                        on_notify.clone(),
                    )
                    .await
                    {
                        Ok(()) => on_changed.emit(()),
                        Err(e) => log::warn!("bundled plugin '{id}' not loaded: {e}"),
                    }
                }
                Err(e) => log::warn!("bundled plugin '{id}' not loaded: {e}"),
            }
        }
        on_ready.emit(());
    });
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

/// Fetch and validate a plugin manifest (ABI check included). Does not install.
pub async fn fetch_plugin_manifest(manifest_url: &str) -> Result<PluginManifest, String> {
    let manifest_json = fetch_text(manifest_url).await?;
    let manifest: PluginManifest =
        serde_json::from_str(&manifest_json).map_err(|e| format!("bad manifest: {e}"))?;
    if manifest.abi_version != ABI_VERSION {
        return Err(format!(
            "unsupported ABI version {} (host supports {ABI_VERSION})",
            manifest.abi_version
        ));
    }
    validate_manifest_limits(&manifest)?;
    Ok(manifest)
}

/// Complete an install given an already-fetched manifest and an explicit Grant
/// the caller has consented to (or auto-granted for bundled plugins).
///
/// Fetches wasm, verifies `wasm_hash` when present, probes link/instantiate,
/// persists the grant, then registers the plugin.
pub async fn complete_install(
    fs: &Rc<RefCell<FileSystem>>,
    manifest: PluginManifest,
    wasm_url: &str,
    grant: Grant,
    on_notify: Callback<(String, String)>,
) -> Result<(), String> {
    let hydrated = REGISTRY.with(|r| r.borrow().hydrated);
    if !hydrated {
        return Err("plugin registry is still loading; retry shortly".to_string());
    }
    validate_manifest_limits(&manifest)?;
    // Reserve before the network fetch so remove/install operations cannot
    // cross and make a successfully removed plugin reappear.
    let _reservation = reserve_install(&manifest.id)?;
    let bytes = fetch_bytes(wasm_url).await?;

    // Content-hash pinning when the manifest names the exact authorised bytes.
    if let Some(expected) = &manifest.wasm_hash {
        let actual = sha256_hex(&bytes);
        if !actual.eq_ignore_ascii_case(expected) {
            return Err(format!("wasm hash mismatch: expected {expected}, got {actual}"));
        }
    }
    validate_memory_limit(&bytes, manifest.max_pages)?;

    // Verify the module actually compiles and links under this grant before
    // persisting anything, so a bad plugin fails the install instead of failing
    // at launch.
    let probe = instantiate_bytes(&manifest, &grant, &bytes, fs, on_notify.clone())
        .map_err(|e| format!("load check failed: {e}"))?;
    drop(probe);

    register_reserved(fs, manifest, bytes, grant).await?;
    Ok(())
}

pub async fn uninstall(id: &str, fs: &Rc<RefCell<FileSystem>>) -> Result<(), String> {
    validate_plugin_id(id)?;
    let (hydrated, install_in_progress) = REGISTRY.with(|r| {
        let reg = r.borrow();
        (reg.hydrated, reg.installing.contains(id))
    });
    if !hydrated {
        return Err("plugin registry is still loading; retry shortly".to_string());
    }
    if install_in_progress {
        return Err(format!("plugin '{id}' is still installing"));
    }

    // Record the user's intent before deleting a bundled payload. If a later
    // cleanup step is interrupted, an installed manifest still wins on reload;
    // once cleanup completes, startup will not silently reinstall the plugin.
    if BUNDLED_PLUGINS.contains(&id) {
        set_bundled_plugin_disabled(fs, id, true)?;
    }

    // Remove the durable payload first. IndexedDB delete is idempotent, so a
    // retry remains safe if a later localStorage cleanup is interrupted.
    wasm_store::delete(id).await?;

    {
        let mut filesystem = fs.borrow_mut();
        let paths = [
            format!("{APPLICATIONS_DIR}/{id}.json"),
            format!("{APPLICATIONS_DIR}/{id}.wasm.b64"),
            format!("/system/config/plugin_{id}.json"),
        ];
        for path in paths {
            if filesystem.exists(&path) {
                filesystem.delete(&path, false)?;
            }
        }
    }

    // Drop the stored grant too, so a reinstall re-prompts (all-or-nothing).
    let mut store = load_permissions(fs);
    if store.grants.remove(id).is_some() {
        save_permissions(fs, &store)?;
    }

    REGISTRY.with(|r| {
        r.borrow_mut().apps.retain(|app| app.manifest.id != id);
    });

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

async fn register_reserved(
    fs: &Rc<RefCell<FileSystem>>,
    manifest: PluginManifest,
    bytes: Vec<u8>,
    grant: Grant,
) -> Result<(), String> {
    let id = manifest.id.clone();
    validate_memory_limit(&bytes, manifest.max_pages)?;

    // Persist raw bytes first, then keep only the small, inspectable manifest
    // in the VFS. Roll back the IndexedDB record if the manifest write fails.
    wasm_store::put(&id, &bytes).await?;
    let manifest_path = format!("{APPLICATIONS_DIR}/{id}.json");
    let manifest_result = (|| -> Result<(), String> {
        let mut fs = fs.borrow_mut();
        if !fs.is_directory(APPLICATIONS_DIR) {
            fs.create_directory(APPLICATIONS_DIR, true)?;
        }
        let raw = serde_json::to_string_pretty(&manifest).map_err(|e| e.to_string())?;
        fs.write_file(&manifest_path, &raw)?;
        Ok(())
    })();
    if let Err(e) = manifest_result {
        let _ = wasm_store::delete(&id).await;
        return Err(e);
    }

    if let Err(e) = save_grant(fs, &id, &grant) {
        let _ = wasm_store::delete(&id).await;
        let mut filesystem = fs.borrow_mut();
        if filesystem.exists(&manifest_path) {
            let _ = filesystem.delete(&manifest_path, false);
        }
        return Err(e);
    }

    // A previously installed unbounded module is intentionally not migrated,
    // but a successful bounded reinstall should still release its base64 quota.
    let legacy_path = format!("{APPLICATIONS_DIR}/{id}.wasm.b64");
    let mut filesystem = fs.borrow_mut();
    if filesystem.exists(&legacy_path) {
        if let Err(e) = filesystem.delete(&legacy_path, false) {
            log::warn!("plugin {id}: installed but could not remove legacy payload: {e}");
        }
    }
    drop(filesystem);

    REGISTRY.with(|r| {
        r.borrow_mut().apps.push(PluginApp {
            manifest,
            bytes,
            grant,
        });
    });
    if BUNDLED_PLUGINS.contains(&id.as_str()) {
        // Installation is already durable. A stale disable marker is harmless
        // while the manifest exists, so do not turn a successful install into
        // a reported failure if this small preference write is interrupted.
        if let Err(e) = set_bundled_plugin_disabled(fs, &id, false) {
            log::warn!("plugin {id}: installed but could not clear disabled marker: {e}");
        }
    }
    Ok(())
}

/// Look up a previously consented grant. Never fabricates one.
pub(crate) fn stored_grant(fs: &Rc<RefCell<FileSystem>>, id: &str) -> Option<Grant> {
    load_permissions(fs).grants.get(id).cloned()
}

fn save_grant(
    fs: &Rc<RefCell<FileSystem>>,
    id: &str,
    grant: &Grant,
) -> Result<(), String> {
    let mut store = load_permissions(fs);
    store.grants.insert(id.to_string(), grant.clone());
    save_permissions(fs, &store)
}

fn load_permissions(fs: &Rc<RefCell<FileSystem>>) -> PermissionsStore {
    fs.borrow()
        .read_file(PERMISSIONS_PATH)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn save_permissions(
    fs: &Rc<RefCell<FileSystem>>,
    store: &PermissionsStore,
) -> Result<(), String> {
    let raw = serde_json::to_string_pretty(store).map_err(|e| e.to_string())?;
    let mut filesystem = fs.borrow_mut();
    if !filesystem.is_directory("/system/config") {
        filesystem.create_directory("/system/config", true)?;
    }
    filesystem.write_file(PERMISSIONS_PATH, &raw)
}

fn load_disabled_bundled_plugins(fs: &Rc<RefCell<FileSystem>>) -> BTreeSet<String> {
    fs.borrow()
        .read_file(DISABLED_BUNDLED_PLUGINS_PATH)
        .ok()
        .and_then(|raw| serde_json::from_str(&raw).ok())
        .unwrap_or_default()
}

fn set_bundled_plugin_disabled(
    fs: &Rc<RefCell<FileSystem>>,
    id: &str,
    disabled: bool,
) -> Result<(), String> {
    let mut ids = load_disabled_bundled_plugins(fs);
    update_bundled_plugin_disabled(&mut ids, id, disabled);

    let raw = serde_json::to_string_pretty(&ids).map_err(|e| e.to_string())?;
    let mut filesystem = fs.borrow_mut();
    if !filesystem.is_directory("/system/config") {
        filesystem.create_directory("/system/config", true)?;
    }
    filesystem.write_file(DISABLED_BUNDLED_PLUGINS_PATH, &raw)
}

fn update_bundled_plugin_disabled(ids: &mut BTreeSet<String>, id: &str, disabled: bool) {
    if disabled {
        ids.insert(id.to_string());
    } else {
        ids.remove(id);
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
    use super::{
        check_pages, decode_legacy_wasm, stored_grant, update_bundled_plugin_disabled,
        validate_manifest_limits, validate_memory_limit, validate_plugin_id, WASM_PAGE_SIZE,
    };
    use crate::filesystem::FileSystem;
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    use std::rc::Rc;

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

    #[test]
    fn stored_grant_returns_none_for_unknown_plugin() {
        // Empty default FS → load_permissions falls back to an empty store.
        // stored_grant must not invent a grant for an unknown id.
        let fs = Rc::new(RefCell::new(FileSystem::default()));
        assert!(stored_grant(&fs, "unknown-plugin").is_none());
    }

    #[test]
    fn legacy_wasm_payload_decodes_for_indexeddb_migration() {
        assert_eq!(
            decode_legacy_wasm("  AGFzbQEAAAA=\n").unwrap(),
            b"\0asm\x01\0\0\0"
        );
        assert_eq!(
            decode_legacy_wasm("not base64").unwrap_err(),
            "corrupt legacy wasm payload"
        );
    }

    #[test]
    fn plugin_memory_requires_a_hard_maximum_within_manifest_cap() {
        let bounded = b"\0asm\x01\0\0\0\x05\x05\x01\x01\x01\x80\x02";
        assert!(validate_memory_limit(bounded, 256).is_ok());

        let unbounded = b"\0asm\x01\0\0\0\x05\x03\x01\x00\x01";
        assert_eq!(
            validate_memory_limit(unbounded, 256).unwrap_err(),
            "plugin memory has no hard maximum"
        );

        let oversized = b"\0asm\x01\0\0\0\x05\x05\x01\x01\x01\x81\x02";
        assert_eq!(
            validate_memory_limit(oversized, 256).unwrap_err(),
            "plugin memory maximum 257 exceeds manifest cap 256"
        );
    }

    #[test]
    fn manifest_cannot_raise_the_host_memory_ceiling() {
        let manifest: crate::plugin::abi::PluginManifest = serde_json::from_str(
            r#"{"id":"large","name":"Large","icon":"L","max_pages":257}"#,
        )
        .unwrap();
        assert_eq!(
            validate_manifest_limits(&manifest).unwrap_err(),
            "manifest max_pages 257 is outside the host range 1..=256"
        );
    }

    #[test]
    fn plugin_ids_cannot_escape_storage_paths() {
        assert!(validate_plugin_id("doc-viewer").is_ok());
        assert_eq!(
            validate_plugin_id("../../system/config/session").unwrap_err(),
            "plugin id must be a lowercase alphanumeric slug with optional hyphens"
        );
    }

    #[test]
    fn bundled_plugin_disable_marker_round_trips() {
        let mut disabled = BTreeSet::new();

        update_bundled_plugin_disabled(&mut disabled, "hello", true);
        assert!(disabled.contains("hello"));

        update_bundled_plugin_disabled(&mut disabled, "hello", false);
        assert!(!disabled.contains("hello"));
    }
}
