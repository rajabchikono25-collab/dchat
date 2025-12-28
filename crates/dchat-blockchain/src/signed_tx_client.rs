//! Signed Transaction Client
//!
//! This module provides client-side support for creating, signing, and submitting
//! signed transaction envelopes. It bridges the wallet module with the chain's
//! signed envelope format.
//!
//! # Usage
//!
//! ```rust,ignore
//! use dchat_blockchain::signed_tx_client::{SignedTxClient, SignedTxClientConfig};
//!
//! // Create client with wallet
//! let mut wallet = Wallet::from_mnemonic(&phrase, None, WalletConfig::default())?;
//! let client = SignedTxClient::new(config, wallet)?;
//!
//! // Submit a signed transaction
//! let tx_id = client.register_user(&user_id, "alice").await?;
//! ```

use crate::client::{ChainRpcClient, HttpRpcClient};
use crate::wallet::Wallet;
use chrono::Utc;
use dchat_chain::signed_envelope::{
    address_from_public_key, chain_ids, EnvelopeBuilder, SignedTransactionEnvelope,
};
use dchat_chain::{CreateChannelTx, RegisterUserTx, SendDirectMessageTx, TransactionType};
use dchat_core::error::{Error, Result};
use dchat_core::types::{ChannelId, MessageId, UserId};
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Configuration for the signed transaction client
#[derive(Debug, Clone)]
pub struct SignedTxClientConfig {
    /// RPC endpoint URL
    pub rpc_url: String,
    /// Chain ID (use chain_ids constants)
    pub chain_id: u32,
    /// Request timeout in seconds
    pub timeout_secs: u64,
}

impl Default for SignedTxClientConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8545".to_string(),
            chain_id: chain_ids::TESTNET_CHAT,
            timeout_secs: 30,
        }
    }
}

impl SignedTxClientConfig {
    /// Create config for mainnet chat chain
    pub fn mainnet_chat(rpc_url: String) -> Self {
        Self {
            rpc_url,
            chain_id: chain_ids::MAINNET_CHAT,
            timeout_secs: 30,
        }
    }

    /// Create config for mainnet currency chain
    pub fn mainnet_currency(rpc_url: String) -> Self {
        Self {
            rpc_url,
            chain_id: chain_ids::MAINNET_CURRENCY,
            timeout_secs: 30,
        }
    }

    /// Create config for testnet chat chain
    pub fn testnet_chat(rpc_url: String) -> Self {
        Self {
            rpc_url,
            chain_id: chain_ids::TESTNET_CHAT,
            timeout_secs: 30,
        }
    }
}

/// Client for submitting signed transaction envelopes
pub struct SignedTxClient {
    config: SignedTxClientConfig,
    rpc_client: Arc<dyn ChainRpcClient>,
    wallet: Arc<RwLock<Wallet>>,
    /// Current nonce (should be fetched from chain, cached here)
    nonce: Arc<RwLock<u64>>,
}

impl SignedTxClient {
    /// Create a new signed transaction client
    pub fn new(config: SignedTxClientConfig, wallet: Wallet) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;

        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            wallet: Arc::new(RwLock::new(wallet)),
            nonce: Arc::new(RwLock::new(0)),
        })
    }

    /// Create with custom RPC client (for testing)
    #[cfg(any(test, feature = "test-mocks"))]
    pub fn with_rpc(
        config: SignedTxClientConfig,
        wallet: Wallet,
        rpc_client: Arc<dyn ChainRpcClient>,
    ) -> Self {
        Self {
            config,
            rpc_client,
            wallet: Arc::new(RwLock::new(wallet)),
            nonce: Arc::new(RwLock::new(0)),
        }
    }

    /// Get the sender's UserId (derived from wallet public key)
    pub fn sender_id(&self) -> Result<UserId> {
        let wallet = self.wallet.read().unwrap();
        let public_key = wallet
            .public_key()
            .ok_or_else(|| Error::crypto("Wallet has no public key"))?;
        let public_key_bytes: &[u8; 32] = public_key.as_bytes();
        let user_id = address_from_public_key(public_key_bytes);
        Ok(UserId(user_id))
    }

    /// Get the current nonce (fetch from chain if needed)
    pub fn current_nonce(&self) -> u64 {
        *self.nonce.read().unwrap()
    }

    /// Set the nonce (call after fetching from chain)
    pub fn set_nonce(&self, nonce: u64) {
        *self.nonce.write().unwrap() = nonce;
    }

    /// Increment and get next nonce
    fn next_nonce(&self) -> u64 {
        let mut nonce = self.nonce.write().unwrap();
        *nonce += 1;
        *nonce
    }

    /// Build and sign an envelope
    fn build_envelope(
        &self,
        tx_type: TransactionType,
        payload: Vec<u8>,
    ) -> Result<SignedTransactionEnvelope> {
        let wallet = self.wallet.read().unwrap();

        let public_key = wallet
            .public_key()
            .ok_or_else(|| Error::crypto("Wallet has no public key"))?;
        let public_key_bytes: [u8; 32] = *public_key.as_bytes();

        let signing_key = wallet
            .signing_key()
            .ok_or_else(|| Error::crypto("Wallet cannot sign (watch-only)"))?;

        let nonce = self.next_nonce();

        let envelope = EnvelopeBuilder::new(self.config.chain_id, public_key_bytes)
            .nonce(nonce)
            .chat_tx(tx_type)
            .payload(payload)
            .build_and_sign(signing_key)
            .map_err(|e| Error::crypto(format!("Failed to build envelope: {}", e)))?;

        Ok(envelope)
    }

    /// Submit a signed envelope to the chain
    async fn submit_envelope(&self, envelope: SignedTransactionEnvelope) -> Result<String> {
        let envelope_bytes = envelope
            .to_bytes()
            .map_err(|e| Error::internal(format!("Failed to serialize envelope: {}", e)))?;

        let tx_hash = self.rpc_client.submit_transaction(envelope_bytes).await?;

        tracing::info!(
            "Submitted signed envelope: sender={}, nonce={}, hash={}",
            Uuid::from_bytes(envelope.sender),
            envelope.nonce,
            tx_hash
        );

        Ok(tx_hash)
    }

    /// Register a new user (signed)
    pub async fn register_user(&self, username: &str) -> Result<(Uuid, String)> {
        let sender_id = self.sender_id()?;
        let wallet = self.wallet.read().unwrap();
        let public_key = wallet
            .public_key()
            .ok_or_else(|| Error::crypto("Wallet has no public key"))?;

        let tx_payload = RegisterUserTx {
            user_id: sender_id.clone(),
            username: username.to_string(),
            public_key: hex::encode(public_key.as_bytes()),
            timestamp: Utc::now(),
            initial_reputation: 0,
        };

        let payload_bytes = bincode::serialize(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        drop(wallet); // Release lock before building envelope

        let envelope = self.build_envelope(TransactionType::RegisterUser, payload_bytes)?;
        let tx_hash = self.submit_envelope(envelope).await?;

        Ok((sender_id.0, tx_hash))
    }

    /// Send a direct message (signed)
    pub async fn send_direct_message(
        &self,
        message_id: MessageId,
        recipient_id: UserId,
        content_hash: &str,
        payload_size: usize,
        relay_node_id: Option<String>,
    ) -> Result<String> {
        let sender_id = self.sender_id()?;

        let tx_payload = SendDirectMessageTx {
            message_id,
            sender_id,
            recipient_id,
            content_hash: content_hash.to_string(),
            timestamp: Utc::now(),
            payload_size,
            relay_node_id,
        };

        let payload_bytes = bincode::serialize(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let envelope = self.build_envelope(TransactionType::SendDirectMessage, payload_bytes)?;
        self.submit_envelope(envelope).await
    }

    /// Create a channel (signed)
    pub async fn create_channel(
        &self,
        channel_id: ChannelId,
        name: &str,
        description: &str,
    ) -> Result<String> {
        let creator_id = self.sender_id()?;

        let tx_payload = CreateChannelTx {
            channel_id,
            name: name.to_string(),
            description: description.to_string(),
            creator_id,
            visibility: dchat_chain::ChannelVisibility::Public,
            timestamp: Utc::now(),
            stake_amount: None,
        };

        let payload_bytes = bincode::serialize(&tx_payload)
            .map_err(|e| Error::internal(format!("Failed to serialize tx: {}", e)))?;

        let envelope = self.build_envelope(TransactionType::CreateChannel, payload_bytes)?;
        self.submit_envelope(envelope).await
    }

    /// Submit a raw signed envelope (for advanced use cases)
    pub async fn submit_raw(&self, envelope: SignedTransactionEnvelope) -> Result<String> {
        self.submit_envelope(envelope).await
    }

    /// Get wallet reference for inspection
    pub fn wallet(&self) -> &Arc<RwLock<Wallet>> {
        &self.wallet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::WalletConfig;
    use dchat_crypto::MnemonicLength;

    #[test]
    fn test_sender_id_derivation() {
        let config = WalletConfig::default();
        let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

        let client = SignedTxClient::new(SignedTxClientConfig::default(), wallet).unwrap();

        let sender_id = client.sender_id().unwrap();

        // Sender ID should be deterministic
        let sender_id2 = client.sender_id().unwrap();
        assert_eq!(sender_id, sender_id2);
    }

    #[test]
    fn test_nonce_increment() {
        let config = WalletConfig::default();
        let (wallet, _phrase) = Wallet::create(config, MnemonicLength::Words12, None).unwrap();

        let client = SignedTxClient::new(SignedTxClientConfig::default(), wallet).unwrap();

        assert_eq!(client.current_nonce(), 0);

        // Build envelope should increment nonce
        let _envelope = client
            .build_envelope(TransactionType::RegisterUser, b"test".to_vec())
            .unwrap();

        assert_eq!(client.current_nonce(), 1);
    }
}
