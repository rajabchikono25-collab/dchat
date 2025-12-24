//! DPL manifest for WASM custom sections

/// Magic bytes for DPL manifest: "DPLM"
pub const DPL_MANIFEST_MAGIC: [u8; 4] = [0x44, 0x50, 0x4C, 0x4D];

/// Custom section name for DPL manifest
pub const DPL_MANIFEST_SECTION_NAME: &str = "dpl_manifest";

/// DPL manifest size in bytes
pub const DPL_MANIFEST_SIZE: usize = 64;

/// DPL manifest embedded in WASM custom section
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct DplManifest {
    /// Magic bytes: "DPLM"
    pub magic: [u8; 4],
    /// SDK major version
    pub sdk_major: u16,
    /// SDK minor version
    pub sdk_minor: u16,
    /// SDK patch version
    pub sdk_patch: u16,
    /// DPL edition year (e.g., 2025)
    pub edition: u16,
    /// ABI version for compatibility
    pub abi_version: u8,
    /// Import profile: 0=legacy, 1=wasi, 2=hybrid
    pub import_profile: u8,
    /// Reserved bytes
    pub reserved1: [u8; 2],
    /// BLAKE3 hash of instruction schema (IDL)
    pub schema_hash: [u8; 32],
    /// Capability flags
    pub capabilities: u64,
    /// Reserved bytes
    pub reserved2: [u8; 8],
}

impl Default for DplManifest {
    fn default() -> Self {
        Self {
            magic: DPL_MANIFEST_MAGIC,
            sdk_major: crate::DPL_VERSION_MAJOR,
            sdk_minor: crate::DPL_VERSION_MINOR,
            sdk_patch: crate::DPL_VERSION_PATCH,
            edition: crate::DPL_EDITION,
            abi_version: crate::ABI_VERSION,
            import_profile: 1, // WASI by default
            reserved1: [0; 2],
            schema_hash: [0; 32],
            capabilities: 0,
            reserved2: [0; 8],
        }
    }
}

impl DplManifest {
    /// Create manifest with schema hash
    pub fn with_schema_hash(mut self, hash: [u8; 32]) -> Self {
        self.schema_hash = hash;
        self
    }

    /// Create manifest with capabilities
    pub fn with_capabilities(mut self, caps: u64) -> Self {
        self.capabilities = caps;
        self
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> [u8; DPL_MANIFEST_SIZE] {
        let mut bytes = [0u8; DPL_MANIFEST_SIZE];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4..6].copy_from_slice(&self.sdk_major.to_le_bytes());
        bytes[6..8].copy_from_slice(&self.sdk_minor.to_le_bytes());
        bytes[8..10].copy_from_slice(&self.sdk_patch.to_le_bytes());
        bytes[10..12].copy_from_slice(&self.edition.to_le_bytes());
        bytes[12] = self.abi_version;
        bytes[13] = self.import_profile;
        bytes[14..16].copy_from_slice(&self.reserved1);
        bytes[16..48].copy_from_slice(&self.schema_hash);
        bytes[48..56].copy_from_slice(&self.capabilities.to_le_bytes());
        bytes[56..64].copy_from_slice(&self.reserved2);
        bytes
    }
}

/// Capability flags
pub mod capabilities {
    /// Program emits events
    pub const EMITS_EVENTS: u64 = 1 << 0;
    /// Program uses CPI
    pub const USES_CPI: u64 = 1 << 1;
    /// Program uses PDAs
    pub const USES_PDAS: u64 = 1 << 2;
    /// Program requires signers
    pub const REQUIRES_SIGNERS: u64 = 1 << 3;
    /// Program uses tokens
    pub const USES_TOKENS: u64 = 1 << 4;
    /// Program uses privacy features
    pub const USES_PRIVACY: u64 = 1 << 5;
    /// Program uses capabilities
    pub const USES_CAPABILITIES: u64 = 1 << 6;
    /// Program is upgradeable
    pub const UPGRADEABLE: u64 = 1 << 7;
}
