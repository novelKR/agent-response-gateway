//! Shared SHA-256 primitive. A digest alone is not authentication or provenance.
pub fn sha256(bytes: &[u8]) -> [u8; 32] {
    ring::digest::digest(&ring::digest::SHA256, bytes)
        .as_ref()
        .try_into()
        .expect("SHA-256 length")
}
