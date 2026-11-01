//! Read-only LPAC attestation. Some Windows kernels reject information class 46.
//! The fallback requires the kernel's exact WIN://NOALLAPPPKG uint64 attribute,
//! not merely an AppContainer token, group membership or a caller-supplied flag.
use std::{io, mem::size_of};
use windows_sys::Win32::{
    Foundation::HANDLE,
    Security::{GetTokenInformation, TokenIsLessPrivilegedAppContainer, TokenSecurityAttributes},
};

fn denied() -> io::Error {
    io::Error::new(
        io::ErrorKind::PermissionDenied,
        "Host LPAC attestation is unavailable or false",
    )
}
#[repr(C)]
#[derive(Clone, Copy)]
struct UnicodeString {
    length: u16,
    maximum_length: u16,
    buffer: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Attribute {
    name: UnicodeString,
    value_type: u16,
    reserved: u16,
    flags: u32,
    value_count: u32,
    values: usize,
}
#[repr(C)]
#[derive(Clone, Copy)]
struct Header {
    version: u16,
    reserved: u16,
    count: u32,
    attributes: usize,
}
fn read_value<T: Copy>(storage: &[u64], used: usize, address: usize) -> io::Result<T> {
    let base = storage.as_ptr() as usize;
    let end = base.checked_add(used).ok_or_else(denied)?;
    if used > std::mem::size_of_val(storage)
        || address < base
        || address.checked_add(size_of::<T>()).is_none_or(|n| n > end)
    {
        return Err(denied());
    }
    // SAFETY: exact copied span lies within the live owned allocation. Unaligned
    // reads avoid assuming alignment of pointer-bearing token information fields.
    Ok(unsafe { std::ptr::read_unaligned(address as *const T) })
}
fn verify_attribute(storage: &[u64], used: usize) -> io::Result<()> {
    let h: Header = read_value(storage, used, storage.as_ptr() as usize)?;
    if h.version != 1 || h.reserved != 0 || !(1..=128).contains(&h.count) {
        return Err(denied());
    }
    let wanted: Vec<u16> = "WIN://NOALLAPPPKG".encode_utf16().collect();
    let mut found = false;
    for i in 0..h.count as usize {
        let address = h
            .attributes
            .checked_add(i * size_of::<Attribute>())
            .ok_or_else(denied)?;
        let a: Attribute = read_value(storage, used, address)?;
        if a.name.length == 0
            || !a.name.length.is_multiple_of(2)
            || a.name.length > 512
            || a.name.maximum_length < a.name.length
        {
            return Err(denied());
        }
        let mut name = Vec::with_capacity(a.name.length as usize / 2);
        for j in 0..a.name.length as usize / 2 {
            name.push(read_value::<u16>(
                storage,
                used,
                a.name.buffer.checked_add(j * 2).ok_or_else(denied)?,
            )?);
        }
        if name == wanted {
            // UINT64, one enabled/inheritable value, no ambiguous duplicates.
            if found
                || a.value_type != 2
                || a.reserved != 0
                || a.flags != 0
                || a.value_count != 1
                || read_value::<u64>(storage, used, a.values)? != 1
            {
                return Err(denied());
            }
            found = true;
        }
    }
    if found { Ok(()) } else { Err(denied()) }
}
pub fn verify_lpac(token: HANDLE) -> io::Result<()> {
    let mut value = 0u32;
    let mut size = 0u32;
    // SAFETY: live query-only token, DWORD out-buffer and writable size.
    if unsafe {
        GetTokenInformation(
            token,
            TokenIsLessPrivilegedAppContainer,
            (&mut value as *mut u32).cast(),
            4,
            &mut size,
        )
    } != 0
    {
        return if size == 4 && value == 1 {
            Ok(())
        } else {
            Err(denied())
        };
    }
    let error = io::Error::last_os_error();
    if error.raw_os_error() != Some(87) {
        return Err(error);
    }
    let mut needed = 0;
    // SAFETY: zero-size query asks only for the bounded allocation requirement.
    unsafe {
        GetTokenInformation(
            token,
            TokenSecurityAttributes,
            std::ptr::null_mut(),
            0,
            &mut needed,
        );
    }
    if !(size_of::<Header>() as u32..=65_536).contains(&needed) {
        return Err(denied());
    }
    let requested_length = needed;
    let mut storage = vec![0u64; (requested_length as usize).div_ceil(8)];
    // SAFETY: aligned owned allocation has at least the API-requested byte capacity.
    if unsafe {
        GetTokenInformation(
            token,
            TokenSecurityAttributes,
            storage.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if needed < size_of::<Header>() as u32 || needed > requested_length {
        return Err(denied());
    }
    verify_attribute(&storage, needed as usize)
}

#[cfg(test)]
mod tests {
    use super::*;
    fn put<T: Copy>(storage: &mut [u64], offset: usize, value: T) {
        assert!(offset + size_of::<T>() <= std::mem::size_of_val(storage));
        // SAFETY: test fixture writes a bounded value inside its owned allocation.
        unsafe {
            std::ptr::write_unaligned((storage.as_mut_ptr() as *mut u8).add(offset).cast(), value);
        }
    }
    fn fixture() -> Vec<u64> {
        let mut s = vec![0u64; 128];
        let base = s.as_ptr() as usize;
        let name: Vec<u16> = "WIN://NOALLAPPPKG".encode_utf16().collect();
        put(
            &mut s,
            0,
            Header {
                version: 1,
                reserved: 0,
                count: 1,
                attributes: base + 32,
            },
        );
        put(
            &mut s,
            32,
            Attribute {
                name: UnicodeString {
                    length: (name.len() * 2) as u16,
                    maximum_length: (name.len() * 2) as u16,
                    buffer: base + 256,
                },
                value_type: 2,
                reserved: 0,
                flags: 0,
                value_count: 1,
                values: base + 512,
            },
        );
        for (i, value) in name.into_iter().enumerate() {
            put(&mut s, 256 + i * 2, value);
        }
        put(&mut s, 512, 1u64);
        s
    }
    #[test]
    fn exact_kernel_lpac_attribute_is_required() {
        let s = fixture();
        verify_attribute(&s, 1024).unwrap();
        for field in ["zero", "type", "flags", "count", "name"] {
            let mut s = fixture();
            let mut a: Attribute = read_value(&s, 1024, s.as_ptr() as usize + 32).unwrap();
            match field {
                "zero" => put(&mut s, 512, 0u64),
                "type" => a.value_type = 6,
                "flags" => a.flags = 0x10,
                "count" => a.value_count = 2,
                "name" => put(&mut s, 256, b'X' as u16),
                _ => unreachable!(),
            }
            put(&mut s, 32, a);
            assert!(verify_attribute(&s, 1024).is_err(), "{field}");
        }
    }
    #[test]
    fn malformed_or_duplicate_pointer_bearing_claims_fail_closed() {
        for field in [
            "header",
            "table",
            "name",
            "value",
            "duplicate",
            "length",
            "used",
        ] {
            let mut s = fixture();
            let base = s.as_ptr() as usize;
            let mut h: Header = read_value(&s, 1024, base).unwrap();
            let mut a: Attribute = read_value(&s, 1024, base + 32).unwrap();
            match field {
                "header" => h.version = 2,
                "table" => h.attributes = usize::MAX,
                "name" => a.name.buffer = base - 1,
                "value" => a.values = base + 1020,
                "duplicate" => {
                    h.count = 2;
                    put(&mut s, 32 + size_of::<Attribute>(), a);
                }
                "length" => a.name.length = 3,
                "used" => {}
                _ => unreachable!(),
            }
            put(&mut s, 0, h);
            put(&mut s, 32, a);
            assert!(
                verify_attribute(&s, if field == "used" { 2048 } else { 1024 }).is_err(),
                "{field}"
            );
        }
    }
    #[test]
    fn an_ordinary_test_process_is_not_a_host_lpac() {
        use windows_sys::Win32::{
            Foundation::CloseHandle,
            Security::TOKEN_QUERY,
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        };
        let mut token = std::ptr::null_mut();
        // SAFETY: current process pseudo-handle and writable token result.
        let opened = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) };
        assert_ne!(opened, 0);
        let result = verify_lpac(token);
        // SAFETY: test owns this query-only token handle.
        unsafe {
            CloseHandle(token);
        }
        assert!(result.is_err());
    }
}
