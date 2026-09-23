//! Bounded executable-header inspection, NOT a full Mach-O loader or code-signature check.
use semwright_types::{Error, Result};
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum Architecture {
    Arm64,
    X86_64,
}
fn arch(cpu: u32) -> Result<Architecture> {
    match cpu {
        0x0100000c => Ok(Architecture::Arm64),
        0x01000007 => Ok(Architecture::X86_64),
        _ => Err(Error::invalid("Unsupported Mach-O architecture")),
    }
}
fn u32at(b: &[u8], i: usize, be: bool) -> Result<u32> {
    let v: [u8; 4] = b
        .get(i..i.checked_add(4).ok_or_else(|| Error::invalid("Overflow"))?)
        .ok_or_else(|| Error::invalid("Truncated Mach-O"))?
        .try_into()
        .map_err(|_| Error::invalid("Truncated field"))?;
    Ok(if be {
        u32::from_be_bytes(v)
    } else {
        u32::from_le_bytes(v)
    })
}
fn u64at(b: &[u8], i: usize) -> Result<u64> {
    let v: [u8; 8] = b
        .get(i..i.checked_add(8).ok_or_else(|| Error::invalid("Overflow"))?)
        .ok_or_else(|| Error::invalid("Truncated fat header"))?
        .try_into()
        .map_err(|_| Error::invalid("Truncated field"))?;
    Ok(u64::from_be_bytes(v))
}
fn thin(b: &[u8]) -> Result<Architecture> {
    if b.len() < 32 || b.get(..4) != Some(&[0xcf, 0xfa, 0xed, 0xfe]) {
        return Err(Error::invalid("64-bit little-endian Mach-O required"));
    }
    let a = arch(u32at(b, 4, false)?)?;
    if u32at(b, 12, false)? != 2 {
        return Err(Error::invalid("Mach-O must be MH_EXECUTE"));
    }
    let n = u32at(b, 16, false)? as usize;
    let size = u32at(b, 20, false)? as usize;
    if n > 4096 || n > size / 8 || size > b.len() - 32 {
        return Err(Error::invalid("Mach-O command budget exceeded"));
    }
    let end = 32 + size;
    let mut pos = 32;
    for _ in 0..n {
        if end - pos < 8 {
            return Err(Error::invalid("Truncated load command"));
        }
        let len = u32at(b, pos + 4, false)? as usize;
        if len < 8 || !len.is_multiple_of(8) || len > end - pos {
            return Err(Error::invalid("Invalid load command extent"));
        }
        pos += len;
    }
    if pos != end {
        return Err(Error::invalid("Command count/length mismatch"));
    }
    Ok(a)
}
pub fn architectures(b: &[u8]) -> Result<Vec<Architecture>> {
    if b.len() > 67_108_864 {
        return Err(Error::invalid("Executable exceeds 64 MiB"));
    }
    if b.starts_with(&[0xcf, 0xfa, 0xed, 0xfe]) {
        return Ok(vec![thin(b)?]);
    }
    let magic = u32at(b, 0, true)?;
    let stride = match magic {
        0xcafebabe => 20,
        0xcafebabf => 32,
        _ => return Err(Error::invalid("Unrecognized executable magic")),
    };
    let count = u32at(b, 4, true)? as usize;
    if count == 0 || count > 32 || b.len() < 8 + count * stride {
        return Err(Error::invalid("Invalid fat architecture table"));
    }
    let start = 8 + count * stride;
    let mut ranges = Vec::new();
    let mut result = Vec::new();
    for index in 0..count {
        let p = 8 + index * stride;
        let cpu = u32at(b, p, true)?;
        let a = arch(cpu)?;
        let (offset, size, align) = if stride == 20 {
            (
                u32at(b, p + 8, true)? as u64,
                u32at(b, p + 12, true)? as u64,
                u32at(b, p + 16, true)?,
            )
        } else {
            (u64at(b, p + 8)?, u64at(b, p + 16)?, u32at(b, p + 24, true)?)
        };
        let end = offset
            .checked_add(size)
            .ok_or_else(|| Error::invalid("Fat slice overflow"))?;
        if align > 30
            || offset < start as u64
            || size < 32
            || end > b.len() as u64
            || offset % (1u64 << align) != 0
        {
            return Err(Error::invalid("Invalid fat slice extent/alignment"));
        }
        if result.contains(&a) || ranges.iter().any(|&(lo, hi)| offset < hi && end > lo) {
            return Err(Error::invalid("Duplicate/overlapping fat slice"));
        }
        if thin(&b[offset as usize..end as usize])? != a {
            return Err(Error::invalid("Fat architecture/header mismatch"));
        }
        ranges.push((offset, end));
        result.push(a);
    }
    Ok(result)
}
#[cfg(test)]
mod tests {
    use super::*;
    fn header(cpu: u32) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        b[4..8].copy_from_slice(&cpu.to_le_bytes());
        b[12..16].copy_from_slice(&2u32.to_le_bytes());
        b
    }
    #[test]
    fn thin_architectures() {
        assert_eq!(
            architectures(&header(0x0100000c)).unwrap(),
            vec![Architecture::Arm64]
        );
        assert_eq!(
            architectures(&header(0x01000007)).unwrap(),
            vec![Architecture::X86_64]
        );
    }
    #[test]
    fn malformed() {
        for n in 0..32 {
            assert!(architectures(&header(0x0100000c)[..n]).is_err());
        }
        assert!(architectures(b"\x7fELFnotmach").is_err());
    }
    #[test]
    fn command_overrun() {
        let mut b = header(0x0100000c);
        b[16] = 1;
        assert!(architectures(&b).is_err());
    }
    #[test]
    fn fat_oob() {
        let mut b = vec![0; 28];
        b[..4].copy_from_slice(&0xcafebabeu32.to_be_bytes());
        b[7] = 1;
        b[8..12].copy_from_slice(&0x0100000cu32.to_be_bytes());
        b[16..20].copy_from_slice(&u32::MAX.to_be_bytes());
        assert!(architectures(&b).is_err());
    }
}

#[cfg(test)]
mod bounded_fat_tests {
    use super::*;
    use proptest::prelude::*;
    fn thin(cpu: u32) -> Vec<u8> {
        let mut b = vec![0u8; 32];
        b[..4].copy_from_slice(&[0xcf, 0xfa, 0xed, 0xfe]);
        b[4..8].copy_from_slice(&cpu.to_le_bytes());
        b[12..16].copy_from_slice(&2u32.to_le_bytes());
        b
    }
    fn universal() -> Vec<u8> {
        let mut b = vec![0; 128];
        b[..4].copy_from_slice(&0xcafebabeu32.to_be_bytes());
        b[4..8].copy_from_slice(&2u32.to_be_bytes());
        for (entry, cpu, offset) in [(8, 0x0100000cu32, 64u32), (28, 0x01000007u32, 96u32)] {
            b[entry..entry + 4].copy_from_slice(&cpu.to_be_bytes());
            b[entry + 8..entry + 12].copy_from_slice(&offset.to_be_bytes());
            b[entry + 12..entry + 16].copy_from_slice(&32u32.to_be_bytes());
            b[offset as usize..offset as usize + 32].copy_from_slice(&thin(cpu));
        }
        b
    }
    #[test]
    fn both_architectures() {
        assert_eq!(
            architectures(&universal()).unwrap(),
            vec![Architecture::Arm64, Architecture::X86_64]
        );
    }
    #[test]
    fn fat_overlap_denied() {
        let mut b = universal();
        b[36..40].copy_from_slice(&64u32.to_be_bytes());
        assert!(architectures(&b).is_err());
    }
    #[test]
    fn fat_architecture_mismatch_denied() {
        let mut b = universal();
        b[100..104].copy_from_slice(&0x0100000cu32.to_le_bytes());
        assert!(architectures(&b).is_err());
    }
    #[test]
    fn fat_duplicate_denied() {
        let mut b = universal();
        b[28..32].copy_from_slice(&0x0100000cu32.to_be_bytes());
        assert!(architectures(&b).is_err());
    }
    #[test]
    fn dylib_not_executable() {
        let mut b = thin(0x0100000c);
        b[12..16].copy_from_slice(&6u32.to_le_bytes());
        assert!(architectures(&b).is_err());
    }
    #[test]
    fn unreasonable_alignment_denied() {
        let mut b = universal();
        b[24..28].copy_from_slice(&31u32.to_be_bytes());
        assert!(architectures(&b).is_err());
    }
    proptest! {#[test]fn arbitrary_headers_do_not_panic(bytes in proptest::collection::vec(any::<u8>(),0..8192)){let _=architectures(&bytes);}}
}
