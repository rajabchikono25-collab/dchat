//! Solana-Compatible Address and Signature Types
//!
//! This module provides Solana ecosystem compatibility:
//! - Base58 address encoding (same as Solana)
//! - Ed25519 signature format (native to both dchat and Solana)
//! - SPL token address derivation
//! - Cross-chain address mapping

use dchat_core::error::{Error, Result};
use dchat_crypto::keys::PublicKey;
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

/// Base58 alphabet used by Solana (Bitcoin alphabet)
const BASE58_ALPHABET: &[u8; 58] = b"123456789ABCDEFGHJKLMNPQRSTUVWXYZabcdefghijkmnopqrstuvwxyz";

/// A Solana-compatible address (32-byte Ed25519 public key in Base58)
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SolanaAddress {
    /// Raw 32-byte public key
    bytes: [u8; 32],
}

impl SolanaAddress {
    /// Create from raw 32-byte public key
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 32 {
            return Err(Error::crypto(format!(
                "Invalid Solana address length: expected 32, got {}",
                bytes.len()
            )));
        }
        
        let mut arr = [0u8; 32];
        arr.copy_from_slice(bytes);
        Ok(Self { bytes: arr })
    }

    /// Create from dchat PublicKey
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Self {
            bytes: *public_key.as_bytes(),
        }
    }

    /// Parse from Base58 string
    pub fn from_base58(s: &str) -> Result<Self> {
        let bytes = base58_decode(s)?;
        Self::from_bytes(&bytes)
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.bytes
    }

    /// Convert to Base58 string (Solana format)
    pub fn to_base58(&self) -> String {
        base58_encode(&self.bytes)
    }

    /// Convert to dchat PublicKey
    pub fn to_public_key(&self) -> PublicKey {
        PublicKey::from_bytes(self.bytes)
    }

    /// Check if this is a valid Solana program address
    pub fn is_on_curve(&self) -> bool {
        // Try to create a valid Ed25519 public key
        ed25519_dalek::VerifyingKey::from_bytes(&self.bytes).is_ok()
    }

    /// Derive a Program Derived Address (PDA) - Solana-style
    /// 
    /// PDAs are off-curve addresses derived from seeds and a program ID.
    /// Used for deterministic account addresses in Solana programs.
    pub fn derive_pda(seeds: &[&[u8]], program_id: &SolanaAddress) -> Result<(Self, u8)> {
        for bump in (0u8..=255).rev() {
            let mut hasher = blake3::Hasher::new();
            for seed in seeds {
                hasher.update(seed);
            }
            hasher.update(&[bump]);
            hasher.update(program_id.as_bytes());
            hasher.update(b"ProgramDerivedAddress");
            
            let hash = hasher.finalize();
            let candidate = Self::from_bytes(hash.as_bytes())?;
            
            // PDA must be off-curve
            if !candidate.is_on_curve() {
                return Ok((candidate, bump));
            }
        }
        
        Err(Error::crypto("Could not find valid PDA bump seed"))
    }

    /// Find associated token address (like Solana's ATA)
    pub fn find_associated_token_address(
        &self,
        token_mint: &SolanaAddress,
        token_program: &SolanaAddress,
    ) -> Result<Self> {
        let seeds: &[&[u8]] = &[
            self.as_bytes(),
            token_program.as_bytes(),
            token_mint.as_bytes(),
        ];
        
        // Use a deterministic program ID for dchat token program
        let ata_program = SolanaAddress::from_bytes(&[
            0x8c, 0x97, 0x25, 0x8f, 0x4e, 0x24, 0x89, 0xf1,
            0xbb, 0x3d, 0x10, 0x29, 0x14, 0x8e, 0x0d, 0x83,
            0x0b, 0x5a, 0x13, 0x99, 0xda, 0xff, 0x10, 0x84,
            0x04, 0x8e, 0x7b, 0xd8, 0xdb, 0xe9, 0xf8, 0x59,
        ])?;
        
        let (pda, _) = Self::derive_pda(seeds, &ata_program)?;
        Ok(pda)
    }
}

impl fmt::Display for SolanaAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl fmt::Debug for SolanaAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "SolanaAddress({})", self.to_base58())
    }
}

impl FromStr for SolanaAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::from_base58(s)
    }
}

/// A Solana-compatible signature (64-byte Ed25519)
#[derive(Clone, PartialEq, Eq)]
pub struct SolanaSignature {
    bytes: [u8; 64],
}

impl Serialize for SolanaSignature {
    fn serialize<S>(&self, serializer: S) -> std::result::Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_base58())
    }
}

impl<'de> Deserialize<'de> for SolanaSignature {
    fn deserialize<D>(deserializer: D) -> std::result::Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        Self::from_base58(&s).map_err(serde::de::Error::custom)
    }
}

impl SolanaSignature {
    /// Create from raw bytes
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        if bytes.len() != 64 {
            return Err(Error::crypto(format!(
                "Invalid signature length: expected 64, got {}",
                bytes.len()
            )));
        }
        
        let mut arr = [0u8; 64];
        arr.copy_from_slice(bytes);
        Ok(Self { bytes: arr })
    }

    /// Parse from Base58 string
    pub fn from_base58(s: &str) -> Result<Self> {
        let bytes = base58_decode(s)?;
        Self::from_bytes(&bytes)
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.bytes
    }

    /// Convert to Base58 string
    pub fn to_base58(&self) -> String {
        base58_encode(&self.bytes)
    }

    /// Verify this signature
    pub fn verify(&self, message: &[u8], public_key: &SolanaAddress) -> Result<bool> {
        use ed25519_dalek::{Signature, VerifyingKey};

        let verifying_key = VerifyingKey::from_bytes(public_key.as_bytes())
            .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

        let signature = Signature::from_bytes(&self.bytes);

        verifying_key.verify_strict(message, &signature)
            .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))?;

        Ok(true)
    }
}

impl fmt::Display for SolanaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

impl fmt::Debug for SolanaSignature {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let short = &self.to_base58()[..16];
        write!(f, "SolanaSignature({}...)", short)
    }
}

/// Trait for types that can be converted to Solana-compatible format
pub trait SolanaCompatible {
    /// Get Solana-compatible address
    fn to_solana_address(&self) -> SolanaAddress;
    
    /// Sign message in Solana-compatible format
    fn sign_solana(&self, message: &[u8]) -> Result<SolanaSignature>;
}

/// Encode bytes to Base58 (Solana/Bitcoin format)
pub fn base58_encode(data: &[u8]) -> String {
    if data.is_empty() {
        return String::new();
    }

    // Count leading zeros
    let leading_zeros = data.iter().take_while(|&&b| b == 0).count();

    // Convert to base58
    let mut result = Vec::new();
    let mut num = data.to_vec();

    while !num.is_empty() && !(num.len() == 1 && num[0] == 0) {
        let mut carry = 0u32;
        let mut new_num = Vec::new();

        for byte in &num {
            carry = carry * 256 + *byte as u32;
            if !new_num.is_empty() || carry / 58 > 0 {
                new_num.push((carry / 58) as u8);
            }
            carry %= 58;
        }

        result.push(BASE58_ALPHABET[carry as usize]);
        num = new_num;
    }

    // Add leading '1's for leading zeros
    for _ in 0..leading_zeros {
        result.push(b'1');
    }

    result.reverse();
    String::from_utf8(result).unwrap()
}

/// Decode Base58 string to bytes
pub fn base58_decode(s: &str) -> Result<Vec<u8>> {
    if s.is_empty() {
        return Ok(Vec::new());
    }

    // Build reverse lookup table
    let mut alphabet_map = [255u8; 128];
    for (i, &c) in BASE58_ALPHABET.iter().enumerate() {
        alphabet_map[c as usize] = i as u8;
    }

    // Count leading '1's
    let leading_ones = s.bytes().take_while(|&b| b == b'1').count();

    // Convert from base58
    let mut result: Vec<u8> = Vec::new();

    for c in s.bytes() {
        if c >= 128 {
            return Err(Error::crypto("Invalid Base58 character"));
        }
        
        let digit = alphabet_map[c as usize];
        if digit == 255 {
            return Err(Error::crypto(format!("Invalid Base58 character: {}", c as char)));
        }

        let mut carry = digit as u32;
        for byte in result.iter_mut().rev() {
            carry += (*byte as u32) * 58;
            *byte = (carry & 0xFF) as u8;
            carry >>= 8;
        }

        while carry > 0 {
            result.insert(0, (carry & 0xFF) as u8);
            carry >>= 8;
        }
    }

    // Add leading zeros
    let mut final_result = vec![0u8; leading_ones];
    final_result.extend(result);

    Ok(final_result)
}

/// Solana token mint information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenMint {
    /// Mint address
    pub address: SolanaAddress,
    /// Token symbol
    pub symbol: String,
    /// Token name
    pub name: String,
    /// Decimal places
    pub decimals: u8,
    /// Total supply (if known)
    pub supply: Option<u64>,
}

impl TokenMint {
    /// Create DCHAT token mint (wrapped DCHAT on Solana)
    pub fn dchat_wrapped() -> Result<Self> {
        // Deterministic mint address for wrapped DCHAT
        let seed = b"dchat_wrapped_token_mint_v1";
        let hash = blake3::hash(seed);
        
        Ok(Self {
            address: SolanaAddress::from_bytes(&hash.as_bytes()[..32])?,
            symbol: "wDCHAT".to_string(),
            name: "Wrapped DCHAT".to_string(),
            decimals: 9, // Same as SOL
            supply: None,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_base58_roundtrip() {
        let original = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07,
            0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
            0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17,
            0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e, 0x1f,
        ];
        
        let encoded = base58_encode(&original);
        let decoded = base58_decode(&encoded).unwrap();
        
        assert_eq!(original.to_vec(), decoded);
    }

    #[test]
    fn test_solana_address_from_pubkey() {
        use dchat_crypto::keys::KeyPair;
        
        let keypair = KeyPair::try_generate().unwrap();
        let public_key = keypair.public_key();
        
        let solana_addr = SolanaAddress::from_public_key(public_key);
        let roundtrip = SolanaAddress::from_base58(&solana_addr.to_base58()).unwrap();
        
        assert_eq!(solana_addr.as_bytes(), roundtrip.as_bytes());
    }

    #[test]
    fn test_solana_address_is_on_curve() {
        use dchat_crypto::keys::KeyPair;
        
        let keypair = KeyPair::try_generate().unwrap();
        let solana_addr = SolanaAddress::from_public_key(keypair.public_key());
        
        // Valid Ed25519 public key should be on curve
        assert!(solana_addr.is_on_curve());
    }

    #[test]
    fn test_pda_derivation() {
        let program_id = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let seeds: &[&[u8]] = &[b"test", b"seed"];
        
        let (pda, bump) = SolanaAddress::derive_pda(seeds, &program_id).unwrap();
        
        // PDA should be off curve
        assert!(!pda.is_on_curve());
        // Bump should be valid u8 (this is always true for u8, but verifies the type)
        let _: u8 = bump;
    }

    #[test]
    fn test_signature_verification() {
        use ed25519_dalek::{Signer, SigningKey};
        
        let signing_key = SigningKey::generate(&mut rand::thread_rng());
        let verifying_key = signing_key.verifying_key();
        
        let message = b"test message";
        let signature = signing_key.sign(message);
        
        let solana_addr = SolanaAddress::from_bytes(&verifying_key.to_bytes()).unwrap();
        let solana_sig = SolanaSignature::from_bytes(&signature.to_bytes()).unwrap();
        
        assert!(solana_sig.verify(message, &solana_addr).unwrap());
    }

    #[test]
    fn test_base58_known_vectors() {
        // Test with known Base58 values
        let test_cases = [
            (vec![0u8], "1"),
            (vec![0, 0, 0, 1], "1112"),
        ];
        
        for (bytes, expected) in test_cases {
            let encoded = base58_encode(&bytes);
            assert_eq!(encoded, expected);
            
            let decoded = base58_decode(expected).unwrap();
            assert_eq!(decoded, bytes);
        }
    }
}
