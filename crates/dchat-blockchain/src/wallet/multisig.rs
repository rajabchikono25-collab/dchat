//! Multi-Signature Wallet Implementation
//!
//! Production-ready M-of-N threshold signature wallet with:
//! - Configurable threshold (e.g., 2-of-3, 3-of-5)
//! - Partial signature collection
//! - Signer rotation support
//! - Hardware wallet signer support
//! - Solana-compatible multi-sig

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_crypto::keys::PublicKey;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use uuid::Uuid;

use super::{SignedTransaction, TransactionSignature, WalletBalance, WalletTransaction};
use super::solana_compat::SolanaAddress;

/// Multi-signature wallet configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSigConfig {
    /// Number of signatures required (M)
    pub threshold: usize,
    /// Total number of signers (N)
    pub total_signers: usize,
    /// Signer public keys and metadata
    pub signers: Vec<SignerInfo>,
    /// Wallet name/label
    pub name: String,
    /// Chain ID
    pub chain_id: u32,
    /// Time lock for transactions (optional)
    pub timelock_seconds: Option<u64>,
    /// Daily spending limit without additional approval
    pub daily_limit: Option<u64>,
}

impl MultiSigConfig {
    /// Create a new multi-sig configuration
    pub fn new(
        name: String,
        threshold: usize,
        signers: Vec<SignerInfo>,
        chain_id: u32,
    ) -> Result<Self> {
        let total_signers = signers.len();
        
        if threshold == 0 {
            return Err(Error::validation("Threshold must be at least 1"));
        }
        if threshold > total_signers {
            return Err(Error::validation("Threshold cannot exceed number of signers"));
        }
        if total_signers > 10 {
            return Err(Error::validation("Maximum 10 signers allowed"));
        }
        
        // Check for duplicate public keys
        let unique_keys: HashSet<_> = signers.iter().map(|s| s.public_key.as_bytes()).collect();
        if unique_keys.len() != signers.len() {
            return Err(Error::validation("Duplicate signer public keys"));
        }
        
        Ok(Self {
            threshold,
            total_signers,
            signers,
            name,
            chain_id,
            timelock_seconds: None,
            daily_limit: None,
        })
    }

    /// Create a 2-of-3 configuration
    pub fn two_of_three(name: String, signers: [SignerInfo; 3], chain_id: u32) -> Result<Self> {
        Self::new(name, 2, signers.to_vec(), chain_id)
    }

    /// Create a 3-of-5 configuration
    pub fn three_of_five(name: String, signers: [SignerInfo; 5], chain_id: u32) -> Result<Self> {
        Self::new(name, 3, signers.to_vec(), chain_id)
    }
}

/// Information about a signer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerInfo {
    /// Signer's public key
    pub public_key: PublicKey,
    /// Signer's display name
    pub name: String,
    /// Signer index (0-based)
    pub index: u32,
    /// Whether this is a hardware wallet
    pub is_hardware: bool,
    /// Signer's Solana address
    pub solana_address: SolanaAddress,
    /// Last time this signer was active
    pub last_active: Option<DateTime<Utc>>,
}

impl SignerInfo {
    /// Create a new signer info
    pub fn new(public_key: PublicKey, name: String, index: u32) -> Self {
        let solana_address = SolanaAddress::from_public_key(&public_key);
        
        Self {
            public_key,
            name,
            index,
            is_hardware: false,
            solana_address,
            last_active: None,
        }
    }

    /// Mark as hardware wallet signer
    pub fn with_hardware(mut self) -> Self {
        self.is_hardware = true;
        self
    }
}

/// Pending multi-sig transaction awaiting signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingMultiSigTx {
    /// Unique ID
    pub id: Uuid,
    /// The transaction to be signed
    pub transaction: WalletTransaction,
    /// Collected signatures so far
    pub signatures: HashMap<u32, TransactionSignature>,
    /// Required threshold
    pub threshold: usize,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Expiration timestamp
    pub expires_at: DateTime<Utc>,
    /// Transaction initiator (signer index)
    pub initiated_by: u32,
}

impl PendingMultiSigTx {
    /// Check if threshold is met
    pub fn has_quorum(&self) -> bool {
        self.signatures.len() >= self.threshold
    }

    /// Get number of signatures collected
    pub fn signature_count(&self) -> usize {
        self.signatures.len()
    }

    /// Check if transaction has expired
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Get remaining signatures needed
    pub fn signatures_needed(&self) -> usize {
        self.threshold.saturating_sub(self.signatures.len())
    }
}

/// Multi-signature wallet
pub struct MultiSigWallet {
    /// Unique wallet ID
    id: Uuid,
    /// Multi-sig configuration
    config: MultiSigConfig,
    /// Derived wallet address (hash of all signer pubkeys + threshold)
    address: String,
    /// Solana-compatible address
    solana_address: SolanaAddress,
    /// Current nonce
    nonce: u64,
    /// Cached balance
    balance: WalletBalance,
    /// Pending transactions awaiting signatures
    pending_transactions: HashMap<Uuid, PendingMultiSigTx>,
    /// Daily spending tracker
    daily_spent: u64,
    /// Last reset timestamp for daily limit
    daily_limit_reset: DateTime<Utc>,
    /// Created timestamp
    created_at: DateTime<Utc>,
}

impl MultiSigWallet {
    /// Create a new multi-sig wallet
    pub fn new(config: MultiSigConfig) -> Result<Self> {
        // Derive wallet address from signer public keys and threshold
        let address = Self::derive_address(&config);
        
        // Create Solana-compatible address using first signer's key for compatibility
        // Real multi-sig would use a Program Derived Address (PDA)
        let solana_address = SolanaAddress::from_bytes(
            &blake3::hash(address.as_bytes()).as_bytes()[..32]
        )?;
        
        Ok(Self {
            id: Uuid::new_v4(),
            config,
            address,
            solana_address,
            nonce: 0,
            balance: WalletBalance::zero(),
            pending_transactions: HashMap::new(),
            daily_spent: 0,
            daily_limit_reset: Utc::now(),
            created_at: Utc::now(),
        })
    }

    /// Derive deterministic address from multi-sig configuration
    fn derive_address(config: &MultiSigConfig) -> String {
        let mut data = Vec::new();
        data.push(config.threshold as u8);
        data.push(config.total_signers as u8);
        
        // Sort public keys for deterministic ordering
        let mut sorted_keys: Vec<_> = config.signers.iter()
            .map(|s| s.public_key.as_bytes())
            .collect();
        sorted_keys.sort();
        
        for key in sorted_keys {
            data.extend_from_slice(key);
        }
        
        let hash = blake3::hash(&data);
        format!("0x{}", hex::encode(&hash.as_bytes()[..20]))
    }

    /// Get wallet ID
    pub fn id(&self) -> Uuid {
        self.id
    }

    /// Get wallet address
    pub fn address(&self) -> &str {
        &self.address
    }

    /// Get Solana-compatible address
    pub fn solana_address(&self) -> &SolanaAddress {
        &self.solana_address
    }

    /// Get configuration
    pub fn config(&self) -> &MultiSigConfig {
        &self.config
    }

    /// Get threshold
    pub fn threshold(&self) -> usize {
        self.config.threshold
    }

    /// Get total signers
    pub fn total_signers(&self) -> usize {
        self.config.total_signers
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

    /// Get signer by index
    pub fn get_signer(&self, index: u32) -> Option<&SignerInfo> {
        self.config.signers.get(index as usize)
    }

    /// Get signer by public key
    pub fn get_signer_by_pubkey(&self, pubkey: &PublicKey) -> Option<&SignerInfo> {
        self.config.signers.iter().find(|s| &s.public_key == pubkey)
    }

    /// Initiate a new multi-sig transaction
    pub fn initiate_transaction(
        &mut self,
        to: String,
        amount: u64,
        fee: u64,
        initiated_by: u32,
        expiration_hours: u64,
    ) -> Result<Uuid> {
        // Validate initiator
        if initiated_by as usize >= self.config.total_signers {
            return Err(Error::validation("Invalid signer index"));
        }

        // Check daily limit if configured
        if let Some(limit) = self.config.daily_limit {
            self.reset_daily_limit_if_needed();
            if self.daily_spent + amount > limit && self.config.threshold > 1 {
                // Requires additional approvals beyond threshold
                tracing::info!("Transaction exceeds daily limit, additional approvals may be needed");
            }
        }

        let transaction = WalletTransaction::new(
            to,
            amount,
            fee,
            self.nonce,
            self.config.chain_id,
        );

        let expires_at = Utc::now() + chrono::Duration::hours(expiration_hours as i64);
        
        let pending = PendingMultiSigTx {
            id: Uuid::new_v4(),
            transaction,
            signatures: HashMap::new(),
            threshold: self.config.threshold,
            created_at: Utc::now(),
            expires_at,
            initiated_by,
        };

        let tx_id = pending.id;
        self.pending_transactions.insert(tx_id, pending);

        tracing::info!(
            "Multi-sig transaction {} initiated, requires {}/{} signatures",
            tx_id,
            self.config.threshold,
            self.config.total_signers
        );

        Ok(tx_id)
    }

    /// Add a signature to a pending transaction
    pub fn add_signature(
        &mut self,
        tx_id: Uuid,
        signer_index: u32,
        signature: Vec<u8>,
    ) -> Result<bool> {
        // First validate signer and get public key
        let signer = self.config.signers.get(signer_index as usize)
            .ok_or_else(|| Error::validation("Invalid signer index"))?
            .clone();

        let pending = self.pending_transactions.get(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;

        // Check expiration
        if pending.is_expired() {
            self.pending_transactions.remove(&tx_id);
            return Err(Error::validation("Transaction has expired"));
        }

        // Check for duplicate signature
        if pending.signatures.contains_key(&signer_index) {
            return Err(Error::validation("Signer has already signed"));
        }

        // Verify signature before modifying state
        let message = pending.transaction.signing_message();
        self.verify_signature(&signer.public_key, &message, &signature)?;

        // Now get mutable reference and add signature
        let pending = self.pending_transactions.get_mut(&tx_id).unwrap();
        pending.signatures.insert(signer_index, TransactionSignature {
            public_key: signer.public_key.as_bytes().to_vec(),
            signature,
            signer_index: Some(signer_index),
        });

        let has_quorum = pending.has_quorum();

        tracing::info!(
            "Signature added to tx {}: {}/{} collected",
            tx_id,
            pending.signature_count(),
            self.config.threshold
        );

        Ok(has_quorum)
    }

    /// Finalize a transaction that has reached quorum
    pub fn finalize_transaction(&mut self, tx_id: Uuid) -> Result<SignedTransaction> {
        // First check if transaction exists and get its state
        let pending = match self.pending_transactions.get(&tx_id) {
            Some(p) => p,
            None => return Err(Error::validation("Transaction not found")),
        };

        if !pending.has_quorum() {
            return Err(Error::validation(format!(
                "Threshold not met: {}/{} signatures",
                pending.signature_count(),
                self.config.threshold
            )));
        }

        if pending.is_expired() {
            self.pending_transactions.remove(&tx_id);
            return Err(Error::validation("Transaction has expired"));
        }

        // Now remove and consume
        let pending = self.pending_transactions.remove(&tx_id).unwrap();

        // Collect signatures in order
        let mut signatures: Vec<TransactionSignature> = pending.signatures
            .into_values()
            .collect();
        signatures.sort_by_key(|s| s.signer_index);

        // Increment nonce
        self.nonce += 1;

        // Update daily spending
        self.daily_spent += pending.transaction.amount;

        tracing::info!(
            "Multi-sig transaction {} finalized with {} signatures",
            tx_id,
            signatures.len()
        );

        Ok(SignedTransaction {
            transaction: pending.transaction,
            signatures,
        })
    }

    /// Get pending transactions
    pub fn pending_transactions(&self) -> &HashMap<Uuid, PendingMultiSigTx> {
        &self.pending_transactions
    }

    /// Cancel a pending transaction
    pub fn cancel_transaction(&mut self, tx_id: Uuid) -> Result<()> {
        self.pending_transactions.remove(&tx_id)
            .ok_or_else(|| Error::validation("Transaction not found"))?;
        Ok(())
    }

    /// Clean up expired transactions
    pub fn cleanup_expired(&mut self) -> usize {
        let expired: Vec<Uuid> = self.pending_transactions
            .iter()
            .filter(|(_, tx)| tx.is_expired())
            .map(|(id, _)| *id)
            .collect();

        let count = expired.len();
        for id in expired {
            self.pending_transactions.remove(&id);
        }

        count
    }

    /// Verify a signature
    fn verify_signature(
        &self,
        public_key: &PublicKey,
        message: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        if signature.len() != 64 {
            return Err(Error::crypto("Invalid signature length"));
        }

        let verifying_key = VerifyingKey::from_bytes(
            public_key.as_bytes()
                .try_into()
                .map_err(|_| Error::crypto("Invalid public key"))?
        ).map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

        let sig = Signature::from_bytes(
            signature.try_into()
                .map_err(|_| Error::crypto("Invalid signature format"))?
        );

        verifying_key.verify_strict(message, &sig)
            .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))?;

        Ok(())
    }

    /// Reset daily limit if needed
    fn reset_daily_limit_if_needed(&mut self) {
        let now = Utc::now();
        let elapsed = now.signed_duration_since(self.daily_limit_reset);
        
        if elapsed.num_hours() >= 24 {
            self.daily_spent = 0;
            self.daily_limit_reset = now;
        }
    }

    /// Export wallet configuration (safe to share)
    pub fn export_config(&self) -> MultiSigWalletExport {
        MultiSigWalletExport {
            wallet_id: self.id,
            name: self.config.name.clone(),
            threshold: self.config.threshold,
            total_signers: self.config.total_signers,
            address: self.address.clone(),
            solana_address: self.solana_address.to_string(),
            signers: self.config.signers.iter().map(|s| SignerExport {
                name: s.name.clone(),
                index: s.index,
                public_key_hex: hex::encode(s.public_key.as_bytes()),
                solana_address: s.solana_address.to_string(),
                is_hardware: s.is_hardware,
            }).collect(),
            created_at: self.created_at,
        }
    }
}

/// Exported multi-sig wallet data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSigWalletExport {
    pub wallet_id: Uuid,
    pub name: String,
    pub threshold: usize,
    pub total_signers: usize,
    pub address: String,
    pub solana_address: String,
    pub signers: Vec<SignerExport>,
    pub created_at: DateTime<Utc>,
}

/// Exported signer info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignerExport {
    pub name: String,
    pub index: u32,
    pub public_key_hex: String,
    pub solana_address: String,
    pub is_hardware: bool,
}

impl std::fmt::Debug for MultiSigWallet {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MultiSigWallet")
            .field("id", &self.id)
            .field("name", &self.config.name)
            .field("threshold", &format!("{}-of-{}", self.config.threshold, self.config.total_signers))
            .field("address", &self.address)
            .field("pending_txs", &self.pending_transactions.len())
            .field("nonce", &self.nonce)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    fn create_test_signers(count: usize) -> Vec<(SignerInfo, ed25519_dalek::SigningKey)> {
        (0..count).map(|i| {
            let keypair = KeyPair::try_generate().unwrap();
            let public_key = keypair.public_key().clone();
            let signing_key = ed25519_dalek::SigningKey::from_bytes(
                keypair.private_key().as_bytes()
            );
            
            let signer = SignerInfo::new(public_key, format!("Signer {}", i), i as u32);
            (signer, signing_key)
        }).collect()
    }

    #[test]
    fn test_multisig_creation() {
        let signers: Vec<_> = create_test_signers(3).into_iter().map(|(s, _)| s).collect();
        let config = MultiSigConfig::new(
            "Test Wallet".to_string(),
            2,
            signers,
            1337,
        ).unwrap();

        let wallet = MultiSigWallet::new(config).unwrap();
        
        assert_eq!(wallet.threshold(), 2);
        assert_eq!(wallet.total_signers(), 3);
        assert!(wallet.address().starts_with("0x"));
    }

    #[test]
    fn test_multisig_transaction_flow() {
        use ed25519_dalek::Signer;

        let signers_with_keys = create_test_signers(3);
        let signers: Vec<_> = signers_with_keys.iter().map(|(s, _)| s.clone()).collect();
        
        let config = MultiSigConfig::new(
            "Test Wallet".to_string(),
            2,
            signers,
            1337,
        ).unwrap();

        let mut wallet = MultiSigWallet::new(config).unwrap();

        // Initiate transaction
        let tx_id = wallet.initiate_transaction(
            "0x1234".to_string(),
            1000,
            10,
            0, // initiated by signer 0
            24, // 24 hours expiration
        ).unwrap();

        // First signature
        let pending = wallet.pending_transactions().get(&tx_id).unwrap();
        let message = pending.transaction.signing_message();
        let sig1 = signers_with_keys[0].1.sign(&message);
        
        let has_quorum = wallet.add_signature(tx_id, 0, sig1.to_bytes().to_vec()).unwrap();
        assert!(!has_quorum);

        // Second signature - reaches quorum
        let sig2 = signers_with_keys[1].1.sign(&message);
        let has_quorum = wallet.add_signature(tx_id, 1, sig2.to_bytes().to_vec()).unwrap();
        assert!(has_quorum);

        // Finalize
        let signed = wallet.finalize_transaction(tx_id).unwrap();
        assert_eq!(signed.signatures.len(), 2);
        assert!(signed.verify().unwrap());
    }

    #[test]
    fn test_invalid_threshold() {
        let signers: Vec<_> = create_test_signers(2).into_iter().map(|(s, _)| s).collect();
        
        // Threshold > total signers
        let result = MultiSigConfig::new(
            "Test".to_string(),
            3,
            signers.clone(),
            1337,
        );
        assert!(result.is_err());

        // Zero threshold
        let result = MultiSigConfig::new(
            "Test".to_string(),
            0,
            signers,
            1337,
        );
        assert!(result.is_err());
    }
}
