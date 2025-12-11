//! Universal Address System
//!
//! Provides a unified address format that works across:
//! - dchat native (0x... hex format)
//! - Solana (Base58 format)
//! - Ethereum-compatible (0x... format)
//! - Bridge addresses

use dchat_core::error::{Error, Result};
use dchat_crypto::keys::{Address, PublicKey};
use serde::{Deserialize, Serialize};
use std::fmt;
use std::str::FromStr;

use super::solana_compat::{base58_decode, base58_encode, SolanaAddress};

/// Supported address formats
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum AddressFormat {
    /// dchat native (0x + 40 hex chars = 20 bytes)
    DchatNative,
    /// Solana format (Base58 encoded 32-byte public key)
    Solana,
    /// Ethereum-compatible (0x + 40 hex chars = 20 bytes)
    Ethereum,
    /// Bridge address (prefixed format for cross-chain)
    Bridge,
}

impl AddressFormat {
    /// Detect format from string
    pub fn detect(s: &str) -> Option<Self> {
        if s.starts_with("0x") && s.len() == 42 {
            // Could be dchat or Ethereum - default to dchat
            Some(AddressFormat::DchatNative)
        } else if s.starts_with("bridge:") {
            Some(AddressFormat::Bridge)
        } else if s.chars().all(|c| {
            c.is_ascii_alphanumeric() && c != '0' && c != 'O' && c != 'I' && c != 'l'
        }) && s.len() >= 32 && s.len() <= 44 {
            Some(AddressFormat::Solana)
        } else {
            None
        }
    }
}

/// A universal address that can represent addresses across chains
#[derive(Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UniversalAddress {
    /// Raw address bytes
    bytes: Vec<u8>,
    /// Original format
    format: AddressFormat,
    /// Original chain ID (if known)
    chain_id: Option<u32>,
}

impl UniversalAddress {
    /// Create from dchat native address
    pub fn from_dchat(address: &Address) -> Self {
        Self {
            bytes: address.as_bytes().to_vec(),
            format: AddressFormat::DchatNative,
            chain_id: Some(1337),
        }
    }

    /// Create from Solana address
    pub fn from_solana(address: &SolanaAddress) -> Self {
        Self {
            bytes: address.as_bytes().to_vec(),
            format: AddressFormat::Solana,
            chain_id: None, // Solana doesn't use chain IDs the same way
        }
    }

    /// Create from public key (auto-derives both formats)
    pub fn from_public_key(public_key: &PublicKey, format: AddressFormat) -> Self {
        match format {
            AddressFormat::Solana => Self {
                bytes: public_key.as_bytes().to_vec(),
                format: AddressFormat::Solana,
                chain_id: None,
            },
            AddressFormat::DchatNative | AddressFormat::Ethereum => {
                let address = public_key.to_address();
                Self {
                    bytes: address.as_bytes().to_vec(),
                    format,
                    chain_id: Some(1337),
                }
            },
            AddressFormat::Bridge => {
                // Bridge format includes both formats
                let address = public_key.to_address();
                Self {
                    bytes: address.as_bytes().to_vec(),
                    format: AddressFormat::Bridge,
                    chain_id: None,
                }
            }
        }
    }

    /// Parse from string (auto-detects format)
    pub fn parse(s: &str) -> Result<Self> {
        let format = AddressFormat::detect(s)
            .ok_or_else(|| Error::validation("Unknown address format"))?;

        match format {
            AddressFormat::DchatNative | AddressFormat::Ethereum => {
                let hex_str = s.strip_prefix("0x").unwrap_or(s);
                let bytes = hex::decode(hex_str)
                    .map_err(|e| Error::validation(format!("Invalid hex: {}", e)))?;
                
                if bytes.len() != 20 {
                    return Err(Error::validation("Invalid address length"));
                }

                Ok(Self {
                    bytes,
                    format,
                    chain_id: if format == AddressFormat::DchatNative { Some(1337) } else { None },
                })
            }
            AddressFormat::Solana => {
                let bytes = base58_decode(s)?;
                if bytes.len() != 32 {
                    return Err(Error::validation("Invalid Solana address length"));
                }

                Ok(Self {
                    bytes,
                    format: AddressFormat::Solana,
                    chain_id: None,
                })
            }
            AddressFormat::Bridge => {
                let parts: Vec<&str> = s.split(':').collect();
                if parts.len() != 3 {
                    return Err(Error::validation("Invalid bridge address format"));
                }
                
                let chain = parts[1];
                let addr = parts[2];
                
                let (bytes, chain_id) = match chain {
                    "dchat" => {
                        let hex_str = addr.strip_prefix("0x").unwrap_or(addr);
                        let bytes = hex::decode(hex_str)
                            .map_err(|e| Error::validation(format!("Invalid hex: {}", e)))?;
                        (bytes, Some(1337))
                    }
                    "solana" => {
                        let bytes = base58_decode(addr)?;
                        (bytes, None)
                    }
                    _ => return Err(Error::validation("Unknown bridge chain")),
                };

                Ok(Self {
                    bytes,
                    format: AddressFormat::Bridge,
                    chain_id,
                })
            }
        }
    }

    /// Get address format
    pub fn format(&self) -> AddressFormat {
        self.format
    }

    /// Get chain ID (if known)
    pub fn chain_id(&self) -> Option<u32> {
        self.chain_id
    }

    /// Get raw bytes
    pub fn as_bytes(&self) -> &[u8] {
        &self.bytes
    }

    /// Convert to dchat native address (if compatible)
    pub fn to_dchat_address(&self) -> Result<Address> {
        match self.format {
            AddressFormat::DchatNative | AddressFormat::Ethereum | AddressFormat::Bridge => {
                if self.bytes.len() == 20 {
                    let mut arr = [0u8; 20];
                    arr.copy_from_slice(&self.bytes);
                    Ok(Address::from_bytes(arr))
                } else {
                    Err(Error::validation("Cannot convert to dchat address"))
                }
            }
            AddressFormat::Solana => {
                // Derive dchat address from Solana pubkey
                if self.bytes.len() == 32 {
                    let hash = blake3::hash(&self.bytes);
                    let mut addr_bytes = [0u8; 20];
                    addr_bytes.copy_from_slice(&hash.as_bytes()[..20]);
                    Ok(Address::from_bytes(addr_bytes))
                } else {
                    Err(Error::validation("Invalid Solana address"))
                }
            }
        }
    }

    /// Convert to Solana address (if compatible)
    pub fn to_solana_address(&self) -> Result<SolanaAddress> {
        match self.format {
            AddressFormat::Solana => {
                SolanaAddress::from_bytes(&self.bytes)
            }
            AddressFormat::DchatNative | AddressFormat::Ethereum | AddressFormat::Bridge => {
                // Cannot directly convert 20-byte address to 32-byte Solana pubkey
                // Would need the original public key
                Err(Error::validation(
                    "Cannot convert to Solana address without public key"
                ))
            }
        }
    }

    /// Format as string for specific chain
    pub fn format_for(&self, target_format: AddressFormat) -> Result<String> {
        match target_format {
            AddressFormat::DchatNative | AddressFormat::Ethereum => {
                let addr = self.to_dchat_address()?;
                Ok(addr.to_hex())
            }
            AddressFormat::Solana => {
                if self.format == AddressFormat::Solana {
                    Ok(base58_encode(&self.bytes))
                } else {
                    Err(Error::validation("Cannot format as Solana address"))
                }
            }
            AddressFormat::Bridge => {
                let chain = match self.format {
                    AddressFormat::Solana => "solana",
                    _ => "dchat",
                };
                let addr = if self.format == AddressFormat::Solana {
                    base58_encode(&self.bytes)
                } else {
                    format!("0x{}", hex::encode(&self.bytes))
                };
                Ok(format!("bridge:{}:{}", chain, addr))
            }
        }
    }

    /// Check if address is valid on a specific chain
    pub fn is_valid_for(&self, target_format: AddressFormat) -> bool {
        match (self.format, target_format) {
            (AddressFormat::Solana, AddressFormat::Solana) => self.bytes.len() == 32,
            (AddressFormat::DchatNative, AddressFormat::DchatNative) => self.bytes.len() == 20,
            (AddressFormat::DchatNative, AddressFormat::Ethereum) => self.bytes.len() == 20,
            (AddressFormat::Ethereum, AddressFormat::DchatNative) => self.bytes.len() == 20,
            (_, AddressFormat::Bridge) => true, // Bridge accepts all
            _ => false,
        }
    }
}

impl fmt::Display for UniversalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.format {
            AddressFormat::Solana => {
                write!(f, "{}", base58_encode(&self.bytes))
            }
            AddressFormat::DchatNative | AddressFormat::Ethereum => {
                write!(f, "0x{}", hex::encode(&self.bytes))
            }
            AddressFormat::Bridge => {
                let chain = match self.format {
                    AddressFormat::Solana => "solana",
                    _ => "dchat",
                };
                write!(f, "bridge:{}:{}", chain, 
                    if self.bytes.len() == 32 {
                        base58_encode(&self.bytes)
                    } else {
                        format!("0x{}", hex::encode(&self.bytes))
                    }
                )
            }
        }
    }
}

impl fmt::Debug for UniversalAddress {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("UniversalAddress")
            .field("address", &self.to_string())
            .field("format", &self.format)
            .field("chain_id", &self.chain_id)
            .finish()
    }
}

impl FromStr for UniversalAddress {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        Self::parse(s)
    }
}

/// Address mapping for cross-chain operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AddressMapping {
    /// dchat native address
    pub dchat: Option<Address>,
    /// Solana address
    pub solana: Option<SolanaAddress>,
    /// Original public key (for deriving all formats)
    pub public_key: Option<PublicKey>,
    /// Verified on-chain (both sides confirmed)
    pub verified: bool,
}

impl AddressMapping {
    /// Create mapping from public key
    pub fn from_public_key(public_key: &PublicKey) -> Self {
        Self {
            dchat: Some(public_key.to_address()),
            solana: Some(SolanaAddress::from_public_key(public_key)),
            public_key: Some(public_key.clone()),
            verified: false,
        }
    }

    /// Create mapping from dchat address only
    pub fn from_dchat(address: Address) -> Self {
        Self {
            dchat: Some(address),
            solana: None,
            public_key: None,
            verified: false,
        }
    }

    /// Create mapping from Solana address only
    pub fn from_solana(address: SolanaAddress) -> Self {
        Self {
            dchat: None,
            solana: Some(address),
            public_key: None,
            verified: false,
        }
    }

    /// Check if mapping is complete (both addresses known)
    pub fn is_complete(&self) -> bool {
        self.dchat.is_some() && self.solana.is_some()
    }

    /// Get universal address for bridge operations
    pub fn to_universal(&self, preferred_format: AddressFormat) -> Option<UniversalAddress> {
        match preferred_format {
            AddressFormat::Solana => {
                self.solana.as_ref().map(UniversalAddress::from_solana)
            }
            AddressFormat::DchatNative => {
                self.dchat.as_ref().map(UniversalAddress::from_dchat)
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    #[test]
    fn test_format_detection() {
        // dchat/Ethereum format
        assert_eq!(
            AddressFormat::detect("0x1234567890abcdef1234567890abcdef12345678"),
            Some(AddressFormat::DchatNative)
        );

        // Bridge format
        assert_eq!(
            AddressFormat::detect("bridge:dchat:0x1234567890abcdef1234567890abcdef12345678"),
            Some(AddressFormat::Bridge)
        );
    }

    #[test]
    fn test_universal_address_from_pubkey() {
        let keypair = KeyPair::try_generate().unwrap();
        let pubkey = keypair.public_key();

        let dchat_addr = UniversalAddress::from_public_key(pubkey, AddressFormat::DchatNative);
        let solana_addr = UniversalAddress::from_public_key(pubkey, AddressFormat::Solana);

        assert_eq!(dchat_addr.format(), AddressFormat::DchatNative);
        assert_eq!(solana_addr.format(), AddressFormat::Solana);

        // Solana should be 32 bytes
        assert_eq!(solana_addr.as_bytes().len(), 32);
        
        // dchat should be 20 bytes
        assert_eq!(dchat_addr.as_bytes().len(), 20);
    }

    #[test]
    fn test_address_parsing() {
        // Parse dchat address
        let addr = UniversalAddress::parse("0x1234567890abcdef1234567890abcdef12345678").unwrap();
        assert_eq!(addr.format(), AddressFormat::DchatNative);

        // Parse Solana address
        let keypair = KeyPair::try_generate().unwrap();
        let solana = SolanaAddress::from_public_key(keypair.public_key());
        let parsed = UniversalAddress::parse(&solana.to_base58()).unwrap();
        assert_eq!(parsed.format(), AddressFormat::Solana);
    }

    #[test]
    fn test_address_mapping() {
        let keypair = KeyPair::try_generate().unwrap();
        let pubkey = keypair.public_key();

        let mapping = AddressMapping::from_public_key(pubkey);
        
        assert!(mapping.is_complete());
        assert!(mapping.dchat.is_some());
        assert!(mapping.solana.is_some());
    }

    #[test]
    fn test_bridge_format() {
        let keypair = KeyPair::try_generate().unwrap();
        let addr = UniversalAddress::from_public_key(
            keypair.public_key(),
            AddressFormat::DchatNative
        );

        let bridge_str = addr.format_for(AddressFormat::Bridge).unwrap();
        assert!(bridge_str.starts_with("bridge:dchat:0x"));
    }
}
