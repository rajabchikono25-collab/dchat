//! Serialization utilities for DPL programs

use crate::error::DplError;

/// Cursor for reading bytes
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    /// Create a new cursor
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    /// Current position
    pub fn position(&self) -> usize {
        self.pos
    }

    /// Remaining bytes
    pub fn remaining(&self) -> usize {
        self.data.len().saturating_sub(self.pos)
    }

    /// Read a single byte
    pub fn read_u8(&mut self) -> Result<u8, DplError> {
        if self.pos >= self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let byte = self.data[self.pos];
        self.pos += 1;
        Ok(byte)
    }

    /// Read u16 (little-endian)
    pub fn read_u16(&mut self) -> Result<u16, DplError> {
        if self.pos + 2 > self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let bytes = [self.data[self.pos], self.data[self.pos + 1]];
        self.pos += 2;
        Ok(u16::from_le_bytes(bytes))
    }

    /// Read u32 (little-endian)
    pub fn read_u32(&mut self) -> Result<u32, DplError> {
        if self.pos + 4 > self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let bytes = [
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ];
        self.pos += 4;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Read u64 (little-endian)
    pub fn read_u64(&mut self) -> Result<u64, DplError> {
        if self.pos + 8 > self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let bytes = [
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
            self.data[self.pos + 4],
            self.data[self.pos + 5],
            self.data[self.pos + 6],
            self.data[self.pos + 7],
        ];
        self.pos += 8;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Read fixed-size bytes
    pub fn read_bytes<const N: usize>(&mut self) -> Result<[u8; N], DplError> {
        if self.pos + N > self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let mut bytes = [0u8; N];
        bytes.copy_from_slice(&self.data[self.pos..self.pos + N]);
        self.pos += N;
        Ok(bytes)
    }

    /// Read variable-length bytes with u32 length prefix
    pub fn read_vec(&mut self) -> Result<&'a [u8], DplError> {
        let len = self.read_u32()? as usize;
        if self.pos + len > self.data.len() {
            return Err(DplError::DeserializationError("Unexpected EOF".into()));
        }
        let bytes = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Ok(bytes)
    }
}

/// Trait for DPL-compatible serialization
pub trait DplSerialize {
    /// Serialize to bytes, return number of bytes written
    fn serialize(&self, buf: &mut [u8]) -> Result<usize, DplError>;
}

/// Trait for DPL-compatible deserialization
pub trait DplDeserialize: Sized {
    /// Deserialize from bytes
    fn deserialize(data: &[u8]) -> Result<Self, DplError>;
}

// Implement for primitive types

impl DplSerialize for u8 {
    fn serialize(&self, buf: &mut [u8]) -> Result<usize, DplError> {
        if buf.is_empty() {
            return Err(DplError::SerializationError("Buffer too small".into()));
        }
        buf[0] = *self;
        Ok(1)
    }
}

impl DplDeserialize for u8 {
    fn deserialize(data: &[u8]) -> Result<Self, DplError> {
        if data.is_empty() {
            return Err(DplError::DeserializationError("Empty data".into()));
        }
        Ok(data[0])
    }
}

impl DplSerialize for u64 {
    fn serialize(&self, buf: &mut [u8]) -> Result<usize, DplError> {
        if buf.len() < 8 {
            return Err(DplError::SerializationError("Buffer too small".into()));
        }
        buf[..8].copy_from_slice(&self.to_le_bytes());
        Ok(8)
    }
}

impl DplDeserialize for u64 {
    fn deserialize(data: &[u8]) -> Result<Self, DplError> {
        if data.len() < 8 {
            return Err(DplError::DeserializationError("Data too short".into()));
        }
        let mut bytes = [0u8; 8];
        bytes.copy_from_slice(&data[..8]);
        Ok(u64::from_le_bytes(bytes))
    }
}

impl DplSerialize for crate::account::Pubkey {
    fn serialize(&self, buf: &mut [u8]) -> Result<usize, DplError> {
        if buf.len() < 32 {
            return Err(DplError::SerializationError("Buffer too small".into()));
        }
        buf[..32].copy_from_slice(&self.0);
        Ok(32)
    }
}

impl DplDeserialize for crate::account::Pubkey {
    fn deserialize(data: &[u8]) -> Result<Self, DplError> {
        if data.len() < 32 {
            return Err(DplError::DeserializationError("Data too short".into()));
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data[..32]);
        Ok(crate::account::Pubkey(bytes))
    }
}
