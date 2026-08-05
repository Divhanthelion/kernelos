//! Guest-side helper library for KernelOS plugins.
//!
//! Plugins are plain `cdylib` WASM modules — no wasm-bindgen.
//!
//! ```ignore
//! kernelos_pdk::kernelos_plugin!(MyPlugin);
//! ```

use serde::{Deserialize, Serialize};

pub const ABI_VERSION: u32 = 1;

// ── ABI types (must stay in sync with host `src/plugin/abi.rs`) ─────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
pub enum UiOp {
    BeginVBox { gap: u8 },
    BeginHBox { gap: u8 },
    End,
    Label {
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        class: Option<String>,
    },
    Button { id: u32, text: String },
    Input {
        id: u32,
        value: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        placeholder: Option<String>,
    },
    Checkbox {
        id: u32,
        checked: bool,
        label: String,
    },
    List {
        items: Vec<String>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        selected: Option<usize>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Init,
    Click { id: u32 },
    InputChanged { id: u32, value: String },
    Tick { millis: u64 },
    #[serde(other)]
    Unknown,
}

// ── Plugin trait ────────────────────────────────────────────────────────────

pub trait Plugin: Default {
    fn init(&mut self) {}
    fn update(&mut self, event: Event) -> bool;
    fn render(&self) -> Vec<UiOp>;
}

// ── Bump arena (alloc / reset) ───────────────────────────────────────────────

struct Arena {
    allocs: Vec<(*mut u8, usize)>,
}

impl Arena {
    const fn new() -> Self {
        Self { allocs: Vec::new() }
    }

    fn alloc(&mut self, len: usize) -> *mut u8 {
        let mut buf = Vec::with_capacity(len);
        let ptr = buf.as_mut_ptr();
        let cap = buf.capacity();
        std::mem::forget(buf);
        self.allocs.push((ptr, cap));
        ptr
    }

    fn reset(&mut self) {
        for (ptr, cap) in self.allocs.drain(..) {
            unsafe {
                let _ = Vec::from_raw_parts(ptr, 0, cap);
            }
        }
    }
}

/// Single-threaded bump arena. Held in `UnsafeCell` rather than `static mut`
/// so we never form a reference to a mutable static (the `static_mut_refs`
/// lint). Soundness rests on the guest being single-threaded wasm: the host
/// does not re-enter plugin code while a host import holds `&mut Arena`, so
/// there is no concurrent access to race against.
struct ArenaHolder(std::cell::UnsafeCell<Option<Arena>>);

// SAFETY: plugins are single-threaded wasm; see ArenaHolder comment above.
unsafe impl Sync for ArenaHolder {}

static ARENA: ArenaHolder = ArenaHolder(std::cell::UnsafeCell::new(None));

fn arena() -> &'static mut Arena {
    // SAFETY: single-threaded guest; see ArenaHolder.
    unsafe {
        let slot = &mut *ARENA.0.get();
        if slot.is_none() {
            *slot = Some(Arena::new());
        }
        slot.as_mut().unwrap()
    }
}

pub fn arena_alloc(len: usize) -> *mut u8 {
    arena().alloc(len)
}

pub fn arena_reset() {
    arena().reset();
}

// ── Host imports (linked only when capability granted) ──────────────────────

extern "C" {
    fn host_notify(ptr: i32, len: i32);
    fn host_persist_read() -> i32;
    fn host_persist_write(ptr: i32, len: i32) -> i32;
    fn host_vfs_read(ptr: i32, len: i32) -> i32;
}

/// Post a notification (requires `Notify` capability).
pub fn notify(title: &str, body: &str) {
    let payload = serde_json::json!({ "title": title, "body": body });
    if let Ok(bytes) = serde_json::to_vec(&payload) {
        let ptr = arena_alloc(bytes.len());
        unsafe {
            std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
            host_notify(ptr as i32, bytes.len() as i32);
        }
    }
}

/// Read plugin persist blob (requires `Persist` capability).
pub fn persist_read() -> Option<String> {
    let hdr = unsafe { host_persist_read() };
    if hdr == 0 {
        return None;
    }
    read_header_payload(hdr as u32)
}

/// Write plugin persist blob (requires `Persist` capability).
pub fn persist_write(data: &str) -> bool {
    let ptr = arena_alloc(data.len());
    unsafe {
        std::ptr::copy_nonoverlapping(data.as_ptr(), ptr, data.len());
        host_persist_write(ptr as i32, data.len() as i32) != 0
    }
}

/// Read a VFS path (requires `VfsRead` capability).
pub fn vfs_read(path: &str) -> Option<String> {
    let path_json = serde_json::to_string(path).ok()?;
    let ptr = arena_alloc(path_json.len());
    unsafe {
        std::ptr::copy_nonoverlapping(path_json.as_ptr(), ptr, path_json.len());
        let hdr = host_vfs_read(ptr as i32, path_json.len() as i32);
        if hdr == 0 {
            return None;
        }
        read_header_payload(hdr as u32)
    }
}

fn read_header_payload(hdr: u32) -> Option<String> {
    unsafe {
        let base = hdr as *const u8;
        let out_ptr = u32::from_le_bytes([*base, *base.add(1), *base.add(2), *base.add(3)]) as usize;
        let out_len = u32::from_le_bytes([
            *base.add(4),
            *base.add(5),
            *base.add(6),
            *base.add(7),
        ]) as usize;
        let slice = std::slice::from_raw_parts(out_ptr as *const u8, out_len);
        String::from_utf8(slice.to_vec()).ok()
    }
}

// ── Export macro ─────────────────────────────────────────────────────────────

/// Generates the WASM exports required by the KernelOS host ABI.
#[macro_export]
macro_rules! kernelos_plugin {
    ($plugin_ty:ty) => {
        static mut INSTANCE: Option<$plugin_ty> = None;
        static mut RENDER_HDR: [i32; 2] = [0, 0];

        fn plugin_instance() -> &'static mut $plugin_ty {
            unsafe {
                if INSTANCE.is_none() {
                    let mut p = <$plugin_ty as ::core::default::Default>::default();
                    p.init();
                    INSTANCE = Some(p);
                }
                INSTANCE.as_mut().unwrap()
            }
        }

        #[no_mangle]
        pub extern "C" fn abi_version() -> i32 {
            ::kernelos_pdk::ABI_VERSION as i32
        }

        #[no_mangle]
        pub extern "C" fn alloc(len: i32) -> i32 {
            if len <= 0 {
                return 0;
            }
            ::kernelos_pdk::arena_alloc(len as usize) as i32
        }

        #[no_mangle]
        pub extern "C" fn reset() {
            ::kernelos_pdk::arena_reset();
        }

        #[no_mangle]
        pub extern "C" fn update(ptr: i32, len: i32) -> i32 {
            if ptr <= 0 || len <= 0 {
                return 0;
            }
            let bytes =
                unsafe { std::slice::from_raw_parts(ptr as *const u8, len as usize) };
            let event: ::kernelos_pdk::Event = match ::serde_json::from_slice(bytes) {
                Ok(e) => e,
                Err(_) => return 0,
            };
            let dirty = ::kernelos_pdk::Plugin::update(plugin_instance(), event);
            if dirty { 1 } else { 0 }
        }

        #[no_mangle]
        pub extern "C" fn render() -> i32 {
            let ops = ::kernelos_pdk::Plugin::render(plugin_instance());
            let json = match ::serde_json::to_vec(&ops) {
                Ok(v) => v,
                Err(_) => return 0,
            };
            let out_ptr = ::kernelos_pdk::arena_alloc(json.len());
            unsafe {
                std::ptr::copy_nonoverlapping(json.as_ptr(), out_ptr, json.len());
                RENDER_HDR[0] = out_ptr as i32;
                RENDER_HDR[1] = json.len() as i32;
                RENDER_HDR.as_ptr() as i32
            }
        }
    };
}
