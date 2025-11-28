//! Currency Chain client for payments, staking, rewards, and economics

use chrono::Utc;
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use dchat_privacy::blind_tokens::CurrencyChainClient as PrivacyCurrencyChainClient;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, RwLock};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::client::{ChainRpcClient, HttpRpcClient, MockRpcClient};
use crate::tokenomics::{BurnReason, TokenomicsManager};

/// Configuration for Currency Chain client
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyChainConfig {
    /// RPC endpoint for currency chain node
    pub rpc_url: String,
    /// WebSocket endpoint for subscriptions
    pub ws_url: Option<String>,
    /// Confirmation threshold (number of blocks)
    pub confirmation_blocks: u32,
    /// Transaction timeout (seconds)
    pub tx_timeout_seconds: u64,
    /// Retry attempts for failed transactions
    pub max_retries: u32,
}

impl Default for CurrencyChainConfig {
    fn default() -> Self {
        Self {
            rpc_url: "http://localhost:8546".to_string(),
            ws_url: Some("ws://localhost:8547".to_string()),
            confirmation_blocks: 6,
            tx_timeout_seconds: 300,
            max_retries: 3,
        }
    }
}

/// Transaction on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyTransaction {
    pub id: Uuid,
    pub tx_type: String, // "payment", "stake", "reward", "slash", "swap"
    pub from: UserId,
    pub to: Option<UserId>,
    pub amount: u64,
    pub status: String, // "pending", "confirmed", "failed"
    pub confirmations: u32,
    pub block_height: u64,
    pub created_at: i64,
}

/// Wallet balance on currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Wallet {
    pub user_id: UserId,
    pub balance: u64,
    pub staked: u64,
    pub rewards_pending: u64,
}

/// Staking position
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakePosition {
    pub user_id: UserId,
    pub amount: u64,
    pub locked_until: i64,
    pub rewards_earned: u64,
}

/// Currency Chain client for payments, staking, rewards, and economics
pub struct CurrencyChainClient {
    config: CurrencyChainConfig,
    /// RPC client for blockchain queries
    rpc_client: Arc<dyn ChainRpcClient>,
    /// Transaction cache
    transactions: Arc<RwLock<HashMap<Uuid, CurrencyTransaction>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
    /// User wallet balances
    wallets: Arc<RwLock<HashMap<UserId, Wallet>>>,
    /// Staking positions
    stakes: Arc<RwLock<HashMap<UserId, StakePosition>>>,
    /// Tokenomics manager (optional - can be shared)
    tokenomics: Option<Arc<TokenomicsManager>>,
    /// Shutdown signal for block sync task
    shutdown_tx: Option<mpsc::Sender<()>>,
    /// Payment transaction hash -> (amount, recipient, confirmed)
    payment_records: Arc<RwLock<HashMap<String, (u64, String, bool)>>>,
    /// Redeemed token signature hashes (for blind token double-spend prevention)
    redeemed_tokens: Arc<RwLock<HashSet<[u8; 32]>>>,
}

impl CurrencyChainClient {
    /// Create new currency chain client with production RPC
    pub fn new(config: CurrencyChainConfig) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;
        
        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: None,
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Create new currency chain client with mock RPC for testing
    pub fn new_mock(config: CurrencyChainConfig) -> Self {
        let rpc_client = MockRpcClient::new();
        
        Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: None,
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
        }
    }

    /// Create new currency chain client with tokenomics integration
    pub fn with_tokenomics(
        config: CurrencyChainConfig,
        tokenomics: Arc<TokenomicsManager>,
    ) -> Result<Self> {
        let rpc_client = HttpRpcClient::new(config.rpc_url.clone())?;
        
        Ok(Self {
            config,
            rpc_client: Arc::new(rpc_client),
            transactions: Arc::new(RwLock::new(HashMap::new())),
            current_block: Arc::new(RwLock::new(1)),
            wallets: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            tokenomics: Some(tokenomics),
            shutdown_tx: None,
            payment_records: Arc::new(RwLock::new(HashMap::new())),
            redeemed_tokens: Arc::new(RwLock::new(HashSet::new())),
        })
    }

    /// Start block synchronization from blockchain
    /// Polls for new blocks and updates transaction confirmations
    pub async fn start_sync(&mut self) -> Result<()> {
        use tokio::time::{sleep, Duration};

        let (shutdown_tx, mut shutdown_rx) = mpsc::channel(1);
        self.shutdown_tx = Some(shutdown_tx);

        let rpc_client = Arc::clone(&self.rpc_client);
        let transactions = Arc::clone(&self.transactions);
        let current_block = Arc::clone(&self.current_block);
        let confirmation_blocks = self.config.confirmation_blocks;

        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = shutdown_rx.recv() => {
                        tracing::info!("Block sync task shutting down");
                        break;
                    }
                    _ = sleep(Duration::from_secs(2)) => {
                        // Poll for new block height
                        match rpc_client.get_current_height().await {
                            Ok(new_height) => {
                                let mut current = current_block.write().unwrap();
                                let old_height = *current;
                                
                                if new_height > old_height {
                                    *current = new_height;
                                    drop(current);
                                    
                                    // Update transaction confirmations
                                    let mut txs = transactions.write().unwrap();
                                    for tx in txs.values_mut() {
                                        if tx.block_height > 0 && tx.status == "pending" {
                                            let confirmations = new_height.saturating_sub(tx.block_height) as u32;
                                            tx.confirmations = confirmations;
                                            
                                            if confirmations >= confirmation_blocks {
                                                tx.status = "confirmed".to_string();
                                                tracing::debug!(
                                                    "Transaction {} confirmed with {} confirmations",
                                                    tx.id, confirmations
                                                );
                                            }
                                        }
                                    }
                                    
                                    tracing::debug!(
                                        "Block sync: height {} -> {} ({} new blocks)",
                                        old_height, new_height, new_height - old_height
                                    );
                                }
                            }
                            Err(e) => {
                                tracing::warn!("Failed to query block height: {}", e);
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Stop block synchronization
    pub async fn stop_sync(&mut self) {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(()).await;
        }
    }

    /// Get tokenomics manager reference
    pub fn get_tokenomics(&self) -> Option<Arc<TokenomicsManager>> {
        self.tokenomics.clone()
    }

    /// Create wallet for user
    pub fn create_wallet(&self, user_id: &UserId, initial_balance: u64) -> Result<Wallet> {
        let wallet = Wallet {
            user_id: user_id.clone(),
            balance: initial_balance,
            staked: 0,
            rewards_pending: 0,
        };
        self.wallets
            .write()
            .unwrap()
            .insert(user_id.clone(), wallet.clone());
        Ok(wallet)
    }

    /// Get wallet balance
    pub fn get_balance(&self, user_id: &UserId) -> Result<u64> {
        let wallets = self.wallets.read().unwrap();
        Ok(wallets.get(user_id).map(|w| w.balance).unwrap_or(0))
    }

    /// Transfer tokens between users
    pub fn transfer(&self, from: &UserId, to: &UserId, amount: u64) -> Result<Uuid> {
        let mut wallets = self.wallets.write().unwrap();

        let from_wallet = wallets
            .get_mut(from)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", from)))?;

        if from_wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                from_wallet.balance, amount
            )));
        }

        // Calculate transaction fee burn (1% default)
        let burn_amount = if let Some(ref tokenomics) = self.tokenomics {
            (amount * tokenomics.get_statistics().burn_rate_bps as u64) / 10000
        } else {
            0
        };

        let net_amount = amount - burn_amount;

        from_wallet.balance -= amount;

        let to_wallet = wallets.entry(to.clone()).or_insert_with(|| Wallet {
            user_id: to.clone(),
            balance: 0,
            staked: 0,
            rewards_pending: 0,
        });

        to_wallet.balance += net_amount;

        // Burn transaction fee
        if burn_amount > 0 {
            if let Some(ref tokenomics) = self.tokenomics {
                let _ =
                    tokenomics.burn_tokens(burn_amount, BurnReason::TransactionFee, from.clone());
            }
        }

        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "payment".to_string(),
            from: from.clone(),
            to: Some(to.clone()),
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Stake tokens for rewards
    pub fn stake(&self, user_id: &UserId, amount: u64, lock_duration_seconds: i64) -> Result<Uuid> {
        let mut wallets = self.wallets.write().unwrap();

        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", user_id)))?;

        if wallet.balance < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient balance: have {}, need {}",
                wallet.balance, amount
            )));
        }

        wallet.balance -= amount;
        wallet.staked += amount;

        let locked_until = Utc::now().timestamp() + lock_duration_seconds;
        let stake_position = StakePosition {
            user_id: user_id.clone(),
            amount,
            locked_until,
            rewards_earned: 0,
        };

        self.stakes
            .write()
            .unwrap()
            .insert(user_id.clone(), stake_position);

        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "stake".to_string(),
            from: user_id.clone(),
            to: None,
            amount,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Claim rewards
    pub fn claim_rewards(&self, user_id: &UserId) -> Result<Uuid> {
        let mut wallets = self.wallets.write().unwrap();
        let wallet = wallets
            .get_mut(user_id)
            .ok_or_else(|| Error::NotFound(format!("User not found: {}", user_id)))?;

        let rewards = wallet.rewards_pending;
        wallet.balance += rewards;
        wallet.rewards_pending = 0;

        let tx = CurrencyTransaction {
            id: Uuid::new_v4(),
            tx_type: "reward".to_string(),
            from: user_id.clone(),
            to: None,
            amount: rewards,
            status: "pending".to_string(),
            confirmations: 0,
            block_height: 0,
            created_at: Utc::now().timestamp(),
        };

        let tx_id = tx.id;
        self.transactions.write().unwrap().insert(tx_id, tx);

        Ok(tx_id)
    }

    /// Get transaction by ID
    pub fn get_transaction(&self, tx_id: &Uuid) -> Result<Option<CurrencyTransaction>> {
        Ok(self.transactions.read().unwrap().get(tx_id).cloned())
    }

    /// Get current block height
    pub fn get_current_block(&self) -> u64 {
        *self.current_block.read().unwrap()
    }

    /// Get all transactions for a user
    pub fn get_user_transactions(&self, user_id: &UserId) -> Result<Vec<CurrencyTransaction>> {
        let txs = self.transactions.read().unwrap();
        Ok(txs
            .values()
            .filter(|tx| tx.from == *user_id)
            .cloned()
            .collect())
    }

    /// Get wallet info
    pub fn get_wallet(&self, user_id: &UserId) -> Result<Option<Wallet>> {
        Ok(self.wallets.read().unwrap().get(user_id).cloned())
    }

    /// Get current block height from blockchain
    pub async fn get_current_height(&self) -> Result<u64> {
        self.rpc_client.get_current_height().await
    }
    
    /// Record a confirmed payment transaction (for blind token verification)
    pub fn record_payment(&self, tx_hash: &str, amount: u64, recipient: &str) {
        self.payment_records
            .write()
            .unwrap()
            .insert(tx_hash.to_string(), (amount, recipient.to_string(), true));
    }
}

/// Implementation of dchat-privacy's CurrencyChainClient trait
/// Provides payment verification and token redemption tracking for blind tokens
impl PrivacyCurrencyChainClient for CurrencyChainClient {
    /// Verify payment transaction on currency chain
    fn verify_payment_transaction(
        &self,
        tx_hash: &str,
        expected_amount: u64,
        expected_recipient: &str,
    ) -> Result<bool> {
        let records = self.payment_records.read().unwrap();
        
        if let Some((amount, recipient, confirmed)) = records.get(tx_hash) {
            if *confirmed && *amount >= expected_amount && recipient == expected_recipient {
                return Ok(true);
            }
        }
        
        // Also check transaction cache for pending/confirmed transfers
        for tx in self.transactions.read().unwrap().values() {
            if tx.tx_type == "payment" && tx.status == "confirmed" {
                // Match by ID or check amount
                if tx.amount >= expected_amount {
                    if let Some(ref to) = tx.to {
                        if to.to_string() == expected_recipient {
                            return Ok(true);
                        }
                    }
                }
            }
        }
        
        Ok(false)
    }
    
    /// Check if token signature has been redeemed
    fn is_token_redeemed(&self, signature_hash: &[u8; 32]) -> Result<bool> {
        Ok(self.redeemed_tokens.read().unwrap().contains(signature_hash))
    }
    
    /// Mark token as redeemed on-chain
    fn mark_token_redeemed(&mut self, signature_hash: [u8; 32]) -> Result<()> {
        self.redeemed_tokens.write().unwrap().insert(signature_hash);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_wallet() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let user_id = UserId(Uuid::new_v4());
        let wallet = client.create_wallet(&user_id, 1000).unwrap();
        assert_eq!(wallet.balance, 1000);
    }

    #[test]
    fn test_transfer() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let alice = UserId(Uuid::new_v4());
        let bob = UserId(Uuid::new_v4());

        client.create_wallet(&alice, 1000).unwrap();
        client.create_wallet(&bob, 0).unwrap();

        let tx_id = client.transfer(&alice, &bob, 100).unwrap();
        let tx = client.get_transaction(&tx_id).unwrap();
        assert!(tx.is_some());

        assert_eq!(client.get_balance(&alice).unwrap(), 900);
        assert_eq!(client.get_balance(&bob).unwrap(), 100);
    }

    #[test]
    fn test_stake() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        let user_id = UserId(Uuid::new_v4());

        client.create_wallet(&user_id, 1000).unwrap();
        let tx_id = client.stake(&user_id, 500, 86400).unwrap();
        let tx = client.get_transaction(&tx_id).unwrap();
        assert!(tx.is_some());

        let wallet = client.get_wallet(&user_id).unwrap().unwrap();
        assert_eq!(wallet.balance, 500);
        assert_eq!(wallet.staked, 500);
    }

    #[tokio::test]
    async fn test_block_sync() {
        let mut client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        
        // Start sync task
        client.start_sync().await.unwrap();
        
        // Give it time to poll once
        tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        
        // Stop sync
        client.stop_sync().await;
    }

    #[test]
    fn test_privacy_payment_verification() {
        let client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        
        // Record a payment
        client.record_payment("tx_abc123", 1000, "recipient_wallet_1");
        
        // Verify payment via privacy trait
        let verified = client.verify_payment_transaction(
            "tx_abc123",
            1000,
            "recipient_wallet_1"
        ).unwrap();
        assert!(verified);
        
        // Wrong amount should fail
        let verified = client.verify_payment_transaction(
            "tx_abc123",
            2000, // more than recorded
            "recipient_wallet_1"
        ).unwrap();
        assert!(!verified);
        
        // Wrong recipient should fail
        let verified = client.verify_payment_transaction(
            "tx_abc123",
            1000,
            "wrong_recipient"
        ).unwrap();
        assert!(!verified);
    }

    #[test]
    fn test_privacy_token_redemption_tracking() {
        let mut client = CurrencyChainClient::new_mock(CurrencyChainConfig::default());
        
        let token_hash: [u8; 32] = [0xCD; 32];
        
        // Initially not redeemed
        assert!(!client.is_token_redeemed(&token_hash).unwrap());
        
        // Mark as redeemed
        client.mark_token_redeemed(token_hash).unwrap();
        
        // Now should be redeemed
        assert!(client.is_token_redeemed(&token_hash).unwrap());
    }
}
