//! ABI types for DPL programs
//!
//! Defines the binary format for instruction envelopes and account blobs.

use crate::account::Pubkey;
use crate::error::{DplError, DplResult};

/// ABI version
pub const ABI_VERSION: u8 = 1;

/// Magic bytes for instruction envelope: "DCHX"
pub const IX_MAGIC: [u8; 4] = *b"DCHX";

/// Magic bytes for accounts blob: "DCHA"
pub const ACCOUNTS_MAGIC: [u8; 4] = *b"DCHA";

/// Maximum payload size (1 MB)
pub const MAX_PAYLOAD_SIZE: usize = 1024 * 1024;

/// Parse the raw input pointer into program components
///
/// # Safety
/// The input pointer must be valid and point to properly formatted data.
pub unsafe fn parse_input(input: *mut u8) -> DplResult<(Pubkey, &'static [u8], &'static [u8])> {
    // Input layout:
    // [0..32] - program ID
    // [32..40] - accounts data length (u64 LE)
    // [40..48] - instruction data length (u64 LE)
    // [48..48+accounts_len] - accounts data
    // [48+accounts_len..] - instruction data

    let program_id_bytes = core::slice::from_raw_parts(input, 32);
    let program_id =
        Pubkey::from_slice(program_id_bytes).ok_or(DplError::InvalidInstructionData)?;

    let accounts_len = u64::from_le_bytes(
        core::slice::from_raw_parts(input.add(32), 8)
            .try_into()
            .map_err(|_| DplError::InvalidInstructionData)?,
    ) as usize;

    let instruction_len = u64::from_le_bytes(
        core::slice::from_raw_parts(input.add(40), 8)
            .try_into()
            .map_err(|_| DplError::InvalidInstructionData)?,
    ) as usize;

    let accounts_data = core::slice::from_raw_parts(input.add(48), accounts_len);
    let instruction_data =
        core::slice::from_raw_parts(input.add(48 + accounts_len), instruction_len);

    Ok((program_id, accounts_data, instruction_data))
}

/// Instruction envelope header
#[derive(Debug, Clone, Copy)]
#[repr(C)]
pub struct IxEnvelope {
    /// Magic bytes: "DCHX"
    pub magic: [u8; 4],
    /// ABI version
    pub version: u8,
    /// Instruction tag
    pub tag: u8,
    /// Reserved for future use
    pub reserved: [u8; 2],
    /// Payload length
    pub payload_len: u32,
}

impl IxEnvelope {
    /// Header size in bytes
    pub const SIZE: usize = 12;

    /// Parse envelope from bytes
    pub fn from_bytes(data: &[u8]) -> Option<Self> {
        if data.len() < Self::SIZE {
            return None;
        }

        let magic: [u8; 4] = data[0..4].try_into().ok()?;
        if magic != IX_MAGIC {
            return None;
        }

        Some(Self {
            magic,
            version: data[4],
            tag: data[5],
            reserved: [data[6], data[7]],
            payload_len: u32::from_le_bytes([data[8], data[9], data[10], data[11]]),
        })
    }

    /// Serialize envelope to bytes
    pub fn to_bytes(&self) -> [u8; Self::SIZE] {
        let mut bytes = [0u8; Self::SIZE];
        bytes[0..4].copy_from_slice(&self.magic);
        bytes[4] = self.version;
        bytes[5] = self.tag;
        bytes[6..8].copy_from_slice(&self.reserved);
        bytes[8..12].copy_from_slice(&self.payload_len.to_le_bytes());
        bytes
    }
}

/// Accounts blob cursor for parsing
pub struct AccountsCursor<'a> {
    data: &'a [u8],
    offset: usize,
    count: usize,
    current: usize,
}

impl<'a> AccountsCursor<'a> {
    /// Parse accounts blob header
    pub fn new(data: &'a [u8]) -> crate::error::DplResult<Self> {
        if data.len() < 8 {
            return Err(crate::error::DplError::AccountDataTooSmall);
        }

        let magic: [u8; 4] = data[0..4]
            .try_into()
            .map_err(|_| crate::error::DplError::AccountDataTooSmall)?;
        if magic != ACCOUNTS_MAGIC {
            return Err(crate::error::DplError::InvalidMagic);
        }

        let count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

        Ok(Self {
            data,
            offset: 8,
            count,
            current: 0,
        })
    }

    /// Number of accounts
    pub fn count(&self) -> usize {
        self.count
    }

    /// Read next account entry
    pub fn next_entry(&mut self) -> Option<AccountEntry<'a>> {
        if self.current >= self.count {
            return None;
        }

        // TOC entry: offset(4) + len(4) + flags(1) + key(32) + owner(32) + motes(8) = 81 bytes
        let toc_offset = 8 + self.current * TOC_ENTRY_SIZE;
        if toc_offset + TOC_ENTRY_SIZE > self.data.len() {
            return None;
        }

        let entry_offset = u32::from_le_bytes([
            self.data[toc_offset],
            self.data[toc_offset + 1],
            self.data[toc_offset + 2],
            self.data[toc_offset + 3],
        ]) as usize;

        let entry_len = u32::from_le_bytes([
            self.data[toc_offset + 4],
            self.data[toc_offset + 5],
            self.data[toc_offset + 6],
            self.data[toc_offset + 7],
        ]) as usize;

        let flags = self.data[toc_offset + 8];

        // Parse key (32 bytes at offset 9)
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&self.data[toc_offset + 9..toc_offset + 41]);
        let key = crate::account::Pubkey::new(key_bytes);

        // Parse owner (32 bytes at offset 41)
        let mut owner_bytes = [0u8; 32];
        owner_bytes.copy_from_slice(&self.data[toc_offset + 41..toc_offset + 73]);
        let owner = crate::account::Pubkey::new(owner_bytes);

        // Parse motes (8 bytes at offset 73)
        let motes = u64::from_le_bytes([
            self.data[toc_offset + 73],
            self.data[toc_offset + 74],
            self.data[toc_offset + 75],
            self.data[toc_offset + 76],
            self.data[toc_offset + 77],
            self.data[toc_offset + 78],
            self.data[toc_offset + 79],
            self.data[toc_offset + 80],
        ]);

        if entry_offset + entry_len > self.data.len() {
            return None;
        }

        self.current += 1;

        Some(AccountEntry {
            key,
            owner,
            motes,
            data: &self.data[entry_offset..entry_offset + entry_len],
            flags,
        })
    }

    /// Read next account entry, returning error if not available
    pub fn next_account(&mut self) -> crate::error::DplResult<AccountEntry<'a>> {
        self.next_entry()
            .ok_or(crate::error::DplError::NotEnoughAccounts)
    }
}

/// Size of a TOC entry in the accounts blob
/// Layout: offset(4) + len(4) + flags(1) + key(32) + owner(32) + motes(8) = 81 bytes
pub const TOC_ENTRY_SIZE: usize = 81;

/// Single account entry from blob
pub struct AccountEntry<'a> {
    /// Account public key
    pub key: crate::account::Pubkey,
    /// Account owner program
    pub owner: crate::account::Pubkey,
    /// Account balance in motes
    pub motes: u64,
    /// Account data
    pub data: &'a [u8],
    /// Flags (writable, signer, etc.)
    pub flags: u8,
}

impl<'a> AccountEntry<'a> {
    /// Check if account is writable
    pub fn is_writable(&self) -> bool {
        self.flags & 0x01 != 0
    }

    /// Check if account is signer
    pub fn is_signer(&self) -> bool {
        self.flags & 0x02 != 0
    }

    /// Get the account key
    pub fn key(&self) -> &crate::account::Pubkey {
        &self.key
    }

    /// Get the account owner
    pub fn owner(&self) -> &crate::account::Pubkey {
        &self.owner
    }

    /// Get the account balance
    pub fn motes(&self) -> u64 {
        self.motes
    }
}
