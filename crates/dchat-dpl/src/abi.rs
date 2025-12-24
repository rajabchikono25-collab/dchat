//! ABI types for DPL programs
//!
//! Defines the binary format for instruction envelopes and account blobs.

/// ABI version
pub const ABI_VERSION: u8 = 1;

/// Magic bytes for instruction envelope: "DCHX"
pub const IX_MAGIC: [u8; 4] = *b"DCHX";

/// Magic bytes for accounts blob: "DCHA"
pub const ACCOUNTS_MAGIC: [u8; 4] = *b"DCHA";

/// Maximum payload size (1 MB)
pub const MAX_PAYLOAD_SIZE: usize = 1024 * 1024;

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
    pub fn new(data: &'a [u8]) -> Option<Self> {
        if data.len() < 8 {
            return None;
        }

        let magic: [u8; 4] = data[0..4].try_into().ok()?;
        if magic != ACCOUNTS_MAGIC {
            return None;
        }

        let count = u32::from_le_bytes([data[4], data[5], data[6], data[7]]) as usize;

        Some(Self {
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

        // TOC entry: offset (4) + len (4) + flags (1) = 9 bytes
        let toc_offset = 8 + self.current * 9;
        if toc_offset + 9 > self.data.len() {
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

        if entry_offset + entry_len > self.data.len() {
            return None;
        }

        self.current += 1;

        Some(AccountEntry {
            data: &self.data[entry_offset..entry_offset + entry_len],
            flags,
        })
    }
}

/// Single account entry from blob
pub struct AccountEntry<'a> {
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
}
