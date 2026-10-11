//! Bounded I/O on the process's controlling console, independent of redirected stdio.
use semwright_types::{Error, ErrorCode, Result};
use std::{fs::File, os::windows::io::AsRawHandle};
use windows_sys::Win32::{
    Foundation::HANDLE,
    System::{
        Console::{
            FlushConsoleInputBuffer, GetConsoleMode, GetConsoleWindow, INPUT_RECORD, KEY_EVENT,
            WriteConsoleW,
        },
        LibraryLoader::{GetModuleHandleW, GetProcAddress},
    },
};

type ReadInput = unsafe extern "system" fn(HANDLE, *mut INPUT_RECORD, u32, *mut u32, u16) -> i32;
const CONSOLE_READ_NOWAIT: u16 = 2;

/// Owned console devices; raw handles never cross the platform boundary.
pub struct ConsoleTerminal {
    input: File,
    output: File,
    read_input: ReadInput,
}

fn unavailable() -> Error {
    Error::new(
        ErrorCode::ConsentRequired,
        "Foreground approval requires the broker's controlling console",
    )
}

impl ConsoleTerminal {
    /// Open the attached console itself. Pipes and files cannot supply an approval response.
    pub fn open() -> Result<Self> {
        // SAFETY: this process-global query has no pointer parameters. CREATE_NO_WINDOW
        // still supplies console devices but no human-visible window/ConPTY console handle.
        if unsafe { GetConsoleWindow() }.is_null() {
            return Err(unavailable());
        }
        let input = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONIN$")
            .map_err(|_| unavailable())?;
        let output = std::fs::OpenOptions::new()
            .read(true)
            .write(true)
            .open("CONOUT$")
            .map_err(|_| unavailable())?;
        for file in [&input, &output] {
            let mut mode = 0;
            // SAFETY: the owned file handle is live and mode is a writable DWORD.
            if unsafe { GetConsoleMode(file.as_raw_handle(), &mut mode) } == 0 {
                return Err(unavailable());
            }
        }
        let kernel: Vec<u16> = "kernel32.dll\0".encode_utf16().collect();
        // SAFETY: kernel is NUL-terminated; Kernel32 remains loaded for the process lifetime.
        let module = unsafe { GetModuleHandleW(kernel.as_ptr()) };
        if module.is_null() {
            return Err(unavailable());
        }
        // SAFETY: module is live and the symbol name is NUL-terminated.
        let address = unsafe { GetProcAddress(module, c"ReadConsoleInputExW".as_ptr().cast()) }
            .ok_or_else(unavailable)?;
        // SAFETY: this public Windows API has the documented ReadInput signature. Resolving
        // its NOWAIT variant prevents a competing console reader from making cancellation hang.
        let read_input = unsafe {
            std::mem::transmute::<unsafe extern "system" fn() -> isize, ReadInput>(address)
        };
        Ok(Self {
            input,
            output,
            read_input,
        })
    }

    /// Consume a bounded batch without blocking, including key-repeat counts.
    pub fn read_keys(&self) -> Result<Vec<char>> {
        // SAFETY: all-zero INPUT_RECORD is a valid writable output buffer for this API.
        let mut records: [INPUT_RECORD; 32] = unsafe { std::mem::zeroed() };
        let mut count = 0;
        // SAFETY: input is owned, records has the declared capacity and count is writable.
        if unsafe {
            (self.read_input)(
                self.input.as_raw_handle(),
                records.as_mut_ptr(),
                records.len() as u32,
                &mut count,
                CONSOLE_READ_NOWAIT,
            )
        } == 0
        {
            return Err(unavailable());
        }
        if count as usize > records.len() {
            return Err(unavailable());
        }
        let mut keys = Vec::new();
        for record in &records[..count as usize] {
            if u32::from(record.EventType) != KEY_EVENT {
                continue;
            }
            // SAFETY: EventType identifies the initialized KeyEvent union member.
            let event = unsafe { record.Event.KeyEvent };
            if event.bKeyDown == 0 {
                continue;
            }
            // SAFETY: ReadConsoleInputExW populates the UnicodeChar union member.
            let character = unsafe { event.uChar.UnicodeChar };
            if character == 0 {
                continue;
            }
            if character > 127 || keys.len() + usize::from(event.wRepeatCount) > 256 {
                return Err(Error::new(
                    ErrorCode::PolicyDenied,
                    "Approval input must be bounded ASCII",
                ));
            }
            keys.extend(std::iter::repeat_n(
                char::from(character as u8),
                usize::from(event.wRepeatCount),
            ));
        }
        Ok(keys)
    }

    /// Write Unicode directly to the controlling screen buffer, never to redirected stdout.
    pub fn write(&self, text: &str) -> Result<()> {
        let encoded: Vec<u16> = text.encode_utf16().collect();
        let mut remaining = encoded.as_slice();
        while !remaining.is_empty() {
            let mut written = 0;
            // SAFETY: output is owned, remaining is a valid UTF-16 slice and written is writable.
            if unsafe {
                WriteConsoleW(
                    self.output.as_raw_handle(),
                    remaining.as_ptr().cast(),
                    remaining.len() as u32,
                    &mut written,
                    std::ptr::null(),
                )
            } == 0
                || written == 0
                || written as usize > remaining.len()
            {
                return Err(unavailable());
            }
            remaining = &remaining[written as usize..];
        }
        Ok(())
    }

    /// Discard expired/type-ahead responses so they cannot satisfy the next challenge.
    pub fn flush(&self) -> Result<()> {
        // SAFETY: input is an owned console input handle opened with write access.
        if unsafe { FlushConsoleInputBuffer(self.input.as_raw_handle()) } == 0 {
            return Err(unavailable());
        }
        Ok(())
    }
}
