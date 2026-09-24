use semwright_types::{Error, ErrorCode, Result};

const IMAGE_FILE_MACHINE_AMD64: u16 = 0x8664;
const IMAGE_FILE_MACHINE_ARM64: u16 = 0xAA64;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PeArchitecture {
    Amd64,
    Arm64,
}

pub fn architecture(bytes: &[u8]) -> Result<PeArchitecture> {
    if bytes.len() < 0x40 || &bytes[..2] != b"MZ" {
        return Err(Error::invalid("Executable is not a PE image"));
    }
    let off = u32::from_le_bytes(bytes[0x3c..0x40].try_into().expect("bounded slice")) as usize;
    let end = off
        .checked_add(24)
        .ok_or_else(|| Error::invalid("PE header offset overflow"))?;
    if end > bytes.len() || &bytes[off..off + 4] != b"PE\0\0" {
        return Err(Error::invalid("Malformed PE signature"));
    }
    let machine = u16::from_le_bytes(bytes[off + 4..off + 6].try_into().expect("bounded slice"));
    match machine {
        IMAGE_FILE_MACHINE_AMD64 => Ok(PeArchitecture::Amd64),
        IMAGE_FILE_MACHINE_ARM64 => Ok(PeArchitecture::Arm64),
        _ => Err(Error::new(
            ErrorCode::Unsupported,
            "PE machine architecture is not supported by Semwright",
        )),
    }
}

pub fn require_native_architecture(bytes: &[u8]) -> Result<PeArchitecture> {
    let found = architecture(bytes)?;
    let wanted = match std::env::consts::ARCH {
        "x86_64" => PeArchitecture::Amd64,
        "aarch64" => PeArchitecture::Arm64,
        _ => {
            return Err(Error::new(
                ErrorCode::Unsupported,
                "Unsupported Windows host architecture",
            ));
        }
    };
    if found != wanted {
        return Err(Error::new(
            ErrorCode::Unsupported,
            "PE architecture does not match the native Windows host",
        ));
    }
    Ok(found)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(machine: u16) -> Vec<u8> {
        let mut b = vec![0u8; 0x100];
        b[0..2].copy_from_slice(b"MZ");
        b[0x3c..0x40].copy_from_slice(&(0x80u32).to_le_bytes());
        b[0x80..0x84].copy_from_slice(b"PE\0\0");
        b[0x84..0x86].copy_from_slice(&machine.to_le_bytes());
        b
    }

    #[test]
    fn parses_amd64_and_arm64() {
        assert_eq!(architecture(&image(0x8664)).unwrap(), PeArchitecture::Amd64);
        assert_eq!(architecture(&image(0xAA64)).unwrap(), PeArchitecture::Arm64);
    }

    #[test]
    fn rejects_truncated_and_unknown_images() {
        assert!(architecture(b"MZ").is_err());
        assert!(architecture(&image(0x014c)).is_err());
    }
}
