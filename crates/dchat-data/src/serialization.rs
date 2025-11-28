//! Serialization utilities for efficient data encoding
//!
//! Provides a unified interface for serializing/deserializing data
//! with support for multiple formats optimized for different use cases.

use serde::{de::DeserializeOwned, Deserialize, Serialize};

use crate::error::{DataError, DataResult};

/// Available serialization formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DataFormat {
    /// Binary format (bincode) - fast and compact
    Binary,
    /// JSON format - human readable, interoperable
    Json,
    /// Compact JSON (no pretty printing)
    JsonCompact,
}

impl DataFormat {
    /// Get file extension for this format
    pub fn extension(&self) -> &'static str {
        match self {
            DataFormat::Binary => "bin",
            DataFormat::Json => "json",
            DataFormat::JsonCompact => "json",
        }
    }

    /// Get MIME type for this format
    pub fn mime_type(&self) -> &'static str {
        match self {
            DataFormat::Binary => "application/octet-stream",
            DataFormat::Json | DataFormat::JsonCompact => "application/json",
        }
    }
}

impl Default for DataFormat {
    fn default() -> Self {
        DataFormat::Binary
    }
}

/// Trait for types that can be serialized/deserialized
pub trait Serializable: Serialize + DeserializeOwned + Sized {
    /// Serialize to bytes using the specified format
    fn serialize_to(&self, format: DataFormat) -> DataResult<Vec<u8>> {
        match format {
            DataFormat::Binary => {
                bincode::serialize(self).map_err(|e| DataError::Serialization(e.to_string()))
            }
            DataFormat::Json => serde_json::to_vec_pretty(self)
                .map_err(|e| DataError::Serialization(e.to_string())),
            DataFormat::JsonCompact => {
                serde_json::to_vec(self).map_err(|e| DataError::Serialization(e.to_string()))
            }
        }
    }

    /// Deserialize from bytes using the specified format
    fn deserialize_from(bytes: &[u8], format: DataFormat) -> DataResult<Self> {
        match format {
            DataFormat::Binary => {
                bincode::deserialize(bytes).map_err(|e| DataError::Serialization(e.to_string()))
            }
            DataFormat::Json | DataFormat::JsonCompact => {
                serde_json::from_slice(bytes).map_err(|e| DataError::Serialization(e.to_string()))
            }
        }
    }

    /// Serialize to binary (default format)
    fn to_bytes(&self) -> DataResult<Vec<u8>> {
        self.serialize_to(DataFormat::Binary)
    }

    /// Deserialize from binary
    fn from_bytes(bytes: &[u8]) -> DataResult<Self> {
        Self::deserialize_from(bytes, DataFormat::Binary)
    }

    /// Serialize to JSON string
    fn to_json(&self) -> DataResult<String> {
        serde_json::to_string_pretty(self).map_err(|e| DataError::Serialization(e.to_string()))
    }

    /// Deserialize from JSON string
    fn from_json(json: &str) -> DataResult<Self> {
        serde_json::from_str(json).map_err(|e| DataError::Serialization(e.to_string()))
    }
}

// Implement Serializable for any type that implements Serialize + DeserializeOwned
impl<T: Serialize + DeserializeOwned> Serializable for T {}

/// Compute size of serialized data without fully serializing
pub fn serialized_size<T: Serialize>(value: &T, format: DataFormat) -> DataResult<usize> {
    match format {
        DataFormat::Binary => bincode::serialized_size(value)
            .map(|s| s as usize)
            .map_err(|e| DataError::Serialization(e.to_string())),
        DataFormat::Json | DataFormat::JsonCompact => {
            // For JSON, we need to actually serialize to get the size
            let bytes = if format == DataFormat::Json {
                serde_json::to_vec_pretty(value)
            } else {
                serde_json::to_vec(value)
            }
            .map_err(|e| DataError::Serialization(e.to_string()))?;
            Ok(bytes.len())
        }
    }
}

/// Wrapper for versioned serialization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Versioned<T> {
    /// Version number for format compatibility
    pub version: u32,
    /// The actual data
    pub data: T,
}

impl<T> Versioned<T> {
    /// Current version for new data
    pub const CURRENT_VERSION: u32 = 1;

    /// Create a new versioned wrapper with current version
    pub fn new(data: T) -> Self {
        Self {
            version: Self::CURRENT_VERSION,
            data,
        }
    }

    /// Create with a specific version
    pub fn with_version(version: u32, data: T) -> Self {
        Self { version, data }
    }

    /// Check if this is the current version
    pub fn is_current(&self) -> bool {
        self.version == Self::CURRENT_VERSION
    }

    /// Extract the inner data
    pub fn into_inner(self) -> T {
        self.data
    }
}

/// Wrapper for data with integrity verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Verified<T> {
    /// The actual data
    pub data: T,
    /// BLAKE3 hash of serialized data
    pub hash: [u8; 32],
}

impl<T: Serialize + DeserializeOwned> Verified<T> {
    /// Create a verified wrapper
    pub fn new(data: T) -> DataResult<Self> {
        let bytes = bincode::serialize(&data)
            .map_err(|e| DataError::Serialization(e.to_string()))?;
        let hash = *blake3::hash(&bytes).as_bytes();
        Ok(Self { data, hash })
    }

    /// Verify the data integrity
    pub fn verify(&self) -> DataResult<bool> {
        let bytes = bincode::serialize(&self.data)
            .map_err(|e| DataError::Serialization(e.to_string()))?;
        let computed_hash = *blake3::hash(&bytes).as_bytes();
        Ok(computed_hash == self.hash)
    }

    /// Verify and extract the data
    pub fn into_verified(self) -> DataResult<T> {
        if self.verify()? {
            Ok(self.data)
        } else {
            Err(DataError::Integrity("Data verification failed".to_string()))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    struct TestData {
        name: String,
        value: u64,
    }

    #[test]
    fn test_binary_roundtrip() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let bytes = data.to_bytes().unwrap();
        let recovered: TestData = TestData::from_bytes(&bytes).unwrap();
        assert_eq!(data, recovered);
    }

    #[test]
    fn test_json_roundtrip() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        let json = data.to_json().unwrap();
        let recovered: TestData = TestData::from_json(&json).unwrap();
        assert_eq!(data, recovered);
    }

    #[test]
    fn test_format_conversion() {
        let data = TestData {
            name: "test".to_string(),
            value: 42,
        };

        // Binary is more compact
        let binary = data.serialize_to(DataFormat::Binary).unwrap();
        let json = data.serialize_to(DataFormat::JsonCompact).unwrap();
        assert!(binary.len() < json.len());

        // Both deserialize correctly
        let from_binary: TestData = TestData::deserialize_from(&binary, DataFormat::Binary).unwrap();
        let from_json: TestData = TestData::deserialize_from(&json, DataFormat::JsonCompact).unwrap();
        assert_eq!(from_binary, from_json);
    }

    #[test]
    fn test_versioned() {
        let data = TestData {
            name: "versioned".to_string(),
            value: 100,
        };

        let versioned = Versioned::new(data.clone());
        assert!(versioned.is_current());
        assert_eq!(versioned.into_inner(), data);
    }

    #[test]
    fn test_verified() {
        let data = TestData {
            name: "verified".to_string(),
            value: 200,
        };

        let verified = Verified::new(data.clone()).unwrap();
        assert!(verified.verify().unwrap());

        let recovered = verified.into_verified().unwrap();
        assert_eq!(recovered, data);
    }

    #[test]
    fn test_verified_tampered() {
        let data = TestData {
            name: "original".to_string(),
            value: 100,
        };

        let mut verified = Verified::new(data).unwrap();
        // Tamper with the data
        verified.data.name = "tampered".to_string();

        assert!(!verified.verify().unwrap());
        assert!(verified.into_verified().is_err());
    }

    #[test]
    fn test_serialized_size() {
        let data = TestData {
            name: "size test".to_string(),
            value: 12345,
        };

        let binary_size = serialized_size(&data, DataFormat::Binary).unwrap();
        let binary_actual = data.serialize_to(DataFormat::Binary).unwrap();
        assert_eq!(binary_size, binary_actual.len());
    }

    #[test]
    fn test_format_extension() {
        assert_eq!(DataFormat::Binary.extension(), "bin");
        assert_eq!(DataFormat::Json.extension(), "json");
    }

    #[test]
    fn test_format_mime_type() {
        assert_eq!(DataFormat::Binary.mime_type(), "application/octet-stream");
        assert_eq!(DataFormat::Json.mime_type(), "application/json");
    }
}
