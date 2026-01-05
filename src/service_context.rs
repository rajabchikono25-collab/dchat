//! Service Context - Shared lazy-initialized services
//!
//! This module provides a `ServiceContext` that lazily initializes and
//! caches commonly used services like chain clients. This eliminates
//! duplicate initialization code across 20+ call sites in main.rs.
//!
//! ## Usage
//!
//! ```rust,ignore
//! // From Config with automatic RPC resolution:
//! let ctx = ServiceContext::builder()
//!     .with_config(&config)
//!     .build()?;
//!
//! // With CLI overrides:
//! let ctx = ServiceContext::builder()
//!     .with_config(&config)
//!     .currency_rpc_override(Some("https://custom.rpc/currency"))
//!     .chat_rpc_override(Some("https://custom.rpc/chat"))
//!     .build()?;
//!
//! // Access clients (lazy-initialized):
//! let currency_chain = ctx.currency_chain().await?;
//! let chat_chain = ctx.chat_chain().await?;
//! let bridge = ctx.cross_chain_bridge().await?;
//! ```
//!
//! ## RPC URL Resolution Priority
//!
//! 1. **CLI Override** - Explicit command-line argument (highest priority)
//! 2. **Environment Variable** - `DCHAT_CURRENCY_CHAIN_RPC_URL` / `DCHAT_CHAT_CHAIN_RPC_URL`
//! 3. **Config File** - `rpc.currency_chain_rpc_url` / `rpc.chat_chain_rpc_url`
//! 4. **Localhost Default** - Only if `DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1`
//!
//! ## Benefits
//!
//! 1. **Deduplication**: Single source of truth for client initialization
//! 2. **Lazy Loading**: Clients only initialized when first accessed
//! 3. **Connection Reuse**: Same Arc instance shared across all consumers
//! 4. **Consistent Config**: All clients use the same resolved RPC endpoints
//! 5. **Unified Resolution**: Single RPC URL resolution logic for entire codebase

use dchat_blockchain::{
    ChatChainClient, ChatChainConfig, CrossChainBridge, CurrencyChainClient, CurrencyChainConfig,
};
use dchat_core::config::Config;
use dchat_core::error::{Error, Result};
use std::sync::Arc;
use tokio::sync::OnceCell;
use tracing::{debug, error, info, warn};

/// Chain type for RPC URL resolution
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChainType {
    Currency,
    Chat,
}

impl std::fmt::Display for ChainType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ChainType::Currency => write!(f, "Currency"),
            ChainType::Chat => write!(f, "Chat"),
        }
    }
}

/// Source of the resolved RPC URL (for debugging/logging)
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RpcUrlSource {
    CliOverride,
    EnvironmentVariable,
    ConfigFile,
    LocalhostDefault,
}

impl std::fmt::Display for RpcUrlSource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            RpcUrlSource::CliOverride => write!(f, "CLI override"),
            RpcUrlSource::EnvironmentVariable => write!(f, "environment variable"),
            RpcUrlSource::ConfigFile => write!(f, "config file"),
            RpcUrlSource::LocalhostDefault => write!(f, "localhost default"),
        }
    }
}

/// Result of RPC URL resolution with source tracking
#[derive(Debug, Clone)]
pub struct ResolvedRpcUrl {
    pub url: String,
    pub source: RpcUrlSource,
    pub chain_type: ChainType,
}

/// Check if localhost defaults are allowed via environment variable
///
/// Returns true if `DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS` is set to "1", "true", or "yes".
/// This should only be used in development/testing environments.
pub fn allow_localhost_chain_rpc_defaults() -> bool {
    match std::env::var("DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS") {
        Ok(v) => {
            let v = v.trim();
            v == "1" || v.eq_ignore_ascii_case("true") || v.eq_ignore_ascii_case("yes")
        }
        Err(_) => false,
    }
}

/// Log a warning if the RPC URL is using HTTP over a non-localhost network.
///
/// This is a security check to alert operators when RPC credentials or
/// sensitive blockchain data may be transmitted in plaintext over public networks.
/// Localhost URLs (127.0.0.1, localhost, [::1]) are considered safe without HTTPS.
fn warn_insecure_rpc_url(url: &str, chain_type: ChainType) {
    if !url.starts_with("https://") {
        // Check if it's localhost (which is safe without HTTPS)
        let is_localhost =
            url.contains("localhost") || url.contains("127.0.0.1") || url.contains("[::1]");

        if !is_localhost {
            warn!(
                "⚠️  {} chain RPC URL '{}' is using HTTP instead of HTTPS. \
                 This is insecure for production use over public networks.",
                chain_type, url
            );
        }
    }
}

/// Resolve RPC URL with priority: CLI override > env var > config > localhost default
///
/// Returns the resolved URL and its source for debugging.
pub fn resolve_chain_rpc(
    chain_type: ChainType,
    config: Option<&Config>,
    cli_override: Option<&str>,
) -> Result<ResolvedRpcUrl> {
    // Priority 1: CLI override (highest)
    if let Some(url) = cli_override {
        let url = url.trim();
        if !url.is_empty() {
            debug!(
                "{} chain RPC URL resolved from CLI override: {}",
                chain_type, url
            );
            warn_insecure_rpc_url(url, chain_type);
            return Ok(ResolvedRpcUrl {
                url: url.to_string(),
                source: RpcUrlSource::CliOverride,
                chain_type,
            });
        }
    }

    // Priority 2: Environment variable
    let env_vars = match chain_type {
        ChainType::Currency => &[
            "DCHAT_CURRENCY_CHAIN_RPC_URL",
            "CURRENCY_CHAIN_RPC",
            "CURRENCY_CHAIN_RPC_URL",
        ],
        ChainType::Chat => &[
            "DCHAT_CHAT_CHAIN_RPC_URL",
            "CHAT_CHAIN_RPC",
            "CHAT_CHAIN_RPC_URL",
        ],
    };

    for env_var in env_vars.iter() {
        if let Ok(url) = std::env::var(env_var) {
            let url = url.trim();
            if !url.is_empty() {
                debug!(
                    "{} chain RPC URL resolved from env var {}: {}",
                    chain_type, env_var, url
                );
                warn_insecure_rpc_url(url, chain_type);
                return Ok(ResolvedRpcUrl {
                    url: url.to_string(),
                    source: RpcUrlSource::EnvironmentVariable,
                    chain_type,
                });
            }
        }
    }

    // Priority 3: Config file
    if let Some(cfg) = config {
        let url = match chain_type {
            ChainType::Currency => cfg.rpc.currency_chain_rpc_url.as_ref(),
            ChainType::Chat => cfg.rpc.chat_chain_rpc_url.as_ref(),
        };
        if let Some(url) = url {
            let url = url.trim();
            if !url.is_empty() {
                debug!(
                    "{} chain RPC URL resolved from config file: {}",
                    chain_type, url
                );
                warn_insecure_rpc_url(url, chain_type);
                return Ok(ResolvedRpcUrl {
                    url: url.to_string(),
                    source: RpcUrlSource::ConfigFile,
                    chain_type,
                });
            }
        }
    }

    // Priority 4: Localhost default (only if explicitly allowed)
    if allow_localhost_chain_rpc_defaults() {
        let default_url = match chain_type {
            ChainType::Currency => CurrencyChainConfig::default().rpc_url,
            ChainType::Chat => ChatChainConfig::default().rpc_url,
        };
        warn!(
            "{} chain RPC URL using localhost default (DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1): {}",
            chain_type, default_url
        );
        // Note: localhost defaults are inherently safe for HTTP, but we still validate for consistency
        warn_insecure_rpc_url(&default_url, chain_type);
        return Ok(ResolvedRpcUrl {
            url: default_url,
            source: RpcUrlSource::LocalhostDefault,
            chain_type,
        });
    }

    // No URL found
    let env_var_name = match chain_type {
        ChainType::Currency => "DCHAT_CURRENCY_CHAIN_RPC_URL",
        ChainType::Chat => "DCHAT_CHAT_CHAIN_RPC_URL",
    };
    let config_key = match chain_type {
        ChainType::Currency => "rpc.currency_chain_rpc_url",
        ChainType::Chat => "rpc.chat_chain_rpc_url",
    };

    Err(Error::Config(format!(
        "{} chain RPC URL not configured.\n\
         Set `{}` in config.toml or env `{}`.\n\
         For local development, set `DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1`.",
        chain_type, config_key, env_var_name
    )))
}

/// Resolve currency chain RPC URL (convenience function)
pub fn resolve_currency_chain_rpc(
    config: Option<&Config>,
    cli_override: Option<&str>,
) -> Result<String> {
    resolve_chain_rpc(ChainType::Currency, config, cli_override).map(|r| r.url)
}

/// Resolve chat chain RPC URL (convenience function)
pub fn resolve_chat_chain_rpc(
    config: Option<&Config>,
    cli_override: Option<&str>,
) -> Result<String> {
    resolve_chain_rpc(ChainType::Chat, config, cli_override).map(|r| r.url)
}

/// Configuration for chain clients extracted from main config
#[derive(Debug, Clone)]
pub struct ChainClientConfig {
    /// Currency chain RPC URL
    pub currency_rpc_url: String,
    /// Chat chain RPC URL
    pub chat_rpc_url: String,
    /// Optional timeout for RPC calls in seconds
    pub timeout_secs: Option<u64>,
    /// Source of currency RPC URL (for debugging)
    pub currency_rpc_source: Option<RpcUrlSource>,
    /// Source of chat RPC URL (for debugging)
    pub chat_rpc_source: Option<RpcUrlSource>,
}

impl Default for ChainClientConfig {
    fn default() -> Self {
        Self {
            currency_rpc_url: "http://localhost:8899".to_string(),
            chat_rpc_url: "http://localhost:8545".to_string(),
            timeout_secs: Some(30),
            currency_rpc_source: None,
            chat_rpc_source: None,
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

    /// Create a new builder for ServiceContext
    ///
    /// This is the recommended way to construct a ServiceContext as it handles
    /// RPC URL resolution with proper priority (CLI > env > config > default).
    pub fn builder<'a>() -> ServiceContextBuilder<'a> {
        ServiceContextBuilder::new()
    }

    /// Create from main Config struct (legacy method, prefer builder())
    ///
    /// Extracts chain RPC URLs from the config, validating required values.
    /// Fails fast at startup if RPC URLs are missing or empty.
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

        // Validate currency RPC URL is not empty
        if currency_rpc.trim().is_empty() {
            return Err(Error::Config(
                "Currency chain RPC URL cannot be empty. Set via config or DCHAT_CURRENCY_CHAIN_RPC_URL environment variable.".to_string()
            ));
        }

        let chat_rpc = chat_rpc_url.ok_or_else(|| {
            Error::validation(
                "Chat chain RPC URL not configured. Set via config or DCHAT_CHAT_CHAIN_RPC_URL environment variable."
            )
        })?;

        // Validate chat RPC URL is not empty
        if chat_rpc.trim().is_empty() {
            return Err(Error::Config(
                "Chat chain RPC URL cannot be empty. Set via config or DCHAT_CHAT_CHAIN_RPC_URL environment variable.".to_string()
            ));
        }

        Ok(Self::new(ChainClientConfig {
            currency_rpc_url: currency_rpc,
            chat_rpc_url: chat_rpc,
            timeout_secs,
            currency_rpc_source: None,
            chat_rpc_source: None,
        }))
    }

    /// Get or initialize the currency chain client
    pub async fn currency_chain(&self) -> Result<Arc<CurrencyChainClient>> {
        if self.config.currency_rpc_url.trim().is_empty() {
            return Err(Error::Config(
                "Currency chain RPC URL not configured (ServiceContext built with require_currency_chain=false)."
                    .to_string(),
            ));
        }

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
        if self.config.chat_rpc_url.trim().is_empty() {
            return Err(Error::Config(
                "Chat chain RPC URL not configured (ServiceContext built with require_chat_chain=false)."
                    .to_string(),
            ));
        }

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

/// Builder for ServiceContext with fluent API
///
/// Handles RPC URL resolution with proper priority:
/// 1. CLI override (highest)
/// 2. Environment variable
/// 3. Config file
/// 4. Localhost default (only if DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS=1)
///
/// # Example
///
/// ```rust,ignore
/// // Basic usage with Config
/// let ctx = ServiceContext::builder()
///     .with_config(&config)
///     .build()?;
///
/// // With CLI overrides
/// let ctx = ServiceContext::builder()
///     .with_config(&config)
///     .currency_rpc_override(args.currency_rpc_url.as_deref())
///     .chat_rpc_override(args.chat_rpc_url.as_deref())
///     .timeout_secs(60)
///     .build()?;
///
/// // Require specific chains only
/// let ctx = ServiceContext::builder()
///     .with_config(&config)
///     .require_currency_chain(true)
///     .require_chat_chain(false) // Don't fail if chat chain not configured
///     .build()?;
/// ```
#[derive(Debug, Default)]
pub struct ServiceContextBuilder<'a> {
    /// Reference to the main config (optional)
    config: Option<&'a Config>,
    /// CLI override for currency chain RPC URL
    currency_rpc_override: Option<&'a str>,
    /// CLI override for chat chain RPC URL
    chat_rpc_override: Option<&'a str>,
    /// Timeout for RPC calls in seconds
    timeout_secs: Option<u64>,
    /// Whether currency chain is required (fail if not configured)
    require_currency_chain: bool,
    /// Whether chat chain is required (fail if not configured)
    require_chat_chain: bool,
}

impl<'a> ServiceContextBuilder<'a> {
    /// Create a new builder
    pub fn new() -> Self {
        Self {
            config: None,
            currency_rpc_override: None,
            chat_rpc_override: None,
            timeout_secs: Some(30),
            require_currency_chain: true,
            require_chat_chain: true,
        }
    }

    /// Set the main Config for RPC URL resolution
    pub fn with_config(mut self, config: &'a Config) -> Self {
        self.config = Some(config);
        self
    }

    /// Override currency chain RPC URL from CLI
    pub fn currency_rpc_override(mut self, url: Option<&'a str>) -> Self {
        self.currency_rpc_override = url;
        self
    }

    /// Override chat chain RPC URL from CLI
    pub fn chat_rpc_override(mut self, url: Option<&'a str>) -> Self {
        self.chat_rpc_override = url;
        self
    }

    /// Set RPC call timeout in seconds
    pub fn timeout_secs(mut self, secs: u64) -> Self {
        self.timeout_secs = Some(secs);
        self
    }

    /// Set whether currency chain is required
    ///
    /// If true (default), build() will fail if currency chain RPC URL cannot be resolved.
    /// If false, build() will succeed but currency_chain() calls may fail later.
    pub fn require_currency_chain(mut self, required: bool) -> Self {
        self.require_currency_chain = required;
        self
    }

    /// Set whether chat chain is required
    ///
    /// If true (default), build() will fail if chat chain RPC URL cannot be resolved.
    /// If false, build() will succeed but chat_chain() calls may fail later.
    pub fn require_chat_chain(mut self, required: bool) -> Self {
        self.require_chat_chain = required;
        self
    }

    /// Build the ServiceContext
    ///
    /// Resolves RPC URLs using the priority: CLI > env > config > localhost default.
    /// Fails if required chains are not configured.
    pub fn build(self) -> Result<ServiceContext> {
        // Resolve currency chain RPC URL
        let currency_result =
            resolve_chain_rpc(ChainType::Currency, self.config, self.currency_rpc_override);

        let (currency_rpc_url, currency_rpc_source) = match currency_result {
            Ok(resolved) => {
                info!(
                    "Currency chain RPC URL: {} (from {})",
                    resolved.url, resolved.source
                );
                (resolved.url, Some(resolved.source))
            }
            Err(e) => {
                if self.require_currency_chain {
                    return Err(e);
                }
                warn!(
                    "Currency chain RPC URL not configured (not required): {}",
                    e
                );
                // Use a placeholder - will fail on actual use
                ("".to_string(), None)
            }
        };

        // Resolve chat chain RPC URL
        let chat_result = resolve_chain_rpc(ChainType::Chat, self.config, self.chat_rpc_override);

        let (chat_rpc_url, chat_rpc_source) = match chat_result {
            Ok(resolved) => {
                info!(
                    "Chat chain RPC URL: {} (from {})",
                    resolved.url, resolved.source
                );
                (resolved.url, Some(resolved.source))
            }
            Err(e) => {
                if self.require_chat_chain {
                    return Err(e);
                }
                warn!("Chat chain RPC URL not configured (not required): {}", e);
                // Use a placeholder - will fail on actual use
                ("".to_string(), None)
            }
        };

        let chain_config = ChainClientConfig {
            currency_rpc_url,
            chat_rpc_url,
            timeout_secs: self.timeout_secs,
            currency_rpc_source,
            chat_rpc_source,
        };

        Ok(ServiceContext::new(chain_config))
    }

    /// Build the ServiceContext wrapped in Arc for sharing
    pub fn build_shared(self) -> Result<Arc<ServiceContext>> {
        self.build().map(Arc::new)
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
