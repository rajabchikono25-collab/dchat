//! Sphinx Packet Format for Onion Routing
//!
//! Implements Sphinx-style packet encoding with layered encryption for unlinkability.
//! Each hop decrypts one layer and forwards the remainder.

use aes_gcm::{
    aead::{Aead, KeyInit, Payload},
    Aes256Gcm, Nonce,
};
use blake3;
use thiserror::Error;

/// Size of encryption keys (256 bits)
pub const KEY_SIZE: usize = 32;

/// Size of nonce for AEAD (96 bits)
pub const NONCE_SIZE: usize = 12;

/// Maximum payload size (1400 bytes for network efficiency)
pub const MAX_PAYLOAD_SIZE: usize = 1400;

/// Size of routing information per hop
pub const ROUTING_INFO_SIZE: usize = 64;

/// Errors that can occur during Sphinx operations
#[derive(Debug, Error)]
pub enum SphinxError {
    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("Decryption failed: {0}")]
    DecryptionFailed(String),

    #[error("Invalid key size: expected {expected}, got {actual}")]
    InvalidKeySize { expected: usize, actual: usize },

    #[error("Invalid nonce size: expected {expected}, got {actual}")]
    InvalidNonceSize { expected: usize, actual: usize },

    #[error("Payload too large: max {max}, got {actual}")]
    PayloadTooLarge { max: usize, actual: usize },

    #[error("Invalid packet format: {0}")]
    InvalidPacketFormat(String),

    #[error("No more hops in packet")]
    NoMoreHops,
}

/// Routing information for a single hop
#[derive(Debug, Clone)]
pub struct RoutingInfo {
    /// Next hop peer ID (or empty for final hop)
    pub next_hop: String,
    /// Relay instructions (e.g., "forward", "deliver")
    pub instructions: String,
}

impl RoutingInfo {
    /// Serialize routing info to bytes (fixed size)
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(ROUTING_INFO_SIZE);

        // Encode next_hop (32 bytes, padded)
        let next_hop_bytes = self.next_hop.as_bytes();
        bytes.extend_from_slice(&next_hop_bytes[..next_hop_bytes.len().min(32)]);
        bytes.resize(32, 0);

        // Encode instructions (32 bytes, padded)
        let instructions_bytes = self.instructions.as_bytes();
        bytes.extend_from_slice(&instructions_bytes[..instructions_bytes.len().min(32)]);
        bytes.resize(64, 0);

        bytes
    }

    /// Deserialize routing info from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SphinxError> {
        if bytes.len() < ROUTING_INFO_SIZE {
            return Err(SphinxError::InvalidPacketFormat(
                "Routing info too short".to_string(),
            ));
        }

        let next_hop = String::from_utf8_lossy(&bytes[..32])
            .trim_end_matches('\0')
            .to_string();
        let instructions = String::from_utf8_lossy(&bytes[32..64])
            .trim_end_matches('\0')
            .to_string();

        Ok(Self {
            next_hop,
            instructions,
        })
    }

    /// Check if this is the final hop
    pub fn is_final_hop(&self) -> bool {
        self.next_hop.is_empty() || self.instructions == "deliver"
    }
}

/// Sphinx packet header containing routing information
#[derive(Debug, Clone)]
pub struct SphinxHeader {
    /// Encrypted routing information for all remaining hops
    pub routing_info: Vec<u8>,
    /// MAC for header integrity
    pub mac: Vec<u8>,
}

impl SphinxHeader {
    /// Create a new Sphinx header
    pub fn new(routing_info: Vec<u8>, mac: Vec<u8>) -> Self {
        Self { routing_info, mac }
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        
        // Routing info length (2 bytes)
        bytes.extend_from_slice(&(self.routing_info.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.routing_info);
        
        // MAC length (2 bytes)
        bytes.extend_from_slice(&(self.mac.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.mac);
        
        bytes
    }

    /// Deserialize header from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<(Self, usize), SphinxError> {
        if bytes.len() < 4 {
            return Err(SphinxError::InvalidPacketFormat(
                "Header too short".to_string(),
            ));
        }

        let mut offset = 0;

        // Read routing info
        let routing_info_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;
        
        if bytes.len() < offset + routing_info_len + 2 {
            return Err(SphinxError::InvalidPacketFormat(
                "Incomplete routing info".to_string(),
            ));
        }
        
        let routing_info = bytes[offset..offset + routing_info_len].to_vec();
        offset += routing_info_len;

        // Read MAC
        let mac_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        offset += 2;
        
        if bytes.len() < offset + mac_len {
            return Err(SphinxError::InvalidPacketFormat(
                "Incomplete MAC".to_string(),
            ));
        }
        
        let mac = bytes[offset..offset + mac_len].to_vec();
        offset += mac_len;

        Ok((Self { routing_info, mac }, offset))
    }
}

/// A Sphinx onion packet
#[derive(Debug, Clone)]
pub struct SphinxPacket {
    /// Packet header with routing information
    pub header: SphinxHeader,
    /// Encrypted payload
    pub payload: Vec<u8>,
}

impl SphinxPacket {
    /// Create a new Sphinx packet
    pub fn new(header: SphinxHeader, payload: Vec<u8>) -> Result<Self, SphinxError> {
        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(SphinxError::PayloadTooLarge {
                max: MAX_PAYLOAD_SIZE,
                actual: payload.len(),
            });
        }

        Ok(Self { header, payload })
    }

    /// Build a layered Sphinx packet for multiple hops
    pub fn build(
        routing_infos: Vec<RoutingInfo>,
        shared_secrets: Vec<Vec<u8>>,
        payload: Vec<u8>,
    ) -> Result<Self, SphinxError> {
        if routing_infos.len() != shared_secrets.len() {
            return Err(SphinxError::InvalidPacketFormat(
                "Mismatched routing info and shared secrets".to_string(),
            ));
        }

        if payload.len() > MAX_PAYLOAD_SIZE {
            return Err(SphinxError::PayloadTooLarge {
                max: MAX_PAYLOAD_SIZE,
                actual: payload.len(),
            });
        }

        // Start with the plaintext payload
        let mut current_payload = payload;

        // Encrypt payload in reverse order (exit -> entry)
        for shared_secret in shared_secrets.iter().rev() {
            current_payload = Self::encrypt_layer(shared_secret, &current_payload)?;
        }

        // Build routing info by concatenating all routing bytes
        let mut all_routing = Vec::new();
        for routing_info in &routing_infos {
            all_routing.extend_from_slice(&routing_info.to_bytes());
        }

        // Generate MAC for header integrity
        let mac = blake3::hash(&all_routing).as_bytes()[..16].to_vec();

        let header = SphinxHeader::new(all_routing, mac);
        Self::new(header, current_payload)
    }

    /// Peel one layer of encryption from the packet
    pub fn peel_layer(&self, shared_secret: &[u8]) -> Result<(RoutingInfo, Self), SphinxError> {
        // Extract first hop's routing info (not encrypted in this simplified version)
        if self.header.routing_info.len() < ROUTING_INFO_SIZE {
            return Err(SphinxError::InvalidPacketFormat(
                "Insufficient routing info".to_string(),
            ));
        }

        let routing_info = RoutingInfo::from_bytes(&self.header.routing_info[..ROUTING_INFO_SIZE])?;
        let remaining_routing = self.header.routing_info[ROUTING_INFO_SIZE..].to_vec();

        // Decrypt one layer of the payload
        let decrypted_payload = Self::decrypt_layer(shared_secret, &self.payload)?;

        // Generate new MAC
        let new_mac = blake3::hash(&remaining_routing).as_bytes()[..16].to_vec();

        // Create new packet for next hop
        let new_header = SphinxHeader::new(remaining_routing, new_mac);
        let new_packet = Self::new(new_header, decrypted_payload)?;

        Ok((routing_info, new_packet))
    }

    /// Encrypt a layer using AES-256-GCM
    fn encrypt_layer(shared_secret: &[u8], data: &[u8]) -> Result<Vec<u8>, SphinxError> {
        if shared_secret.len() != KEY_SIZE {
            return Err(SphinxError::InvalidKeySize {
                expected: KEY_SIZE,
                actual: shared_secret.len(),
            });
        }

        // Derive encryption key and nonce from shared secret
        let key_material = blake3::hash(shared_secret);
        let key = key_material.as_bytes();
        let mut nonce_data = key.to_vec();
        nonce_data.extend_from_slice(b"nonce");
        let nonce_hash = blake3::hash(&nonce_data);
        let nonce_bytes = &nonce_hash.as_bytes()[..NONCE_SIZE];
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| SphinxError::EncryptionFailed(e.to_string()))?;

        let ciphertext = cipher
            .encrypt(nonce, Payload { msg: data, aad: b"" })
            .map_err(|e| SphinxError::EncryptionFailed(e.to_string()))?;

        Ok(ciphertext)
    }

    /// Decrypt a layer using AES-256-GCM
    fn decrypt_layer(shared_secret: &[u8], data: &[u8]) -> Result<Vec<u8>, SphinxError> {
        if shared_secret.len() != KEY_SIZE {
            return Err(SphinxError::InvalidKeySize {
                expected: KEY_SIZE,
                actual: shared_secret.len(),
            });
        }

        // Derive encryption key and nonce from shared secret
        let key_material = blake3::hash(shared_secret);
        let key = key_material.as_bytes();
        let mut nonce_data = key.to_vec();
        nonce_data.extend_from_slice(b"nonce");
        let nonce_hash = blake3::hash(&nonce_data);
        let nonce_bytes = &nonce_hash.as_bytes()[..NONCE_SIZE];
        let nonce = Nonce::from_slice(nonce_bytes);

        let cipher = Aes256Gcm::new_from_slice(key)
            .map_err(|e| SphinxError::DecryptionFailed(e.to_string()))?;

        let plaintext = cipher
            .decrypt(nonce, Payload { msg: data, aad: b"" })
            .map_err(|e| SphinxError::DecryptionFailed(e.to_string()))?;

        Ok(plaintext)
    }

    /// Serialize packet to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = self.header.to_bytes();
        
        // Payload length (2 bytes)
        bytes.extend_from_slice(&(self.payload.len() as u16).to_be_bytes());
        bytes.extend_from_slice(&self.payload);
        
        bytes
    }

    /// Deserialize packet from bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, SphinxError> {
        let (header, offset) = SphinxHeader::from_bytes(bytes)?;

        if bytes.len() < offset + 2 {
            return Err(SphinxError::InvalidPacketFormat(
                "Missing payload length".to_string(),
            ));
        }

        let payload_len = u16::from_be_bytes([bytes[offset], bytes[offset + 1]]) as usize;
        let payload_start = offset + 2;

        if bytes.len() < payload_start + payload_len {
            return Err(SphinxError::InvalidPacketFormat(
                "Incomplete payload".to_string(),
            ));
        }

        let payload = bytes[payload_start..payload_start + payload_len].to_vec();

        Self::new(header, payload)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_shared_secret(seed: u8) -> Vec<u8> {
        vec![seed; KEY_SIZE]
    }

    #[test]
    fn test_routing_info_serialization() {
        let routing = RoutingInfo {
            next_hop: "peer123".to_string(),
            instructions: "forward".to_string(),
        };

        let bytes = routing.to_bytes();
        assert_eq!(bytes.len(), ROUTING_INFO_SIZE);

        let deserialized = RoutingInfo::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.next_hop, "peer123");
        assert_eq!(deserialized.instructions, "forward");
    }

    #[test]
    fn test_routing_info_final_hop() {
        let final_hop = RoutingInfo {
            next_hop: "".to_string(),
            instructions: "deliver".to_string(),
        };
        assert!(final_hop.is_final_hop());

        let intermediate = RoutingInfo {
            next_hop: "peer456".to_string(),
            instructions: "forward".to_string(),
        };
        assert!(!intermediate.is_final_hop());
    }

    #[test]
    fn test_sphinx_header_serialization() {
        let header = SphinxHeader::new(vec![1, 2, 3, 4], vec![5, 6, 7, 8]);
        let bytes = header.to_bytes();

        let (deserialized, _) = SphinxHeader::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.routing_info, vec![1, 2, 3, 4]);
        assert_eq!(deserialized.mac, vec![5, 6, 7, 8]);
    }

    #[test]
    fn test_sphinx_packet_size_limit() {
        let header = SphinxHeader::new(vec![1, 2, 3], vec![4, 5, 6]);
        let oversized_payload = vec![0u8; MAX_PAYLOAD_SIZE + 1];

        let result = SphinxPacket::new(header, oversized_payload);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SphinxError::PayloadTooLarge { .. }));
    }

    #[test]
    fn test_encrypt_decrypt_layer() {
        let shared_secret = create_test_shared_secret(42);
        let plaintext = b"Hello, Sphinx!";

        let ciphertext = SphinxPacket::encrypt_layer(&shared_secret, plaintext).unwrap();
        assert_ne!(ciphertext, plaintext);

        let decrypted = SphinxPacket::decrypt_layer(&shared_secret, &ciphertext).unwrap();
        assert_eq!(decrypted, plaintext);
    }

    #[test]
    fn test_sphinx_packet_build_and_peel() {
        let routing_infos = vec![
            RoutingInfo {
                next_hop: "peer1".to_string(),
                instructions: "forward".to_string(),
            },
            RoutingInfo {
                next_hop: "peer2".to_string(),
                instructions: "forward".to_string(),
            },
            RoutingInfo {
                next_hop: "".to_string(),
                instructions: "deliver".to_string(),
            },
        ];

        let shared_secrets = vec![
            create_test_shared_secret(1),
            create_test_shared_secret(2),
            create_test_shared_secret(3),
        ];

        let payload = b"Secret message".to_vec();

        // Build the packet
        let packet = SphinxPacket::build(routing_infos.clone(), shared_secrets.clone(), payload.clone()).unwrap();

        // Peel first layer
        let (routing1, packet1) = packet.peel_layer(&shared_secrets[0]).unwrap();
        assert_eq!(routing1.next_hop, "peer1");
        assert_eq!(routing1.instructions, "forward");

        // Peel second layer
        let (routing2, packet2) = packet1.peel_layer(&shared_secrets[1]).unwrap();
        assert_eq!(routing2.next_hop, "peer2");
        assert_eq!(routing2.instructions, "forward");

        // Peel third layer (final hop)
        let (routing3, final_packet) = packet2.peel_layer(&shared_secrets[2]).unwrap();
        assert_eq!(routing3.next_hop, "");
        assert_eq!(routing3.instructions, "deliver");
        assert!(routing3.is_final_hop());

        // Final payload should be decrypted
        assert_eq!(final_packet.payload, payload);
    }

    #[test]
    fn test_sphinx_packet_serialization() {
        let routing_infos = vec![
            RoutingInfo {
                next_hop: "peer1".to_string(),
                instructions: "forward".to_string(),
            },
            RoutingInfo {
                next_hop: "".to_string(),
                instructions: "deliver".to_string(),
            },
        ];

        let shared_secrets = vec![
            create_test_shared_secret(1),
            create_test_shared_secret(2),
        ];

        let payload = b"Test payload".to_vec();

        let packet = SphinxPacket::build(routing_infos, shared_secrets, payload).unwrap();
        let bytes = packet.to_bytes();

        let deserialized = SphinxPacket::from_bytes(&bytes).unwrap();
        assert_eq!(deserialized.header.routing_info, packet.header.routing_info);
        assert_eq!(deserialized.payload, packet.payload);
    }

    #[test]
    fn test_invalid_key_size() {
        let bad_secret = vec![0u8; 16]; // Wrong size
        let data = b"test data";

        let result = SphinxPacket::encrypt_layer(&bad_secret, data);
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), SphinxError::InvalidKeySize { .. }));
    }

    #[test]
    fn test_decryption_with_wrong_key() {
        let secret1 = create_test_shared_secret(1);
        let secret2 = create_test_shared_secret(2);
        let plaintext = b"Secret data";

        let ciphertext = SphinxPacket::encrypt_layer(&secret1, plaintext).unwrap();
        let result = SphinxPacket::decrypt_layer(&secret2, &ciphertext);

        // Should fail because wrong key
        assert!(result.is_err());
    }

    #[test]
    fn test_multiple_layer_encryption() {
        let secrets = vec![
            create_test_shared_secret(1),
            create_test_shared_secret(2),
            create_test_shared_secret(3),
        ];

        let plaintext = b"Multi-layer test";
        let mut encrypted = plaintext.to_vec();

        // Encrypt with all keys
        for secret in &secrets {
            encrypted = SphinxPacket::encrypt_layer(secret, &encrypted).unwrap();
        }

        // Decrypt in reverse order
        for secret in secrets.iter().rev() {
            encrypted = SphinxPacket::decrypt_layer(secret, &encrypted).unwrap();
        }

        assert_eq!(encrypted, plaintext);
    }
}
