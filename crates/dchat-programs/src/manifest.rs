//! DPL Manifest Parser for WASM Custom Sections
//!
//! This module parses the "dpl_manifest" custom section embedded in DPL programs,
//! which contains metadata about the program's SDK version, capabilities, and
//! schema information.
//!
//! # Manifest Format (64 bytes)
//!
//! | Offset | Size | Field                | Description                              |
//! |--------|------|----------------------|------------------------------------------|
//! | 0      | 4    | Magic                | "DPLM" (0x44, 0x50, 0x4C, 0x4D)          |
//! | 4      | 2    | SDK Major Version    | Major version of dchat-dpl SDK           |
//! | 6      | 2    | SDK Minor Version    | Minor version of dchat-dpl SDK           |
//! | 8      | 2    | SDK Patch Version    | Patch version of dchat-dpl SDK           |
//! | 10     | 2    | Edition              | DPL edition (2025 = 0x07E9)              |
//! | 12     | 1    | ABI Version          | ABI version for compatibility            |
//! | 13     | 1    | Import Profile       | 0=legacy, 1=wasi, 2=hybrid               |
//! | 14     | 2    | Reserved             | Must be zero                             |
//! | 16     | 32   | Schema Hash          | BLAKE3 hash of instruction schema (IDL)  |
//! | 48     | 8    | Capabilities         | Bitflags for program capabilities        |
//! | 56     | 8    | Reserved             | Must be zero                             |

use serde::{Deserialize, Serialize};

/// Magic bytes for DPL manifest: "DPLM"
pub const DPL_MANIFEST_MAGIC: [u8; 4] = [0x44, 0x50, 0x4C, 0x4D];

/// Custom section name for DPL manifest
pub const DPL_MANIFEST_SECTION_NAME: &str = "dpl_manifest";

/// DPL manifest size in bytes
pub const DPL_MANIFEST_SIZE: usize = 64;

/// Import profile indicating which host functions the program expects
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum ImportProfile {
    /// Legacy wasm32-unknown-unknown (env module only)
    Legacy = 0,
    /// WASI-based (wasi_snapshot_preview1 module)
    Wasi = 1,
    /// Hybrid (both env and wasi modules)
    Hybrid = 2,
}

impl TryFrom<u8> for ImportProfile {
    type Error = ManifestError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        match value {
            0 => Ok(ImportProfile::Legacy),
            1 => Ok(ImportProfile::Wasi),
            2 => Ok(ImportProfile::Hybrid),
            _ => Err(ManifestError::InvalidImportProfile(value)),
        }
    }
}

bitflags::bitflags! {
    /// Program capabilities declared in manifest
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    pub struct Capabilities: u64 {
        /// Program emits events
        const EMITS_EVENTS = 1 << 0;
        /// Program uses CPI (cross-program invocation)
        const USES_CPI = 1 << 1;
        /// Program uses PDAs (program derived addresses)
        const USES_PDAS = 1 << 2;
        /// Program requires signer verification
        const REQUIRES_SIGNERS = 1 << 3;
        /// Program uses token operations
        const USES_TOKENS = 1 << 4;
        /// Program uses privacy features
        const USES_PRIVACY = 1 << 5;
        /// Program uses capability tokens
        const USES_CAPABILITIES = 1 << 6;
        /// Program has upgrade authority
        const UPGRADEABLE = 1 << 7;
        /// Program uses governance features
        const USES_GOVERNANCE = 1 << 8;
        /// Program uses staking features
        const USES_STAKING = 1 << 9;
    }
}

// Manual serde implementation for Capabilities
impl Serialize for Capabilities {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_u64(self.bits())
    }
}

impl<'de> Deserialize<'de> for Capabilities {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let bits = u64::deserialize(deserializer)?;
        Ok(Capabilities::from_bits_truncate(bits))
    }
}

/// Parsed DPL manifest from WASM custom section
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DplManifest {
    /// SDK major version
    pub sdk_major: u16,
    /// SDK minor version
    pub sdk_minor: u16,
    /// SDK patch version
    pub sdk_patch: u16,
    /// DPL edition year
    pub edition: u16,
    /// ABI version for compatibility
    pub abi_version: u8,
    /// Import profile (legacy/wasi/hybrid)
    pub import_profile: ImportProfile,
    /// BLAKE3 hash of instruction schema
    pub schema_hash: [u8; 32],
    /// Program capabilities
    pub capabilities: Capabilities,
}

impl DplManifest {
    /// Parse manifest from raw bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self, ManifestError> {
        if data.len() != DPL_MANIFEST_SIZE {
            return Err(ManifestError::InvalidSize {
                expected: DPL_MANIFEST_SIZE,
                actual: data.len(),
            });
        }

        // Check magic
        if &data[0..4] != &DPL_MANIFEST_MAGIC {
            return Err(ManifestError::InvalidMagic);
        }

        // Parse fields
        let sdk_major = u16::from_le_bytes([data[4], data[5]]);
        let sdk_minor = u16::from_le_bytes([data[6], data[7]]);
        let sdk_patch = u16::from_le_bytes([data[8], data[9]]);
        let edition = u16::from_le_bytes([data[10], data[11]]);
        let abi_version = data[12];
        let import_profile = ImportProfile::try_from(data[13])?;

        // Check reserved bytes
        if data[14] != 0 || data[15] != 0 {
            return Err(ManifestError::ReservedNotZero);
        }

        // Parse schema hash
        let mut schema_hash = [0u8; 32];
        schema_hash.copy_from_slice(&data[16..48]);

        // Parse capabilities
        let cap_bits = u64::from_le_bytes([
            data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
        ]);
        let capabilities = Capabilities::from_bits_truncate(cap_bits);

        // Check remaining reserved bytes
        for i in 56..64 {
            if data[i] != 0 {
                return Err(ManifestError::ReservedNotZero);
            }
        }

        Ok(Self {
            sdk_major,
            sdk_minor,
            sdk_patch,
            edition,
            abi_version,
            import_profile,
            schema_hash,
            capabilities,
        })
    }

    /// Serialize manifest to bytes
    pub fn to_bytes(&self) -> [u8; DPL_MANIFEST_SIZE] {
        let mut data = [0u8; DPL_MANIFEST_SIZE];

        // Magic
        data[0..4].copy_from_slice(&DPL_MANIFEST_MAGIC);

        // Version info
        data[4..6].copy_from_slice(&self.sdk_major.to_le_bytes());
        data[6..8].copy_from_slice(&self.sdk_minor.to_le_bytes());
        data[8..10].copy_from_slice(&self.sdk_patch.to_le_bytes());
        data[10..12].copy_from_slice(&self.edition.to_le_bytes());
        data[12] = self.abi_version;
        data[13] = self.import_profile as u8;
        // Reserved [14..16] already zero

        // Schema hash
        data[16..48].copy_from_slice(&self.schema_hash);

        // Capabilities
        data[48..56].copy_from_slice(&self.capabilities.bits().to_le_bytes());

        // Reserved [56..64] already zero

        data
    }

    /// Get formatted SDK version string
    pub fn sdk_version_string(&self) -> String {
        format!("{}.{}.{}", self.sdk_major, self.sdk_minor, self.sdk_patch)
    }

    /// Check if program is compatible with current runtime
    pub fn is_compatible(&self, runtime_abi_version: u8) -> bool {
        self.abi_version <= runtime_abi_version
    }
}

/// Errors from manifest parsing
#[derive(Debug, Clone, thiserror::Error)]
pub enum ManifestError {
    /// Invalid manifest size
    #[error("Invalid manifest size: expected {expected}, got {actual}")]
    InvalidSize {
        /// Expected size
        expected: usize,
        /// Actual size
        actual: usize,
    },

    /// Invalid magic bytes
    #[error("Invalid manifest magic bytes (expected DPLM)")]
    InvalidMagic,

    /// Reserved bytes not zero
    #[error("Reserved bytes must be zero")]
    ReservedNotZero,

    /// Invalid import profile value
    #[error("Invalid import profile: {0}")]
    InvalidImportProfile(u8),

    /// Custom section not found
    #[error("DPL manifest custom section not found")]
    NotFound,

    /// Parse error
    #[error("Failed to parse manifest: {0}")]
    ParseError(String),
}

/// Extract DPL manifest from WASM bytecode
///
/// Parses the WASM binary to find and extract the "dpl_manifest" custom section.
pub fn extract_manifest(bytecode: &[u8]) -> Result<Option<DplManifest>, ManifestError> {
    // Quick validation
    if bytecode.len() < 8 {
        return Err(ManifestError::ParseError("Bytecode too short".into()));
    }

    // Check WASM magic
    if &bytecode[0..4] != &[0x00, 0x61, 0x73, 0x6d] {
        return Err(ManifestError::ParseError("Invalid WASM magic".into()));
    }

    // Parse sections looking for custom section with our name
    let mut offset = 8; // Skip magic and version

    while offset < bytecode.len() {
        // Read section ID
        let section_id = bytecode[offset];
        offset += 1;

        // Read section size (LEB128)
        let (section_size, bytes_read) = read_leb128_u32(&bytecode[offset..])?;
        offset += bytes_read;

        if section_id == 0 {
            // Custom section - check name
            let section_start = offset;
            let (name_len, name_bytes_read) = read_leb128_u32(&bytecode[offset..])?;
            offset += name_bytes_read;

            if offset + name_len as usize > bytecode.len() {
                return Err(ManifestError::ParseError("Section name overflow".into()));
            }

            let name = &bytecode[offset..offset + name_len as usize];
            offset += name_len as usize;

            if name == DPL_MANIFEST_SECTION_NAME.as_bytes() {
                // Found our manifest section
                let data_len = section_size as usize - (offset - section_start);
                if offset + data_len > bytecode.len() {
                    return Err(ManifestError::ParseError("Manifest data overflow".into()));
                }

                let manifest_data = &bytecode[offset..offset + data_len];
                return Ok(Some(DplManifest::from_bytes(manifest_data)?));
            }

            // Skip rest of this custom section
            let remaining = section_size as usize - (offset - section_start);
            offset += remaining;
        } else {
            // Skip non-custom section
            offset += section_size as usize;
        }
    }

    // No manifest found - this is OK for legacy contracts
    Ok(None)
}

/// Read LEB128-encoded u32
fn read_leb128_u32(data: &[u8]) -> Result<(u32, usize), ManifestError> {
    let mut result: u32 = 0;
    let mut shift = 0;
    let mut bytes_read = 0;

    for &byte in data.iter() {
        bytes_read += 1;
        result |= ((byte & 0x7f) as u32) << shift;

        if byte & 0x80 == 0 {
            return Ok((result, bytes_read));
        }

        shift += 7;
        if shift >= 32 {
            return Err(ManifestError::ParseError("LEB128 overflow".into()));
        }
    }

    Err(ManifestError::ParseError("Unexpected end of LEB128".into()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_roundtrip() {
        let manifest = DplManifest {
            sdk_major: 0,
            sdk_minor: 1,
            sdk_patch: 0,
            edition: 2025,
            abi_version: 1,
            import_profile: ImportProfile::Wasi,
            schema_hash: [0x42; 32],
            capabilities: Capabilities::EMITS_EVENTS | Capabilities::USES_PDAS,
        };

        let bytes = manifest.to_bytes();
        let parsed = DplManifest::from_bytes(&bytes).unwrap();

        assert_eq!(parsed.sdk_major, 0);
        assert_eq!(parsed.sdk_minor, 1);
        assert_eq!(parsed.sdk_patch, 0);
        assert_eq!(parsed.edition, 2025);
        assert_eq!(parsed.abi_version, 1);
        assert_eq!(parsed.import_profile, ImportProfile::Wasi);
        assert_eq!(parsed.schema_hash, [0x42; 32]);
        assert!(parsed.capabilities.contains(Capabilities::EMITS_EVENTS));
        assert!(parsed.capabilities.contains(Capabilities::USES_PDAS));
    }

    #[test]
    fn test_manifest_version_string() {
        let manifest = DplManifest {
            sdk_major: 1,
            sdk_minor: 2,
            sdk_patch: 3,
            edition: 2025,
            abi_version: 1,
            import_profile: ImportProfile::Legacy,
            schema_hash: [0; 32],
            capabilities: Capabilities::empty(),
        };

        assert_eq!(manifest.sdk_version_string(), "1.2.3");
    }

    #[test]
    fn test_invalid_magic() {
        let mut data = [0u8; 64];
        data[0..4].copy_from_slice(b"XXXX");

        assert!(matches!(
            DplManifest::from_bytes(&data),
            Err(ManifestError::InvalidMagic)
        ));
    }

    #[test]
    fn test_invalid_size() {
        let data = [0u8; 32];
        assert!(matches!(
            DplManifest::from_bytes(&data),
            Err(ManifestError::InvalidSize { .. })
        ));
    }
}
