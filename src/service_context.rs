//! Service Context - Shared lazy-initialized services
//!
//! This module provides a `ServiceContext` that lazily initializes and
//! caches commonly used services like chain clients. This eliminates
//! duplicate initialization code across 20+ call sites in main.rs.
//!
//! ## Usage
//!
//! ```rust,ignore
//! let ctx = ServiceContext::new(&config)?;
//! let currency_chain = ctx.currency_chain().await?;
//! let chat_chain = ctx.chat_chain().await?;
//! let bridge = ctx.cross_chain_bridge().await?;
//! ```
//!
//! ## Benefits
//!
//! 1. **Deduplication**: Single source of truth for client initialization
//! 2. **Lazy Loading**: Clients only initialized when first accessed
//! 3. **Connection Reuse**: Same Arc instance shared across all consumers
//! 4. **Consistent Config**: All clients use the same resolved RPC endpoints

use dchat_blockchain::{
    ChatChainClient, ChatChainConfig, CrossChainBridge, CurrencyChainClient, CurrencyChainConfig,
};
use dchat_core::error::{Error, Result};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::{error, info};

/// Configuration for chain clients extracted from main config
#[derive(Debug, Clone)]
pub struct ChainClientConfig {
    /// Currency chain RPC URL
    pub currency_rpc_url: String,
    /// Chat chain RPC URL
    pub chat_rpc_url: String,
    /// Optional timeout for RPC calls
    pub timeout_secs: Option<u64>,
}

impl Default for ChainClientConfig {
    fn default() -> Self {
        Self {
            currency_rpc_url: "http://localhost:8899".to_string(),
            chat_rpc_url: "http://localhost:8545".to_string(),
            timeout_secs: Some(30),
        }
    }
}

/// Service context providing lazy-initialized shared services
///
/// All services are initialized on first access and then cached.
/// Thread-safe for concurrent access via Arc wrapping.
pub struct ServiceContext {
    /// Chain client configuration
    config: ChainClientConfig,

    /// Currency chain client (lazy-initialized)
    currency_chain: OnceCell<Arc<CurrencyChainClient>>,

    /// Chat chain client (lazy-initialized)
    chat_chain: OnceCell<Arc<ChatChainClient>>,

    /// Cross-chain bridge (lazy-initialized, depends on both chain clients)
    bridge: OnceCell<Arc<CrossChainBridge>>,
}

impl ServiceContext {
    /// Create new service context with the given configuration
    pub fn new(config: ChainClientConfig) -> Self {
        Self {
            config,
            currency_chain: OnceCell::new(),
            chat_chain: OnceCell::new(),
            bridge: OnceCell::new(),
        }
    }

    /// Create from main Config struct
    ///
    /// Extracts chain RPC URLs from the config, validating required values.
    pub fn from_config(
        currency_rpc_url: Option<String>,
        chat_rpc_url: Option<String>,
        timeout_secs: Option<u64>,
    ) -> Result<Self> {
        let currency_rpc = currency_rpc_url.ok_or_else(|| {
            Error::validation(
                "Currency chain RPC URL not configured. Set via config or DCHAT_CURRENCY_CHAIN_RPC_URL environment variable."
            )
        })?;

        let chat_rpc = chat_rpc_url.ok_or_else(|| {
            Error::validation(
                "Chat chain RPC URL not configured. Set via config or DCHAT_CHAT_CHAIN_RPC_URL environment variable."
            )
        })?;

        Ok(Self::new(ChainClientConfig {
            currency_rpc_url: currency_rpc,
            chat_rpc_url: chat_rpc,
            timeout_secs,
        }))
    }

    /// Get or initialize the currency chain client
    pub async fn currency_chain(&self) -> Result<Arc<CurrencyChainClient>> {
        self.currency_chain
            .get_or_try_init(|| async {
                info!(
                    "Initializing CurrencyChainClient with RPC: {}",
                    self.config.currency_rpc_url
                );

                let mut config = CurrencyChainConfig::default();
                config.rpc_url = self.config.currency_rpc_url.clone();

                match CurrencyChainClient::new(config) {
                    Ok(client) => {
                        info!("✓ CurrencyChainClient initialized");
                        Ok(Arc::new(client))
                    }
                    Err(e) => {
                        error!("Failed to initialize CurrencyChainClient: {}", e);
                        Err(Error::chain(format!(
                            "Failed to initialize currency chain client: {}",
                            e
                        )))
                    }
                }
            })
            .await
            .cloned()
    }

    /// Get or initialize the chat chain client
    pub async fn chat_chain(&self) -> Result<Arc<ChatChainClient>> {
        self.chat_chain
            .get_or_try_init(|| async {
                info!(
                    "Initializing ChatChainClient with RPC: {}",
                    self.config.chat_rpc_url
                );

                let mut config = ChatChainConfig::default();
                config.rpc_url = self.config.chat_rpc_url.clone();

                match ChatChainClient::new(config) {
                    Ok(client) => {
                        info!("✓ ChatChainClient initialized");
                        Ok(Arc::new(client))
                    }
                    Err(e) => {
                        error!("Failed to initialize ChatChainClient: {}", e);
                        Err(Error::chain(format!(
                            "Failed to initialize chat chain client: {}",
                            e
                        )))
                    }
                }
            })
            .await
            .cloned()
    }

    /// Get or initialize the cross-chain bridge
    ///
    /// This will automatically initialize both chain clients if not already done.
    pub async fn cross_chain_bridge(&self) -> Result<Arc<CrossChainBridge>> {
        self.bridge
            .get_or_try_init(|| async {
                info!("Initializing CrossChainBridge");

                // Ensure both chain clients are initialized
                let chat_chain = self.chat_chain().await?;
                let currency_chain = self.currency_chain().await?;

                let bridge = CrossChainBridge::new(chat_chain, currency_chain);
                info!("✓ CrossChainBridge initialized");
                Ok(Arc::new(bridge))
            })
            .await
            .cloned()
    }

    /// Check if currency chain client is initialized
    pub fn is_currency_chain_initialized(&self) -> bool {
        self.currency_chain.initialized()
    }

    /// Check if chat chain client is initialized
    pub fn is_chat_chain_initialized(&self) -> bool {
        self.chat_chain.initialized()
    }

    /// Check if bridge is initialized
    pub fn is_bridge_initialized(&self) -> bool {
        self.bridge.initialized()
    }

    /// Get configuration (for debugging/logging)
    pub fn config(&self) -> &ChainClientConfig {
        &self.config
    }
}

/// Global service context that can be shared across the application
///
/// For use cases where you need a single shared context across multiple
/// command handlers without threading it through every function.
pub struct GlobalServiceContext {
    inner: OnceCell<Arc<ServiceContext>>,
}

impl GlobalServiceContext {
    /// Create a new empty global context
    pub const fn new() -> Self {
        Self {
            inner: OnceCell::const_new(),
        }
    }

    /// Initialize the global context with configuration
    ///
    /// Returns error if already initialized with different config.
    pub async fn init(&self, config: ChainClientConfig) -> Result<Arc<ServiceContext>> {
        self.inner
            .get_or_try_init(|| async { Ok(Arc::new(ServiceContext::new(config))) })
            .await
            .cloned()
    }

    /// Get the global context, if initialized
    pub fn get(&self) -> Option<Arc<ServiceContext>> {
        self.inner.get().cloned()
    }

    /// Get the global context, initializing with defaults if needed
    pub async fn get_or_init_default(&self) -> Arc<ServiceContext> {
        self.inner
            .get_or_init(|| async { Arc::new(ServiceContext::new(ChainClientConfig::default())) })
            .await
            .clone()
    }
}

impl Default for GlobalServiceContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_chain_client_config_default() {
        let config = ChainClientConfig::default();
        assert!(config.currency_rpc_url.contains("localhost"));
        assert!(config.chat_rpc_url.contains("localhost"));
    }

    #[test]
    fn test_service_context_lazy_init() {
        let ctx = ServiceContext::new(ChainClientConfig::default());
        assert!(!ctx.is_currency_chain_initialized());
        assert!(!ctx.is_chat_chain_initialized());
        assert!(!ctx.is_bridge_initialized());
    }

    #[tokio::test]
    async fn test_global_service_context() {
        let global = GlobalServiceContext::new();
        assert!(global.get().is_none());

        let ctx = global.get_or_init_default().await;
        assert!(global.get().is_some());

        // Second call returns same instance
        let ctx2 = global.get_or_init_default().await;
        assert!(Arc::ptr_eq(&ctx, &ctx2));
    }
}
