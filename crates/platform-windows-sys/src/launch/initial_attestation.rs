//! Read-only attestation of the actual Host-created process before ResumeThread.
//! Identity verification does not establish successful application startup.
#[path = "initial_lpac.rs"]
mod lpac;

use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, io, mem};
use windows_sys::Win32::{
    Foundation::{CloseHandle, HANDLE},
    Security::{
        GetTokenInformation, SID_AND_ATTRIBUTES, TOKEN_GROUPS, TOKEN_INFORMATION_CLASS,
        TOKEN_QUERY, TokenAppContainerSid, TokenCapabilities, TokenIsAppContainer, TokenUser,
    },
    System::Threading::OpenProcessToken,
};
type Capability = (Vec<u8>, u32);

fn denied(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::PermissionDenied, message)
}
struct Token(HANDLE);
impl Drop for Token {
    fn drop(&mut self) {
        // SAFETY: this guard owns the query-only handle returned by OpenProcessToken.
        unsafe {
            CloseHandle(self.0);
        }
    }
}
struct TokenBuffer {
    words: Vec<u64>,
    used: usize,
}
impl TokenBuffer {
    fn query(token: HANDLE, class: TOKEN_INFORMATION_CLASS) -> io::Result<Self> {
        let mut needed = 0u32;
        // SAFETY: a live query-only token; null/zero requests the required size.
        let _ = unsafe { GetTokenInformation(token, class, std::ptr::null_mut(), 0, &mut needed) };
        if !(4..=65_536).contains(&needed) {
            return Err(denied("Token information size is outside the fixed bound"));
        }
        let requested_length = needed;
        let mut words = vec![0u64; (requested_length as usize).div_ceil(8)];
        // SAFETY: the aligned allocation contains at least needed writable bytes.
        if unsafe {
            GetTokenInformation(token, class, words.as_mut_ptr().cast(), needed, &mut needed)
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        if needed < 4 || needed > requested_length {
            return Err(denied("Token information changed beyond its allocation"));
        }
        Ok(Self {
            words,
            used: needed as usize,
        })
    }
    fn base(&self) -> usize {
        self.words.as_ptr() as usize
    }
    fn read<T: Copy>(&self, address: usize) -> io::Result<T> {
        let base = self.base();
        let end = base
            .checked_add(self.used)
            .ok_or_else(|| denied("Token allocation range overflow"))?;
        if self.used > mem::size_of_val(self.words.as_slice())
            || address < base
            || address
                .checked_add(mem::size_of::<T>())
                .is_none_or(|last| last > end)
        {
            return Err(denied("Token pointer is outside its owned allocation"));
        }
        // SAFETY: the complete copied span is inside the live owned allocation.
        Ok(unsafe { std::ptr::read_unaligned(address as *const T) })
    }
    fn sid(&self, address: usize) -> io::Result<Vec<u8>> {
        let revision: u8 = self.read(address)?;
        let count: u8 = self.read(
            address
                .checked_add(1)
                .ok_or_else(|| denied("SID address overflow"))?,
        )?;
        if revision != 1 || count > 15 {
            return Err(denied("Token contains an invalid SID header"));
        }
        let length = 8 + usize::from(count) * 4;
        let mut result = Vec::with_capacity(length);
        for offset in 0..length {
            result.push(
                self.read::<u8>(
                    address
                        .checked_add(offset)
                        .ok_or_else(|| denied("SID address overflow"))?,
                )?,
            );
        }
        Ok(result)
    }
    fn first_sid(&self) -> io::Result<Vec<u8>> {
        // TOKEN_USER and TOKEN_APPCONTAINER_INFORMATION both begin with PSID.
        let address: usize = self.read(self.base())?;
        self.sid(address)
    }
    fn capabilities(&self) -> io::Result<Vec<Capability>> {
        let count: u32 = self.read(self.base())?;
        if count > 64 {
            return Err(denied("Token capability count exceeds the fixed bound"));
        }
        let start = self
            .base()
            .checked_add(mem::offset_of!(TOKEN_GROUPS, Groups))
            .ok_or_else(|| denied("Capability array overflow"))?;
        let mut result = Vec::with_capacity(count as usize);
        for index in 0..count as usize {
            let offset = index
                .checked_mul(mem::size_of::<SID_AND_ATTRIBUTES>())
                .ok_or_else(|| denied("Capability array overflow"))?;
            let address = start
                .checked_add(offset)
                .ok_or_else(|| denied("Capability array overflow"))?;
            let entry: SID_AND_ATTRIBUTES = self.read(address)?;
            result.push((self.sid(entry.Sid as usize)?, entry.Attributes));
        }
        Ok(result)
    }
}

fn capability_set(entries: &[Capability]) -> io::Result<BTreeMap<Vec<u8>, u32>> {
    if entries.len() > 64 {
        return Err(denied("Capability count exceeds the fixed bound"));
    }
    let mut result = BTreeMap::new();
    for (sid, attributes) in entries {
        if result
            .insert(sid.clone(), *attributes)
            .is_some_and(|previous| previous != *attributes)
        {
            return Err(denied("One capability SID has conflicting attributes"));
        }
    }
    Ok(result)
}

fn require_exact_capabilities(
    actual: &[Capability],
    expected: &[Capability],
) -> io::Result<Vec<Capability>> {
    // Identical repeats carry the same authority. Never combine attribute bits.
    // The launcher request itself is not changed or deduplicated by this check.
    let actual = capability_set(actual)?;
    let wanted = capability_set(expected)?;
    if actual != wanted {
        return Err(denied(
            "Actual capabilities differ from requested identities or attributes",
        ));
    }
    Ok(actual.into_iter().collect())
}

pub(super) fn verify(
    process: HANDLE,
    expected_user: &[u8],
    expected_container: &[u8],
    expected_capabilities: &[Capability],
) -> io::Result<serde_json::Value> {
    let mut handle = std::ptr::null_mut();
    // SAFETY: live process handle and writable out-parameter; query access only.
    if unsafe { OpenProcessToken(process, TOKEN_QUERY, &mut handle) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let token = Token(handle);
    let mut is_container = 0u32;
    let mut returned = 0u32;
    // SAFETY: TokenIsAppContainer returns one DWORD into this exact-sized buffer.
    if unsafe {
        GetTokenInformation(
            token.0,
            TokenIsAppContainer,
            (&mut is_container as *mut u32).cast(),
            mem::size_of::<u32>() as u32,
            &mut returned,
        )
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    if returned != 4 || is_container != 1 {
        return Err(denied("Initial process is not an AppContainer"));
    }
    lpac::verify_lpac(token.0)?;
    let user = TokenBuffer::query(token.0, TokenUser)?.first_sid()?;
    let container = TokenBuffer::query(token.0, TokenAppContainerSid)?.first_sid()?;
    if user != expected_user || container != expected_container {
        return Err(denied(
            "Initial process user or AppContainer identity differs",
        ));
    }
    let actual = TokenBuffer::query(token.0, TokenCapabilities)?.capabilities()?;
    let observed_entry_count = actual.len();
    let capabilities = require_exact_capabilities(&actual, expected_capabilities)?;
    let expected_distinct = capability_set(expected_capabilities)?.len();
    let observed = capabilities
        .iter()
        .map(|(sid, attributes)| {
            serde_json::json!({
                "sid_sha256": hex::encode(Sha256::digest(sid)),
                "attributes": attributes,
            })
        })
        .collect::<Vec<_>>();
    Ok(serde_json::json!({
        "appcontainer": true,
        "lpac": true,
        "user_matches_requested": true,
        "appcontainer_matches_requested": true,
        "user_sid_sha256": hex::encode(Sha256::digest(&user)),
        "appcontainer_sid_sha256": hex::encode(Sha256::digest(&container)),
        "capability_count": capabilities.len(),
        "capability_entries_observed": observed_entry_count,
        "capability_entries_requested": expected_capabilities.len(),
        "capability_identities_requested": expected_distinct,
        "capability_comparison": "exact SID/attributes sets; identical repeats coalesced",
        "capabilities": observed,
        "capabilities_match_requested": true,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use windows_sys::Win32::System::Threading::GetCurrentProcess;
    #[test]
    fn owner_process_is_not_an_initial_lpac() {
        let owner = crate::identity::current_user_sid_bytes().unwrap();
        // SAFETY: current-process pseudohandle; the verifier only queries it.
        let process = unsafe { GetCurrentProcess() };
        assert!(verify(process, &owner, &owner, &[]).is_err());
    }
    #[test]
    fn capability_identity_attributes_and_extras_are_not_ignored() {
        let one = (vec![1, 2, 3], 4);
        let two = (vec![4, 5, 6], 4);
        require_exact_capabilities(&[], &[]).unwrap();
        require_exact_capabilities(&[one.clone()], &[one.clone()]).unwrap();
        assert!(require_exact_capabilities(&[one.clone()], &[]).is_err());
        assert!(require_exact_capabilities(&[two], &[one.clone()]).is_err());
        assert!(require_exact_capabilities(&[(one.0.clone(), 0)], &[one.clone()]).is_err());
        // ARM emulation plus network can request the same registryRead pair twice.
        require_exact_capabilities(&[one.clone()], &[one.clone(), one.clone()]).unwrap();
        require_exact_capabilities(&[one.clone(), one.clone()], &[one.clone()]).unwrap();
        assert!(
            require_exact_capabilities(&[one.clone(), (one.0.clone(), 0)], &[one.clone()]).is_err()
        );
        assert!(
            require_exact_capabilities(&[one.clone()], &[one.clone(), (one.0.clone(), 0)]).is_err()
        );
    }
    #[test]
    fn pointer_bearing_token_data_is_bounded() {
        let buffer = TokenBuffer {
            words: vec![0; 16],
            used: 128,
        };
        assert!(buffer.read::<usize>(usize::MAX).is_err());
        assert!(buffer.sid(buffer.base() - 1).is_err());
        assert!(buffer.sid(buffer.base() + buffer.used).is_err());
        assert!(buffer.sid(buffer.base()).is_err());
    }
    fn put<T: Copy>(buffer: &mut TokenBuffer, offset: usize, value: T) {
        assert!(
            offset
                .checked_add(mem::size_of::<T>())
                .is_some_and(|end| end <= mem::size_of_val(buffer.words.as_slice()))
        );
        // SAFETY: test-only write is bounded by the live owned allocation.
        unsafe {
            std::ptr::write_unaligned(
                (buffer.words.as_mut_ptr() as *mut u8).add(offset).cast(),
                value,
            );
        }
    }
    #[test]
    fn valid_pointer_bearing_token_fields_and_truncations() {
        let mut buffer = TokenBuffer {
            words: vec![0; 16],
            used: 128,
        };
        let base = buffer.base();
        let start = mem::offset_of!(TOKEN_GROUPS, Groups);
        let stride = mem::size_of::<SID_AND_ATTRIBUTES>();
        let sid_one = vec![1u8, 1, 0, 0, 0, 0, 0, 15, 7, 0, 0, 0];
        let sid_two = vec![1u8, 1, 0, 0, 0, 0, 0, 15, 8, 0, 0, 0];
        for (offset, bytes) in [(64, &sid_one), (80, &sid_two)] {
            for (index, byte) in bytes.iter().enumerate() {
                put(&mut buffer, offset + index, *byte);
            }
        }
        put(&mut buffer, 0, 2u32);
        put(
            &mut buffer,
            start,
            SID_AND_ATTRIBUTES {
                Sid: (base + 64) as _,
                Attributes: 4,
            },
        );
        put(
            &mut buffer,
            start + stride,
            SID_AND_ATTRIBUTES {
                Sid: (base + 80) as _,
                Attributes: 2,
            },
        );
        assert_eq!(
            buffer.capabilities().unwrap(),
            vec![(sid_one.clone(), 4), (sid_two.clone(), 2)]
        );
        buffer.used = start + 2 * stride - 1;
        assert!(
            buffer
                .read::<SID_AND_ATTRIBUTES>(base + start + stride)
                .is_err()
        );
        assert!(buffer.capabilities().is_err());
        buffer.used = 128;
        put(
            &mut buffer,
            start + stride,
            SID_AND_ATTRIBUTES {
                Sid: (base + 128) as _,
                Attributes: 2,
            },
        );
        assert!(buffer.capabilities().is_err());
        put(
            &mut buffer,
            start + stride,
            SID_AND_ATTRIBUTES {
                Sid: (base + 80) as _,
                Attributes: 2,
            },
        );
        put(&mut buffer, 0, 65u32);
        assert!(buffer.capabilities().is_err());
        put(&mut buffer, 0, base + 64);
        assert_eq!(buffer.first_sid().unwrap(), sid_one);
    }
}
