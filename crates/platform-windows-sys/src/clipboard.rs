use semwright_types::{Error, ErrorCode, Result};
use std::{slice, thread, time::Duration};
use windows::Win32::{
    Foundation::{GlobalFree, HANDLE, HGLOBAL},
    System::{
        DataExchange::{
            CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, SetClipboardData,
        },
        Memory::{GMEM_MOVEABLE, GMEM_ZEROINIT, GlobalAlloc, GlobalLock, GlobalSize, GlobalUnlock},
        Ole::CF_UNICODETEXT,
    },
};

const MAX_UTF16_BYTES: usize = 4 * 1024 * 1024;

struct ClipboardGuard;
impl Drop for ClipboardGuard {
    fn drop(&mut self) {
        // SAFETY: paired with successful OpenClipboard on this thread.
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

fn open() -> Result<ClipboardGuard> {
    for _ in 0..8 {
        // SAFETY: no owner window; current process accesses clipboard only within this guard.
        if unsafe { OpenClipboard(None) }.is_ok() {
            return Ok(ClipboardGuard);
        }
        thread::sleep(Duration::from_millis(10));
    }
    Err(Error::new(ErrorCode::Conflict, "Windows clipboard is busy"))
}

pub fn read_text() -> Result<String> {
    let _guard = open()?;
    // SAFETY: clipboard remains open for the lifetime of the returned kernel handle use.
    let raw = unsafe { GetClipboardData(CF_UNICODETEXT.0 as u32) }
        .map_err(|_| Error::new(ErrorCode::Unavailable, "CF_UNICODETEXT is unavailable"))?;
    let memory = HGLOBAL(raw.0);
    // SAFETY: memory is owned by the clipboard; lock is released before CloseClipboard.
    let ptr = unsafe { GlobalLock(memory) };
    if ptr.is_null() {
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Clipboard memory could not be locked",
        ));
    }
    let size = unsafe { GlobalSize(memory) };
    if size == 0 || size > MAX_UTF16_BYTES || size % 2 != 0 {
        let _ = unsafe { GlobalUnlock(memory) };
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Clipboard text exceeds safe size",
        ));
    }
    let units = unsafe { slice::from_raw_parts(ptr.cast::<u16>(), size / 2) };
    let end = units.iter().position(|u| *u == 0).unwrap_or(units.len());
    let result = String::from_utf16(&units[..end])
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "Clipboard UTF-16 was invalid"));
    let _ = unsafe { GlobalUnlock(memory) };
    result
}

pub fn write_text(value: &str) -> Result<()> {
    let units: Vec<u16> = value.encode_utf16().chain(std::iter::once(0)).collect();
    let bytes = units
        .len()
        .checked_mul(2)
        .ok_or_else(|| Error::invalid("Clipboard size overflow"))?;
    if bytes > MAX_UTF16_BYTES {
        return Err(Error::new(
            ErrorCode::ResourceExhausted,
            "Clipboard text exceeds safe size",
        ));
    }
    let _guard = open()?;
    // SAFETY: allocation size is bounded above; memory remains owned here until SetClipboardData.
    let memory = unsafe { GlobalAlloc(GMEM_MOVEABLE | GMEM_ZEROINIT, bytes) }
        .map_err(|_| Error::new(ErrorCode::ResourceExhausted, "Clipboard allocation failed"))?;
    let ptr = unsafe { GlobalLock(memory) };
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Clipboard allocation lock failed",
        ));
    }
    unsafe {
        std::ptr::copy_nonoverlapping(units.as_ptr(), ptr.cast::<u16>(), units.len());
    }
    let _ = unsafe { GlobalUnlock(memory) };
    if unsafe { EmptyClipboard() }.is_err() {
        let _ = unsafe { GlobalFree(Some(memory)) };
        return Err(Error::new(
            ErrorCode::BackendFailed,
            "Clipboard clear failed",
        ));
    }
    match unsafe { SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(memory.0))) } {
        Ok(_) => Ok(()), // ownership transferred to the system
        Err(_) => {
            let _ = unsafe { GlobalFree(Some(memory)) };
            Err(Error::new(
                ErrorCode::BackendFailed,
                "Clipboard write failed",
            ))
        }
    }
}
