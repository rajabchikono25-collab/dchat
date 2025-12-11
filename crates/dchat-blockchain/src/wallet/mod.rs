//! Production-Ready Wallet Module for dchat
//!
//! This module provides:
//! - Normal (single-key) wallets
//! - Multi-signature wallets (M-of-N threshold)
//! - Temporary/burner wallets with expiration
//! - Solana-compatible address format (Ed25519 + Base58)
//! - Hardware wallet integration support
//!
//! # Security
//! - All private keys are zeroized on drop
//! - BIP-39 mnemonic backup support
//! - Constant-time cryptographic operations
//! - Production-ready error handling

pub mod normal;
pub mod multisig;
pub mod burner;
pub mod solana_compat;
pub mod address;

pub use normal::{Wallet, WalletConfig, WalletType, WalletExport};
pub use multisig::{MultiSigWallet, MultiSigConfig, SignerInfo, PendingMultiSigTx};
pub use burner::{BurnerWallet, BurnerWalletConfig, BurnerWalletManager, BurnerStats, DestructionReason};
pub use solana_compat::{SolanaAddress, SolanaSignature, SolanaCompatible, TokenMint};
pub use address::{UniversalAddress, AddressFormat, AddressMapping};

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

/// Wallet balance information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletBalance {
    /// Native DCHAT token balance (in smallest unit)
    pub dchat: u64,
    /// Staked DCHAT amount
    pub staked: u64,
    /// Pending rewards
    pub pending_rewards: u64,
    /// Locked balance (vesting, timelock)
    pub locked: u64,
    /// SPL/bridged token balances
    pub tokens: Vec<TokenBalance>,
}

/// Token balance for non-native tokens
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenBalance {
    /// Token mint address (Solana) or contract address
    pub mint: String,
    /// Token symbol
    pub symbol: String,
    /// Balance in smallest unit
    pub amount: u64,
    /// Decimal places
    pub decimals: u8,
}

impl WalletBalance {
    /// Create a new empty balance
    pub fn zero() -> Self {
        Self {
            dchat: 0,
            staked: 0,
            pending_rewards: 0,
            locked: 0,
            tokens: Vec::new(),
        }
    }

    /// Get total available balance (dchat - locked)
    pub fn available(&self) -> u64 {
        self.dchat.saturating_sub(self.locked)
    }

    /// Get total balance including staked
    pub fn total(&self) -> u64 {
        self.dchat
            .saturating_add(self.staked)
            .saturating_add(self.pending_rewards)
    }
}

/// Transaction to be signed by a wallet
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletTransaction {
    /// Unique transaction ID
    pub id: uuid::Uuid,
    /// Recipient address
    pub to: String,
    /// Amount to transfer
    pub amount: u64,
    /// Transaction fee
    pub fee: u64,
    /// Optional memo/data
    pub memo: Option<String>,
    /// Nonce for replay protection
    pub nonce: u64,
    /// Chain ID
    pub chain_id: u32,
    /// Timestamp
    pub timestamp: chrono::DateTime<chrono::Utc>,
}

impl WalletTransaction {
    /// Create a new transaction
    pub fn new(to: String, amount: u64, fee: u64, nonce: u64, chain_id: u32) -> Self {
        Self {
            id: uuid::Uuid::new_v4(),
            to,
            amount,
            fee,
            memo: None,
            nonce,
            chain_id,
            timestamp: chrono::Utc::now(),
        }
    }

    /// Serialize for signing
    pub fn signing_message(&self) -> Vec<u8> {
        // Create deterministic signing message
        let mut msg = Vec::new();
        msg.extend_from_slice(&self.chain_id.to_le_bytes());
        msg.extend_from_slice(&self.nonce.to_le_bytes());
        msg.extend_from_slice(self.to.as_bytes());
        msg.extend_from_slice(&self.amount.to_le_bytes());
        msg.extend_from_slice(&self.fee.to_le_bytes());
        if let Some(ref memo) = self.memo {
            msg.extend_from_slice(memo.as_bytes());
        }
        msg.extend_from_slice(&self.timestamp.timestamp().to_le_bytes());
        
        // Hash for consistent length
        blake3::hash(&msg).as_bytes().to_vec()
    }
}

/// Signed transaction ready for submission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignedTransaction {
    /// Original transaction
    pub transaction: WalletTransaction,
    /// Signature(s)
    pub signatures: Vec<TransactionSignature>,
}

/// A signature on a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionSignature {
    /// Signer's public key
    pub public_key: Vec<u8>,
    /// Ed25519 signature
    pub signature: Vec<u8>,
    /// Signer index (for multi-sig)
    pub signer_index: Option<u32>,
}

impl SignedTransaction {
    /// Verify all signatures on the transaction
    pub fn verify(&self) -> Result<bool> {
        use ed25519_dalek::{Signature, VerifyingKey};

        let message = self.transaction.signing_message();

        for sig in &self.signatures {
            if sig.public_key.len() != 32 {
                return Err(Error::crypto("Invalid public key length"));
            }
            if sig.signature.len() != 64 {
                return Err(Error::crypto("Invalid signature length"));
            }

            let verifying_key = VerifyingKey::from_bytes(
                sig.public_key
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::crypto("Invalid public key format"))?,
            )
            .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

            let signature = Signature::from_bytes(
                sig.signature
                    .as_slice()
                    .try_into()
                    .map_err(|_| Error::crypto("Invalid signature format"))?,
            );

            verifying_key
                .verify_strict(&message, &signature)
                .map_err(|e| Error::crypto(format!("Signature verification failed: {}", e)))?;
        }

        Ok(true)
    }
}
