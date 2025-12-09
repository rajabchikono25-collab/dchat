//! Chain bootstrap system
//!
//! Brings chat chain, currency chain, and bridge online when first validator stakes and starts

use dchat_core::error::{Error, Result};
use ed25519_dalek::SigningKey;
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use tokio::sync::{mpsc, RwLock};
use tracing::{error, info, warn};

use super::genesis::{GenesisCoordinator, ChatGenesisBlock, CurrencyGenesisBlock};

/// Bootstrap status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BootstrapStatus {
    /// Not started
    NotStarted,
    /// Waiting for first validator
    WaitingForFirstValidator,
    /// First validator staked
    FirstValidatorStaked,
    /// Creating genesis blocks
    CreatingGenesis,
    /// Genesis blocks created
    GenesisCreated,
    /// Submitting genesis to chains
    SubmittingGenesis,
    /// Chat chain online
    ChatChainOnline,
    /// Currency chain online
    CurrencyChainOnline,
    /// Bridge initializing
    BridgeInitializing,
    /// Bridge online
    BridgeOnline,
    /// Fully operational
    FullyOperational,
    /// Bootstrap failed
    Failed(String),
}

/// Bootstrap event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BootstrapEvent {
    FirstValidatorRegistered(String),
    FirstValidatorStaked(u64),
    ChatGenesisCreated(String),
    CurrencyGenesisCreated(String),
    ChatChainStarted,
    CurrencyChainStarted,
    BridgeStarted,
    BootstrapComplete,
    BootstrapFailed(String),
}

/// Bootstrap coordinator
pub struct BootstrapCoordinator {
    status: Arc<RwLock<BootstrapStatus>>,
    event_tx: mpsc::UnboundedSender<BootstrapEvent>,
    chat_rpc: String,
    currency_rpc: String,
    first_validator_key: Option<SigningKey>,
}

impl BootstrapCoordinator {
    /// Create a new bootstrap coordinator
    pub fn new(chat_rpc: String, currency_rpc: String) -> (Self, mpsc::UnboundedReceiver<BootstrapEvent>) {
        let (event_tx, event_rx) = mpsc::unbounded_channel();

        let coordinator = Self {
            status: Arc::new(RwLock::new(BootstrapStatus::NotStarted)),
            event_tx,
            chat_rpc,
            currency_rpc,
            first_validator_key: None,
        };

        (coordinator, event_rx)
    }

    /// Get current bootstrap status
    pub async fn get_status(&self) -> BootstrapStatus {
        self.status.read().await.clone()
    }

    /// Wait for first validator to stake and start chains
    pub async fn wait_for_first_validator(&mut self) -> Result<()> {
        info!("⏳ Waiting for first validator to stake and start chains...");

        self.update_status(BootstrapStatus::WaitingForFirstValidator).await;

        // Poll currency chain for first validator stake
        use reqwest::Client as HttpClient;
        use serde_json::json;
        
        let client = HttpClient::new();
        let mut poll_count = 0;
        const MAX_POLL_ATTEMPTS: u32 = 300; // 5 minutes at 1 second intervals
        const POLL_INTERVAL_MS: u64 = 1000;
        
        loop {
            poll_count += 1;
            if poll_count > MAX_POLL_ATTEMPTS {
                return Err(Error::chain("Timeout waiting for first validator stake".to_string()));
            }
            
            // Query currency chain for validator registrations
            let payload = json!({
                "method": "currency.get_validator_count",
                "params": {},
                "jsonrpc": "2.0",
                "id": poll_count,
            });
            
            match client
                .post(&self.currency_rpc)
                .json(&payload)
                .timeout(std::time::Duration::from_secs(5))
                .send()
                .await
            {
                Ok(response) => {
                    if let Ok(body) = response.json::<serde_json::Value>().await {
                        if let Some(count) = body["result"]["count"].as_u64() {
                            if count > 0 {
                                info!("✅ First validator detected on currency chain");
                                break;
                            }
                        }
                    }
                }
                Err(e) => {
                    warn!("Failed to poll currency chain (attempt {}): {}", poll_count, e);
                }
            }
            
            tokio::time::sleep(tokio::time::Duration::from_millis(POLL_INTERVAL_MS)).await;
        }

        Ok(())
    }

    /// Register first validator and initialize chains
    pub async fn register_first_validator(&mut self, validator_key: SigningKey, stake_amount: u64) -> Result<()> {
        let verifying_key = validator_key.verifying_key();
        let validator_key_hex = hex::encode(verifying_key.as_bytes());

        info!("🎉 First validator registered!");
        info!("   Public key: {}", validator_key_hex);
        info!("   Stake amount: {} tokens", stake_amount);

        self.first_validator_key = Some(validator_key);

        self.send_event(BootstrapEvent::FirstValidatorRegistered(validator_key_hex));
        self.send_event(BootstrapEvent::FirstValidatorStaked(stake_amount));

        self.update_status(BootstrapStatus::FirstValidatorStaked).await;

        Ok(())
    }

    /// Initialize chains with genesis blocks
    pub async fn initialize_chains(&mut self) -> Result<(ChatGenesisBlock, CurrencyGenesisBlock)> {
        info!("🚀 Initializing chains with genesis blocks...");

        self.update_status(BootstrapStatus::CreatingGenesis).await;

        let validator_key = self.first_validator_key.take().ok_or_else(|| {
            Error::chain("First validator key not set".to_string())
        })?;

        // Create and submit genesis blocks
        let (chat_genesis, currency_genesis) = GenesisCoordinator::initialize_chains(
            validator_key.clone(),
            &self.chat_rpc,
            &self.currency_rpc,
        )
        .await?;

        self.first_validator_key = Some(validator_key);

        self.send_event(BootstrapEvent::ChatGenesisCreated(chat_genesis.hash.clone()));
        self.send_event(BootstrapEvent::CurrencyGenesisCreated(currency_genesis.hash.clone()));

        self.update_status(BootstrapStatus::GenesisCreated).await;

        Ok((chat_genesis, currency_genesis))
    }

    /// Start chat chain
    pub async fn start_chat_chain(&self) -> Result<()> {
        info!("🔗 Starting chat chain...");

        self.update_status(BootstrapStatus::SubmittingGenesis).await;

        // In production, this would start the chat chain consensus
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "chat.start_chain",
            "params": {},
            "jsonrpc": "2.0",
            "id": 1,
        });

        let client = HttpClient::new();
        let response = client
            .post(&self.chat_rpc)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to start chat chain: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Chat chain start failed: status {}",
                response.status()
            )));
        }

        self.send_event(BootstrapEvent::ChatChainStarted);
        self.update_status(BootstrapStatus::ChatChainOnline).await;

        info!("✅ Chat chain is online!");

        Ok(())
    }

    /// Start currency chain
    pub async fn start_currency_chain(&self) -> Result<()> {
        info!("💰 Starting currency chain...");

        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "currency.start_chain",
            "params": {},
            "jsonrpc": "2.0",
            "id": 1,
        });

        let client = HttpClient::new();
        let response = client
            .post(&self.currency_rpc)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to start currency chain: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Currency chain start failed: status {}",
                response.status()
            )));
        }

        self.send_event(BootstrapEvent::CurrencyChainStarted);
        self.update_status(BootstrapStatus::CurrencyChainOnline).await;

        info!("✅ Currency chain is online!");

        Ok(())
    }

    /// Initialize and start bridge
    pub async fn start_bridge(&self) -> Result<()> {
        info!("🌉 Initializing cross-chain bridge...");

        self.update_status(BootstrapStatus::BridgeInitializing).await;

        // In production, this would initialize the bridge with both chain states
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "bridge.initialize",
            "params": {
                "chat_rpc": self.chat_rpc,
                "currency_rpc": self.currency_rpc,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        // Bridge RPC endpoint (separate service or embedded)
        let bridge_rpc = std::env::var("BRIDGE_RPC")
            .unwrap_or_else(|_| "http://localhost:9000".to_string());

        let client = HttpClient::new();
        let response = client
            .post(&bridge_rpc)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to start bridge: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Bridge start failed: status {}",
                response.status()
            )));
        }

        self.send_event(BootstrapEvent::BridgeStarted);
        self.update_status(BootstrapStatus::BridgeOnline).await;

        info!("✅ Bridge is online!");

        Ok(())
    }

    /// Complete bootstrap sequence
    pub async fn complete_bootstrap(&self) -> Result<()> {
        self.send_event(BootstrapEvent::BootstrapComplete);
        self.update_status(BootstrapStatus::FullyOperational).await;

        info!("🎊 MAINNET BOOTSTRAP COMPLETE!");
        info!("   ✅ Chat chain operational");
        info!("   ✅ Currency chain operational");
        info!("   ✅ Bridge operational");
        info!("   ✅ First validator active");
        info!("");
        info!("🚀 dchat mainnet is now LIVE!");

        Ok(())
    }

    /// Execute full bootstrap sequence
    pub async fn execute_full_bootstrap(&mut self, validator_key: SigningKey, stake_amount: u64) -> Result<()> {
        info!("═══════════════════════════════════════");
        info!("     MAINNET BOOTSTRAP SEQUENCE");
        info!("═══════════════════════════════════════");

        // Step 1: Register first validator
        self.register_first_validator(validator_key, stake_amount).await?;

        // Step 2: Create genesis blocks
        let (_chat_genesis, _currency_genesis) = self.initialize_chains().await?;

        // Step 3: Start chat chain
        self.start_chat_chain().await?;

        // Step 4: Start currency chain
        self.start_currency_chain().await?;

        // Step 5: Start bridge
        self.start_bridge().await?;

        // Step 6: Complete
        self.complete_bootstrap().await?;

        Ok(())
    }

    /// Handle bootstrap failure
    #[allow(dead_code)]
    async fn handle_failure(&self, error: &str) {
        error!("❌ Bootstrap failed: {}", error);
        self.send_event(BootstrapEvent::BootstrapFailed(error.to_string()));
        self.update_status(BootstrapStatus::Failed(error.to_string())).await;
    }

    /// Update status
    async fn update_status(&self, status: BootstrapStatus) {
        let mut current_status = self.status.write().await;
        *current_status = status;
    }

    /// Send event
    fn send_event(&self, event: BootstrapEvent) {
        let _ = self.event_tx.send(event);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[tokio::test]
    async fn test_bootstrap_status() {
        let chat_rpc = "http://localhost:8080".to_string();
        let currency_rpc = "http://localhost:8081".to_string();

        let (coordinator, mut event_rx) = BootstrapCoordinator::new(chat_rpc, currency_rpc);

        let status = coordinator.get_status().await;
        assert_eq!(status, BootstrapStatus::NotStarted);
    }

    #[tokio::test]
    async fn test_first_validator_registration() {
        let chat_rpc = "http://localhost:8080".to_string();
        let currency_rpc = "http://localhost:8081".to_string();

        let (mut coordinator, mut event_rx) = BootstrapCoordinator::new(chat_rpc, currency_rpc);

        let signing_key = SigningKey::generate(&mut OsRng);
        let stake_amount = dchat_core::config::constants::MIN_VALIDATOR_STAKE;

        let result = coordinator.register_first_validator(signing_key, stake_amount).await;
        assert!(result.is_ok());

        let status = coordinator.get_status().await;
        assert_eq!(status, BootstrapStatus::FirstValidatorStaked);

        // Check events
        let event1 = event_rx.try_recv();
        assert!(matches!(event1, Ok(BootstrapEvent::FirstValidatorRegistered(_))));

        let event2 = event_rx.try_recv();
        assert!(matches!(event2, Ok(BootstrapEvent::FirstValidatorStaked(_))));
    }
}
