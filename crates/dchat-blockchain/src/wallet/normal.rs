//! Normal (single-key) wallet implementation
//!
//! Production-ready single-signer wallet with:
//! - BIP-39 mnemonic backup
//! - Hierarchical key derivation (BIP-44)
//! - Solana-compatible address format
//! - Hardware wallet support interface

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_crypto::keys::{Address, KeyDerivation, PrivateKey, PublicKey};
use dchat_crypto::{Mnemonic, MnemonicLength, Seed};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use zeroize::ZeroizeOnDrop;

use super::{SignedTransaction, TransactionSignature, WalletBalance, WalletTransaction};
use super::solana_compat::SolanaAddress;

/// Wallet type enumeration
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletType {
    /// Standard single-signature wallet
    Normal,
    /// Multi-signature wallet (requires M-of-N signatures)
    MultiSig,
    /// Temporary burner wallet with expiration
    Burner,
    /// Hardware wallet (keys never leave device)
    Hardware,
    /// Watch-only wallet (no signing capability)
    WatchOnly,
}

/// Wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConfig {
    /// Wallet name/label
    pub name: String,
    /// Wallet type
    pub wallet_type: WalletType,
    /// BIP-44 account index
    pub account_index: u32,
    /// Default address index
    pub address_index: u32,
    /// Chain ID for transactions
    pub chain_id: u32,
    /// Enable Solana compatibility mode
    pub solana_compatible: bool,
}

impl Default for WalletConfig {
    fn default() -> Self {
        Self {
            name: "Default Wallet".to_string(),
            wallet_type: WalletType::Normal,
            account_index: 0,
            address_index: 0,
            chain_id: 1337, // dchat chain ID
            solana_compatible: true,
        }
    }
}

/// A production-ready wallet implementation
#[derive(ZeroizeOnDrop)]
pub struct Wallet {
    /// Wallet unique identifier
    #[zeroize(skip)]
    id: uuid::Uuid,
    /// Wallet configuration
    #[zeroize(skip)]
    config: WalletConfig,
    /// Master private key (from seed)
    master_key: Option<PrivateKey>,
    /// Derived signing key for current account
    signing_key: Option<SigningKey>,
    /// Public key
    #[zeroize(skip)]
    public_key: Option<PublicKey>,
    /// Blockchain address
    #[zeroize(skip)]
    address: Option<Address>,
    /// Solana-compatible address
    #[zeroize(skip)]
    solana_address: Option<SolanaAddress>,
    /// Transaction nonce (for replay protection)
    #[zeroize(skip)]
    nonce: u64,
    /// Cached balance (should be refreshed from chain)
    #[zeroize(skip)]
    balance: WalletBalance,
    /// Created timestamp
    #[zeroize(skip)]
    created_at: DateTime<Utc>,
    /// Address derivation cache
    #[zeroize(skip)]
    derived_addresses: HashMap<u32, Address>,
}

impl Wallet {
    /// Create a new wallet from a fresh mnemonic
    ///
    /// # Arguments
    /// * `config` - Wallet configuration
    /// * `mnemonic_length` - Number of words (12, 15, 18, 21, or 24)
    /// * `passphrase` - Optional passphrase for additional security
    ///
    /// # Returns
    /// Tuple of (Wallet, Mnemonic phrase for backup)
    pub fn create(
        config: WalletConfig,
        mnemonic_length: MnemonicLength,
        passphrase: Option<&str>,
    ) -> Result<(Self, String)> {
        let mnemonic = Mnemonic::generate(mnemonic_length)?;
        let phrase = mnemonic.phrase();
        
        let wallet = Self::from_mnemonic(&phrase, passphrase, config)?;
        
        Ok((wallet, phrase))
    }

    /// Restore a wallet from a mnemonic phrase
    pub fn from_mnemonic(
        phrase: &str,
        passphrase: Option<&str>,
        config: WalletConfig,
    ) -> Result<Self> {
        let mnemonic = Mnemonic::from_phrase(phrase)?;
        let seed = Seed::from_mnemonic(&mnemonic, passphrase)?;
        let master_key = seed.to_master_key()?;
        
        Self::from_master_key(master_key, config)
    }

    /// Create wallet from existing master key
    pub fn from_master_key(master_key: PrivateKey, config: WalletConfig) -> Result<Self> {
        // Derive account key using BIP-44 path: m/44'/1337'/account'/0/index
        let path = [44, 1337, config.account_index, 0, config.address_index];
        let derived_key = KeyDerivation::derive_key_path(&master_key, &path)?;
        
        // Create signing key
        let signing_key = SigningKey::from_bytes(derived_key.as_bytes());
        let verifying_key = signing_key.verifying_key();
        
        // Create public key and addresses
        let public_key = PublicKey::from_bytes(verifying_key.to_bytes());
        let address = public_key.to_address();
        let solana_address = SolanaAddress::from_public_key(&public_key);
        
        Ok(Self {
            id: uuid::Uuid::new_v4(),
            config,
            master_key: Some(master_key),
            signing_key: Some(signing_key),
            public_key: Some(public_key),
            address: Some(address),
            solana_address: Some(solana_address),
            nonce: 0,
            balance: WalletBalance::zero(),
            created_at: Utc::now(),
            derived_addresses: HashMap::new(),
        })
    }

    /// Create a watch-only wallet from public key
    pub fn watch_only(public_key: PublicKey, config: WalletConfig) -> Self {
        let address = public_key.to_address();
        let solana_address = SolanaAddress::from_public_key(&public_key);
        
        Self {
            id: uuid::Uuid::new_v4(),
            config: WalletConfig {
                wallet_type: WalletType::WatchOnly,
                ..config
            },
            master_key: None,
            signing_key: None,
            public_key: Some(public_key),
            address: Some(address),
            solana_address: Some(solana_address),
            nonce: 0,
            balance: WalletBalance::zero(),
            created_at: Utc::now(),
            derived_addresses: HashMap::new(),
        }
    }

    /// Get wallet ID
    pub fn id(&self) -> uuid::Uuid {
        self.id
    }

    /// Get wallet type
    pub fn wallet_type(&self) -> WalletType {
        self.config.wallet_type
    }

    /// Get wallet name
    pub fn name(&self) -> &str {
        &self.config.name
    }

    /// Get public key
    pub fn public_key(&self) -> Option<&PublicKey> {
        self.public_key.as_ref()
    }

    /// Get blockchain address (dchat format)
    pub fn address(&self) -> Option<&Address> {
        self.address.as_ref()
    }

    /// Get Solana-compatible address
    pub fn solana_address(&self) -> Option<&SolanaAddress> {
        self.solana_address.as_ref()
    }

    /// Get current nonce
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Set nonce (should be fetched from chain)
    pub fn set_nonce(&mut self, nonce: u64) {
        self.nonce = nonce;
    }

    /// Get cached balance
    pub fn balance(&self) -> &WalletBalance {
        &self.balance
    }

    /// Update cached balance
    pub fn set_balance(&mut self, balance: WalletBalance) {
        self.balance = balance;
    }

    /// Check if wallet can sign transactions
    pub fn can_sign(&self) -> bool {
        self.signing_key.is_some()
    }

    /// Sign a transaction
    pub fn sign_transaction(&mut self, tx: WalletTransaction) -> Result<SignedTransaction> {
        let signing_key = self.signing_key.as_ref()
            .ok_or_else(|| Error::crypto("Wallet cannot sign (watch-only or locked)"))?;
        
        let public_key = self.public_key.as_ref()
            .ok_or_else(|| Error::crypto("Public key not available"))?;

        // Get message to sign
        let message = tx.signing_message();
        
        // Sign with Ed25519
        let signature = signing_key.sign(&message);
        
        // Increment nonce after successful signing
        self.nonce += 1;
        
        Ok(SignedTransaction {
            transaction: tx,
            signatures: vec![TransactionSignature {
                public_key: public_key.as_bytes().to_vec(),
                signature: signature.to_bytes().to_vec(),
                signer_index: None,
            }],
        })
    }

    /// Sign arbitrary message (for authentication)
    pub fn sign_message(&self, message: &[u8]) -> Result<Vec<u8>> {
        let signing_key = self.signing_key.as_ref()
            .ok_or_else(|| Error::crypto("Wallet cannot sign"))?;
        
        // Hash message first for consistent length
        let hash = blake3::hash(message);
        let signature = signing_key.sign(hash.as_bytes());
        
        Ok(signature.to_bytes().to_vec())
    }

    /// Sign raw message without hashing (for Solana transactions)
    pub fn sign_raw(&self, message: &[u8]) -> Result<[u8; 64]> {
        let signing_key = self.signing_key.as_ref()
            .ok_or_else(|| Error::crypto("Wallet cannot sign"))?;
        
        let signature = signing_key.sign(message);
        Ok(signature.to_bytes())
    }

    /// Get signing key reference for Solana transaction signing
    pub fn signing_key(&self) -> Option<&SigningKey> {
        self.signing_key.as_ref()
    }

    /// Sign a Solana transaction
    /// 
    /// This signs a Solana transaction and places the signature at the specified index
    pub fn sign_solana_transaction(
        &self,
        tx: &mut crate::solana::SolanaTransaction,
        signer_index: usize,
    ) -> Result<()> {
        let signing_key = self.signing_key.as_ref()
            .ok_or_else(|| Error::crypto("Wallet cannot sign (watch-only or locked)"))?;
        
        tx.sign(signing_key, signer_index)?;
        Ok(())
    }

    /// Derive a new address at the specified index
    pub fn derive_address(&mut self, index: u32) -> Result<Address> {
        // Check cache first
        if let Some(addr) = self.derived_addresses.get(&index) {
            return Ok(addr.clone());
        }

        let master_key = self.master_key.as_ref()
            .ok_or_else(|| Error::crypto("Master key not available for derivation"))?;

        // Derive key at new index
        let path = [44, 1337, self.config.account_index, 0, index];
        let derived_key = KeyDerivation::derive_key_path(master_key, &path)?;
        let public_key = derived_key.public_key();
        let address = public_key.to_address();
        
        // Cache the address
        self.derived_addresses.insert(index, address.clone());
        
        Ok(address)
    }

    /// Get all derived addresses
    pub fn derived_addresses(&self) -> &HashMap<u32, Address> {
        &self.derived_addresses
    }

    /// Create a simple transfer transaction
    pub fn create_transfer(&self, to: &str, amount: u64, fee: u64) -> WalletTransaction {
        WalletTransaction::new(
            to.to_string(),
            amount,
            fee,
            self.nonce,
            self.config.chain_id,
        )
    }

    /// Export public key in various formats
    pub fn export_public_key(&self) -> Result<WalletExport> {
        let public_key = self.public_key.as_ref()
            .ok_or_else(|| Error::crypto("Public key not available"))?;
        
        Ok(WalletExport {
            wallet_id: self.id,
            name: self.config.name.clone(),
            wallet_type: self.config.wallet_type,
            public_key_hex: hex::encode(public_key.as_bytes()),
            address_dchat: self.address.as_ref().map(|a| a.to_hex()),
            address_solana: self.solana_address.as_ref().map(|a| a.to_string()),
            created_at: self.created_at,
        })
    }
}

/// Wallet export data (safe to share)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletExport {
    pub wallet_id: uuid::Uuid,
    pub name: String,
    pub wallet_type: WalletType,
    pub public_key_hex: String,
    pub address_dchat: Option<String>,
    pub address_solana: Option<String>,
    pub created_at: DateTime<Utc>,
}

impl std::fmt::Debug for Wallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Wallet")
            .field("id", &self.id)
            .field("name", &self.config.name)
            .field("wallet_type", &self.config.wallet_type)
            .field("address", &self.address.as_ref().map(|a| a.to_hex()))
            .field("solana_address", &self.solana_address.as_ref().map(|a| a.to_string()))
            .field("can_sign", &self.can_sign())
            .field("nonce", &self.nonce)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_wallet_creation() {
        let config = WalletConfig::default();
        let (wallet, phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();
        
        assert!(wallet.can_sign());
        assert!(wallet.address().is_some());
        assert!(wallet.solana_address().is_some());
        assert!(!phrase.is_empty());
        
        // Verify mnemonic restoration produces same address
        let config2 = WalletConfig::default();
        let wallet2 = Wallet::from_mnemonic(&phrase, None, config2).unwrap();
        
        assert_eq!(
            wallet.address().unwrap().to_hex(),
            wallet2.address().unwrap().to_hex()
        );
    }

    #[test]
    fn test_wallet_signing() {
        let config = WalletConfig::default();
        let (mut wallet, _) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();
        
        let tx = wallet.create_transfer(
            "0x1234567890abcdef1234567890abcdef12345678",
            1000,
            10,
        );
        
        let signed = wallet.sign_transaction(tx).unwrap();
        assert!(signed.verify().unwrap());
        assert_eq!(wallet.nonce(), 1); // Nonce incremented
    }

    #[test]
    fn test_address_derivation() {
        let config = WalletConfig::default();
        let (mut wallet, _) = Wallet::create(config, MnemonicLength::Words24, None).unwrap();
        
        let addr0 = wallet.derive_address(0).unwrap();
        let addr1 = wallet.derive_address(1).unwrap();
        let addr2 = wallet.derive_address(0).unwrap(); // Should use cache
        
        assert_ne!(addr0.to_hex(), addr1.to_hex());
        assert_eq!(addr0.to_hex(), addr2.to_hex());
    }

    #[test]
    fn test_watch_only_wallet() {
        let config = WalletConfig::default();
        let (wallet, _) = Wallet::create(config.clone(), MnemonicLength::Words12, None).unwrap();
        
        let watch = Wallet::watch_only(wallet.public_key().unwrap().clone(), config);
        
        assert!(!watch.can_sign());
        assert_eq!(watch.wallet_type(), WalletType::WatchOnly);
        assert_eq!(
            watch.address().unwrap().to_hex(),
            wallet.address().unwrap().to_hex()
        );
    }
}
