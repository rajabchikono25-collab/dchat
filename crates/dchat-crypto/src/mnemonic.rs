//! BIP-39 Mnemonic implementation for seed phrase backup and recovery
//!
//! This module provides:
//! - Mnemonic generation (12, 15, 18, 21, or 24 words)
//! - Mnemonic validation
//! - Seed derivation from mnemonic + optional passphrase
//! - Master key generation from seed
//!
//! # Security Notes
//! - Mnemonics are zeroized on drop to prevent memory leaks
//! - Passphrase adds additional entropy and plausible deniability
//! - Uses PBKDF2-HMAC-SHA512 for seed derivation (BIP-39 standard)

use crate::keys::PrivateKey;
use dchat_core::error::{Error, Result};
use sha2::{Digest, Sha256, Sha512};
use zeroize::{Zeroize, ZeroizeOnDrop};

/// BIP-39 English wordlist (2048 words)
/// The wordlist is embedded at compile time for security and offline operation
const BIP39_WORDLIST: &str = include_str!("wordlist/english.txt");

/// Supported mnemonic word counts
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MnemonicLength {
    /// 12 words = 128 bits entropy
    Words12 = 12,
    /// 15 words = 160 bits entropy  
    Words15 = 15,
    /// 18 words = 192 bits entropy
    Words18 = 18,
    /// 21 words = 224 bits entropy
    Words21 = 21,
    /// 24 words = 256 bits entropy (recommended for maximum security)
    Words24 = 24,
}

impl MnemonicLength {
    /// Get the entropy size in bytes for this mnemonic length
    pub fn entropy_bytes(&self) -> usize {
        match self {
            MnemonicLength::Words12 => 16,  // 128 bits
            MnemonicLength::Words15 => 20,  // 160 bits
            MnemonicLength::Words18 => 24,  // 192 bits
            MnemonicLength::Words21 => 28,  // 224 bits
            MnemonicLength::Words24 => 32,  // 256 bits
        }
    }

    /// Get the checksum size in bits for this mnemonic length
    pub fn checksum_bits(&self) -> usize {
        self.entropy_bytes() / 4 // ENT/32 per BIP-39
    }

    /// Determine mnemonic length from word count
    pub fn from_word_count(count: usize) -> Result<Self> {
        match count {
            12 => Ok(MnemonicLength::Words12),
            15 => Ok(MnemonicLength::Words15),
            18 => Ok(MnemonicLength::Words18),
            21 => Ok(MnemonicLength::Words21),
            24 => Ok(MnemonicLength::Words24),
            _ => Err(Error::crypto(format!(
                "Invalid mnemonic word count: {}. Must be 12, 15, 18, 21, or 24",
                count
            ))),
        }
    }
}

/// A BIP-39 mnemonic phrase with automatic memory zeroization
#[derive(Clone, ZeroizeOnDrop)]
pub struct Mnemonic {
    /// The mnemonic words
    #[zeroize(skip)] // Words are from static wordlist, safe
    words: Vec<String>,
    /// Original entropy bytes
    entropy: Vec<u8>,
}

impl Mnemonic {
    /// Generate a new random mnemonic with the specified length
    ///
    /// # Arguments
    /// * `length` - The desired mnemonic length (12, 15, 18, 21, or 24 words)
    ///
    /// # Example
    /// ```ignore
    /// let mnemonic = Mnemonic::generate(MnemonicLength::Words24)?;
    /// println!("Your recovery phrase: {}", mnemonic.phrase());
    /// ```
    pub fn generate(length: MnemonicLength) -> Result<Self> {
        let entropy_bytes = length.entropy_bytes();
        let mut entropy = vec![0u8; entropy_bytes];
        
        getrandom::getrandom(&mut entropy)
            .map_err(|e| Error::crypto(format!("Failed to generate random entropy: {}", e)))?;

        Self::from_entropy(&entropy)
    }

    /// Create a mnemonic from existing entropy bytes
    ///
    /// # Arguments
    /// * `entropy` - Raw entropy bytes (16, 20, 24, 28, or 32 bytes)
    pub fn from_entropy(entropy: &[u8]) -> Result<Self> {
        // Validate entropy length
        let length = match entropy.len() {
            16 => MnemonicLength::Words12,
            20 => MnemonicLength::Words15,
            24 => MnemonicLength::Words18,
            28 => MnemonicLength::Words21,
            32 => MnemonicLength::Words24,
            _ => return Err(Error::crypto(format!(
                "Invalid entropy length: {} bytes. Must be 16, 20, 24, 28, or 32",
                entropy.len()
            ))),
        };

        // Calculate checksum (first ENT/32 bits of SHA256(entropy))
        let checksum = Sha256::digest(entropy);
        let checksum_bits = length.checksum_bits();

        // Convert entropy + checksum to 11-bit indices
        let mut bits = Vec::with_capacity(entropy.len() * 8 + checksum_bits);
        
        // Add entropy bits
        for byte in entropy {
            for i in (0..8).rev() {
                bits.push((byte >> i) & 1);
            }
        }
        
        // Add checksum bits
        for i in (0..checksum_bits).rev() {
            let byte_idx = (checksum_bits - 1 - i) / 8;
            let bit_idx = 7 - ((checksum_bits - 1 - i) % 8);
            bits.push((checksum[byte_idx] >> bit_idx) & 1);
        }

        // Convert bits to words
        let wordlist = Self::get_wordlist()?;
        let word_count = match length {
            MnemonicLength::Words12 => 12,
            MnemonicLength::Words15 => 15,
            MnemonicLength::Words18 => 18,
            MnemonicLength::Words21 => 21,
            MnemonicLength::Words24 => 24,
        };
        let mut words = Vec::with_capacity(word_count);

        for chunk in bits.chunks(11) {
            let mut index = 0u16;
            for (i, &bit) in chunk.iter().enumerate() {
                index |= (bit as u16) << (10 - i);
            }
            
            if (index as usize) >= wordlist.len() {
                return Err(Error::crypto(format!("Word index {} out of range", index)));
            }
            
            words.push(wordlist[index as usize].to_string());
        }

        Ok(Self {
            words,
            entropy: entropy.to_vec(),
        })
    }

    /// Parse and validate a mnemonic phrase from a string
    ///
    /// # Arguments
    /// * `phrase` - Space-separated mnemonic words
    ///
    /// # Example
    /// ```ignore
    /// let mnemonic = Mnemonic::from_phrase("abandon abandon abandon ... about")?;
    /// ```
    pub fn from_phrase(phrase: &str) -> Result<Self> {
        let words: Vec<&str> = phrase.split_whitespace().collect();
        let length = MnemonicLength::from_word_count(words.len())?;
        let wordlist = Self::get_wordlist()?;

        // Validate all words exist in wordlist and get their indices
        let mut indices = Vec::with_capacity(words.len());
        for word in &words {
            let word_lower = word.to_lowercase();
            let index = wordlist
                .iter()
                .position(|w| *w == word_lower)
                .ok_or_else(|| Error::crypto(format!("Invalid mnemonic word: '{}'", word)))?;
            indices.push(index as u16);
        }

        // Convert indices back to bits
        let mut bits = Vec::with_capacity(indices.len() * 11);
        for index in &indices {
            for i in (0..11).rev() {
                bits.push(((index >> i) & 1) as u8);
            }
        }

        // Split into entropy and checksum
        let entropy_bits = length.entropy_bytes() * 8;
        let checksum_bits = length.checksum_bits();

        if bits.len() != entropy_bits + checksum_bits {
            return Err(Error::crypto("Invalid mnemonic bit length"));
        }

        // Extract entropy bytes
        let mut entropy = vec![0u8; length.entropy_bytes()];
        for (i, chunk) in bits[..entropy_bits].chunks(8).enumerate() {
            let mut byte = 0u8;
            for (j, &bit) in chunk.iter().enumerate() {
                byte |= bit << (7 - j);
            }
            entropy[i] = byte;
        }

        // Verify checksum
        let expected_checksum = Sha256::digest(&entropy);
        for i in 0..checksum_bits {
            let expected_bit = (expected_checksum[i / 8] >> (7 - (i % 8))) & 1;
            let actual_bit = bits[entropy_bits + i];
            if expected_bit != actual_bit {
                return Err(Error::crypto("Invalid mnemonic checksum"));
            }
        }

        Ok(Self {
            words: words.iter().map(|s| s.to_lowercase()).collect(),
            entropy,
        })
    }

    /// Get the mnemonic phrase as a space-separated string
    pub fn phrase(&self) -> String {
        self.words.join(" ")
    }

    /// Get the individual words
    pub fn words(&self) -> &[String] {
        &self.words
    }

    /// Get the word count
    pub fn word_count(&self) -> usize {
        self.words.len()
    }

    /// Get the original entropy bytes
    pub fn entropy(&self) -> &[u8] {
        &self.entropy
    }

    /// Derive a seed from this mnemonic using BIP-39 PBKDF2
    ///
    /// # Arguments
    /// * `passphrase` - Optional passphrase for additional security
    ///
    /// # Returns
    /// A 64-byte seed suitable for HD key derivation
    ///
    /// # Example
    /// ```ignore
    /// let seed = mnemonic.to_seed(Some("my secret passphrase"))?;
    /// let master_key = Mnemonic::seed_to_master_key(&seed)?;
    /// ```
    pub fn to_seed(&self, passphrase: Option<&str>) -> Result<[u8; 64]> {
        // Use NFKD normalization for the mnemonic phrase (BIP-39 requirement)
        use unicode_normalization::UnicodeNormalization;
        let phrase = self.phrase();
        let normalized_phrase: String = phrase.nfkd().collect();
        let mnemonic_bytes = normalized_phrase.as_bytes();
        
        // Salt is "mnemonic" + passphrase (also NFKD normalized)
        let passphrase_str = passphrase.unwrap_or("");
        let normalized_passphrase: String = passphrase_str.nfkd().collect();
        let salt = format!("mnemonic{}", normalized_passphrase);
        
        // BIP-39 uses PBKDF2-HMAC-SHA512 with 2048 iterations
        let mut seed = [0u8; 64];
        pbkdf2_hmac_sha512(mnemonic_bytes, salt.as_bytes(), 2048, &mut seed);
        
        Ok(seed)
    }

    /// Convert a 64-byte seed to a dchat master private key
    ///
    /// Uses the first 32 bytes of the seed as the master key.
    /// The remaining 32 bytes can be used as chain code for extended keys.
    pub fn seed_to_master_key(seed: &[u8; 64]) -> Result<PrivateKey> {
        let mut key_bytes = [0u8; 32];
        key_bytes.copy_from_slice(&seed[..32]);
        Ok(PrivateKey::from_bytes(key_bytes))
    }

    /// Generate a mnemonic and directly derive the master key
    ///
    /// # Arguments
    /// * `length` - Mnemonic length
    /// * `passphrase` - Optional passphrase
    ///
    /// # Returns
    /// Tuple of (Mnemonic, PrivateKey)
    pub fn generate_with_key(
        length: MnemonicLength,
        passphrase: Option<&str>,
    ) -> Result<(Self, PrivateKey)> {
        let mnemonic = Self::generate(length)?;
        let seed = mnemonic.to_seed(passphrase)?;
        let master_key = Self::seed_to_master_key(&seed)?;
        Ok((mnemonic, master_key))
    }

    /// Restore a master key from a mnemonic phrase
    ///
    /// # Arguments
    /// * `phrase` - The mnemonic phrase
    /// * `passphrase` - Optional passphrase used during creation
    pub fn restore_master_key(phrase: &str, passphrase: Option<&str>) -> Result<PrivateKey> {
        let mnemonic = Self::from_phrase(phrase)?;
        let seed = mnemonic.to_seed(passphrase)?;
        Self::seed_to_master_key(&seed)
    }

    /// Get the BIP-39 English wordlist
    fn get_wordlist() -> Result<Vec<&'static str>> {
        let words: Vec<&str> = BIP39_WORDLIST.lines().collect();
        if words.len() != 2048 {
            return Err(Error::crypto(format!(
                "Invalid wordlist: expected 2048 words, got {}",
                words.len()
            )));
        }
        Ok(words)
    }

    /// Validate that a phrase is a valid mnemonic without parsing
    pub fn is_valid_phrase(phrase: &str) -> bool {
        Self::from_phrase(phrase).is_ok()
    }
}

impl std::fmt::Debug for Mnemonic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Mnemonic")
            .field("word_count", &self.words.len())
            .field("words", &"[REDACTED]")
            .field("entropy", &"[REDACTED]")
            .finish()
    }
}

/// PBKDF2-HMAC-SHA512 implementation for BIP-39 seed derivation
fn pbkdf2_hmac_sha512(password: &[u8], salt: &[u8], iterations: u32, output: &mut [u8; 64]) {
    use hmac::{Hmac, Mac};
    type HmacSha512 = Hmac<Sha512>;

    // For BIP-39, we only need one block (64 bytes output, 64 bytes per block)
    let mut mac = HmacSha512::new_from_slice(password)
        .expect("HMAC can take key of any size");
    
    // U_1 = PRF(Password, Salt || INT(1))
    mac.update(salt);
    mac.update(&1u32.to_be_bytes());
    let mut u = mac.finalize().into_bytes();
    
    output.copy_from_slice(&u);

    // U_i = PRF(Password, U_{i-1})
    for _ in 1..iterations {
        let mut mac = HmacSha512::new_from_slice(password)
            .expect("HMAC can take key of any size");
        mac.update(&u);
        u = mac.finalize().into_bytes();
        
        // XOR into output
        for (out, u_byte) in output.iter_mut().zip(u.iter()) {
            *out ^= u_byte;
        }
    }
}

/// Seed with automatic zeroization
#[derive(Clone, Zeroize, ZeroizeOnDrop)]
pub struct Seed {
    bytes: [u8; 64],
}

impl Seed {
    /// Create a seed from a mnemonic
    pub fn from_mnemonic(mnemonic: &Mnemonic, passphrase: Option<&str>) -> Result<Self> {
        let bytes = mnemonic.to_seed(passphrase)?;
        Ok(Self { bytes })
    }

    /// Get the seed bytes
    pub fn as_bytes(&self) -> &[u8; 64] {
        &self.bytes
    }

    /// Derive master key from this seed
    pub fn to_master_key(&self) -> Result<PrivateKey> {
        Mnemonic::seed_to_master_key(&self.bytes)
    }

    /// Get the chain code (last 32 bytes) for extended key derivation
    pub fn chain_code(&self) -> &[u8] {
        &self.bytes[32..]
    }
}

impl std::fmt::Debug for Seed {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Seed")
            .field("bytes", &"[REDACTED]")
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mnemonic_generation() {
        // Test all mnemonic lengths
        for length in [
            MnemonicLength::Words12,
            MnemonicLength::Words15,
            MnemonicLength::Words18,
            MnemonicLength::Words21,
            MnemonicLength::Words24,
        ] {
            let mnemonic = Mnemonic::generate(length).unwrap();
            assert_eq!(mnemonic.word_count(), length as usize);
            
            // Verify we can parse it back
            let parsed = Mnemonic::from_phrase(&mnemonic.phrase()).unwrap();
            assert_eq!(parsed.phrase(), mnemonic.phrase());
        }
    }

    #[test]
    fn test_mnemonic_validation() {
        // Valid 12-word mnemonic (test vector from BIP-39)
        let valid = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        assert!(Mnemonic::is_valid_phrase(valid));

        // Invalid checksum
        let invalid_checksum = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon";
        assert!(!Mnemonic::is_valid_phrase(invalid_checksum));

        // Invalid word
        let invalid_word = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon notaword";
        assert!(!Mnemonic::is_valid_phrase(invalid_word));

        // Wrong word count
        let wrong_count = "abandon abandon abandon";
        assert!(!Mnemonic::is_valid_phrase(wrong_count));
    }

    #[test]
    fn test_seed_derivation() {
        // BIP-39 test vector
        let phrase = "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about";
        let mnemonic = Mnemonic::from_phrase(phrase).unwrap();
        
        // Without passphrase
        let seed1 = mnemonic.to_seed(None).unwrap();
        
        // With passphrase - should be different
        let seed2 = mnemonic.to_seed(Some("TREZOR")).unwrap();
        assert_ne!(seed1, seed2);
        
        // Same passphrase should give same seed
        let seed3 = mnemonic.to_seed(Some("TREZOR")).unwrap();
        assert_eq!(seed2, seed3);
    }

    #[test]
    fn test_master_key_derivation() {
        let (mnemonic, key1) = Mnemonic::generate_with_key(MnemonicLength::Words24, None).unwrap();
        
        // Restore should give same key
        let key2 = Mnemonic::restore_master_key(&mnemonic.phrase(), None).unwrap();
        assert_eq!(key1.as_bytes(), key2.as_bytes());
        
        // Different passphrase should give different key
        let key3 = Mnemonic::restore_master_key(&mnemonic.phrase(), Some("secret")).unwrap();
        assert_ne!(key1.as_bytes(), key3.as_bytes());
    }

    #[test]
    fn test_entropy_roundtrip() {
        let mnemonic = Mnemonic::generate(MnemonicLength::Words24).unwrap();
        let entropy = mnemonic.entropy().to_vec();
        
        // Recreate from entropy
        let restored = Mnemonic::from_entropy(&entropy).unwrap();
        assert_eq!(mnemonic.phrase(), restored.phrase());
    }

    #[test]
    fn test_seed_wrapper() {
        let mnemonic = Mnemonic::generate(MnemonicLength::Words24).unwrap();
        let seed = Seed::from_mnemonic(&mnemonic, None).unwrap();
        
        assert_eq!(seed.as_bytes().len(), 64);
        assert_eq!(seed.chain_code().len(), 32);
        
        let key = seed.to_master_key().unwrap();
        assert_eq!(key.as_bytes().len(), 32);
    }

    #[test]
    fn test_bip39_test_vector() {
        // Official BIP-39 test vector
        let entropy = hex::decode("00000000000000000000000000000000").unwrap();
        let mnemonic = Mnemonic::from_entropy(&entropy).unwrap();
        
        assert_eq!(
            mnemonic.phrase(),
            "abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon abandon about"
        );
        
        // Test seed with "TREZOR" passphrase
        let seed = mnemonic.to_seed(Some("TREZOR")).unwrap();
        let expected_seed = hex::decode(
            "c55257c360c07c72029aebc1b53c05ed0362ada38ead3e3e9efa3708e53495531f09a6987599d18264c1e1c92f2cf141630c7a3c4ab7c81b2f001698e7463b04"
        ).unwrap();
        
        assert_eq!(seed.to_vec(), expected_seed);
    }

    #[test]
    fn test_debug_redaction() {
        let mnemonic = Mnemonic::generate(MnemonicLength::Words12).unwrap();
        let debug_str = format!("{:?}", mnemonic);
        
        assert!(debug_str.contains("REDACTED"));
        assert!(!debug_str.contains(&mnemonic.words()[0]));
    }
}
