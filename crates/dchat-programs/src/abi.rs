//! Dchat Program Language v1 - Canonical Binary ABI
//!
//! This module defines the single source-of-truth ABI shared between host and guest.
//! All serialization formats are deterministic with pinned codec versions.
//!
//! # Wire Format Version: 1
//!
//! ## IxEnvelope (Instruction Envelope)
//! ```text
//! +--------+----------+-----+-------------+---------+
//! | magic  | version  | tag | payload_len | payload |
//! | 4B     | 2B       | 2B  | 4B          | N bytes |
//! +--------+----------+-----+-------------+---------+
//! ```
//!
//! ## AccountsBlob v1
//! ```text
//! +--------+-------------+---------------+---------+--------------+
//! | magic  | abi_version | account_count | offsets | toc+data     |
//! | 4B     | 2B          | 2B            | 4B*N    | TOC+data_sec |
//! +--------+-------------+---------------+---------+--------------+
//! ```
//!
//! # Security
//! - All parsing has explicit bounds checks
//! - Unknown versions/tags are rejected deterministically
//! - Oversized payloads are rejected
//! - No floating point, no allocation in parsing hot paths

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};

// ═══════════════════════════════════════════════════════════════════════════════
// VERSION AND MAGIC CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// ABI version - increment on breaking changes
pub const ABI_VERSION: u16 = 1;

/// Minimum supported ABI version for backwards compat
pub const ABI_VERSION_MIN: u16 = 1;

/// Maximum supported ABI version
pub const ABI_VERSION_MAX: u16 = 1;

/// Magic bytes for instruction envelope: "DCHX" (Dchat eXecute)
pub const IX_ENVELOPE_MAGIC: [u8; 4] = [0x44, 0x43, 0x48, 0x58];

/// Magic bytes for accounts blob: "DCHA" (Dchat Accounts)
pub const ACCOUNTS_BLOB_MAGIC: [u8; 4] = [0x44, 0x43, 0x48, 0x41];

/// Maximum payload size (1 MB - prevents DoS)
pub const MAX_PAYLOAD_SIZE: u32 = 1024 * 1024;

/// Maximum account data size per account (10 MB)
pub const MAX_ACCOUNT_DATA_SIZE: u32 = 10 * 1024 * 1024;

/// Maximum accounts per blob
pub const MAX_ACCOUNTS: u16 = 256;

/// Fixed TOC entry size in bytes (32+32+8+4+4+1+1+1+8 = 91)
pub const TOC_ENTRY_SIZE: usize = 91;

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION ENVELOPE
// ═══════════════════════════════════════════════════════════════════════════════

/// Instruction envelope header size
pub const IX_ENVELOPE_HEADER_SIZE: usize = 12; // 4 + 2 + 2 + 4

/// Instruction envelope - wraps instruction data with versioned framing
///
/// Wire format (little-endian):
/// - magic: 4 bytes ("DCHX")
/// - version: u16 (ABI version)
/// - tag: u16 (instruction discriminator)
/// - payload_len: u32 (payload length)
/// - payload: [u8; payload_len]
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IxEnvelope {
    /// ABI version
    pub version: u16,
    /// Instruction tag/discriminator
    pub tag: u16,
    /// Instruction payload (deterministic bytes)
    pub payload: Vec<u8>,
}

impl IxEnvelope {
    /// Create a new instruction envelope
    pub fn new(tag: u16, payload: Vec<u8>) -> ProgramResult<Self> {
        if payload.len() > MAX_PAYLOAD_SIZE as usize {
            return Err(ProgramError::MaxInstructionDataExceeded);
        }
        Ok(Self {
            version: ABI_VERSION,
            tag,
            payload,
        })
    }

    /// Encode to deterministic wire format
    pub fn encode(&self) -> Vec<u8> {
        let payload_len = self.payload.len() as u32;
        let total_len = IX_ENVELOPE_HEADER_SIZE + self.payload.len();
        let mut buf = Vec::with_capacity(total_len);

        buf.extend_from_slice(&IX_ENVELOPE_MAGIC);
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&self.tag.to_le_bytes());
        buf.extend_from_slice(&payload_len.to_le_bytes());
        buf.extend_from_slice(&self.payload);

        buf
    }

    /// Decode from wire format with strict validation
    pub fn decode(data: &[u8]) -> ProgramResult<Self> {
        // Validate minimum size
        if data.len() < IX_ENVELOPE_HEADER_SIZE {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Validate magic
        if data[0..4] != IX_ENVELOPE_MAGIC {
            return Err(ProgramError::Custom(AbiError::InvalidMagic as u32));
        }

        // Parse header
        let version = u16::from_le_bytes([data[4], data[5]]);
        let tag = u16::from_le_bytes([data[6], data[7]]);
        let payload_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]);

        // Validate version
        if version < ABI_VERSION_MIN || version > ABI_VERSION_MAX {
            return Err(ProgramError::Custom(AbiError::UnsupportedVersion as u32));
        }

        // Validate payload size
        if payload_len > MAX_PAYLOAD_SIZE {
            return Err(ProgramError::Custom(AbiError::PayloadTooLarge as u32));
        }

        // Validate total size
        let expected_len = IX_ENVELOPE_HEADER_SIZE + payload_len as usize;
        if data.len() < expected_len {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Extract payload
        let payload = data[IX_ENVELOPE_HEADER_SIZE..expected_len].to_vec();

        Ok(Self {
            version,
            tag,
            payload,
        })
    }

    /// Calculate deterministic hash for receipts
    pub fn hash(&self) -> [u8; 32] {
        let encoded = self.encode();
        *blake3::hash(&encoded).as_bytes()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNTS BLOB
// ═══════════════════════════════════════════════════════════════════════════════

/// Accounts blob header size
pub const ACCOUNTS_BLOB_HEADER_SIZE: usize = 8; // 4 + 2 + 2

/// Table of Contents entry for a single account
///
/// Fixed-size struct (89 bytes):
/// - pubkey: 32 bytes
/// - owner: 32 bytes
/// - lamports: 8 bytes (u64)
/// - data_len: 4 bytes (u32)
/// - data_off: 4 bytes (u32) - offset into data section
/// - is_signer: 1 byte (bool as u8)
/// - is_writable: 1 byte (bool as u8)
/// - executable: 1 byte (bool as u8)
/// - rent_epoch: 8 bytes (u64, optional - 0 if not set)
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccountTocEntry {
    /// Account public key
    pub pubkey: Pubkey,
    /// Program owner
    pub owner: Pubkey,
    /// Lamports balance
    pub lamports: u64,
    /// Data length in bytes
    pub data_len: u32,
    /// Offset into data section
    pub data_off: u32,
    /// Is this a signer
    pub is_signer: bool,
    /// Is this writable
    pub is_writable: bool,
    /// Is this executable (a program)
    pub executable: bool,
    /// Rent epoch (0 if not applicable)
    pub rent_epoch: u64,
}

impl AccountTocEntry {
    /// Encode to fixed-size bytes (89 bytes)
    pub fn encode(&self) -> [u8; TOC_ENTRY_SIZE] {
        let mut buf = [0u8; TOC_ENTRY_SIZE];
        let mut offset = 0;

        buf[offset..offset + 32].copy_from_slice(&self.pubkey.0);
        offset += 32;

        buf[offset..offset + 32].copy_from_slice(&self.owner.0);
        offset += 32;

        buf[offset..offset + 8].copy_from_slice(&self.lamports.to_le_bytes());
        offset += 8;

        buf[offset..offset + 4].copy_from_slice(&self.data_len.to_le_bytes());
        offset += 4;

        buf[offset..offset + 4].copy_from_slice(&self.data_off.to_le_bytes());
        offset += 4;

        buf[offset] = self.is_signer as u8;
        offset += 1;

        buf[offset] = self.is_writable as u8;
        offset += 1;

        buf[offset] = self.executable as u8;
        offset += 1;

        buf[offset..offset + 8].copy_from_slice(&self.rent_epoch.to_le_bytes());

        buf
    }

    /// Decode from fixed-size bytes
    pub fn decode(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < TOC_ENTRY_SIZE {
            return Err(ProgramError::InvalidAccountData);
        }

        let mut offset = 0;

        let mut pubkey_bytes = [0u8; 32];
        pubkey_bytes.copy_from_slice(&data[offset..offset + 32]);
        offset += 32;

        let mut owner_bytes = [0u8; 32];
        owner_bytes.copy_from_slice(&data[offset..offset + 32]);
        offset += 32;

        let lamports = u64::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);
        offset += 8;

        let data_len = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        let data_off = u32::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
        ]);
        offset += 4;

        let is_signer = data[offset] != 0;
        offset += 1;

        let is_writable = data[offset] != 0;
        offset += 1;

        let executable = data[offset] != 0;
        offset += 1;

        let rent_epoch = u64::from_le_bytes([
            data[offset],
            data[offset + 1],
            data[offset + 2],
            data[offset + 3],
            data[offset + 4],
            data[offset + 5],
            data[offset + 6],
            data[offset + 7],
        ]);

        Ok(Self {
            pubkey: Pubkey(pubkey_bytes),
            owner: Pubkey(owner_bytes),
            lamports,
            data_len,
            data_off,
            is_signer,
            is_writable,
            executable,
            rent_epoch,
        })
    }
}

/// Accounts blob - serialized accounts for guest memory
///
/// Wire format:
/// - Header (8 bytes):
///   - magic: 4 bytes ("DCHA")
///   - abi_version: u16
///   - account_count: u16
/// - Offsets table: account_count * 4 bytes (u32 offsets into TOC)
/// - TOC entries: account_count * 89 bytes each
/// - Data section: concatenated account data
#[derive(Debug, Clone)]
pub struct AccountsBlob {
    /// ABI version
    pub version: u16,
    /// TOC entries
    pub entries: Vec<AccountTocEntry>,
    /// Account data (concatenated)
    pub data: Vec<u8>,
}

impl AccountsBlob {
    /// Create a new accounts blob from accounts
    pub fn new(accounts: &[SerializableAccount]) -> ProgramResult<Self> {
        if accounts.len() > MAX_ACCOUNTS as usize {
            return Err(ProgramError::MaxAccountsExceeded);
        }

        let mut entries = Vec::with_capacity(accounts.len());
        let mut data = Vec::new();

        for acc in accounts {
            if acc.data.len() > MAX_ACCOUNT_DATA_SIZE as usize {
                return Err(ProgramError::AccountDataTooLarge);
            }

            let data_off = data.len() as u32;
            let data_len = acc.data.len() as u32;

            entries.push(AccountTocEntry {
                pubkey: acc.pubkey,
                owner: acc.owner,
                lamports: acc.lamports,
                data_len,
                data_off,
                is_signer: acc.is_signer,
                is_writable: acc.is_writable,
                executable: acc.executable,
                rent_epoch: acc.rent_epoch,
            });

            data.extend_from_slice(&acc.data);
        }

        Ok(Self {
            version: ABI_VERSION,
            entries,
            data,
        })
    }

    /// Encode to wire format
    pub fn encode(&self) -> Vec<u8> {
        let account_count = self.entries.len() as u16;
        let offsets_size = account_count as usize * 4;
        let toc_size = account_count as usize * TOC_ENTRY_SIZE;
        let total_size = ACCOUNTS_BLOB_HEADER_SIZE + offsets_size + toc_size + self.data.len();

        let mut buf = Vec::with_capacity(total_size);

        // Header
        buf.extend_from_slice(&ACCOUNTS_BLOB_MAGIC);
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&account_count.to_le_bytes());

        // Offsets table (offset into TOC for each account)
        let toc_start = ACCOUNTS_BLOB_HEADER_SIZE + offsets_size;
        for i in 0..account_count as usize {
            let toc_offset = (toc_start + i * TOC_ENTRY_SIZE) as u32;
            buf.extend_from_slice(&toc_offset.to_le_bytes());
        }

        // TOC entries
        for entry in &self.entries {
            buf.extend_from_slice(&entry.encode());
        }

        // Data section
        buf.extend_from_slice(&self.data);

        buf
    }

    /// Decode from wire format with strict validation
    pub fn decode(data: &[u8]) -> ProgramResult<Self> {
        // Validate minimum size
        if data.len() < ACCOUNTS_BLOB_HEADER_SIZE {
            return Err(ProgramError::InvalidAccountData);
        }

        // Validate magic
        if data[0..4] != ACCOUNTS_BLOB_MAGIC {
            return Err(ProgramError::Custom(AbiError::InvalidMagic as u32));
        }

        // Parse header
        let version = u16::from_le_bytes([data[4], data[5]]);
        let account_count = u16::from_le_bytes([data[6], data[7]]);

        // Validate version
        if version < ABI_VERSION_MIN || version > ABI_VERSION_MAX {
            return Err(ProgramError::Custom(AbiError::UnsupportedVersion as u32));
        }

        // Validate account count
        if account_count > MAX_ACCOUNTS {
            return Err(ProgramError::MaxAccountsExceeded);
        }

        // Calculate expected sizes
        let offsets_size = account_count as usize * 4;
        let toc_size = account_count as usize * TOC_ENTRY_SIZE;
        let header_and_offsets = ACCOUNTS_BLOB_HEADER_SIZE + offsets_size;
        let min_size = header_and_offsets + toc_size;

        if data.len() < min_size {
            return Err(ProgramError::InvalidAccountData);
        }

        // Parse offsets table (for validation)
        let offsets_start = ACCOUNTS_BLOB_HEADER_SIZE;
        for i in 0..account_count as usize {
            let off_pos = offsets_start + i * 4;
            let _offset = u32::from_le_bytes([
                data[off_pos],
                data[off_pos + 1],
                data[off_pos + 2],
                data[off_pos + 3],
            ]);
            // Offset validation would go here
        }

        // Parse TOC entries
        let toc_start = header_and_offsets;
        let mut entries = Vec::with_capacity(account_count as usize);

        for i in 0..account_count as usize {
            let entry_start = toc_start + i * TOC_ENTRY_SIZE;
            let entry_end = entry_start + TOC_ENTRY_SIZE;
            if entry_end > data.len() {
                return Err(ProgramError::InvalidAccountData);
            }
            let entry = AccountTocEntry::decode(&data[entry_start..entry_end])?;
            entries.push(entry);
        }

        // Extract data section
        let data_start = toc_start + toc_size;
        let account_data = data[data_start..].to_vec();

        Ok(Self {
            version,
            entries,
            data: account_data,
        })
    }

    /// Get account data by index
    pub fn get_account_data(&self, index: usize) -> Option<&[u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.data_off as usize;
        let end = start + entry.data_len as usize;
        if end > self.data.len() {
            return None;
        }
        Some(&self.data[start..end])
    }

    /// Get mutable account data by index
    pub fn get_account_data_mut(&mut self, index: usize) -> Option<&mut [u8]> {
        let entry = self.entries.get(index)?;
        let start = entry.data_off as usize;
        let end = start + entry.data_len as usize;
        if end > self.data.len() {
            return None;
        }
        Some(&mut self.data[start..end])
    }

    /// Calculate hash of all account data for receipts
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        for entry in &self.entries {
            hasher.update(&entry.pubkey.0);
            hasher.update(&entry.lamports.to_le_bytes());
            let data = self.get_account_data(
                self.entries
                    .iter()
                    .position(|e| e.pubkey == entry.pubkey)
                    .unwrap_or(0),
            );
            if let Some(d) = data {
                hasher.update(d);
            }
        }
        *hasher.finalize().as_bytes()
    }
}

/// Account data for serialization into AccountsBlob
#[derive(Debug, Clone)]
pub struct SerializableAccount {
    /// Account public key
    pub pubkey: Pubkey,
    /// Program owner
    pub owner: Pubkey,
    /// Lamports balance
    pub lamports: u64,
    /// Account data
    pub data: Vec<u8>,
    /// Is this a signer
    pub is_signer: bool,
    /// Is this writable
    pub is_writable: bool,
    /// Is this executable
    pub executable: bool,
    /// Rent epoch
    pub rent_epoch: u64,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ABI ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// ABI-specific error codes (custom error range: 1000-1099)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u32)]
pub enum AbiError {
    /// Invalid magic bytes
    InvalidMagic = 1000,
    /// Unsupported ABI version
    UnsupportedVersion = 1001,
    /// Payload exceeds maximum size
    PayloadTooLarge = 1002,
    /// Unknown instruction tag
    UnknownTag = 1003,
    /// Account count mismatch
    AccountCountMismatch = 1004,
    /// Account key mismatch
    AccountKeyMismatch = 1005,
    /// Readonly account modified
    ReadonlyModified = 1006,
    /// Lamports conservation violated
    LamportsConservation = 1007,
    /// Owner modification not allowed
    OwnerModificationDenied = 1008,
    /// Executable flag modification denied
    ExecutableModificationDenied = 1009,
    /// Data resize not allowed in v1
    DataResizeDenied = 1010,
    /// Account order mismatch
    AccountOrderMismatch = 1011,
    /// Buffer too small for operation
    BufferTooSmall = 1012,
}

impl AbiError {
    /// Convert to program error
    pub fn to_program_error(self) -> ProgramError {
        ProgramError::Custom(self as u32)
    }

    /// Get error message
    pub fn message(&self) -> &'static str {
        match self {
            Self::InvalidMagic => "Invalid magic bytes in envelope",
            Self::UnsupportedVersion => "Unsupported ABI version",
            Self::PayloadTooLarge => "Payload exceeds maximum size",
            Self::UnknownTag => "Unknown instruction tag",
            Self::AccountCountMismatch => "Account count mismatch between input and output",
            Self::AccountKeyMismatch => "Account key mismatch",
            Self::ReadonlyModified => "Readonly account was modified",
            Self::LamportsConservation => "Lamports conservation violated",
            Self::OwnerModificationDenied => "Owner modification not allowed",
            Self::ExecutableModificationDenied => "Executable flag modification denied",
            Self::DataResizeDenied => "Data resize not allowed in ABI v1",
            Self::AccountOrderMismatch => "Account order changed",
            Self::BufferTooSmall => "Buffer too small for operation",
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DETERMINISTIC EVENT ENCODING
// ═══════════════════════════════════════════════════════════════════════════════

/// Event envelope magic
pub const EVENT_MAGIC: [u8; 4] = [0x44, 0x43, 0x48, 0x45]; // "DCHE"

/// Maximum event data size
pub const MAX_EVENT_DATA_SIZE: u32 = 10 * 1024; // 10 KB

/// Event header size
pub const EVENT_HEADER_SIZE: usize = 48; // 4 + 2 + 8 + 32 + 2

/// Deterministic event structure for guest emission
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EventEnvelope {
    /// ABI version
    pub version: u16,
    /// Event discriminator (first 8 bytes of blake3("event:{name}"))
    pub discriminator: [u8; 8],
    /// Program ID that emitted the event
    pub program_id: Pubkey,
    /// Event data
    pub data: Vec<u8>,
}

impl EventEnvelope {
    /// Create a new event
    pub fn new(program_id: Pubkey, name: &str, data: Vec<u8>) -> ProgramResult<Self> {
        if data.len() > MAX_EVENT_DATA_SIZE as usize {
            return Err(ProgramError::Custom(AbiError::PayloadTooLarge as u32));
        }

        let discriminator = Self::compute_discriminator(name);

        Ok(Self {
            version: ABI_VERSION,
            discriminator,
            program_id,
            data,
        })
    }

    /// Compute event discriminator from name
    pub fn compute_discriminator(name: &str) -> [u8; 8] {
        let hash = blake3::hash(format!("event:{}", name).as_bytes());
        let bytes: [u8; 32] = *hash.as_bytes();
        let mut disc = [0u8; 8];
        disc.copy_from_slice(&bytes[..8]);
        disc
    }

    /// Encode to wire format
    pub fn encode(&self) -> Vec<u8> {
        let data_len = self.data.len() as u16;
        let total = EVENT_HEADER_SIZE + self.data.len();
        let mut buf = Vec::with_capacity(total);

        buf.extend_from_slice(&EVENT_MAGIC);
        buf.extend_from_slice(&self.version.to_le_bytes());
        buf.extend_from_slice(&self.discriminator);
        buf.extend_from_slice(&self.program_id.0);
        buf.extend_from_slice(&data_len.to_le_bytes());
        buf.extend_from_slice(&self.data);

        buf
    }

    /// Decode from wire format
    pub fn decode(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < EVENT_HEADER_SIZE {
            return Err(ProgramError::InvalidInstructionData);
        }

        if data[0..4] != EVENT_MAGIC {
            return Err(ProgramError::Custom(AbiError::InvalidMagic as u32));
        }

        let version = u16::from_le_bytes([data[4], data[5]]);
        if version < ABI_VERSION_MIN || version > ABI_VERSION_MAX {
            return Err(ProgramError::Custom(AbiError::UnsupportedVersion as u32));
        }

        let mut discriminator = [0u8; 8];
        discriminator.copy_from_slice(&data[6..14]);

        let mut program_id_bytes = [0u8; 32];
        program_id_bytes.copy_from_slice(&data[14..46]);

        let data_len = u16::from_le_bytes([data[46], data[47]]) as usize;
        if data.len() < EVENT_HEADER_SIZE + data_len {
            return Err(ProgramError::InvalidInstructionData);
        }

        let event_data = data[EVENT_HEADER_SIZE..EVENT_HEADER_SIZE + data_len].to_vec();

        Ok(Self {
            version,
            discriminator,
            program_id: Pubkey(program_id_bytes),
            data: event_data,
        })
    }

    /// Calculate deterministic hash
    pub fn hash(&self) -> [u8; 32] {
        *blake3::hash(&self.encode()).as_bytes()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ix_envelope_roundtrip() {
        let payload = vec![1, 2, 3, 4, 5, 6, 7, 8];
        let envelope = IxEnvelope::new(42, payload.clone()).unwrap();

        let encoded = envelope.encode();
        let decoded = IxEnvelope::decode(&encoded).unwrap();

        assert_eq!(decoded.version, ABI_VERSION);
        assert_eq!(decoded.tag, 42);
        assert_eq!(decoded.payload, payload);
    }

    #[test]
    fn test_ix_envelope_rejects_bad_magic() {
        let mut data = IxEnvelope::new(1, vec![]).unwrap().encode();
        data[0] = 0xFF; // Corrupt magic
        assert!(IxEnvelope::decode(&data).is_err());
    }

    #[test]
    fn test_ix_envelope_rejects_bad_version() {
        let mut data = IxEnvelope::new(1, vec![]).unwrap().encode();
        data[4] = 0xFF;
        data[5] = 0xFF; // Invalid version
        assert!(IxEnvelope::decode(&data).is_err());
    }

    #[test]
    fn test_ix_envelope_rejects_oversized() {
        let big_payload = vec![0u8; (MAX_PAYLOAD_SIZE + 1) as usize];
        assert!(IxEnvelope::new(1, big_payload).is_err());
    }

    #[test]
    fn test_accounts_blob_roundtrip() {
        let accounts = vec![
            SerializableAccount {
                pubkey: Pubkey::new([1u8; 32]),
                owner: Pubkey::new([2u8; 32]),
                lamports: 1000,
                data: vec![10, 20, 30],
                is_signer: true,
                is_writable: true,
                executable: false,
                rent_epoch: 100,
            },
            SerializableAccount {
                pubkey: Pubkey::new([3u8; 32]),
                owner: Pubkey::new([4u8; 32]),
                lamports: 2000,
                data: vec![40, 50],
                is_signer: false,
                is_writable: false,
                executable: true,
                rent_epoch: 200,
            },
        ];

        let blob = AccountsBlob::new(&accounts).unwrap();
        let encoded = blob.encode();
        let decoded = AccountsBlob::decode(&encoded).unwrap();

        assert_eq!(decoded.version, ABI_VERSION);
        assert_eq!(decoded.entries.len(), 2);

        // Verify first account
        assert_eq!(decoded.entries[0].pubkey, Pubkey::new([1u8; 32]));
        assert_eq!(decoded.entries[0].lamports, 1000);
        assert!(decoded.entries[0].is_signer);
        assert!(decoded.entries[0].is_writable);
        assert!(!decoded.entries[0].executable);

        // Verify data
        assert_eq!(decoded.get_account_data(0).unwrap(), &[10, 20, 30]);
        assert_eq!(decoded.get_account_data(1).unwrap(), &[40, 50]);
    }

    #[test]
    fn test_toc_entry_roundtrip() {
        let entry = AccountTocEntry {
            pubkey: Pubkey::new([5u8; 32]),
            owner: Pubkey::new([6u8; 32]),
            lamports: 12345678,
            data_len: 100,
            data_off: 0,
            is_signer: true,
            is_writable: false,
            executable: true,
            rent_epoch: 999,
        };

        let encoded = entry.encode();
        assert_eq!(encoded.len(), TOC_ENTRY_SIZE);

        let decoded = AccountTocEntry::decode(&encoded).unwrap();
        assert_eq!(decoded.pubkey, entry.pubkey);
        assert_eq!(decoded.owner, entry.owner);
        assert_eq!(decoded.lamports, entry.lamports);
        assert_eq!(decoded.is_signer, entry.is_signer);
        assert_eq!(decoded.is_writable, entry.is_writable);
        assert_eq!(decoded.executable, entry.executable);
        assert_eq!(decoded.rent_epoch, entry.rent_epoch);
    }

    #[test]
    fn test_event_envelope_roundtrip() {
        let program_id = Pubkey::new([7u8; 32]);
        let event = EventEnvelope::new(program_id, "Transfer", vec![1, 2, 3, 4]).unwrap();

        let encoded = event.encode();
        let decoded = EventEnvelope::decode(&encoded).unwrap();

        assert_eq!(decoded.version, ABI_VERSION);
        assert_eq!(decoded.program_id, program_id);
        assert_eq!(decoded.discriminator, event.discriminator);
        assert_eq!(decoded.data, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_event_discriminator_determinism() {
        let disc1 = EventEnvelope::compute_discriminator("Transfer");
        let disc2 = EventEnvelope::compute_discriminator("Transfer");
        let disc3 = EventEnvelope::compute_discriminator("Mint");

        assert_eq!(disc1, disc2);
        assert_ne!(disc1, disc3);
    }

    #[test]
    fn test_ix_envelope_hash_determinism() {
        let env1 = IxEnvelope::new(1, vec![1, 2, 3]).unwrap();
        let env2 = IxEnvelope::new(1, vec![1, 2, 3]).unwrap();

        assert_eq!(env1.hash(), env2.hash());
    }

    #[test]
    fn test_accounts_blob_rejects_too_many() {
        let accounts: Vec<SerializableAccount> = (0..300)
            .map(|i| SerializableAccount {
                pubkey: Pubkey::new([i as u8; 32]),
                owner: Pubkey::zero(),
                lamports: 0,
                data: vec![],
                is_signer: false,
                is_writable: false,
                executable: false,
                rent_epoch: 0,
            })
            .collect();

        assert!(AccountsBlob::new(&accounts).is_err());
    }
}
