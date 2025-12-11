//! Burner/Temporary Wallet Implementation
//!
//! Production-ready temporary wallets with:
//! - Time-based expiration
//! - Message/transaction count limits
//! - Automatic key destruction
//! - Privacy-focused design

use chrono::{DateTime, Duration, Utc};
use dchat_core::error::{Error, Result};
use dchat_crypto::keys::{Address, KeyPair, PublicKey};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;
use zeroize::ZeroizeOnDrop;

use super::{SignedTransaction, TransactionSignature, WalletBalance, WalletTransaction};
use super::solana_compat::SolanaAddress;

/// Burner wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnerWalletConfig {
    /// Optional name/label
    pub name: Option<String>,
    /// Maximum lifetime in seconds (None = no time limit)
    pub max_lifetime_seconds: Option<u64>,
    /// Maximum transactions allowed (None = unlimited)
    pub max_transactions: Option<u32>,
    /// Maximum total value transferable (None = unlimited)
    pub max_total_value: Option<u64>,
    /// Auto-destroy on expiration
    pub auto_destroy: bool,
    /// Chain ID
    pub chain_id: u32,
    /// Optional parent wallet ID (for tracking)
    pub parent_wallet_id: Option<Uuid>,
}

impl Default for BurnerWalletConfig {
    fn default() -> Self {
        Self {
            name: None,
            max_lifetime_seconds: Some(24 * 60 * 60), // 24 hours default
            max_transactions: Some(100),
            max_total_value: None,
            auto_destroy: true,
            chain_id: 1337,
            parent_wallet_id: None,
        }
    }
}

impl BurnerWalletConfig {
    /// Create config for a quick one-time-use burner
    pub fn one_time() -> Self {
        Self {
            max_transactions: Some(1),
            max_lifetime_seconds: Some(60 * 60), // 1 hour
            ..Default::default()
        }
    }

    /// Create config for a 24-hour session
    pub fn daily_session() -> Self {
        Self {
            max_lifetime_seconds: Some(24 * 60 * 60),
            max_transactions: Some(100),
            ..Default::default()
        }
    }

    /// Create config for a privacy-focused temporary identity
    pub fn privacy_session(hours: u64, max_txs: u32) -> Self {
        Self {
            max_lifetime_seconds: Some(hours * 60 * 60),
            max_transactions: Some(max_txs),
            ..Default::default()
        }
    }
}

/// Burner wallet statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct BurnerStats {
    /// Total transactions executed
    pub transaction_count: u32,
    /// Total value transferred out
    pub total_value_sent: u64,
    /// Total value received
    pub total_value_received: u64,
    /// Last activity timestamp
    pub last_activity: Option<DateTime<Utc>>,
}

/// Reason for burner wallet destruction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DestructionReason {
    /// Time limit reached
    TimeExpired,
    /// Transaction limit reached
    TransactionLimitReached,
    /// Value limit reached
    ValueLimitReached,
    /// Manual destruction by user
    ManualDestruction,
    /// Parent wallet requested destruction
    ParentRequest,
    /// Security event triggered destruction
    SecurityEvent,
}

/// A temporary burner wallet with automatic expiration
#[derive(ZeroizeOnDrop)]
pub struct BurnerWallet {
    /// Unique wallet ID
    #[zeroize(skip)]
    id: Uuid,
    /// Configuration
    #[zeroize(skip)]
    config: BurnerWalletConfig,
    /// Signing key (zeroized on drop)
    signing_key: SigningKey,
    /// Public key
    #[zeroize(skip)]
    public_key: PublicKey,
    /// Blockchain address
    #[zeroize(skip)]
    address: Address,
    /// Solana-compatible address
    #[zeroize(skip)]
    solana_address: SolanaAddress,
    /// Current nonce
    #[zeroize(skip)]
    nonce: u64,
    /// Cached balance
    #[zeroize(skip)]
    balance: WalletBalance,
    /// Creation timestamp
    #[zeroize(skip)]
    created_at: DateTime<Utc>,
    /// Expiration timestamp (if time-limited)
    #[zeroize(skip)]
    expires_at: Option<DateTime<Utc>>,
    /// Usage statistics
    #[zeroize(skip)]
    stats: BurnerStats,
    /// Whether wallet is still active
    #[zeroize(skip)]
    is_active: bool,
    /// Destruction reason (if destroyed)
    #[zeroize(skip)]
    destruction_reason: Option<DestructionReason>,
}

impl BurnerWallet {
    /// Create a new burner wallet with random keys
    pub fn create(config: BurnerWalletConfig) -> Result<Self> {
        let keypair = KeyPair::try_generate()
            .map_err(|e| Error::crypto(format!("Failed to generate keypair: {}", e)))?;
        
        let signing_key = SigningKey::from_bytes(keypair.private_key().as_bytes());
        let verifying_key = signing_key.verifying_key();
        
        let public_key = PublicKey::from_bytes(verifying_key.to_bytes());
        let address = public_key.to_address();
        let solana_address = SolanaAddress::from_public_key(&public_key);
        
        let created_at = Utc::now();
        let expires_at = config.max_lifetime_seconds
            .map(|secs| created_at + Duration::seconds(secs as i64));
        
        Ok(Self {
            id: Uuid::new_v4(),
            config,
            signing_key,
            public_key,
            address,
            solana_address,
            nonce: 0,
            balance: WalletBalance::zero(),
            created_at,
            expires_at,
            stats: BurnerStats::default(),
            is_active: true,
            destruction_reason: None,
        })
    }

    /// Get wallet ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get wallet name
    pub fn name(&self) -> Option<&str> {
        self.config.name.as_deref()
    }

    /// Get public key
    pub fn public_key(&self) -> &PublicKey {
        &self.public_key
    }

    /// Get blockchain address
    pub fn address(&self) -> &Address {
        &self.address
    }

    /// Get Solana-compatible address
    pub fn solana_address(&self) -> &SolanaAddress {
        &self.solana_address
    }

    /// Get creation timestamp
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Get expiration timestamp
    pub fn expires_at(&self) -> Option<DateTime<Utc>> {
        self.expires_at
    }

    /// Get current nonce
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Set nonce
    pub fn set_nonce(&mut self, nonce: u64) {
        self.nonce = nonce;
    }

    /// Get balance
    pub fn balance(&self) -> &WalletBalance {
        &self.balance
    }

    /// Set balance
    pub fn set_balance(&mut self, balance: WalletBalance) {
        self.balance = balance;
    }

    /// Get statistics
    pub fn stats(&self) -> &BurnerStats {
        &self.stats
    }

    /// Check if wallet is still active
    pub fn is_active(&self) -> bool {
        self.is_active && !self.is_expired()
    }

    /// Check if wallet has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            if Utc::now() > expires_at {
                return true;
            }
        }
        false
    }

    /// Get remaining lifetime (if time-limited)
    pub fn remaining_lifetime(&self) -> Option<Duration> {
        self.expires_at.map(|exp| {
            let remaining = exp.signed_duration_since(Utc::now());
            if remaining.num_seconds() > 0 {
                remaining
            } else {
                Duration::zero()
            }
        })
    }

    /// Get remaining transactions (if transaction-limited)
    pub fn remaining_transactions(&self) -> Option<u32> {
        self.config.max_transactions.map(|max| {
            max.saturating_sub(self.stats.transaction_count)
        })
    }

    /// Check if wallet can execute a transaction
    pub fn can_transact(&self, amount: u64) -> Result<()> {
        if !self.is_active {
            return Err(Error::validation("Burner wallet is no longer active"));
        }

        if self.is_expired() {
            return Err(Error::validation("Burner wallet has expired"));
        }

        if let Some(max_txs) = self.config.max_transactions {
            if self.stats.transaction_count >= max_txs {
                return Err(Error::validation("Transaction limit reached"));
            }
        }

        if let Some(max_value) = self.config.max_total_value {
            if self.stats.total_value_sent + amount > max_value {
                return Err(Error::validation("Value limit would be exceeded"));
            }
        }

        Ok(())
    }

    /// Sign a transaction
    pub fn sign_transaction(&mut self, tx: WalletTransaction) -> Result<SignedTransaction> {
        self.can_transact(tx.amount)?;
        
        let message = tx.signing_message();
        let signature = self.signing_key.sign(&message);
        
        // Update statistics
        self.stats.transaction_count += 1;
        self.stats.total_value_sent += tx.amount;
        self.stats.last_activity = Some(Utc::now());
        self.nonce += 1;
        
        // Check if limits are now reached
        self.check_and_handle_limits();
        
        Ok(SignedTransaction {
            transaction: tx,
            signatures: vec![TransactionSignature {
                public_key: self.public_key.as_bytes().to_vec(),
                signature: signature.to_bytes().to_vec(),
                signer_index: None,
            }],
        })
    }

    /// Sign arbitrary message
    pub fn sign_message(&self, message: &[u8]) -> Result<Vec<u8>> {
        if !self.is_active() {
            return Err(Error::validation("Burner wallet is not active"));
        }
        
        let hash = blake3::hash(message);
        let signature = self.signing_key.sign(hash.as_bytes());
        Ok(signature.to_bytes().to_vec())
    }

    /// Create a transfer transaction
    pub fn create_transfer(&self, to: &str, amount: u64, fee: u64) -> Result<WalletTransaction> {
        self.can_transact(amount)?;
        
        Ok(WalletTransaction::new(
            to.to_string(),
            amount,
            fee,
            self.nonce,
            self.config.chain_id,
        ))
    }

    /// Record incoming value
    pub fn record_incoming(&mut self, amount: u64) {
        self.stats.total_value_received += amount;
        self.stats.last_activity = Some(Utc::now());
    }

    /// Manually destroy the wallet (zeroizes keys)
    pub fn destroy(mut self) -> BurnerWalletSummary {
        self.destruction_reason = Some(DestructionReason::ManualDestruction);
        self.is_active = false;
        
        self.create_summary()
    }

    /// Check limits and handle auto-destruction
    fn check_and_handle_limits(&mut self) {
        // Check transaction limit
        if let Some(max_txs) = self.config.max_transactions {
            if self.stats.transaction_count >= max_txs {
                self.is_active = false;
                self.destruction_reason = Some(DestructionReason::TransactionLimitReached);
                tracing::info!("Burner wallet {} reached transaction limit", self.id);
                return;
            }
        }

        // Check value limit
        if let Some(max_value) = self.config.max_total_value {
            if self.stats.total_value_sent >= max_value {
                self.is_active = false;
                self.destruction_reason = Some(DestructionReason::ValueLimitReached);
                tracing::info!("Burner wallet {} reached value limit", self.id);
            }
        }
    }

    /// Create summary (for after destruction)
    fn create_summary(&self) -> BurnerWalletSummary {
        BurnerWalletSummary {
            id: self.id,
            address: self.address.to_hex(),
            solana_address: self.solana_address.to_string(),
            created_at: self.created_at,
            destroyed_at: Utc::now(),
            destruction_reason: self.destruction_reason,
            stats: self.stats.clone(),
            remaining_balance: self.balance.clone(),
        }
    }

    /// Export public info (safe to share)
    pub fn export(&self) -> BurnerWalletExport {
        BurnerWalletExport {
            id: self.id,
            name: self.config.name.clone(),
            address: self.address.to_hex(),
            solana_address: self.solana_address.to_string(),
            public_key_hex: hex::encode(self.public_key.as_bytes()),
            created_at: self.created_at,
            expires_at: self.expires_at,
            is_active: self.is_active(),
            remaining_transactions: self.remaining_transactions(),
            stats: self.stats.clone(),
        }
    }
}

/// Summary after wallet destruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnerWalletSummary {
    pub id: Uuid,
    pub address: String,
    pub solana_address: String,
    pub created_at: DateTime<Utc>,
    pub destroyed_at: DateTime<Utc>,
    pub destruction_reason: Option<DestructionReason>,
    pub stats: BurnerStats,
    pub remaining_balance: WalletBalance,
}

/// Exported burner wallet info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnerWalletExport {
    pub id: Uuid,
    pub name: Option<String>,
    pub address: String,
    pub solana_address: String,
    pub public_key_hex: String,
    pub created_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub is_active: bool,
    pub remaining_transactions: Option<u32>,
    pub stats: BurnerStats,
}

impl std::fmt::Debug for BurnerWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BurnerWallet")
            .field("id", &self.id)
            .field("address", &self.address.to_hex())
            .field("is_active", &self.is_active())
            .field("expires_at", &self.expires_at)
            .field("remaining_txs", &self.remaining_transactions())
            .field("tx_count", &self.stats.transaction_count)
            .finish()
    }
}

/// Manager for multiple burner wallets
pub struct BurnerWalletManager {
    /// Active burner wallets
    wallets: HashMap<Uuid, BurnerWallet>,
    /// Destroyed wallet summaries (for audit)
    destroyed: Vec<BurnerWalletSummary>,
    /// Maximum active burners per parent
    max_per_parent: usize,
    /// Maximum total active burners
    max_total: usize,
}

impl BurnerWalletManager {
    /// Create a new manager
    pub fn new() -> Self {
        Self {
            wallets: HashMap::new(),
            destroyed: Vec::new(),
            max_per_parent: 10,
            max_total: 100,
        }
    }

    /// Create a new burner wallet
    pub fn create_burner(&mut self, config: BurnerWalletConfig) -> Result<Uuid> {
        // Check limits
        if self.wallets.len() >= self.max_total {
            return Err(Error::validation("Maximum burner wallet limit reached"));
        }

        // Check per-parent limit
        if let Some(parent_id) = config.parent_wallet_id {
            let parent_count = self.wallets.values()
                .filter(|w| w.config.parent_wallet_id == Some(parent_id))
                .count();
            
            if parent_count >= self.max_per_parent {
                return Err(Error::validation("Maximum burners per parent reached"));
            }
        }

        let wallet = BurnerWallet::create(config)?;
        let id = wallet.id();
        
        self.wallets.insert(id, wallet);
        
        Ok(id)
    }

    /// Get a burner wallet
    pub fn get(&self, id: Uuid) -> Option<&BurnerWallet> {
        self.wallets.get(&id)
    }

    /// Get a mutable burner wallet
    pub fn get_mut(&mut self, id: Uuid) -> Option<&mut BurnerWallet> {
        self.wallets.get_mut(&id)
    }

    /// Destroy a burner wallet
    pub fn destroy(&mut self, id: Uuid) -> Result<BurnerWalletSummary> {
        let wallet = self.wallets.remove(&id)
            .ok_or_else(|| Error::validation("Burner wallet not found"))?;
        
        let summary = wallet.destroy();
        self.destroyed.push(summary.clone());
        
        Ok(summary)
    }

    /// Cleanup expired wallets
    pub fn cleanup_expired(&mut self) -> Vec<BurnerWalletSummary> {
        let expired: Vec<Uuid> = self.wallets.iter()
            .filter(|(_, w)| w.is_expired())
            .map(|(id, _)| *id)
            .collect();

        let mut summaries = Vec::new();
        for id in expired {
            if let Some(mut wallet) = self.wallets.remove(&id) {
                wallet.destruction_reason = Some(DestructionReason::TimeExpired);
                wallet.is_active = false;
                let summary = wallet.create_summary();
                summaries.push(summary.clone());
                self.destroyed.push(summary);
            }
        }

        summaries
    }

    /// Get all active burners
    pub fn active_burners(&self) -> Vec<&BurnerWallet> {
        self.wallets.values().filter(|w| w.is_active()).collect()
    }

    /// Get destruction history
    pub fn destruction_history(&self) -> &[BurnerWalletSummary] {
        &self.destroyed
    }

    /// Get count of active burners
    pub fn active_count(&self) -> usize {
        self.wallets.values().filter(|w| w.is_active()).count()
    }
}

impl Default for BurnerWalletManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_burner_creation() {
        let config = BurnerWalletConfig::default();
        let wallet = BurnerWallet::create(config).unwrap();
        
        assert!(wallet.is_active());
        assert!(wallet.expires_at().is_some());
        assert_eq!(wallet.stats().transaction_count, 0);
    }

    #[test]
    fn test_transaction_limit() {
        let config = BurnerWalletConfig {
            max_transactions: Some(2),
            max_lifetime_seconds: None,
            ..Default::default()
        };
        
        let mut wallet = BurnerWallet::create(config).unwrap();
        
        // First transaction
        let tx1 = wallet.create_transfer("0x1234", 100, 1).unwrap();
        wallet.sign_transaction(tx1).unwrap();
        assert!(wallet.is_active());
        
        // Second transaction (reaches limit)
        let tx2 = wallet.create_transfer("0x1234", 100, 1).unwrap();
        wallet.sign_transaction(tx2).unwrap();
        assert!(!wallet.is_active());
        
        // Third transaction should fail
        let result = wallet.can_transact(100);
        assert!(result.is_err());
    }

    #[test]
    fn test_value_limit() {
        let config = BurnerWalletConfig {
            max_total_value: Some(500),
            max_transactions: None,
            max_lifetime_seconds: None,
            ..Default::default()
        };
        
        let mut wallet = BurnerWallet::create(config).unwrap();
        
        // Transaction within limit
        let tx1 = wallet.create_transfer("0x1234", 400, 1).unwrap();
        wallet.sign_transaction(tx1).unwrap();
        assert!(wallet.is_active());
        
        // Transaction that would exceed limit
        let result = wallet.can_transact(200);
        assert!(result.is_err());
    }

    #[test]
    fn test_burner_manager() {
        let mut manager = BurnerWalletManager::new();
        
        let config = BurnerWalletConfig::daily_session();
        let id = manager.create_burner(config).unwrap();
        
        assert_eq!(manager.active_count(), 1);
        
        let wallet = manager.get(id).unwrap();
        assert!(wallet.is_active());
        
        let summary = manager.destroy(id).unwrap();
        assert_eq!(summary.id, id);
        assert_eq!(manager.active_count(), 0);
        assert_eq!(manager.destruction_history().len(), 1);
    }

    #[test]
    fn test_one_time_burner() {
        let config = BurnerWalletConfig::one_time();
        let mut wallet = BurnerWallet::create(config).unwrap();
        
        assert_eq!(wallet.remaining_transactions(), Some(1));
        
        let tx = wallet.create_transfer("0x1234", 100, 1).unwrap();
        wallet.sign_transaction(tx).unwrap();
        
        assert!(!wallet.is_active());
        assert_eq!(wallet.remaining_transactions(), Some(0));
    }
}
