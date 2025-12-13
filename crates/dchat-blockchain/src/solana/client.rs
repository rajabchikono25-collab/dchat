//! High-Level Solana Client
//!
//! Unified interface combining RPC, transaction building, and wallet signing
//! for seamless Solana blockchain interaction.

use dchat_core::error::{Error, Result};
use ed25519_dalek::SigningKey;
use std::sync::Arc;
use tokio::sync::RwLock;

use super::accounts::SystemProgram;
use super::program::{BridgeProgram, BridgeState};
use super::rpc::{AccountInfoResponse, Commitment, RpcError, SolanaRpcClient, SolanaRpcConfig};
use super::spl_token::SplToken;
use super::transaction::{SolanaTransaction, TransactionBuilder};
use super::{Cluster, LAMPORTS_PER_SOL};
use crate::wallet::solana_compat::{base58_decode, SolanaAddress};

/// High-level Solana client for dchat
pub struct SolanaClient {
    /// RPC client
    rpc: SolanaRpcClient,
    /// Cluster
    cluster: Cluster,
    /// Bridge program (if configured)
    bridge: Option<BridgeProgram>,
    /// wDCHAT mint address
    wdchat_mint: Option<SolanaAddress>,
}

impl SolanaClient {
    /// Create a new Solana client
    pub fn new(cluster: Cluster) -> Result<Self> {
        let config = SolanaRpcConfig::for_cluster(cluster);
        let rpc = SolanaRpcClient::new(config)?;

        Ok(Self {
            rpc,
            cluster,
            bridge: None,
            wdchat_mint: None,
        })
    }

    /// Create with custom RPC endpoint
    pub fn with_endpoint(rpc_url: &str) -> Result<Self> {
        let config = SolanaRpcConfig::custom(rpc_url.to_string(), None);
        let rpc = SolanaRpcClient::new(config)?;

        Ok(Self {
            rpc,
            cluster: Cluster::Custom,
            bridge: None,
            wdchat_mint: None,
        })
    }

    /// Configure bridge program
    pub fn with_bridge(
        mut self,
        program_id: SolanaAddress,
        wdchat_mint: SolanaAddress,
    ) -> Result<Self> {
        self.bridge = Some(BridgeProgram::new(program_id.clone(), wdchat_mint.clone())?);
        self.wdchat_mint = Some(wdchat_mint);
        Ok(self)
    }

    /// Get RPC client
    pub fn rpc(&self) -> &SolanaRpcClient {
        &self.rpc
    }

    /// Get cluster
    pub fn cluster(&self) -> Cluster {
        self.cluster
    }

    /// Get bridge program
    pub fn bridge(&self) -> Option<&BridgeProgram> {
        self.bridge.as_ref()
    }

    // ============ Account Operations ============

    /// Get SOL balance for an address
    pub async fn get_balance(&self, address: &SolanaAddress) -> Result<u64> {
        self.rpc
            .get_balance(address)
            .await
            .map_err(|e| Error::network(format!("Failed to get balance: {}", e)))
    }

    /// Get SOL balance in SOL (not lamports)
    pub async fn get_balance_sol(&self, address: &SolanaAddress) -> Result<f64> {
        let lamports = self.get_balance(address).await?;
        Ok(lamports as f64 / LAMPORTS_PER_SOL as f64)
    }

    /// Get wDCHAT token balance
    pub async fn get_wdchat_balance(&self, owner: &SolanaAddress) -> Result<u64> {
        let mint = self
            .wdchat_mint
            .as_ref()
            .ok_or_else(|| Error::validation("wDCHAT mint not configured"))?;

        let ata = SplToken::get_associated_token_address(owner, mint)?;

        match self.rpc.get_token_account_balance(&ata).await {
            Ok(balance) => Ok(balance.as_u64()),
            Err(RpcError::Rpc { code, .. }) if code == -32602 => {
                // Account doesn't exist
                Ok(0)
            }
            Err(e) => Err(Error::network(format!(
                "Failed to get token balance: {}",
                e
            ))),
        }
    }

    /// Get account info
    pub async fn get_account(
        &self,
        address: &SolanaAddress,
    ) -> Result<Option<AccountInfoResponse>> {
        self.rpc
            .get_account_info(address)
            .await
            .map_err(|e| Error::network(format!("Failed to get account: {}", e)))
    }

    /// Check if account exists
    pub async fn account_exists(&self, address: &SolanaAddress) -> Result<bool> {
        Ok(self.get_account(address).await?.is_some())
    }

    // ============ Transaction Operations ============

    /// Get latest blockhash for transactions (returns bytes)
    pub async fn get_blockhash_bytes(&self) -> Result<[u8; 32]> {
        let response = self
            .rpc
            .get_latest_blockhash()
            .await
            .map_err(|e| Error::network(format!("Failed to get blockhash: {}", e)))?;

        let bytes = base58_decode(&response.blockhash)
            .map_err(|e| Error::network(format!("Failed to decode blockhash: {}", e)))?;

        if bytes.len() != 32 {
            return Err(Error::validation("Invalid blockhash length"));
        }

        let mut result = [0u8; 32];
        result.copy_from_slice(&bytes);
        Ok(result)
    }

    /// Get latest blockhash as string
    pub async fn get_blockhash(&self) -> Result<String> {
        let response = self
            .rpc
            .get_latest_blockhash()
            .await
            .map_err(|e| Error::network(format!("Failed to get blockhash: {}", e)))?;
        Ok(response.blockhash)
    }

    /// Send SOL to an address
    pub async fn transfer_sol(
        &self,
        from: &SolanaAddress,
        to: &SolanaAddress,
        amount_lamports: u64,
        signer: &SigningKey,
    ) -> Result<String> {
        // Build transfer instruction
        let transfer_ix = SystemProgram::transfer(from, to, amount_lamports)?;

        // Get blockhash
        let blockhash = self.get_blockhash_bytes().await?;

        // Build transaction
        let mut tx = TransactionBuilder::new()
            .payer(from.clone())
            .instruction(transfer_ix)
            .build(blockhash)?;

        // Sign
        tx.sign(signer, 0)?;

        // Send
        let tx_base64 = tx.to_base64()?;
        let signature = self
            .rpc
            .send_transaction(&tx_base64)
            .await
            .map_err(|e| Error::network(format!("Failed to send transaction: {}", e)))?;

        Ok(signature)
    }

    /// Transfer wDCHAT tokens
    pub async fn transfer_wdchat(
        &self,
        from_owner: &SolanaAddress,
        to_owner: &SolanaAddress,
        amount: u64,
        signer: &SigningKey,
    ) -> Result<String> {
        let mint = self
            .wdchat_mint
            .as_ref()
            .ok_or_else(|| Error::validation("wDCHAT mint not configured"))?;

        // Get ATAs
        let from_ata = SplToken::get_associated_token_address(from_owner, mint)?;
        let to_ata = SplToken::get_associated_token_address(to_owner, mint)?;

        // Get blockhash
        let blockhash = self.get_blockhash_bytes().await?;

        // Check if destination ATA exists
        let mut builder = TransactionBuilder::new().payer(from_owner.clone());

        // Create ATA if needed
        if !self.account_exists(&to_ata).await? {
            let create_ata_ix =
                SplToken::create_associated_token_account_instruction(from_owner, to_owner, mint)?;
            builder = builder.instruction(create_ata_ix);
        }

        // Add transfer instruction (checked transfer for safety)
        let transfer_ix = SplToken::transfer_checked(
            &from_ata, mint, &to_ata, from_owner, amount, 9, // wDCHAT decimals
        )?;
        builder = builder.instruction(transfer_ix);

        // Build and sign
        let mut tx = builder.build(blockhash)?;
        tx.sign(signer, 0)?;

        // Send
        let tx_base64 = tx.to_base64()?;
        let signature = self
            .rpc
            .send_transaction(&tx_base64)
            .await
            .map_err(|e| Error::network(format!("Failed to send transaction: {}", e)))?;

        Ok(signature)
    }

    /// Send and confirm transaction
    pub async fn send_and_confirm(
        &self,
        tx: &SolanaTransaction,
        _commitment: Commitment,
    ) -> Result<String> {
        // Send
        let tx_base64 = tx.to_base64()?;
        let signature = self
            .rpc
            .send_transaction(&tx_base64)
            .await
            .map_err(|e| Error::network(format!("Failed to send: {}", e)))?;

        // Confirm with 60 second timeout
        self.rpc
            .confirm_transaction(&signature, std::time::Duration::from_secs(60))
            .await
            .map_err(|e| Error::network(format!("Failed to confirm: {}", e)))?;

        Ok(signature)
    }

    // ============ Bridge Operations ============

    /// Get bridge state
    pub async fn get_bridge_state(&self) -> Result<BridgeState> {
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| Error::validation("Bridge not configured"))?;

        let state_address = bridge.get_bridge_state_address()?;
        let account = self
            .rpc
            .get_account_info(&state_address)
            .await
            .map_err(|e| Error::network(format!("Failed to get bridge state: {}", e)))?
            .ok_or_else(|| Error::validation("Bridge state account not found"))?;

        // Decode account data
        let data = account
            .data
            .as_bytes()
            .ok_or_else(|| Error::validation("Failed to decode bridge state data"))?;

        BridgeState::from_bytes(&data)
    }

    /// Burn wDCHAT to bridge back to dchat
    /// Returns the signature of the burn transaction
    pub async fn bridge_to_dchat(
        &self,
        user: &SolanaAddress,
        amount: u64,
        dchat_recipient: [u8; 32],
        signer: &SigningKey,
    ) -> Result<String> {
        let bridge = self
            .bridge
            .as_ref()
            .ok_or_else(|| Error::validation("Bridge not configured"))?;

        // Build burn instruction
        let burn_ix = bridge.burn_wrapped(user, amount, dchat_recipient)?;

        // Get blockhash
        let blockhash = self.get_blockhash_bytes().await?;

        // Build transaction
        let mut tx = TransactionBuilder::new()
            .payer(user.clone())
            .instruction(burn_ix)
            .build(blockhash)?;

        // Sign
        tx.sign(signer, 0)?;

        // Send and confirm
        self.send_and_confirm(&tx, Commitment::Finalized).await
    }

    // ============ Utility Methods ============

    /// Request airdrop (devnet/testnet only)
    pub async fn request_airdrop(
        &self,
        address: &SolanaAddress,
        amount_sol: f64,
    ) -> Result<String> {
        if self.cluster == Cluster::Mainnet {
            return Err(Error::validation("Airdrop not available on mainnet"));
        }

        let lamports = (amount_sol * LAMPORTS_PER_SOL as f64) as u64;
        self.rpc
            .request_airdrop(address, lamports)
            .await
            .map_err(|e| Error::network(format!("Failed to request airdrop: {}", e)))
    }

    /// Get minimum rent-exempt balance for account size
    pub async fn get_rent_exemption(&self, data_len: usize) -> Result<u64> {
        self.rpc
            .get_minimum_balance_for_rent_exemption(data_len)
            .await
            .map_err(|e| Error::network(format!("Failed to get rent exemption: {}", e)))
    }

    /// Simulate a transaction without sending
    pub async fn simulate(&self, tx: &SolanaTransaction) -> Result<()> {
        let tx_base64 = tx.to_base64()?;
        self.rpc
            .simulate_transaction(&tx_base64)
            .await
            .map_err(|e| Error::network(format!("Simulation failed: {}", e)))?;
        Ok(())
    }
}

/// Thread-safe Solana client wrapper
pub type SharedSolanaClient = Arc<RwLock<SolanaClient>>;

/// Create a shared Solana client
pub fn create_shared_client(cluster: Cluster) -> Result<SharedSolanaClient> {
    Ok(Arc::new(RwLock::new(SolanaClient::new(cluster)?)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_creation() {
        let client = SolanaClient::new(Cluster::Devnet);
        assert!(client.is_ok());
    }

    #[test]
    fn test_cluster_detection() {
        let client = SolanaClient::new(Cluster::Mainnet).unwrap();
        assert_eq!(client.cluster(), Cluster::Mainnet);
    }

    #[tokio::test]
    #[ignore] // Requires network
    async fn test_get_balance() {
        let client = SolanaClient::new(Cluster::Devnet).unwrap();
        let test_addr = SolanaAddress::from_base58("11111111111111111111111111111111").unwrap();
        let balance = client.get_balance(&test_addr).await;
        // System program has 1 lamport
        assert!(balance.is_ok());
    }
}
