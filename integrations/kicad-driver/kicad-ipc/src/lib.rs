// SPDX-License-Identifier: GPL-3.0-or-later
//! Safe ownership wrapper for the statically linked native KiCad IPC core.
//!
//! No interpreter or helper process is started. The C ABI copies request bytes,
//! returns an owned NUL-terminated JSON response, and serializes each instance.
use serde_json::{Value, json};
use std::{
    cell::Cell,
    ffi::{CStr, c_char, c_int},
    marker::PhantomData,
    os::unix::ffi::OsStrExt,
    path::Path,
};

pub const DEFAULT_CONFIG: &str = "/workspace/kicad-config/connection.json";
const MAX_MESSAGE: usize = 1_048_576;

unsafe extern "C" {
    fn KiOpen(path: *const c_char, length: c_int) -> *mut c_char;
    fn KiCall(handle: u64, request: *const c_char, length: c_int) -> *mut c_char;
    fn KiClose(handle: u64);
    fn KiFreeString(value: *mut c_char);
}

fn error(message: &str) -> Value {
    json!({"code":"BackendFailed", "message":message, "outcome_known":false})
}

struct Response(*mut c_char);
impl Drop for Response {
    fn drop(&mut self) {
        // SAFETY: KiOpen/KiCall transfer exactly one allocation, freed exactly once here.
        unsafe { KiFreeString(self.0) };
    }
}

fn decode(pointer: *mut c_char) -> Result<Value, Value> {
    if pointer.is_null() {
        return Err(error("Native core returned a null response"));
    }
    let response = Response(pointer);
    // SAFETY: the bundled ABI guarantees a live, NUL-terminated allocation until free.
    let bytes = unsafe { CStr::from_ptr(response.0) }.to_bytes();
    if bytes.len() > MAX_MESSAGE {
        return Err(error("Native core response exceeds its contract"));
    }
    let value: Value =
        serde_json::from_slice(bytes).map_err(|_| error("Invalid native core response"))?;
    match (value.get("value"), value.get("error")) {
        (Some(value), None) => Ok(value.clone()),
        (None, Some(error)) => Err(error.clone()),
        _ => Err(error("Native core response is not a result envelope")),
    }
}

/// Owns one core handle. `&mut self` serializes Rust callers; the core also has bounded admission.
/// The marker deliberately prevents sharing this handle through `Sync`.
pub struct Client {
    handle: u64,
    _not_sync: PhantomData<Cell<()>>,
}
impl Client {
    pub fn open(path: &Path) -> Result<Self, Value> {
        let bytes = path.as_os_str().as_bytes();
        if bytes.is_empty() || bytes.len() > 4096 || bytes.contains(&0) {
            return Err(error("Invalid owner configuration path"));
        }
        // SAFETY: pointer and explicit length describe live immutable bytes copied during this call.
        let result = decode(unsafe { KiOpen(bytes.as_ptr().cast(), bytes.len() as c_int) })?;
        let handle = result
            .get("handle")
            .and_then(Value::as_u64)
            .filter(|h| *h != 0)
            .ok_or_else(|| error("Native core did not allocate a handle"))?;
        Ok(Self {
            handle,
            _not_sync: PhantomData,
        })
    }
    fn request(&mut self, request: Value) -> Result<Value, Value> {
        let bytes =
            serde_json::to_vec(&request).map_err(|_| error("Cannot encode native request"))?;
        if bytes.is_empty() || bytes.len() > MAX_MESSAGE {
            return Err(error("Native request exceeds its byte budget"));
        }
        // SAFETY: the handle remains owned by self, and request bytes live through the copying call.
        decode(unsafe { KiCall(self.handle, bytes.as_ptr().cast(), bytes.len() as c_int) })
    }
    pub fn capabilities(&mut self) -> Result<Value, Value> {
        self.request(json!({"operation":"catalog"}))
    }
    pub fn health(&mut self) -> Result<Value, Value> {
        self.request(json!({"operation":"health"}))
    }
    pub fn execute(&mut self, command: &str, digest: &str, args: Value) -> Result<Value, Value> {
        self.request(json!({"operation":"execute", "command":command, "descriptor_sha256":digest, "args":args}))
    }
}
impl Drop for Client {
    fn drop(&mut self) {
        // SAFETY: this is the single owning Rust handle. Closing also drops the Unix socket.
        unsafe { KiClose(self.handle) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn client_is_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Client>();
    }
    #[test]
    fn embedded_nul_path_fails_before_ffi() {
        use std::ffi::OsStr;
        assert!(Client::open(Path::new(OsStr::from_bytes(b"/tmp/a\0b"))).is_err());
    }
    #[test]
    fn malformed_handle_response_fails() {
        assert!(decode(std::ptr::null_mut()).is_err());
    }
}
