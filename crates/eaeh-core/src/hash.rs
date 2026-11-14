//! Hashing and randomness helpers.

use crate::error::{HarnessError, Result};
use sha2::{Digest, Sha256};

pub fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push(HEX[(b >> 4) as usize] as char);
        s.push(HEX[(b & 0x0f) as usize] as char);
    }
    s
}

pub fn from_hex(s: &str) -> Result<Vec<u8>> {
    if !s.len().is_multiple_of(2) {
        return Err(HarnessError::invalid_input("hex string has odd length"));
    }
    (0..s.len())
        .step_by(2)
        .map(|i| {
            u8::from_str_radix(&s[i..i + 2], 16)
                .map_err(|_| HarnessError::invalid_input("invalid hex string"))
        })
        .collect()
}

pub fn sha256(data: &[u8]) -> [u8; 32] {
    let digest = Sha256::digest(data);
    let mut out = [0u8; 32];
    out.copy_from_slice(digest.as_slice());
    out
}

pub fn sha256_hex(data: &[u8]) -> String {
    to_hex(&sha256(data))
}

/// SHA-256 of the canonical JSON form (object keys sorted – `serde_json::Map` is a
/// BTreeMap because the `preserve_order` feature is not enabled in this workspace).
pub fn canonical_json_hash(value: &serde_json::Value) -> String {
    sha256_hex(canonical_json(value).as_bytes())
}

pub fn canonical_json(value: &serde_json::Value) -> String {
    // serde_json::to_string cannot fail for a Value.
    serde_json::to_string(value).unwrap_or_default()
}

/// Cryptographically secure random bytes from the OS.
pub fn random_bytes<const N: usize>() -> Result<[u8; N]> {
    let mut buf = [0u8; N];
    getrandom::fill(&mut buf)
        .map_err(|e| HarnessError::internal(format!("os randomness unavailable: {e}")))?;
    Ok(buf)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_roundtrip_and_known_digest() {
        assert_eq!(to_hex(&[0, 255, 16]), "00ff10");
        assert_eq!(from_hex("00ff10").unwrap(), vec![0, 255, 16]);
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn canonical_json_is_key_order_independent() {
        let a: serde_json::Value = serde_json::from_str(r#"{"b":1,"a":2}"#).unwrap();
        let b: serde_json::Value = serde_json::from_str(r#"{"a":2,"b":1}"#).unwrap();
        assert_eq!(canonical_json_hash(&a), canonical_json_hash(&b));
    }

    #[test]
    fn random_bytes_differ() {
        assert_ne!(random_bytes::<32>().unwrap(), random_bytes::<32>().unwrap());
    }
}
