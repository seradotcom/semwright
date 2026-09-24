use semwright_types::{Error, ErrorCode, Result};
use serde_json::{Value, json};
use std::path::PathBuf;
use windows::{
    Win32::{
        Foundation::{CloseHandle, FILETIME, HWND, LPARAM, RECT, WPARAM},
        System::Threading::{
            GetProcessTimes, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
            QueryFullProcessImageNameW,
        },
        UI::WindowsAndMessaging::{
            EnumWindows, GetForegroundWindow, GetWindowRect, GetWindowTextW,
            GetWindowThreadProcessId, IsWindowVisible, PostMessageW, SWP_NOACTIVATE, SWP_NOZORDER,
            SetForegroundWindow, SetWindowPos, WM_CLOSE,
        },
    },
    core::{BOOL, PWSTR},
};

#[derive(Clone, Debug)]
pub struct NativeWindow {
    pub hwnd: HWND,
    pub pid: u32,
    pub title: String,
    pub image: Option<PathBuf>,
    pub rect: RECT,
}

unsafe extern "system" fn collect(hwnd: HWND, lparam: LPARAM) -> BOOL {
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    if !unsafe { IsWindowVisible(hwnd) }.as_bool() {
        return BOOL(1);
    }
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    let windows = unsafe { &mut *(lparam.0 as *mut Vec<HWND>) };
    windows.push(hwnd);
    BOOL(1)
}

fn title(hwnd: HWND) -> String {
    let mut buf = vec![0u16; 8192];
    // SAFETY: HWND came from EnumWindows; the writable buffer is bounded and ephemeral.
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) };
    if n <= 0 {
        String::new()
    } else {
        String::from_utf16_lossy(&buf[..n as usize])
    }
}

fn image(pid: u32) -> Option<PathBuf> {
    // SAFETY: requested access is query-only and no handle inheritance is enabled.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = vec![0u16; 32_768];
    let mut len = buf.len() as u32;
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    let value = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    }
    .ok()
    .map(|_| PathBuf::from(String::from_utf16_lossy(&buf[..len as usize])));
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    unsafe {
        let _ = CloseHandle(handle);
    }
    value
}

pub fn process_creation_time(pid: u32) -> Result<u64> {
    // SAFETY: query-only process handle, explicitly closed below.
    let handle =
        // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
        unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.map_err(|_| {
            Error::new(
                ErrorCode::StaleReference,
                "Windows process no longer exists",
            )
        })?;
    let mut creation = FILETIME::default();
    let mut exit = FILETIME::default();
    let mut kernel = FILETIME::default();
    let mut user = FILETIME::default();
    let result =
        // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
        unsafe { GetProcessTimes(handle, &mut creation, &mut exit, &mut kernel, &mut user) };
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    unsafe {
        let _ = CloseHandle(handle);
    }
    result.map_err(|_| {
        Error::new(
            ErrorCode::StaleReference,
            "Windows process generation could not be read",
        )
    })?;
    Ok(((creation.dwHighDateTime as u64) << 32) | creation.dwLowDateTime as u64)
}

pub fn enumerate() -> Result<Vec<NativeWindow>> {
    let mut handles = Vec::new();
    // SAFETY: callback only appends HWND values to the live vector referenced by LPARAM.
    unsafe {
        EnumWindows(
            Some(collect),
            LPARAM((&mut handles as *mut Vec<HWND>) as isize),
        )
    }
    .map_err(|_| Error::new(ErrorCode::BackendFailed, "EnumWindows failed"))?;
    let mut out = Vec::with_capacity(handles.len());
    for hwnd in handles {
        let mut pid = 0u32;
        // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
        unsafe {
            GetWindowThreadProcessId(hwnd, Some(&mut pid));
        }
        if pid == 0 {
            continue;
        }
        let mut rect = RECT::default();
        // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
        if unsafe { GetWindowRect(hwnd, &mut rect) }.is_err() {
            continue;
        }
        out.push(NativeWindow {
            hwnd,
            pid,
            title: title(hwnd),
            image: image(pid),
            rect,
        });
    }
    Ok(out)
}

pub fn foreground() -> Option<HWND> {
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    let hwnd = unsafe { GetForegroundWindow() };
    if hwnd.0.is_null() { None } else { Some(hwnd) }
}

pub fn focus(hwnd: HWND) -> Result<()> {
    // Windows intentionally restricts foreground activation. Do not use AttachThreadInput hacks.
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    if !unsafe { SetForegroundWindow(hwnd) }.as_bool() {
        return Err(Error::new(
            ErrorCode::Conflict,
            "Windows denied foreground activation",
        ));
    }
    Ok(())
}

pub fn move_resize(hwnd: HWND, x: i32, y: i32, width: i32, height: i32) -> Result<()> {
    if width <= 0 || height <= 0 {
        return Err(Error::invalid("Window size must be positive"));
    }
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    unsafe {
        SetWindowPos(
            hwnd,
            None,
            x,
            y,
            width,
            height,
            SWP_NOZORDER | SWP_NOACTIVATE,
        )
    }
    .map_err(|_| Error::new(ErrorCode::BackendFailed, "Window geometry change failed"))
}

pub fn close(hwnd: HWND) -> Result<()> {
    // SAFETY: the HWND/process handle and output storage come from the synchronous Win32 enumeration/query path and remain valid for this call.
    unsafe { PostMessageW(Some(hwnd), WM_CLOSE, WPARAM(0), LPARAM(0)) }
        .map_err(|_| Error::new(ErrorCode::BackendFailed, "WM_CLOSE could not be posted"))
}

pub fn to_json(w: &NativeWindow) -> Value {
    json!({
        "pid": w.pid,
        "title": w.title,
        "image": w.image,
        "bounds": { "x": w.rect.left, "y": w.rect.top,
                    "width": w.rect.right - w.rect.left, "height": w.rect.bottom - w.rect.top }
    })
}
