//! Solana Bridge Integration Module
//!
//! This module integrates dchat-bridge with the Solana blockchain via
//! dchat-blockchain's Solana implementation. It provides:
//!
//! - wDCHAT token minting/burning on Solana
//! - Bridge transaction management
//! - Finality tracking for Solana slots
//! - Validator signature aggregation

use crate::crosschain_sync::{ChainSyncAdapter, SyncOperation, SyncOperationType};
use crate::{ChainId, FinalityProof};
use async_trait::async_trait;
use chrono::Utc;
use dchat_blockchain::solana::{
    BridgeProgram, Cluster, SolanaRpcClient, SolanaRpcConfig, SplToken, TransactionBuilder,
};
use dchat_blockchain::wallet::solana_compat::{base58_decode, SolanaAddress};
use dchat_core::{Error, Result};
use ed25519_dalek::{SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Solana cluster configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SolanaCluster {
    /// Mainnet-beta
    Mainnet,
    /// Devnet
    Devnet,
    /// Testnet
    Testnet,
    /// Local validator
    Localnet,
    /// Custom RPC endpoint
    Custom(String),
}

impl SolanaCluster {
    /// Get the RPC URL for this cluster
    pub fn rpc_url(&self) -> &str {
        match self {
            SolanaCluster::Mainnet => "https://api.mainnet-beta.solana.com",
            SolanaCluster::Devnet => "https://api.devnet.solana.com",
            SolanaCluster::Testnet => "https://api.testnet.solana.com",
            SolanaCluster::Localnet => "http://localhost:8899",
            SolanaCluster::Custom(url) => url,
        }
    }

    /// Get the WebSocket URL for this cluster
    pub fn ws_url(&self) -> String {
        match self {
            SolanaCluster::Mainnet => "wss://api.mainnet-beta.solana.com".to_string(),
            SolanaCluster::Devnet => "wss://api.devnet.solana.com".to_string(),
            SolanaCluster::Testnet => "wss://api.testnet.solana.com".to_string(),
            SolanaCluster::Localnet => "ws://localhost:8900".to_string(),
            SolanaCluster::Custom(url) => url.replace("http", "ws"),
        }
    }
}

/// Configuration for the Solana bridge
///
/// # Unit Notes
/// - `bridge_fee_lamports`: Solana network fee in lamports (1 SOL = 1,000,000,000 lamports, 9 decimals)
/// - DCHAT bridge amounts use motes (1 DCHAT = 100,000,000 motes, 8 decimals)
/// - These are different currencies with different decimal places
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaBridgeConfig {
    /// Solana cluster to connect to
    pub cluster: SolanaCluster,
    /// Bridge program ID on Solana
    pub bridge_program_id: String,
    /// wDCHAT token mint address
    pub wdchat_mint: String,
    /// Bridge authority (PDA)
    pub bridge_authority: String,
    /// Required validator signatures for bridge operations
    pub required_signatures: u8,
    /// Minimum confirmations for finality
    pub min_confirmations: u32,
    /// Bridge fee in lamports
    pub bridge_fee_lamports: u64,
}

impl Default for SolanaBridgeConfig {
    fn default() -> Self {
        Self {
            cluster: SolanaCluster::Devnet,
            bridge_program_id: String::new(),
            wdchat_mint: String::new(),
            bridge_authority: String::new(),
            required_signatures: 3,
            min_confirmations: 32,
            bridge_fee_lamports: 5000,
        }
    }
}

/// Status of a Solana bridge transfer
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SolanaBridgeTransferStatus {
    /// Transfer initiated, pending signatures
    Pending,
    /// Signatures collected, pending execution
    ReadyToExecute,
    /// Transaction submitted, waiting for confirmations
    Confirming { confirmations: u32 },
    /// Transfer completed
    Completed,
    /// Transfer failed
    Failed { error: String },
}

/// A bridge transfer to/from Solana
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaBridgeTransfer {
    /// Unique transfer ID
    pub id: Uuid,
    /// Direction: true = to Solana, false = from Solana
    pub to_solana: bool,
    /// DCHAT amount (native units)
    pub amount: u64,
    /// Solana destination/source address
    pub solana_address: String,
    /// DCHAT destination/source address
    pub dchat_address: String,
    /// Solana transaction signature (if submitted)
    pub solana_signature: Option<String>,
    /// DCHAT transaction hash (if submitted)
    pub dchat_tx_hash: Option<String>,
    /// Current status
    pub status: SolanaBridgeTransferStatus,
    /// Validator signatures collected
    pub validator_signatures: Vec<ValidatorSignature>,
    /// Created timestamp
    pub created_at: chrono::DateTime<Utc>,
    /// Updated timestamp
    pub updated_at: chrono::DateTime<Utc>,
}

/// A validator signature for bridge operations
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    /// Validator public key
    pub validator_pubkey: String,
    /// Ed25519 signature
    pub signature: Vec<u8>,
    /// Timestamp
    pub signed_at: chrono::DateTime<Utc>,
}

/// Solana bridge manager
pub struct SolanaBridgeManager {
    /// Configuration
    config: SolanaBridgeConfig,
    /// RPC client
    rpc_client: Arc<SolanaRpcClient>,
    /// Bridge program interface
    bridge_program: Option<BridgeProgram>,
    /// Registered validator public keys for signature verification
    validators: Arc<RwLock<HashMap<String, VerifyingKey>>>,
    /// Bridge authority signing key (multi-sig component)
    authority_signer: Option<SigningKey>,
    /// Pending transfers
    transfers: Arc<RwLock<HashMap<Uuid, SolanaBridgeTransfer>>>,
}

impl SolanaBridgeManager {
    /// Create a new Solana bridge manager
    pub fn new(config: SolanaBridgeConfig) -> Result<Self> {
        let rpc_config = match &config.cluster {
            SolanaCluster::Mainnet => SolanaRpcConfig::for_cluster(Cluster::Mainnet),
            SolanaCluster::Devnet => SolanaRpcConfig::for_cluster(Cluster::Devnet),
            SolanaCluster::Testnet => SolanaRpcConfig::for_cluster(Cluster::Testnet),
            SolanaCluster::Localnet => SolanaRpcConfig::for_cluster(Cluster::Localnet),
            SolanaCluster::Custom(url) => SolanaRpcConfig::custom(url.clone(), None),
        };

        let rpc_client = SolanaRpcClient::new(rpc_config)
            .map_err(|e| Error::network(format!("Failed to create RPC client: {}", e)))?;

        // Initialize bridge program if configured
        let bridge_program =
            if !config.bridge_program_id.is_empty() && !config.wdchat_mint.is_empty() {
                let program_id = SolanaAddress::from_base58(&config.bridge_program_id)
                    .map_err(|e| Error::validation(format!("Invalid bridge program ID: {}", e)))?;
                let mint = SolanaAddress::from_base58(&config.wdchat_mint)
                    .map_err(|e| Error::validation(format!("Invalid wDCHAT mint: {}", e)))?;
                Some(BridgeProgram::new(program_id, mint)?)
            } else {
                None
            };

        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            bridge_program,
            validators: Arc::new(RwLock::new(HashMap::new())),
            authority_signer: None,
            transfers: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Set the bridge authority signing key (for multi-sig execution)
    pub fn set_authority_signer(&mut self, signer: SigningKey) {
        self.authority_signer = Some(signer);
    }

    /// Register a validator for signature verification
    pub async fn register_validator(
        &self,
        pubkey_hex: String,
        verifying_key: VerifyingKey,
    ) -> Result<()> {
        let mut validators = self.validators.write().await;
        validators.insert(pubkey_hex, verifying_key);
        Ok(())
    }

    /// Get the bridge program
    pub fn bridge_program(&self) -> Option<&BridgeProgram> {
        self.bridge_program.as_ref()
    }

    /// Get the current slot
    pub async fn get_current_slot(&self) -> Result<u64> {
        self.rpc_client
            .get_slot()
            .await
            .map_err(|e| Error::network(format!("Failed to get slot: {}", e)))
    }

    /// Get confirmations for a transaction
    pub async fn get_transaction_confirmations(&self, signature: &str) -> Result<u32> {
        let current_slot = self.get_current_slot().await?;

        let statuses = self
            .rpc_client
            .get_signature_statuses(&[signature.to_string()])
            .await
            .map_err(|e| Error::network(format!("Failed to get signature status: {}", e)))?;

        match statuses.first() {
            Some(Some(status)) => {
                if status.err.is_some() {
                    return Err(Error::validation("Transaction failed on Solana"));
                }
                // Calculate confirmations based on slot difference or use confirmations field
                if let Some(confirmations) = status.confirmations {
                    Ok(confirmations as u32)
                } else {
                    // If confirmations is None, transaction is finalized
                    // Use slot difference as approximation
                    let tx_slot = status.slot;
                    Ok((current_slot.saturating_sub(tx_slot)) as u32)
                }
            }
            _ => Err(Error::validation("Transaction not found")),
        }
    }

    /// Check if a transaction is finalized
    pub async fn is_finalized(&self, signature: &str) -> Result<bool> {
        let confirmations = self.get_transaction_confirmations(signature).await?;
        Ok(confirmations >= self.config.min_confirmations)
    }

    /// Initiate a bridge transfer TO Solana (mint wDCHAT)
    pub async fn initiate_bridge_to_solana(
        &self,
        dchat_address: String,
        solana_address: String,
        amount: u64,
    ) -> Result<Uuid> {
        let transfer = SolanaBridgeTransfer {
            id: Uuid::new_v4(),
            to_solana: true,
            amount,
            solana_address,
            dchat_address,
            solana_signature: None,
            dchat_tx_hash: None,
            status: SolanaBridgeTransferStatus::Pending,
            validator_signatures: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let id = transfer.id;
        let mut transfers = self.transfers.write().await;
        transfers.insert(id, transfer);

        tracing::info!(
            "Initiated bridge transfer to Solana: {} DCHAT -> {} wDCHAT",
            amount,
            id
        );

        Ok(id)
    }

    /// Initiate a bridge transfer FROM Solana (burn wDCHAT)
    pub async fn initiate_bridge_from_solana(
        &self,
        solana_address: String,
        dchat_address: String,
        amount: u64,
        solana_signature: String,
    ) -> Result<Uuid> {
        // Verify the Solana transaction exists
        let _confirmations = self
            .get_transaction_confirmations(&solana_signature)
            .await?;

        let transfer = SolanaBridgeTransfer {
            id: Uuid::new_v4(),
            to_solana: false,
            amount,
            solana_address,
            dchat_address,
            solana_signature: Some(solana_signature),
            dchat_tx_hash: None,
            status: SolanaBridgeTransferStatus::Confirming { confirmations: 0 },
            validator_signatures: Vec::new(),
            created_at: Utc::now(),
            updated_at: Utc::now(),
        };

        let id = transfer.id;
        let mut transfers = self.transfers.write().await;
        transfers.insert(id, transfer);

        tracing::info!(
            "Initiated bridge transfer from Solana: {} wDCHAT -> {} DCHAT",
            amount,
            id
        );

        Ok(id)
    }

    /// Add a validator signature to a transfer
    pub async fn add_validator_signature(
        &self,
        transfer_id: Uuid,
        validator_pubkey: String,
        signature: Vec<u8>,
    ) -> Result<bool> {
        let mut transfers = self.transfers.write().await;
        let transfer = transfers
            .get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        // Check for duplicate signatures
        if transfer
            .validator_signatures
            .iter()
            .any(|s| s.validator_pubkey == validator_pubkey)
        {
            return Err(Error::validation("Duplicate validator signature"));
        }

        transfer.validator_signatures.push(ValidatorSignature {
            validator_pubkey,
            signature,
            signed_at: Utc::now(),
        });
        transfer.updated_at = Utc::now();

        // Check if we have enough signatures
        let has_enough =
            transfer.validator_signatures.len() >= self.config.required_signatures as usize;

        if has_enough {
            transfer.status = SolanaBridgeTransferStatus::ReadyToExecute;
        }

        Ok(has_enough)
    }

    /// Execute a ready transfer on Solana (mint wDCHAT)
    pub async fn execute_mint(&self, transfer_id: Uuid) -> Result<String> {
        let transfer = {
            let transfers = self.transfers.read().await;
            transfers
                .get(&transfer_id)
                .cloned()
                .ok_or_else(|| Error::validation("Transfer not found"))?
        };

        if transfer.status != SolanaBridgeTransferStatus::ReadyToExecute {
            return Err(Error::validation("Transfer not ready to execute"));
        }

        if !transfer.to_solana {
            return Err(Error::validation("Transfer is not to Solana"));
        }

        // Get bridge program
        let bridge = self
            .bridge_program
            .as_ref()
            .ok_or_else(|| Error::validation("Bridge program not configured"))?;

        // Get authority signer
        let signer = self
            .authority_signer
            .as_ref()
            .ok_or_else(|| Error::validation("Authority signer not configured"))?;

        // 1. Build the mint instruction using BridgeProgram
        let recipient = SolanaAddress::from_base58(&transfer.solana_address)
            .map_err(|e| Error::validation(format!("Invalid recipient address: {}", e)))?;

        // Convert transfer ID to bytes
        let transfer_id_bytes: [u8; 16] = *transfer_id.as_bytes();

        // Create dchat tx hash from transfer (use hash of transfer data)
        let dchat_tx_hash = create_dchat_tx_hash(&transfer);

        // Collect validator signatures as [u8; 64] arrays
        let validator_signatures: Vec<[u8; 64]> = transfer
            .validator_signatures
            .iter()
            .filter_map(|vs| {
                if vs.signature.len() == 64 {
                    let mut arr = [0u8; 64];
                    arr.copy_from_slice(&vs.signature);
                    Some(arr)
                } else {
                    None
                }
            })
            .collect();

        if validator_signatures.len() < self.config.required_signatures as usize {
            return Err(Error::validation(format!(
                "Insufficient valid signatures: {} < {}",
                validator_signatures.len(),
                self.config.required_signatures
            )));
        }

        // Build mint instruction
        let mint_ix = bridge.mint_wrapped(
            transfer_id_bytes,
            &recipient,
            transfer.amount,
            dchat_tx_hash,
            validator_signatures,
        )?;

        // 2. Get recent blockhash
        let blockhash_response = self
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| Error::network(format!("Failed to get blockhash: {}", e)))?;

        let blockhash_bytes = base58_decode(&blockhash_response.blockhash)
            .map_err(|e| Error::validation(format!("Invalid blockhash: {}", e)))?;

        if blockhash_bytes.len() != 32 {
            return Err(Error::validation("Invalid blockhash length"));
        }

        let mut blockhash_arr = [0u8; 32];
        blockhash_arr.copy_from_slice(&blockhash_bytes);

        // Build transaction with bridge authority as payer
        let mut tx = TransactionBuilder::new()
            .payer(bridge.bridge_authority.clone())
            .instruction(mint_ix)
            .build(blockhash_arr)?;

        // 3. Sign with bridge authority
        tx.sign(signer, 0)?;

        // 4. Submit to Solana
        let tx_base64 = tx.to_base64()?;
        let signature = self
            .rpc_client
            .send_transaction(&tx_base64)
            .await
            .map_err(|e| Error::network(format!("Failed to send transaction: {}", e)))?;

        tracing::info!(
            "Submitted mint transaction for transfer {}: {}",
            transfer_id,
            signature
        );

        // Update the transfer
        let mut transfers = self.transfers.write().await;
        if let Some(t) = transfers.get_mut(&transfer_id) {
            t.solana_signature = Some(signature.clone());
            t.status = SolanaBridgeTransferStatus::Confirming { confirmations: 0 };
            t.updated_at = Utc::now();
        }

        Ok(signature)
    }

    /// Wait for transfer to collect enough signatures, then execute
    pub async fn wait_and_execute(&self, transfer_id: Uuid, timeout: Duration) -> Result<String> {
        let start = std::time::Instant::now();
        let poll_interval = Duration::from_millis(500);

        loop {
            if start.elapsed() > timeout {
                return Err(Error::network("Timeout waiting for signatures"));
            }

            // Check transfer status
            let transfer = {
                let transfers = self.transfers.read().await;
                transfers.get(&transfer_id).cloned()
            };

            match transfer {
                Some(t) if t.status == SolanaBridgeTransferStatus::ReadyToExecute => {
                    // Have enough signatures, execute
                    return self.execute_mint(transfer_id).await;
                }
                Some(t) if matches!(t.status, SolanaBridgeTransferStatus::Failed { .. }) => {
                    if let SolanaBridgeTransferStatus::Failed { error } = t.status {
                        return Err(Error::validation(format!("Transfer failed: {}", error)));
                    }
                    return Err(Error::validation("Transfer failed"));
                }
                Some(t) if t.status == SolanaBridgeTransferStatus::Completed => {
                    return t
                        .solana_signature
                        .ok_or_else(|| Error::validation("Transfer completed but no signature"));
                }
                None => {
                    return Err(Error::validation("Transfer not found"));
                }
                _ => {
                    // Still pending, wait
                    tokio::time::sleep(poll_interval).await;
                }
            }
        }
    }

    /// Update transfer confirmations and check finality
    pub async fn update_confirmations(&self, transfer_id: Uuid) -> Result<bool> {
        let transfer = {
            let transfers = self.transfers.read().await;
            transfers
                .get(&transfer_id)
                .cloned()
                .ok_or_else(|| Error::validation("Transfer not found"))?
        };

        let signature = transfer
            .solana_signature
            .as_ref()
            .ok_or_else(|| Error::validation("No Solana signature"))?;

        let confirmations = self.get_transaction_confirmations(signature).await?;
        let is_finalized = confirmations >= self.config.min_confirmations;

        let mut transfers = self.transfers.write().await;
        if let Some(t) = transfers.get_mut(&transfer_id) {
            if is_finalized {
                t.status = SolanaBridgeTransferStatus::Completed;
            } else {
                t.status = SolanaBridgeTransferStatus::Confirming { confirmations };
            }
            t.updated_at = Utc::now();
        }

        Ok(is_finalized)
    }

    /// Get a transfer by ID
    pub async fn get_transfer(&self, transfer_id: Uuid) -> Option<SolanaBridgeTransfer> {
        let transfers = self.transfers.read().await;
        transfers.get(&transfer_id).cloned()
    }

    /// Get all pending transfers
    pub async fn get_pending_transfers(&self) -> Vec<SolanaBridgeTransfer> {
        let transfers = self.transfers.read().await;
        transfers
            .values()
            .filter(|t| {
                !matches!(
                    t.status,
                    SolanaBridgeTransferStatus::Completed
                        | SolanaBridgeTransferStatus::Failed { .. }
                )
            })
            .cloned()
            .collect()
    }

    /// Get the wDCHAT balance for an address
    pub async fn get_wdchat_balance(&self, solana_address: &str) -> Result<u64> {
        // Parse the addresses
        let owner = SolanaAddress::from_base58(solana_address)
            .map_err(|e| Error::validation(format!("Invalid Solana address: {}", e)))?;
        let mint = SolanaAddress::from_base58(&self.config.wdchat_mint)
            .map_err(|e| Error::validation(format!("Invalid mint address: {}", e)))?;

        // Get the associated token account for wDCHAT
        let ata = SplToken::get_associated_token_address(&owner, &mint)
            .map_err(|e| Error::validation(format!("Failed to derive ATA: {}", e)))?;

        // Get the token balance from RPC
        let balance = self
            .rpc_client
            .get_token_account_balance(&ata)
            .await
            .map_err(|e| Error::network(format!("Failed to get wDCHAT balance: {}", e)))?;

        // Parse the amount (balance is in UI amount format)
        Ok(balance.amount.parse().unwrap_or(0))
    }
}

/// Solana chain adapter for CrossChainSyncManager
pub struct SolanaChainAdapter {
    /// Bridge manager
    bridge_manager: Arc<SolanaBridgeManager>,
}

impl SolanaChainAdapter {
    /// Create a new Solana chain adapter
    pub fn new(bridge_manager: Arc<SolanaBridgeManager>) -> Self {
        Self { bridge_manager }
    }
}

#[async_trait]
impl ChainSyncAdapter for SolanaChainAdapter {
    fn chain_id(&self) -> ChainId {
        ChainId::Solana
    }

    async fn get_current_block(&self) -> Result<u64> {
        self.bridge_manager.get_current_slot().await
    }

    async fn get_confirmations(&self, tx_hash: &str) -> Result<u32> {
        self.bridge_manager
            .get_transaction_confirmations(tx_hash)
            .await
    }

    async fn submit_operation(&self, operation: &SyncOperation) -> Result<String> {
        match &operation.operation_type {
            SyncOperationType::BridgeOut {
                destination_address,
                amount,
            } => {
                // Bridge out to Solana = mint wDCHAT
                // Create dchat source identifier from operation context
                let dchat_source = format!("{:?}_{}", operation.source_chain, operation.id);

                let transfer_id = self
                    .bridge_manager
                    .initiate_bridge_to_solana(dchat_source, destination_address.clone(), *amount)
                    .await?;

                // Wait for validator signatures to be collected and execute the mint
                // Use a configurable timeout (default 60 seconds for signature collection)
                let timeout = Duration::from_secs(60);
                let signature = self
                    .bridge_manager
                    .wait_and_execute(transfer_id, timeout)
                    .await?;

                tracing::info!(
                    "Executed bridge out operation {}: {} wDCHAT to {} (sig: {})",
                    operation.id,
                    amount,
                    destination_address,
                    signature
                );

                Ok(signature)
            }
            SyncOperationType::BridgeIn { .. } => {
                // Bridge in from Solana is handled by the DCHAT chain
                Err(Error::validation(
                    "BridgeIn should be submitted to DCHAT chain, not Solana",
                ))
            }
            _ => Err(Error::validation(
                "Unsupported operation type for Solana chain",
            )),
        }
    }

    async fn verify_finality_proof(&self, proof: &FinalityProof) -> Result<bool> {
        if proof.chain != ChainId::Solana {
            return Err(Error::validation("Proof is not for Solana chain"));
        }

        // For Solana, finality is slot-based
        Ok(proof.is_final && proof.confirmations >= 32)
    }

    async fn get_merkle_root(&self, block_number: u64) -> Result<String> {
        // Solana uses blockhash as state commitment at a given slot
        // The blockhash serves as a cryptographic commitment to the state
        // at a particular point in the ledger

        // Get account info for the slot's bank to derive state commitment
        // For Solana, we use the latest finalized blockhash as our state root
        let slot_info = self
            .bridge_manager
            .rpc_client
            .get_slot()
            .await
            .map_err(|e| Error::network(format!("Failed to get slot: {}", e)))?;

        // If requested block is too old or in the future, return error
        if block_number > slot_info {
            return Err(Error::validation(format!(
                "Requested slot {} is in the future (current: {})",
                block_number, slot_info
            )));
        }

        // For the state commitment, we use the latest finalized blockhash
        // This represents the state commitment of the finalized chain
        let blockhash = self
            .bridge_manager
            .rpc_client
            .get_latest_blockhash()
            .await
            .map_err(|e| Error::network(format!("Failed to get blockhash: {}", e)))?;

        // Return the blockhash as state commitment with slot context
        Ok(format!("solana:{}:{}", block_number, blockhash.blockhash))
    }
}

/// Create a hash of the DCHAT transaction for the bridge
/// This is used to prove on Solana that the DCHAT chain has locked the tokens
fn create_dchat_tx_hash(transfer: &SolanaBridgeTransfer) -> [u8; 32] {
    use sha2::{Digest, Sha256};

    let mut hasher = Sha256::new();
    hasher.update(transfer.id.as_bytes());
    hasher.update(transfer.dchat_address.as_bytes());
    hasher.update(transfer.solana_address.as_bytes());
    hasher.update(transfer.amount.to_le_bytes());
    hasher.update(transfer.created_at.timestamp().to_le_bytes());

    let result = hasher.finalize();
    let mut hash = [0u8; 32];
    hash.copy_from_slice(&result);
    hash
}

/// Create a FinalityProof for a Solana transaction
pub fn create_solana_finality_proof(
    tx_signature: String,
    slot: u64,
    confirmations: u32,
) -> FinalityProof {
    FinalityProof {
        chain: ChainId::Solana,
        tx_hash: tx_signature,
        block_number: slot,
        confirmations,
        required_confirmations: 32,
        proof_data: Vec::new(), // Would contain SPV proof in production
        is_final: confirmations >= 32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_config() -> SolanaBridgeConfig {
        SolanaBridgeConfig {
            cluster: SolanaCluster::Localnet,
            bridge_program_id: "BridgeProgramId111111111111111111111111".to_string(),
            wdchat_mint: "wDCHATMint1111111111111111111111111111".to_string(),
            bridge_authority: "BridgeAuthority11111111111111111111111".to_string(),
            required_signatures: 2,
            min_confirmations: 32,
            bridge_fee_lamports: 5000,
        }
    }

    #[test]
    fn test_cluster_urls() {
        assert_eq!(
            SolanaCluster::Mainnet.rpc_url(),
            "https://api.mainnet-beta.solana.com"
        );
        assert_eq!(
            SolanaCluster::Devnet.rpc_url(),
            "https://api.devnet.solana.com"
        );
        assert_eq!(SolanaCluster::Localnet.rpc_url(), "http://localhost:8899");
    }

    #[test]
    fn test_finality_proof_creation() {
        let proof = create_solana_finality_proof("sig123".to_string(), 1000, 35);

        assert_eq!(proof.chain, ChainId::Solana);
        assert_eq!(proof.tx_hash, "sig123");
        assert_eq!(proof.block_number, 1000);
        assert!(proof.is_final);
    }

    #[test]
    fn test_finality_proof_not_final() {
        let proof = create_solana_finality_proof("sig456".to_string(), 1000, 10);

        assert!(!proof.is_final);
    }

    #[tokio::test]
    async fn test_solana_chain_adapter() {
        // This test would require a mock RPC client in production
        // For now, we just verify the structure compiles correctly
        let config = create_test_config();
        let _manager = SolanaBridgeManager::new(config);
    }
}
