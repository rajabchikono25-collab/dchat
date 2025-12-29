//! Message Header for Double Ratchet
//!
//! The header is sent with each encrypted message to enable the
//! receiver to decrypt it.

use serde::{Deserialize, Serialize};

/// Message header containing ratchet state information
#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct MessageHeader {
    /// Sender's current ratchet public key (DH public key)
    pub dh_public: [u8; 32],
    /// Number of messages in the previous sending chain
    pub previous_chain_length: u32,
    /// Message number in the current sending chain
    pub message_index: u32,
}

impl MessageHeader {
    /// Create a new message header
    pub fn new(dh_public: [u8; 32], previous_chain_length: u32, message_index: u32) -> Self {
        Self {
            dh_public,
            previous_chain_length,
            message_index,
        }
    }

    /// Encode the header for use as AEAD associated data
    pub fn encode(&self) -> Vec<u8> {
        let mut data = Vec::with_capacity(40);
        data.extend_from_slice(&self.dh_public);
        data.extend_from_slice(&self.previous_chain_length.to_le_bytes());
        data.extend_from_slice(&self.message_index.to_le_bytes());
        data
    }

    /// Decode header from bytes
    pub fn decode(data: &[u8]) -> Option<Self> {
        if data.len() < 40 {
            return None;
        }

        let mut dh_public = [0u8; 32];
        dh_public.copy_from_slice(&data[0..32]);

        let previous_chain_length = u32::from_le_bytes(data[32..36].try_into().ok()?);
        let message_index = u32::from_le_bytes(data[36..40].try_into().ok()?);

        Some(Self {
            dh_public,
            previous_chain_length,
            message_index,
        })
    }

    /// Get the DH public key as x25519 PublicKey
    pub fn dh_public_key(&self) -> x25519_dalek::PublicKey {
        x25519_dalek::PublicKey::from(self.dh_public)
    }
}

/// Encoder/decoder for message headers with optional encryption
pub struct HeaderEncoder;

impl HeaderEncoder {
    /// Encode a header for transmission (with length prefix)
    pub fn encode(header: &MessageHeader) -> Vec<u8> {
        let encoded = bincode::serialize(header).unwrap_or_else(|_| header.encode());
        let mut result = Vec::with_capacity(2 + encoded.len());
        result.extend_from_slice(&(encoded.len() as u16).to_le_bytes());
        result.extend_from_slice(&encoded);
        result
    }

    /// Decode a header from bytes
    pub fn decode(data: &[u8]) -> Option<(MessageHeader, usize)> {
        if data.len() < 2 {
            return None;
        }

        let len = u16::from_le_bytes(data[0..2].try_into().ok()?) as usize;
        if data.len() < 2 + len {
            return None;
        }

        let header: MessageHeader = bincode::deserialize(&data[2..2 + len]).ok()?;
        Some((header, 2 + len))
    }

    /// Encode header + ciphertext for wire format
    pub fn encode_message(header: &MessageHeader, ciphertext: &[u8]) -> Vec<u8> {
        let header_bytes = Self::encode(header);
        let mut result = Vec::with_capacity(header_bytes.len() + ciphertext.len());
        result.extend_from_slice(&header_bytes);
        result.extend_from_slice(ciphertext);
        result
    }

    /// Decode header + ciphertext from wire format
    pub fn decode_message(data: &[u8]) -> Option<(MessageHeader, Vec<u8>)> {
        let (header, header_len) = Self::decode(data)?;
        let ciphertext = data[header_len..].to_vec();
        Some((header, ciphertext))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_encode_decode() {
        let header = MessageHeader::new([1u8; 32], 5, 42);

        let encoded = header.encode();
        let decoded = MessageHeader::decode(&encoded).unwrap();

        assert_eq!(header, decoded);
    }

    #[test]
    fn test_header_encoder() {
        let header = MessageHeader::new([2u8; 32], 10, 100);
        let ciphertext = b"encrypted data here";

        let wire = HeaderEncoder::encode_message(&header, ciphertext);
        let (decoded_header, decoded_ct) = HeaderEncoder::decode_message(&wire).unwrap();

        assert_eq!(header, decoded_header);
        assert_eq!(&decoded_ct, ciphertext);
    }

    #[test]
    fn test_header_as_aad() {
        let header = MessageHeader::new([3u8; 32], 0, 0);
        let aad = header.encode();

        // AAD should be exactly 40 bytes
        assert_eq!(aad.len(), 40);

        // Should be reproducible
        let aad2 = header.encode();
        assert_eq!(aad, aad2);
    }
}
