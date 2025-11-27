//! Currency Chain Block Synchronization
//!
//! Real-time block streaming from the currency chain for:
//! - Reward tracking and distribution
//! - Slashing event monitoring
//! - Cross-chain state reconciliation
//! - Fork detection and resolution

use dchat_chain::Transaction;
use dchat_core::error::Result;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use tokio::sync::{RwLock, broadcast};
use tokio::time::{interval, Duration};
use tracing::{debug, error, info, warn};

/// Currency chain block header
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyBlockHeader {
    /// Block number/height
    pub block_number: u64,
    /// Block hash
    pub block_hash: String,
    /// Parent block hash
    pub parent_hash: String,
    /// Block timestamp (Unix timestamp)
    pub timestamp: u64,
    /// State root hash
    pub state_root: String,
    /// Transaction root hash
    pub transactions_root: String,
    /// Validator who proposed this block
    pub proposer: String,
}

/// Full currency chain block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyBlock {
    /// Block header
    pub header: CurrencyBlockHeader,
    /// List of transactions in this block
    pub transactions: Vec<Transaction>,
    /// Number of confirmations
    pub confirmations: u32,
}

/// Fork information for resolution
#[derive(Debug, Clone)]
pub struct ForkInfo {
    /// Common ancestor block number
    pub common_ancestor: u64,
    /// Competing chain A
    pub chain_a: Vec<CurrencyBlockHeader>,
    /// Competing chain B
    pub chain_b: Vec<CurrencyBlockHeader>,
    /// Detected at timestamp
    pub detected_at: std::time::SystemTime,
}

/// Block sync status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SyncStatus {
    /// Not yet started
    Idle,
    /// Initial sync in progress
    Syncing,
    /// Caught up, receiving real-time updates
    Live,
    /// Disconnected, attempting reconnection
    Reconnecting,
    /// Fork detected, resolving
    ResolvingFork,
}

/// Configuration for block synchronization
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlockSyncConfig {
    /// Currency chain WebSocket endpoint
    pub ws_url: String,
    /// Currency chain RPC endpoint (fallback)
    pub rpc_url: String,
    /// Maximum blocks to cache
    pub max_cache_size: usize,
    /// Confirmation threshold (number of blocks)
    pub confirmation_threshold: u32,
    /// Reconnection delay (milliseconds)
    pub reconnect_delay_ms: u64,
    /// Maximum reconnection attempts
    pub max_reconnect_attempts: u32,
    /// Fork resolution timeout (seconds)
    pub fork_resolution_timeout_secs: u64,
}

impl Default for BlockSyncConfig {
    fn default() -> Self {
        Self {
            ws_url: "ws://localhost:8546".to_string(),
            rpc_url: "http://localhost:8545".to_string(),
            max_cache_size: 1000,
            confirmation_threshold: 6,
            reconnect_delay_ms: 5000,
            max_reconnect_attempts: 10,
            fork_resolution_timeout_secs: 300, // 5 minutes
        }
    }
}

/// Block synchronization manager
pub struct BlockSyncManager {
    /// Configuration
    config: BlockSyncConfig,
    /// Current sync status
    status: Arc<RwLock<SyncStatus>>,
    /// Cached blocks (block_number -> block)
    block_cache: Arc<RwLock<HashMap<u64, CurrencyBlock>>>,
    /// Latest confirmed block number
    latest_confirmed_block: Arc<RwLock<u64>>,
    /// Pending blocks (not yet confirmed)
    pending_blocks: Arc<RwLock<VecDeque<CurrencyBlock>>>,
    /// Active fork information
    active_fork: Arc<RwLock<Option<ForkInfo>>>,
    /// Reconnection attempt counter
    reconnect_attempts: Arc<RwLock<u32>>,
    /// Block confirmation event broadcaster
    block_tx: broadcast::Sender<CurrencyBlock>,
}

impl BlockSyncManager {
    /// Create new block sync manager
    pub fn new(config: BlockSyncConfig) -> Self {
        let (block_tx, _) = broadcast::channel(1000); // Buffer up to 1000 block events
        Self {
            config,
            status: Arc::new(RwLock::new(SyncStatus::Idle)),
            block_cache: Arc::new(RwLock::new(HashMap::new())),
            latest_confirmed_block: Arc::new(RwLock::new(0)),
            pending_blocks: Arc::new(RwLock::new(VecDeque::new())),
            active_fork: Arc::new(RwLock::new(None)),
            reconnect_attempts: Arc::new(RwLock::new(0)),
            block_tx,
        }
    }

    /// Subscribe to confirmed block events
    pub fn subscribe(&self) -> broadcast::Receiver<CurrencyBlock> {
        self.block_tx.subscribe()
    }

    /// Start block synchronization
    pub async fn start(&self) -> Result<()> {
        info!("🔄 Starting currency chain block synchronization");
        *self.status.write().await = SyncStatus::Syncing;

        // Initial sync from RPC
        self.initial_sync().await?;

        // Start WebSocket streaming
        self.start_websocket_streaming().await?;

        // Start background tasks
        self.spawn_background_tasks().await;

        info!("✅ Block synchronization started successfully");
        Ok(())
    }

    /// Perform initial sync from RPC endpoint
    async fn initial_sync(&self) -> Result<()> {
        info!("📥 Starting initial block sync from RPC");

        // Get latest block number from RPC
        let latest_block = self.fetch_latest_block_number_rpc().await?;
        info!("📊 Latest currency chain block: {}", latest_block);

        // Sync recent blocks (last 1000 or configured max)
        let start_block = latest_block.saturating_sub(self.config.max_cache_size as u64);
        
        for block_num in start_block..=latest_block {
            match self.fetch_block_rpc(block_num).await {
                Ok(block) => {
                    self.add_block_to_cache(block).await?;
                }
                Err(e) => {
                    warn!("⚠️ Failed to fetch block {}: {}", block_num, e);
                    // Continue with next block
                }
            }

            // Log progress every 100 blocks
            if block_num % 100 == 0 {
                debug!("Synced up to block {}/{}", block_num, latest_block);
            }
        }

        *self.latest_confirmed_block.write().await = latest_block;
        info!("✅ Initial sync complete: {} blocks cached", self.block_cache.read().await.len());

        Ok(())
    }

    /// Start WebSocket streaming for real-time updates
    async fn start_websocket_streaming(&self) -> Result<()> {
        
        use tokio_tungstenite::{connect_async, tungstenite::protocol::Message};
        use futures_util::{StreamExt, SinkExt};
        
        info!("🔌 Connecting to currency chain WebSocket: {}", self.config.ws_url);

        let sync_manager = self.clone_refs();
        
        tokio::spawn(async move {
            loop {
                match connect_async(&sync_manager.config.ws_url).await {
                    Ok((ws_stream, _)) => {
                        info!("✅ WebSocket connected");
                        *sync_manager.status.write().await = SyncStatus::Live;
                        *sync_manager.reconnect_attempts.write().await = 0;

                        let (mut write, mut read) = ws_stream.split();

                        // Subscribe to new block headers
                        let subscribe_msg = serde_json::json!({
                            "jsonrpc": "2.0",
                            "id": 1,
                            "method": "eth_subscribe",
                            "params": ["newHeads"]
                        });

                        if let Err(e) = write.send(Message::Text(subscribe_msg.to_string())).await {
                            error!("Failed to send subscription: {}", e);
                            sync_manager.handle_connection_failure().await;
                            continue;
                        }

                        // Process incoming messages
                        while let Some(msg) = read.next().await {
                            match msg {
                                Ok(Message::Text(text)) => {
                                    if let Err(e) = sync_manager.handle_ws_message(&text).await {
                                        warn!("Failed to process WebSocket message: {}", e);
                                    }
                                }
                                Ok(Message::Ping(data)) => {
                                    if let Err(e) = write.send(Message::Pong(data)).await {
                                        error!("Failed to send pong: {}", e);
                                        break;
                                    }
                                }
                                Ok(Message::Close(_)) => {
                                    warn!("WebSocket closed by remote");
                                    break;
                                }
                                Err(e) => {
                                    error!("WebSocket error: {}", e);
                                    break;
                                }
                                _ => {}
                            }
                        }

                        info!("WebSocket connection closed, reconnecting...");
                        sync_manager.handle_connection_failure().await;
                    }
                    Err(e) => {
                        error!("Failed to connect WebSocket: {}", e);
                        sync_manager.handle_connection_failure().await;
                        
                        let delay = sync_manager.config.reconnect_delay_ms;
                        tokio::time::sleep(Duration::from_millis(delay)).await;
                    }
                }
            }
        });

        Ok(())
    }

    /// Handle incoming WebSocket message
    async fn handle_ws_message(&self, text: &str) -> Result<()> {
        use dchat_core::error::Error;
        
        let json: serde_json::Value = serde_json::from_str(text)
            .map_err(|e| Error::chain(format!("Failed to parse WebSocket message: {}", e)))?;

        // Check if it's a subscription notification
        if let Some(params) = json.get("params") {
            if let Some(result) = params.get("result") {
                // Parse block header from notification
                let block_number_hex = result["number"]
                    .as_str()
                    .ok_or_else(|| Error::chain("Missing block number in WebSocket notification"))?;
                
                let block_number_hex = block_number_hex.trim_start_matches("0x");
                let block_number = u64::from_str_radix(block_number_hex, 16)
                    .map_err(|e| Error::chain(format!("Failed to parse block number: {}", e)))?;

                // Fetch full block via RPC
                match self.fetch_block_rpc(block_number).await {
                    Ok(block) => {
                        self.process_new_block(block).await?;
                    }
                    Err(e) => {
                        warn!("Failed to fetch block {} from RPC: {}", block_number, e);
                    }
                }
            }
        }

        Ok(())
    }

    /// Spawn background tasks for block processing
    async fn spawn_background_tasks(&self) {
        let sync_manager = self.clone_refs();

        // Task 1: Block polling (if WebSocket fails)
        tokio::spawn(async move {
            sync_manager.block_polling_task().await;
        });

        let sync_manager = self.clone_refs();

        // Task 2: Fork detection
        tokio::spawn(async move {
            sync_manager.fork_detection_task().await;
        });

        let sync_manager = self.clone_refs();

        // Task 3: Block confirmation
        tokio::spawn(async move {
            sync_manager.block_confirmation_task().await;
        });
    }

    /// Block polling task (fallback when WebSocket unavailable)
    async fn block_polling_task(&self) {
        let mut poll_interval = interval(Duration::from_secs(2));

        loop {
            poll_interval.tick().await;

            let status = *self.status.read().await;
            if status != SyncStatus::Live && status != SyncStatus::Syncing {
                continue;
            }

            match self.fetch_latest_block_number_rpc().await {
                Ok(latest_block) => {
                    let current_latest = *self.latest_confirmed_block.read().await;
                    
                    if latest_block > current_latest {
                        // Fetch new blocks
                        for block_num in (current_latest + 1)..=latest_block {
                            match self.fetch_block_rpc(block_num).await {
                                Ok(block) => {
                                    if let Err(e) = self.process_new_block(block).await {
                                        error!("Failed to process block {}: {}", block_num, e);
                                    }
                                }
                                Err(e) => {
                                    warn!("Failed to fetch block {}: {}", block_num, e);
                                }
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to fetch latest block number: {}", e);
                    self.handle_connection_failure().await;
                }
            }
        }
    }

    /// Fork detection task
    async fn fork_detection_task(&self) {
        let mut check_interval = interval(Duration::from_secs(10));

        loop {
            check_interval.tick().await;

            if let Err(e) = self.check_for_forks().await {
                error!("Fork detection error: {}", e);
            }
        }
    }

    /// Block confirmation task
    async fn block_confirmation_task(&self) {
        let mut confirm_interval = interval(Duration::from_secs(5));

        loop {
            confirm_interval.tick().await;

            if let Err(e) = self.confirm_pending_blocks().await {
                error!("Block confirmation error: {}", e);
            }
        }
    }

    /// Process newly received block
    pub async fn process_new_block(&self, block: CurrencyBlock) -> Result<()> {
        debug!("📦 Processing new block #{}", block.header.block_number);

        // Check for fork
        if self.is_fork(&block).await? {
            warn!("🍴 Fork detected at block {}", block.header.block_number);
            self.handle_fork(block.clone()).await?;
        }

        // Add to pending blocks
        let mut pending = self.pending_blocks.write().await;
        pending.push_back(block.clone());

        // Add to cache
        self.add_block_to_cache(block).await?;

        Ok(())
    }

    /// Check if block represents a fork
    async fn is_fork(&self, block: &CurrencyBlock) -> Result<bool> {
        let cache = self.block_cache.read().await;
        
        // Check if parent block exists and matches
        if let Some(parent) = cache.get(&(block.header.block_number - 1)) {
            Ok(parent.header.block_hash != block.header.parent_hash)
        } else {
            // Parent not in cache, assume no fork
            Ok(false)
        }
    }

    /// Handle fork detection
    async fn handle_fork(&self, conflicting_block: CurrencyBlock) -> Result<()> {
        warn!("🍴 Handling fork at block {}", conflicting_block.header.block_number);
        
        *self.status.write().await = SyncStatus::ResolvingFork;

        // Find common ancestor
        let common_ancestor = self.find_common_ancestor(&conflicting_block).await?;
        
        // Fetch competing chain blocks from common ancestor to conflicting block
        let chain_b_headers = self.fetch_competing_chain_headers(
            common_ancestor,
            conflicting_block.header.parent_hash.clone()
        ).await.unwrap_or_default();
        
        // Build fork info
        let fork_info = ForkInfo {
            common_ancestor,
            chain_a: vec![conflicting_block.header.clone()],
            chain_b: chain_b_headers,
            detected_at: std::time::SystemTime::now(),
        };

        *self.active_fork.write().await = Some(fork_info);

        // Resolve fork (choose longest chain)
        self.resolve_fork().await?;

        Ok(())
    }
    
    /// Fetch competing chain headers by walking back from parent hash
    async fn fetch_competing_chain_headers(
        &self,
        common_ancestor: u64,
        start_parent_hash: String,
    ) -> Result<Vec<CurrencyBlockHeader>> {
        let cache = self.block_cache.read().await;
        let mut headers = Vec::new();
        let mut current_parent_hash = start_parent_hash;
        
        // Walk back through cached blocks to build competing chain
        for block_num in (common_ancestor + 1..=*self.latest_confirmed_block.read().await).rev() {
            if let Some(block) = cache.get(&block_num) {
                if block.header.block_hash == current_parent_hash {
                    headers.push(block.header.clone());
                    current_parent_hash = block.header.parent_hash.clone();
                } else if block.header.parent_hash == current_parent_hash {
                    // Found an alternative block at this height
                    headers.push(block.header.clone());
                    current_parent_hash = block.header.parent_hash.clone();
                }
            }
        }
        
        // Reverse to get chronological order
        headers.reverse();
        Ok(headers)
    }

    /// Find common ancestor block
    async fn find_common_ancestor(&self, block: &CurrencyBlock) -> Result<u64> {
        let mut current_num = block.header.block_number - 1;
        let cache = self.block_cache.read().await;

        while current_num > 0 {
            if cache.contains_key(&current_num) {
                return Ok(current_num);
            }
            current_num -= 1;
        }

        Ok(0) // Genesis
    }

    /// Resolve fork by choosing canonical chain
    async fn resolve_fork(&self) -> Result<()> {
        info!("🔧 Resolving fork");

        let fork_opt = self.active_fork.read().await.clone();
        
        if let Some(fork) = fork_opt {
            // Calculate total difficulty for each chain (using length as proxy in PoS context)
            // In production, this would use actual difficulty or stake weight
            let chain_a_weight = fork.chain_a.len();
            let chain_b_weight = fork.chain_b.len();
            
            if chain_a_weight >= chain_b_weight {
                info!("✅ Chain A selected as canonical (weight: {} vs {})", chain_a_weight, chain_b_weight);
                // Chain A is already in our cache, no reorg needed
            } else {
                info!("✅ Chain B selected as canonical (weight: {} vs {})", chain_b_weight, chain_a_weight);
                // Reorganize cache: remove chain_a blocks and apply chain_b blocks
                self.reorganize_to_chain(&fork.chain_b, fork.common_ancestor).await?;
            }

            // Clear fork state
            *self.active_fork.write().await = None;
            
            // Emit fork resolution event for monitoring
            info!(
                "🔗 Fork resolved: common_ancestor={}, chain_a_len={}, chain_b_len={}",
                fork.common_ancestor,
                fork.chain_a.len(),
                fork.chain_b.len()
            );
        }

        *self.status.write().await = SyncStatus::Live;
        Ok(())
    }
    
    /// Reorganize chain to apply winning fork
    async fn reorganize_to_chain(
        &self,
        canonical_headers: &[CurrencyBlockHeader],
        common_ancestor: u64,
    ) -> Result<()> {
        let mut cache = self.block_cache.write().await;
        
        // Remove blocks after common ancestor (orphaned blocks)
        let latest = *self.latest_confirmed_block.read().await;
        for block_num in (common_ancestor + 1)..=latest {
            if let Some(removed) = cache.remove(&block_num) {
                info!("🗑️ Removed orphaned block {} ({})", block_num, removed.header.block_hash);
            }
        }
        drop(cache);
        
        // Fetch and apply canonical chain blocks
        for header in canonical_headers {
            match self.fetch_block_rpc(header.block_number).await {
                Ok(block) => {
                    let mut cache = self.block_cache.write().await;
                    cache.insert(block.header.block_number, block.clone());
                    info!("✅ Applied canonical block {} ({})", 
                          block.header.block_number, 
                          block.header.block_hash);
                }
                Err(e) => {
                    warn!("⚠️ Failed to fetch canonical block {}: {}", header.block_number, e);
                }
            }
        }
        
        // Update latest confirmed block to end of canonical chain
        if let Some(last_header) = canonical_headers.last() {
            *self.latest_confirmed_block.write().await = last_header.block_number;
        }
        
        Ok(())
    }

    /// Check for forks in recent blocks
    async fn check_for_forks(&self) -> Result<()> {
        // Verify last N blocks are consistent
        let latest = *self.latest_confirmed_block.read().await;
        let cache = self.block_cache.read().await;

        for i in 0..self.config.confirmation_threshold {
            let block_num = latest.saturating_sub(i as u64);
            if block_num == 0 {
                break;
            }

            if let Some(block) = cache.get(&block_num) {
                // Verify parent hash chain
                if block_num > 1 {
                    if let Some(parent) = cache.get(&(block_num - 1)) {
                        if parent.header.block_hash != block.header.parent_hash {
                            warn!("⚠️ Fork detected: parent hash mismatch at block {}", block_num);
                            return self.handle_fork(block.clone()).await;
                        }
                    }
                }
            }
        }

        Ok(())
    }

    /// Confirm pending blocks
    async fn confirm_pending_blocks(&self) -> Result<()> {
        let mut pending = self.pending_blocks.write().await;
        let latest = *self.latest_confirmed_block.read().await;

        // Confirm blocks that have enough confirmations
        while let Some(block) = pending.front() {
            let confirmations = latest.saturating_sub(block.header.block_number) as u32;
            
            if confirmations >= self.config.confirmation_threshold {
                let confirmed = pending.pop_front().unwrap();
                info!("✅ Block {} confirmed ({} confirmations)", 
                      confirmed.header.block_number, confirmations);
                
                // Emit confirmation event
                self.emit_block_confirmed(&confirmed).await;
            } else {
                break;
            }
        }

        Ok(())
    }

    /// Emit block confirmed event
    async fn emit_block_confirmed(&self, block: &CurrencyBlock) {
        // Broadcast block confirmation to all subscribers
        match self.block_tx.send(block.clone()) {
            Ok(receiver_count) => {
                debug!("Block {} confirmation event sent to {} subscribers", 
                      block.header.block_number, receiver_count);
            }
            Err(e) => {
                // No active subscribers, which is fine
                debug!("No subscribers for block {} confirmation: {}", 
                      block.header.block_number, e);
            }
        }
    }

    /// Add block to cache
    async fn add_block_to_cache(&self, block: CurrencyBlock) -> Result<()> {
        let mut cache = self.block_cache.write().await;
        
        // Remove oldest blocks if cache is full
        while cache.len() >= self.config.max_cache_size {
            if let Some(&min_key) = cache.keys().min() {
                cache.remove(&min_key);
            }
        }

        cache.insert(block.header.block_number, block);
        Ok(())
    }

    /// Handle connection failure
    async fn handle_connection_failure(&self) {
        warn!("⚠️ Connection failure detected");
        
        let mut attempts = self.reconnect_attempts.write().await;
        *attempts += 1;

        if *attempts >= self.config.max_reconnect_attempts {
            error!("❌ Maximum reconnection attempts reached");
            *self.status.write().await = SyncStatus::Idle;
            return;
        }

        *self.status.write().await = SyncStatus::Reconnecting;
        
        info!("🔄 Reconnecting (attempt {}/{})", *attempts, self.config.max_reconnect_attempts);
        
        tokio::time::sleep(Duration::from_millis(self.config.reconnect_delay_ms)).await;
    }

    /// Fetch latest block number from RPC
    async fn fetch_latest_block_number_rpc(&self) -> Result<u64> {
        use dchat_core::error::Error;
        use serde_json::json;
        
        let client = reqwest::Client::new();
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_blockNumber",
            "params": [],
            "id": 1
        });

        let response = client
            .post(&self.config.rpc_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::chain(format!("Failed to parse RPC response: {}", e)))?;

        // Extract block number from JSON-RPC response
        let block_hex = json["result"]
            .as_str()
            .ok_or_else(|| Error::chain("Missing or invalid result field"))?;
        
        // Parse hex string (with or without 0x prefix)
        let block_hex = block_hex.trim_start_matches("0x");
        let block_number = u64::from_str_radix(block_hex, 16)
            .map_err(|e| Error::chain(format!("Failed to parse block number: {}", e)))?;

        Ok(block_number)
    }

    /// Fetch block from RPC
    async fn fetch_block_rpc(&self, block_number: u64) -> Result<CurrencyBlock> {
        use dchat_core::error::Error;
        use serde_json::json;
        
        let client = reqwest::Client::new();
        
        // Convert block number to hex
        let block_param = format!("0x{:x}", block_number);
        
        let request_body = json!({
            "jsonrpc": "2.0",
            "method": "eth_getBlockByNumber",
            "params": [block_param, true], // true = include transactions
            "id": 1
        });

        let response = client
            .post(&self.config.rpc_url)
            .json(&request_body)
            .send()
            .await
            .map_err(|e| Error::network(format!("RPC request failed: {}", e)))?;

        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::chain(format!("Failed to parse RPC response: {}", e)))?;

        // Extract block data from JSON-RPC response
        let block_data = json["result"]
            .as_object()
            .ok_or_else(|| Error::chain("Missing or invalid result field"))?;

        // Parse block header
        let block_hash = block_data["hash"]
            .as_str()
            .ok_or_else(|| Error::chain("Missing block hash"))?
            .to_string();
        
        let parent_hash = block_data["parentHash"]
            .as_str()
            .ok_or_else(|| Error::chain("Missing parent hash"))?
            .to_string();
        
        let timestamp_hex = block_data["timestamp"]
            .as_str()
            .ok_or_else(|| Error::chain("Missing timestamp"))?
            .trim_start_matches("0x");
        let timestamp = u64::from_str_radix(timestamp_hex, 16)
            .map_err(|e| Error::chain(format!("Failed to parse timestamp: {}", e)))?;
        
        let state_root = block_data["stateRoot"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000")
            .to_string();
        
        let transactions_root = block_data["transactionsRoot"]
            .as_str()
            .unwrap_or("0x0000000000000000000000000000000000000000000000000000000000000000")
            .to_string();
        
        let proposer = block_data["miner"]
            .as_str()
            .unwrap_or("unknown")
            .to_string();

        // Parse transactions using dedicated parser
        use dchat_chain::CurrencyTransactionParser;
        
        // Convert Map back to Value for the parser
        let block_value = serde_json::Value::Object(block_data.clone());
        let parsed_transactions = CurrencyTransactionParser::parse_block_transactions(&block_value)
            .unwrap_or_else(|e| {
                tracing::warn!("Failed to parse block transactions: {}", e);
                Vec::new()
            });

        // Convert parsed transactions to generic Transaction objects
        let transactions: Vec<dchat_chain::Transaction> = parsed_transactions
            .into_iter()
            .map(|parsed_tx| {
                use dchat_chain::{TransactionType, TransactionStatus};
                use uuid::Uuid;
                
                // Serialize transaction data to bytes for the Transaction payload
                let payload_bytes = bincode::serialize(&format!("{:?}", parsed_tx.data))
                    .unwrap_or_default();
                
                // Create a generic transaction from parsed currency transaction
                dchat_chain::Transaction {
                    tx_id: Uuid::new_v4(),
                    tx_type: TransactionType::SendDirectMessage, // Currency txs don't map directly
                    payload: payload_bytes,
                    tx_hash: parsed_tx.tx_hash.clone(),
                    status: TransactionStatus::Confirmed { 
                        block_height: block_number, 
                        block_hash: block_hash.clone() 
                    },
                    submitted_at: chrono::Utc::now(),
                    confirmed_at: Some(chrono::Utc::now()),
                    fee_paid: 0,
                }
            })
            .collect();

        Ok(CurrencyBlock {
            header: CurrencyBlockHeader {
                block_number,
                block_hash,
                parent_hash,
                timestamp,
                state_root,
                transactions_root,
                proposer,
            },
            transactions,
            confirmations: 0,
        })
    }

    /// Get current sync status
    pub async fn get_status(&self) -> SyncStatus {
        *self.status.read().await
    }

    /// Get latest confirmed block number
    pub async fn get_latest_confirmed_block(&self) -> u64 {
        *self.latest_confirmed_block.read().await
    }

    /// Get block by number
    pub async fn get_block(&self, block_number: u64) -> Option<CurrencyBlock> {
        self.block_cache.read().await.get(&block_number).cloned()
    }

    /// Get active fork information
    pub async fn get_active_fork(&self) -> Option<ForkInfo> {
        self.active_fork.read().await.clone()
    }

    /// Clone internal references for spawned tasks
    fn clone_refs(&self) -> Self {
        Self {
            config: self.config.clone(),
            status: Arc::clone(&self.status),
            block_cache: Arc::clone(&self.block_cache),
            latest_confirmed_block: Arc::clone(&self.latest_confirmed_block),
            pending_blocks: Arc::clone(&self.pending_blocks),
            active_fork: Arc::clone(&self.active_fork),
            reconnect_attempts: Arc::clone(&self.reconnect_attempts),
            block_tx: self.block_tx.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_block_sync_manager_creation() {
        let config = BlockSyncConfig::default();
        let manager = BlockSyncManager::new(config);
        
        assert_eq!(manager.get_status().await, SyncStatus::Idle);
        assert_eq!(manager.get_latest_confirmed_block().await, 0);
    }

    #[tokio::test]
    async fn test_add_block_to_cache() {
        let config = BlockSyncConfig::default();
        let manager = BlockSyncManager::new(config);

        let block = CurrencyBlock {
            header: CurrencyBlockHeader {
                block_number: 1,
                block_hash: "0x123".to_string(),
                parent_hash: "0x000".to_string(),
                timestamp: 1234567890,
                state_root: "0xabc".to_string(),
                transactions_root: "0xdef".to_string(),
                proposer: "validator1".to_string(),
            },
            transactions: vec![],
            confirmations: 0,
        };

        manager.add_block_to_cache(block.clone()).await.unwrap();
        
        let retrieved = manager.get_block(1).await;
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().header.block_number, 1);
    }

    #[tokio::test]
    async fn test_fork_detection() {
        let config = BlockSyncConfig::default();
        let manager = BlockSyncManager::new(config);

        // Add parent block
        let parent = CurrencyBlock {
            header: CurrencyBlockHeader {
                block_number: 1,
                block_hash: "0x111".to_string(),
                parent_hash: "0x000".to_string(),
                timestamp: 1234567890,
                state_root: "0xabc".to_string(),
                transactions_root: "0xdef".to_string(),
                proposer: "validator1".to_string(),
            },
            transactions: vec![],
            confirmations: 0,
        };
        manager.add_block_to_cache(parent).await.unwrap();

        // Add conflicting block
        let conflicting = CurrencyBlock {
            header: CurrencyBlockHeader {
                block_number: 2,
                block_hash: "0x222".to_string(),
                parent_hash: "0x999".to_string(), // Wrong parent
                timestamp: 1234567900,
                state_root: "0xghi".to_string(),
                transactions_root: "0xjkl".to_string(),
                proposer: "validator2".to_string(),
            },
            transactions: vec![],
            confirmations: 0,
        };

        let is_fork = manager.is_fork(&conflicting).await.unwrap();
        assert!(is_fork);
    }
}
