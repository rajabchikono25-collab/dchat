//! Node Startup Integration Tests
//!
//! Tests node initialization flows using mock chain clients to verify
//! wiring and startup sequences without requiring live blockchain nodes.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::RwLock;

/// Test double for CurrencyChainClient
/// Provides a lightweight mock for testing node startup without blockchain dependencies
pub struct MockCurrencyChainClient {
    /// Simulated current block height
    block_height: Arc<RwLock<u64>>,
    /// Mock wallet balances: address -> balance
    balances: Arc<RwLock<HashMap<String, u64>>>,
    /// Mock staking positions: address -> staked amount
    stakes: Arc<RwLock<HashMap<String, u64>>>,
    /// Track method calls for verification
    method_calls: Arc<RwLock<Vec<MethodCall>>>,
    /// Simulated RPC latency
    latency_ms: u64,
    /// Should operations fail?
    should_fail: Arc<RwLock<bool>>,
}

/// Test double for ChatChainClient
pub struct MockChatChainClient {
    /// Simulated block height
    block_height: Arc<RwLock<u64>>,
    /// Registered identities: user_id -> public_key
    identities: Arc<RwLock<HashMap<String, Vec<u8>>>>,
    /// Channel metadata
    channels: Arc<RwLock<HashMap<String, ChannelInfo>>>,
    /// Reputation scores
    reputation: Arc<RwLock<HashMap<String, u32>>>,
    /// Track method calls for verification
    method_calls: Arc<RwLock<Vec<MethodCall>>>,
    /// Simulated RPC latency
    latency_ms: u64,
    /// Should operations fail?
    should_fail: Arc<RwLock<bool>>,
}

/// Record of a method call for test verification
#[derive(Debug, Clone)]
pub struct MethodCall {
    pub method: String,
    pub args: Vec<String>,
    pub timestamp: std::time::Instant,
}

/// Simple channel info for mock
#[derive(Debug, Clone)]
pub struct ChannelInfo {
    pub name: String,
    pub owner: String,
    pub created_at: i64,
}

impl MockCurrencyChainClient {
    /// Create new mock client with default state
    pub fn new() -> Self {
        Self {
            block_height: Arc::new(RwLock::new(1)),
            balances: Arc::new(RwLock::new(HashMap::new())),
            stakes: Arc::new(RwLock::new(HashMap::new())),
            method_calls: Arc::new(RwLock::new(Vec::new())),
            latency_ms: 0,
            should_fail: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with initial balances
    pub fn with_balances(mut self, balances: HashMap<String, u64>) -> Self {
        self.balances = Arc::new(RwLock::new(balances));
        self
    }

    /// Create with simulated latency
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = latency_ms;
        self
    }

    /// Set failure mode
    pub async fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.write().await = should_fail;
    }

    /// Get current block height
    pub async fn get_block_height(&self) -> Result<u64, String> {
        self.record_call("get_block_height", vec![]).await;
        self.maybe_delay().await;
        self.maybe_fail().await?;
        Ok(*self.block_height.read().await)
    }

    /// Advance block height
    pub async fn advance_blocks(&self, count: u64) {
        let mut height = self.block_height.write().await;
        *height += count;
    }

    /// Get balance for address
    pub async fn get_balance(&self, address: &str) -> Result<u64, String> {
        self.record_call("get_balance", vec![address.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let balances = self.balances.read().await;
        Ok(*balances.get(address).unwrap_or(&0))
    }

    /// Set balance for address
    pub async fn set_balance(&self, address: &str, amount: u64) {
        let mut balances = self.balances.write().await;
        balances.insert(address.to_string(), amount);
    }

    /// Get stake for address
    pub async fn get_stake(&self, address: &str) -> Result<u64, String> {
        self.record_call("get_stake", vec![address.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let stakes = self.stakes.read().await;
        Ok(*stakes.get(address).unwrap_or(&0))
    }

    /// Register stake
    pub async fn stake(&self, address: &str, amount: u64) -> Result<(), String> {
        self.record_call("stake", vec![address.to_string(), amount.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let mut stakes = self.stakes.write().await;
        *stakes.entry(address.to_string()).or_insert(0) += amount;
        Ok(())
    }

    /// Get all method calls for verification
    pub async fn get_method_calls(&self) -> Vec<MethodCall> {
        self.method_calls.read().await.clone()
    }

    /// Clear method call history
    pub async fn clear_method_calls(&self) {
        self.method_calls.write().await.clear();
    }

    async fn record_call(&self, method: &str, args: Vec<String>) {
        let mut calls = self.method_calls.write().await;
        calls.push(MethodCall {
            method: method.to_string(),
            args,
            timestamp: std::time::Instant::now(),
        });
    }

    async fn maybe_delay(&self) {
        if self.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        }
    }

    async fn maybe_fail(&self) -> Result<(), String> {
        if *self.should_fail.read().await {
            Err("Mock client configured to fail".to_string())
        } else {
            Ok(())
        }
    }
}

impl Default for MockCurrencyChainClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockChatChainClient {
    /// Create new mock client with default state
    pub fn new() -> Self {
        Self {
            block_height: Arc::new(RwLock::new(1)),
            identities: Arc::new(RwLock::new(HashMap::new())),
            channels: Arc::new(RwLock::new(HashMap::new())),
            reputation: Arc::new(RwLock::new(HashMap::new())),
            method_calls: Arc::new(RwLock::new(Vec::new())),
            latency_ms: 0,
            should_fail: Arc::new(RwLock::new(false)),
        }
    }

    /// Create with simulated latency
    pub fn with_latency(mut self, latency_ms: u64) -> Self {
        self.latency_ms = latency_ms;
        self
    }

    /// Set failure mode
    pub async fn set_should_fail(&self, should_fail: bool) {
        *self.should_fail.write().await = should_fail;
    }

    /// Get current block height
    pub async fn get_block_height(&self) -> Result<u64, String> {
        self.record_call("get_block_height", vec![]).await;
        self.maybe_delay().await;
        self.maybe_fail().await?;
        Ok(*self.block_height.read().await)
    }

    /// Advance block height
    pub async fn advance_blocks(&self, count: u64) {
        let mut height = self.block_height.write().await;
        *height += count;
    }

    /// Register identity
    pub async fn register_identity(
        &self,
        user_id: &str,
        public_key: Vec<u8>,
    ) -> Result<(), String> {
        self.record_call("register_identity", vec![user_id.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let mut identities = self.identities.write().await;
        identities.insert(user_id.to_string(), public_key);
        Ok(())
    }

    /// Lookup identity
    pub async fn lookup_identity(&self, user_id: &str) -> Result<Option<Vec<u8>>, String> {
        self.record_call("lookup_identity", vec![user_id.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let identities = self.identities.read().await;
        Ok(identities.get(user_id).cloned())
    }

    /// Create channel
    pub async fn create_channel(
        &self,
        channel_id: &str,
        name: &str,
        owner: &str,
    ) -> Result<(), String> {
        self.record_call(
            "create_channel",
            vec![channel_id.to_string(), name.to_string()],
        )
        .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let mut channels = self.channels.write().await;
        channels.insert(
            channel_id.to_string(),
            ChannelInfo {
                name: name.to_string(),
                owner: owner.to_string(),
                created_at: chrono::Utc::now().timestamp(),
            },
        );
        Ok(())
    }

    /// Get channel
    pub async fn get_channel(&self, channel_id: &str) -> Result<Option<ChannelInfo>, String> {
        self.record_call("get_channel", vec![channel_id.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let channels = self.channels.read().await;
        Ok(channels.get(channel_id).cloned())
    }

    /// Get reputation score
    pub async fn get_reputation(&self, user_id: &str) -> Result<u32, String> {
        self.record_call("get_reputation", vec![user_id.to_string()])
            .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let reputation = self.reputation.read().await;
        Ok(*reputation.get(user_id).unwrap_or(&100)) // Default 100 reputation
    }

    /// Set reputation score
    pub async fn set_reputation(&self, user_id: &str, score: u32) -> Result<(), String> {
        self.record_call(
            "set_reputation",
            vec![user_id.to_string(), score.to_string()],
        )
        .await;
        self.maybe_delay().await;
        self.maybe_fail().await?;

        let mut reputation = self.reputation.write().await;
        reputation.insert(user_id.to_string(), score);
        Ok(())
    }

    /// Get all method calls for verification
    pub async fn get_method_calls(&self) -> Vec<MethodCall> {
        self.method_calls.read().await.clone()
    }

    /// Clear method call history
    pub async fn clear_method_calls(&self) {
        self.method_calls.write().await.clear();
    }

    async fn record_call(&self, method: &str, args: Vec<String>) {
        let mut calls = self.method_calls.write().await;
        calls.push(MethodCall {
            method: method.to_string(),
            args,
            timestamp: std::time::Instant::now(),
        });
    }

    async fn maybe_delay(&self) {
        if self.latency_ms > 0 {
            tokio::time::sleep(Duration::from_millis(self.latency_ms)).await;
        }
    }

    async fn maybe_fail(&self) -> Result<(), String> {
        if *self.should_fail.read().await {
            Err("Mock client configured to fail".to_string())
        } else {
            Ok(())
        }
    }
}

impl Default for MockChatChainClient {
    fn default() -> Self {
        Self::new()
    }
}

/// Test fixture for node startup tests
pub struct NodeTestFixture {
    pub currency_client: MockCurrencyChainClient,
    pub chat_client: MockChatChainClient,
    pub temp_dir: PathBuf,
}

impl NodeTestFixture {
    /// Create new test fixture with fresh mock clients
    pub fn new() -> Self {
        Self {
            currency_client: MockCurrencyChainClient::new(),
            chat_client: MockChatChainClient::new(),
            temp_dir: std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4())),
        }
    }

    /// Create fixture with pre-configured state
    pub fn with_initial_state(balances: HashMap<String, u64>, latency_ms: u64) -> Self {
        Self {
            currency_client: MockCurrencyChainClient::new()
                .with_balances(balances)
                .with_latency(latency_ms),
            chat_client: MockChatChainClient::new().with_latency(latency_ms),
            temp_dir: std::env::temp_dir().join(format!("dchat_test_{}", uuid::Uuid::new_v4())),
        }
    }

    /// Cleanup temp directory
    pub fn cleanup(&self) {
        if self.temp_dir.exists() {
            let _ = std::fs::remove_dir_all(&self.temp_dir);
        }
    }
}

impl Default for NodeTestFixture {
    fn default() -> Self {
        Self::new()
    }
}

impl Drop for NodeTestFixture {
    fn drop(&mut self) {
        self.cleanup();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_mock_currency_client_creation() {
        let client = MockCurrencyChainClient::new();
        let height = client.get_block_height().await.unwrap();
        assert_eq!(height, 1);
    }

    #[tokio::test]
    async fn test_mock_currency_client_balances() {
        let mut initial_balances = HashMap::new();
        initial_balances.insert("alice".to_string(), 1000);
        initial_balances.insert("bob".to_string(), 500);

        let client = MockCurrencyChainClient::new().with_balances(initial_balances);

        assert_eq!(client.get_balance("alice").await.unwrap(), 1000);
        assert_eq!(client.get_balance("bob").await.unwrap(), 500);
        assert_eq!(client.get_balance("carol").await.unwrap(), 0);
    }

    #[tokio::test]
    async fn test_mock_currency_client_staking() {
        let client = MockCurrencyChainClient::new();

        client.stake("validator1", 10000).await.unwrap();
        client.stake("validator1", 5000).await.unwrap();

        assert_eq!(client.get_stake("validator1").await.unwrap(), 15000);
    }

    #[tokio::test]
    async fn test_mock_currency_client_block_advancement() {
        let client = MockCurrencyChainClient::new();

        assert_eq!(client.get_block_height().await.unwrap(), 1);

        client.advance_blocks(10).await;
        assert_eq!(client.get_block_height().await.unwrap(), 11);
    }

    #[tokio::test]
    async fn test_mock_currency_client_failure_mode() {
        let client = MockCurrencyChainClient::new();

        // Should succeed initially
        assert!(client.get_block_height().await.is_ok());

        // Enable failure mode
        client.set_should_fail(true).await;
        assert!(client.get_block_height().await.is_err());

        // Disable failure mode
        client.set_should_fail(false).await;
        assert!(client.get_block_height().await.is_ok());
    }

    #[tokio::test]
    async fn test_mock_currency_client_method_tracking() {
        let client = MockCurrencyChainClient::new();

        client.get_block_height().await.unwrap();
        client.get_balance("alice").await.unwrap();
        client.stake("bob", 100).await.unwrap();

        let calls = client.get_method_calls().await;
        assert_eq!(calls.len(), 3);
        assert_eq!(calls[0].method, "get_block_height");
        assert_eq!(calls[1].method, "get_balance");
        assert_eq!(calls[2].method, "stake");
    }

    #[tokio::test]
    async fn test_mock_chat_client_creation() {
        let client = MockChatChainClient::new();
        let height = client.get_block_height().await.unwrap();
        assert_eq!(height, 1);
    }

    #[tokio::test]
    async fn test_mock_chat_client_identity() {
        let client = MockChatChainClient::new();

        // Initially no identity
        assert!(client.lookup_identity("alice").await.unwrap().is_none());

        // Register identity
        client
            .register_identity("alice", vec![1, 2, 3, 4])
            .await
            .unwrap();

        // Now should find it
        let pk = client.lookup_identity("alice").await.unwrap().unwrap();
        assert_eq!(pk, vec![1, 2, 3, 4]);
    }

    #[tokio::test]
    async fn test_mock_chat_client_channels() {
        let client = MockChatChainClient::new();

        // Create channel
        client
            .create_channel("general", "General Chat", "alice")
            .await
            .unwrap();

        // Get channel
        let channel = client.get_channel("general").await.unwrap().unwrap();
        assert_eq!(channel.name, "General Chat");
        assert_eq!(channel.owner, "alice");
    }

    #[tokio::test]
    async fn test_mock_chat_client_reputation() {
        let client = MockChatChainClient::new();

        // Default reputation
        assert_eq!(client.get_reputation("alice").await.unwrap(), 100);

        // Set custom reputation
        client.set_reputation("alice", 150).await.unwrap();
        assert_eq!(client.get_reputation("alice").await.unwrap(), 150);
    }

    #[tokio::test]
    async fn test_node_fixture_creation() {
        let fixture = NodeTestFixture::new();

        // Both clients should be accessible
        assert_eq!(fixture.currency_client.get_block_height().await.unwrap(), 1);
        assert_eq!(fixture.chat_client.get_block_height().await.unwrap(), 1);
    }

    #[tokio::test]
    async fn test_node_fixture_with_initial_state() {
        let mut balances = HashMap::new();
        balances.insert("genesis".to_string(), 1_000_000);

        let fixture = NodeTestFixture::with_initial_state(balances, 0);

        assert_eq!(
            fixture
                .currency_client
                .get_balance("genesis")
                .await
                .unwrap(),
            1_000_000
        );
    }

    #[tokio::test]
    async fn test_mock_client_latency() {
        let client = MockCurrencyChainClient::new().with_latency(50); // 50ms latency

        let start = std::time::Instant::now();
        client.get_block_height().await.unwrap();
        let elapsed = start.elapsed();

        // Should have taken at least 50ms
        assert!(elapsed >= Duration::from_millis(50));
    }

    /// Integration test: Simulates validator node startup sequence
    #[tokio::test]
    async fn test_validator_startup_sequence() {
        let fixture = NodeTestFixture::new();

        // 1. Validator checks current block height
        let height = fixture.currency_client.get_block_height().await.unwrap();
        assert!(height > 0, "Block height should be positive");

        // 2. Validator registers stake
        fixture
            .currency_client
            .stake("validator_001", 10_000)
            .await
            .unwrap();

        // 3. Validator registers identity on chat chain
        let pubkey = vec![0u8; 32]; // Mock public key
        fixture
            .chat_client
            .register_identity("validator_001", pubkey)
            .await
            .unwrap();

        // Verify the sequence was recorded
        let currency_calls = fixture.currency_client.get_method_calls().await;
        let chat_calls = fixture.chat_client.get_method_calls().await;

        assert!(currency_calls
            .iter()
            .any(|c| c.method == "get_block_height"));
        assert!(currency_calls.iter().any(|c| c.method == "stake"));
        assert!(chat_calls.iter().any(|c| c.method == "register_identity"));
    }

    /// Integration test: Simulates relay node startup sequence
    #[tokio::test]
    async fn test_relay_startup_sequence() {
        let fixture = NodeTestFixture::new();

        // 1. Relay checks stake requirement
        let stake = fixture
            .currency_client
            .get_stake("relay_001")
            .await
            .unwrap();
        assert_eq!(stake, 0, "New relay should have no stake");

        // 2. Relay registers stake for incentives
        fixture
            .currency_client
            .stake("relay_001", 1_000)
            .await
            .unwrap();

        // 3. Relay verifies stake was recorded
        let new_stake = fixture
            .currency_client
            .get_stake("relay_001")
            .await
            .unwrap();
        assert_eq!(new_stake, 1_000);

        // 4. Relay registers on chat chain for message routing
        fixture
            .chat_client
            .register_identity("relay_001", vec![1, 2, 3])
            .await
            .unwrap();

        // Verify startup sequence completed
        let calls = fixture.currency_client.get_method_calls().await;
        assert_eq!(calls.len(), 3); // get_stake, stake, get_stake
    }

    /// Integration test: Simulates user node startup sequence
    #[tokio::test]
    async fn test_user_startup_sequence() {
        let mut initial_balances = HashMap::new();
        initial_balances.insert("user_alice".to_string(), 100);

        let fixture = NodeTestFixture::with_initial_state(initial_balances, 0);

        // 1. User checks their balance
        let balance = fixture
            .currency_client
            .get_balance("user_alice")
            .await
            .unwrap();
        assert_eq!(balance, 100);

        // 2. User registers identity
        fixture
            .chat_client
            .register_identity("user_alice", vec![0xAA; 32])
            .await
            .unwrap();

        // 3. User checks default reputation
        let reputation = fixture
            .chat_client
            .get_reputation("user_alice")
            .await
            .unwrap();
        assert_eq!(reputation, 100); // Default reputation

        // 4. User joins default channel
        let channel = fixture.chat_client.get_channel("global").await.unwrap();
        assert!(channel.is_none(), "Global channel may not exist yet");
    }

    /// Integration test: Handles chain client failures gracefully
    #[tokio::test]
    async fn test_startup_with_chain_failure() {
        let fixture = NodeTestFixture::new();

        // Simulate chain being unavailable
        fixture.currency_client.set_should_fail(true).await;

        // Startup should fail gracefully
        let result = fixture.currency_client.get_block_height().await;
        assert!(result.is_err());

        // Chain comes back online
        fixture.currency_client.set_should_fail(false).await;

        // Retry should succeed
        let result = fixture.currency_client.get_block_height().await;
        assert!(result.is_ok());
    }
}
