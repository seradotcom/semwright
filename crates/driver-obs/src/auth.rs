use crate::{Fault, FaultKind, Result};
use base64::{Engine, engine::general_purpose::STANDARD};
use sha2::{Digest, Sha256};
use zeroize::{Zeroize, Zeroizing};

pub struct Secret(Zeroizing<Vec<u8>>);

impl Secret {
    pub fn new(bytes: Vec<u8>) -> Result<Self> {
        let bytes = Zeroizing::new(bytes);
        if bytes.is_empty()
            || bytes.len() > 1024
            || std::str::from_utf8(&bytes).is_err()
            || bytes.contains(&0)
        {
            return Err(Fault::new(FaultKind::Authentication));
        }
        Ok(Self(bytes))
    }
}

pub fn proof(password: &Secret, salt: &str, challenge: &str) -> Result<Zeroizing<String>> {
    for part in [salt, challenge] {
        if part.len() != 44 {
            return Err(Fault::new(FaultKind::Authentication));
        }
        let bytes = STANDARD
            .decode(part)
            .map_err(|_| Fault::new(FaultKind::Authentication))?;
        if bytes.len() != 32 || STANDARD.encode(&bytes) != part {
            return Err(Fault::new(FaultKind::Authentication));
        }
    }
    let mut first = Sha256::new();
    first.update(password.0.as_slice());
    first.update(salt.as_bytes());
    let mut hash: [u8; 32] = first.finalize().into();
    let secret = Zeroizing::new(STANDARD.encode(hash));
    hash.zeroize();

    let mut second = Sha256::new();
    second.update(secret.as_bytes());
    second.update(challenge.as_bytes());
    let mut hash: [u8; 32] = second.finalize().into();
    let result = Zeroizing::new(STANDARD.encode(hash));
    hash.zeroize();
    Ok(result)
}
