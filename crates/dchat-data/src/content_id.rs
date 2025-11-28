//! Content-addressable identifiers using BLAKE3 hashing
//!
//! Provides a consistent way to generate unique identifiers for content
//! based on cryptographic hashes, enabling deduplication and integrity verification.

use serde::{Deserialize, Serialize};
use std::fmt;

/// Error type for content ID operations
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContentIdError(pub String);

impl fmt::Display for ContentIdError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentIdError: {}", self.0)
    }
}

impl std::error::Error for ContentIdError {}

/// Content-addressable identifier based on BLAKE3 hash
///
/// ContentId is a 32-byte hash that uniquely identifies content.
/// Two identical pieces of content will always have the same ContentId.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ContentId([u8; 32]);

impl ContentId {
    /// Size of the content ID in bytes
    pub const SIZE: usize = 32;

    /// Create a ContentId from raw bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Compute ContentId from arbitrary data
    pub fn from_data(data: &[u8]) -> Self {
        let hash = blake3::hash(data);
        Self(*hash.as_bytes())
    }

    /// Compute ContentId from multiple data chunks
    pub fn from_chunks(chunks: &[&[u8]]) -> Self {
        let mut hasher = blake3::Hasher::new();
        for chunk in chunks {
            hasher.update(chunk);
        }
        Self(*hasher.finalize().as_bytes())
    }

    /// Parse ContentId from hex string
    pub fn from_hex(hex_str: &str) -> Result<Self, ContentIdError> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| ContentIdError(format!("Invalid hex: {}", e)))?;
        
        if bytes.len() != Self::SIZE {
            return Err(ContentIdError(format!(
                "Invalid length: expected {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Parse ContentId from base64 string
    pub fn from_base64(b64_str: &str) -> Result<Self, ContentIdError> {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(b64_str)
            .map_err(|e| ContentIdError(format!("Invalid base64: {}", e)))?;

        if bytes.len() != Self::SIZE {
            return Err(ContentIdError(format!(
                "Invalid length: expected {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            )));
        }

        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        Ok(Self(arr))
    }

    /// Get the raw bytes of the content ID
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Convert to base64 string
    pub fn to_base64(&self) -> String {
        use base64::Engine;
        base64::engine::general_purpose::STANDARD.encode(self.0)
    }

    /// Verify that data matches this ContentId
    pub fn verify(&self, data: &[u8]) -> bool {
        let computed = Self::from_data(data);
        computed == *self
    }

    /// Returns true if the ContentId is all zeros (null ID)
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }

    /// Create a null (all zeros) ContentId
    pub fn zero() -> Self {
        Self([0u8; 32])
    }
}

impl fmt::Debug for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ContentId({})", &self.to_hex()[..16])
    }
}

impl fmt::Display for ContentId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex())
    }
}

impl AsRef<[u8]> for ContentId {
    fn as_ref(&self) -> &[u8] {
        &self.0
    }
}

impl From<[u8; 32]> for ContentId {
    fn from(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }
}

impl TryFrom<&[u8]> for ContentId {
    type Error = ContentIdError;

    fn try_from(bytes: &[u8]) -> Result<Self, Self::Error> {
        if bytes.len() != Self::SIZE {
            return Err(ContentIdError(format!(
                "Invalid length: expected {} bytes, got {}",
                Self::SIZE,
                bytes.len()
            )));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self(arr))
    }
}

/// Builder for computing ContentId incrementally
pub struct ContentIdBuilder {
    hasher: blake3::Hasher,
}

impl ContentIdBuilder {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            hasher: blake3::Hasher::new(),
        }
    }

    /// Add data to the hash computation
    pub fn update(&mut self, data: &[u8]) -> &mut Self {
        self.hasher.update(data);
        self
    }

    /// Finalize and produce the ContentId
    pub fn finalize(self) -> ContentId {
        ContentId(*self.hasher.finalize().as_bytes())
    }
}

impl Default for ContentIdBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_content_id_deterministic() {
        let data = b"hello world";
        let id1 = ContentId::from_data(data);
        let id2 = ContentId::from_data(data);
        assert_eq!(id1, id2);
    }

    #[test]
    fn test_content_id_different_data() {
        let id1 = ContentId::from_data(b"hello");
        let id2 = ContentId::from_data(b"world");
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_content_id_hex_roundtrip() {
        let data = b"test data";
        let id = ContentId::from_data(data);
        let hex = id.to_hex();
        let parsed = ContentId::from_hex(&hex).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_content_id_base64_roundtrip() {
        let data = b"test data";
        let id = ContentId::from_data(data);
        let b64 = id.to_base64();
        let parsed = ContentId::from_base64(&b64).unwrap();
        assert_eq!(id, parsed);
    }

    #[test]
    fn test_content_id_verify() {
        let data = b"verify me";
        let id = ContentId::from_data(data);
        assert!(id.verify(data));
        assert!(!id.verify(b"wrong data"));
    }

    #[test]
    fn test_content_id_builder() {
        let mut builder = ContentIdBuilder::new();
        builder.update(b"hello ");
        builder.update(b"world");
        let id = builder.finalize();

        let direct = ContentId::from_data(b"hello world");
        assert_eq!(id, direct);
    }

    #[test]
    fn test_content_id_zero() {
        let zero = ContentId::zero();
        assert!(zero.is_zero());

        let non_zero = ContentId::from_data(b"data");
        assert!(!non_zero.is_zero());
    }

    #[test]
    fn test_content_id_from_chunks() {
        let id = ContentId::from_chunks(&[b"hello ", b"world"]);
        let direct = ContentId::from_data(b"hello world");
        assert_eq!(id, direct);
    }
}
