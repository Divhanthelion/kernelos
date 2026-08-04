//! Safe guest linear-memory access. Re-derives the view after every guest call.

use js_sys::{Function, Uint8Array, WebAssembly};
use wasm_bindgen::JsValue;

#[derive(Debug)]
pub enum MemoryError {
    OutOfBounds { ptr: u32, len: u32, mem_len: u32 },
    Js(String),
}

impl std::fmt::Display for MemoryError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MemoryError::OutOfBounds { ptr, len, mem_len } => {
                write!(f, "guest memory OOB: ptr={ptr} len={len} mem={mem_len}")
            }
            MemoryError::Js(msg) => write!(f, "{msg}"),
        }
    }
}

impl From<MemoryError> for String {
    fn from(e: MemoryError) -> String {
        e.to_string()
    }
}

pub struct GuestMemory {
    memory: WebAssembly::Memory,
}

impl GuestMemory {
    pub fn new(memory: WebAssembly::Memory) -> Self {
        Self { memory }
    }

    pub fn memory(&self) -> &WebAssembly::Memory {
        &self.memory
    }

    /// Always re-derive — never cache the returned view.
    fn view(&self) -> Uint8Array {
        Uint8Array::new(&self.memory.buffer())
    }

    pub fn byte_length(&self) -> u32 {
        self.view().byte_length()
    }

    pub fn read(&self, ptr: u32, len: u32) -> Result<Vec<u8>, MemoryError> {
        let mem_len = self.byte_length();
        let end = ptr.checked_add(len).ok_or(MemoryError::OutOfBounds {
            ptr,
            len,
            mem_len,
        })?;
        if end > mem_len {
            return Err(MemoryError::OutOfBounds { ptr, len, mem_len });
        }
        let view = self.view();
        let mut out = vec![0u8; len as usize];
        view.subarray(ptr, end).copy_to(&mut out);
        Ok(out)
    }

    pub fn write(&self, ptr: u32, bytes: &[u8]) -> Result<(), MemoryError> {
        let len = bytes.len() as u32;
        let mem_len = self.byte_length();
        let end = ptr.checked_add(len).ok_or(MemoryError::OutOfBounds {
            ptr,
            len,
            mem_len,
        })?;
        if end > mem_len {
            return Err(MemoryError::OutOfBounds { ptr, len, mem_len });
        }
        self.view().subarray(ptr, end).copy_from(bytes);
        Ok(())
    }

    pub fn read_header(&self, hdr_ptr: u32) -> Result<(u32, u32), MemoryError> {
        let hdr = self.read(hdr_ptr, 8)?;
        let out_ptr = u32::from_le_bytes(hdr[0..4].try_into().unwrap());
        let out_len = u32::from_le_bytes(hdr[4..8].try_into().unwrap());
        Ok((out_ptr, out_len))
    }

    pub fn write_header(&self, hdr_ptr: u32, out_ptr: u32, out_len: u32) -> Result<(), MemoryError> {
        let mut hdr = [0u8; 8];
        hdr[0..4].copy_from_slice(&out_ptr.to_le_bytes());
        hdr[4..8].copy_from_slice(&out_len.to_le_bytes());
        self.write(hdr_ptr, &hdr)
    }
}

/// Call a guest export and re-wrap memory afterward (grow may have detached views).
pub fn call_i32(func: &Function, args: &[&JsValue]) -> Result<i32, String> {
    let result = func
        .apply(&JsValue::UNDEFINED, &js_sys::Array::from_iter(args.iter().copied()))
        .map_err(|e| format!("guest call failed: {e:?}"))?;
    result
        .as_f64()
        .map(|v| v as i32)
        .ok_or_else(|| "guest call returned non-number".into())
}

pub fn call_void(func: &Function) -> Result<(), String> {
    func.call0(&JsValue::UNDEFINED)
        .map(|_| ())
        .map_err(|e| format!("guest call failed: {e:?}"))
}
