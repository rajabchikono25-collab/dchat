//! Integration tests for slashing implementation with currency chain
//!
//! Tests the complete slashing flow:
//! 1. Dispute resolution voting
//! 2. Stake query from currency chain
//! 3. Slash execution
//! 4. Reward distribution

use dchat_chain::{
    CurrencyChainClient, DisputeResolver, DisputeStatus, DisputeType, HttpCurrencyChainClient,
    SlashingConfig, SlashingEvent,
};
use dchat_core::error::Result;
use std::sync::Arc;

/// Mock currency chain client for testing
struct MockCurrencyChainClient {
    stakes: std::sync::Mutex<std::collections::HashMap<Vec<u8>, u64>>,
    slashes: std::sync::Mutex<Vec<(Vec<u8>, u64, String)>>,
    rewards: std::sync::Mutex<Vec<(Vec<u8>, u64)>>,
}

impl MockCurrencyChainClient {
    fn new() -> Self {
        Self {
            stakes: std::sync::Mutex::new(std::collections::HashMap::new()),
            slashes: std::sync::Mutex::new(Vec::new()),
            rewards: std::sync::Mutex::new(Vec::new()),
        }
    }

    fn set_stake(&self, validator_key: &[u8], amount: u64) {
        self.stakes
            .lock()
            .unwrap()
            .insert(validator_key.to_vec(), amount);
    }

    fn get_slashes(&self) -> Vec<(Vec<u8>, u64, String)> {
        self.slashes.lock().unwrap().clone()
    }

    fn get_rewards(&self) -> Vec<(Vec<u8>, u64)> {
        self.rewards.lock().unwrap().clone()
    }
}

#[async_trait::async_trait]
impl CurrencyChainClient for MockCurrencyChainClient {
    async fn get_validator_stake(&self, validator_key: &[u8]) -> Result<u64> {
        let stakes = self.stakes.lock().unwrap();
        Ok(*stakes.get(validator_key).unwrap_or(&0))
    }

    async fn execute_slash(
        &self,
        validator_key: &[u8],
        slash_amount: u64,
        reason: &str,
    ) -> Result<String> {
        // Record slash
        self.slashes.lock().unwrap().push((
            validator_key.to_vec(),
            slash_amount,
            reason.to_string(),
        ));

        // Reduce stake
        let mut stakes = self.stakes.lock().unwrap();
        if let Some(stake) = stakes.get_mut(validator_key) {
            *stake = stake.saturating_sub(slash_amount);
        }

        Ok(format!("tx_{}", uuid::Uuid::new_v4()))
    }

    async fn transfer_reward(&self, recipient_key: &[u8], amount: u64) -> Result<String> {
        self.rewards
            .lock()
            .unwrap()
            .push((recipient_key.to_vec(), amount));

        Ok(format!("reward_tx_{}", uuid::Uuid::new_v4()))
    }
}

#[tokio::test]
async fn test_slash_accused_after_vote() {
    // Setup
    let mock_client = Arc::new(MockCurrencyChainClient::new());
    let accused_key = b"accused_validator";
    let claimant_key = b"claimant_validator";

    // Set initial stakes
    mock_client.set_stake(accused_key, 10000);
    mock_client.set_stake(claimant_key, 5000);

    let mut resolver = DisputeResolver::new()
        .with_slashing_config(SlashingConfig {
            base_slash_rate: 0.30,
            false_claim_multiplier: 1.5,
            claimant_reward_rate: 0.10,
            min_dispute_stake: 1000,
        })
        .with_currency_chain_client(mock_client.clone());

    // Submit claim
    let claim_id = resolver
        .submit_claim(
            DisputeType::ForkDetected,
            String::from_utf8(claimant_key.to_vec()).unwrap(),
            String::from_utf8(accused_key.to_vec()).unwrap(),
            b"fork_evidence".to_vec(),
        )
        .unwrap();

    // Skip to voting
    resolver.set_claim_status(&claim_id, DisputeStatus::UnderVote).unwrap();

    // Resolve with 70% vote for claimant (above 66% threshold)
    resolver.resolve_dispute(claim_id.clone(), 0.70).await.unwrap();

    // Verify slash was executed
    let slashes = mock_client.get_slashes();
    assert_eq!(slashes.len(), 1);
    assert_eq!(slashes[0].0, accused_key);
    assert_eq!(slashes[0].1, 3000); // 30% of 10000
    assert!(slashes[0].2.contains("Dispute resolved"));

    // Verify reward was sent
    let rewards = mock_client.get_rewards();
    assert_eq!(rewards.len(), 1);
    assert_eq!(rewards[0].0, claimant_key);
    assert_eq!(rewards[0].1, 300); // 10% of 3000

    // Verify stake was reduced
    let remaining_stake = mock_client.get_validator_stake(accused_key).await.unwrap();
    assert_eq!(remaining_stake, 7000);

    // Verify slashing event was recorded
    let events = resolver.get_slashing_events();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].slash_amount, 3000);
    assert_eq!(events[0].original_stake, 10000);
    assert_eq!(events[0].reward_amount, 300);
}

#[tokio::test]
async fn test_slash_claimant_for_false_claim() {
    // Setup
    let mock_client = Arc::new(MockCurrencyChainClient::new());
    let accused_key = b"accused_validator";
    let claimant_key = b"claimant_validator";

    // Set initial stakes
    mock_client.set_stake(accused_key, 10000);
    mock_client.set_stake(claimant_key, 5000);

    let mut resolver = DisputeResolver::new()
        .with_slashing_config(SlashingConfig {
            base_slash_rate: 0.30,
            false_claim_multiplier: 1.5, // 45% total
            claimant_reward_rate: 0.10,
            min_dispute_stake: 1000,
        })
        .with_currency_chain_client(mock_client.clone());

    // Submit claim
    let claim_id = resolver
        .submit_claim(
            DisputeType::ForkDetected,
            String::from_utf8(claimant_key.to_vec()).unwrap(),
            String::from_utf8(accused_key.to_vec()).unwrap(),
            b"fork_evidence".to_vec(),
        )
        .unwrap();

    // Skip to voting
    resolver.set_claim_status(&claim_id, DisputeStatus::UnderVote).unwrap();

    // Resolve with 20% vote for claimant (below 34% inverse threshold)
    resolver.resolve_dispute(claim_id.clone(), 0.20).await.unwrap();

    // Verify claimant was slashed (false claim penalty)
    let slashes = mock_client.get_slashes();
    assert_eq!(slashes.len(), 1);
    assert_eq!(slashes[0].0, claimant_key);
    assert_eq!(slashes[0].1, 2250); // 45% of 5000 (30% * 1.5)
    assert!(slashes[0].2.contains("False claim"));

    // Verify reward was sent to accused
    let rewards = mock_client.get_rewards();
    assert_eq!(rewards.len(), 1);
    assert_eq!(rewards[0].0, accused_key);
    assert_eq!(rewards[0].1, 225); // 10% of 2250

    // Verify claimant stake was reduced
    let remaining_stake = mock_client.get_validator_stake(claimant_key).await.unwrap();
    assert_eq!(remaining_stake, 2750);
}

#[tokio::test]
async fn test_inconclusive_dispute_no_slash() {
    // Setup
    let mock_client = Arc::new(MockCurrencyChainClient::new());
    let accused_key = b"accused_validator";
    let claimant_key = b"claimant_validator";

    mock_client.set_stake(accused_key, 10000);
    mock_client.set_stake(claimant_key, 5000);

    let mut resolver = DisputeResolver::new()
        .with_currency_chain_client(mock_client.clone());

    let claim_id = resolver
        .submit_claim(
            DisputeType::ForkDetected,
            String::from_utf8(claimant_key.to_vec()).unwrap(),
            String::from_utf8(accused_key.to_vec()).unwrap(),
            b"fork_evidence".to_vec(),
        )
        .unwrap();

    // Skip to voting
    resolver.set_claim_status(&claim_id, DisputeStatus::UnderVote).unwrap();

    // Resolve with 50% vote (inconclusive)
    resolver.resolve_dispute(claim_id.clone(), 0.50).await.unwrap();

    // Verify no slashes occurred
    let slashes = mock_client.get_slashes();
    assert_eq!(slashes.len(), 0);

    let rewards = mock_client.get_rewards();
    assert_eq!(rewards.len(), 0);

    // Verify claim was dismissed
    let claim = resolver.get_claim(&claim_id).unwrap();
    assert_eq!(claim.status, DisputeStatus::Dismissed);
}

#[tokio::test]
async fn test_insufficient_stake_error() {
    // Setup
    let mock_client = Arc::new(MockCurrencyChainClient::new());
    let accused_key = b"accused_validator";
    let claimant_key = b"claimant_validator";

    // Set stake below minimum
    mock_client.set_stake(accused_key, 500); // Below min_dispute_stake of 1000
    mock_client.set_stake(claimant_key, 5000);

    let mut resolver = DisputeResolver::new()
        .with_slashing_config(SlashingConfig {
            base_slash_rate: 0.30,
            false_claim_multiplier: 1.5,
            claimant_reward_rate: 0.10,
            min_dispute_stake: 1000,
        })
        .with_currency_chain_client(mock_client.clone());

    let claim_id = resolver
        .submit_claim(
            DisputeType::ForkDetected,
            String::from_utf8(claimant_key.to_vec()).unwrap(),
            String::from_utf8(accused_key.to_vec()).unwrap(),
            b"fork_evidence".to_vec(),
        )
        .unwrap();

    // Skip to voting
    resolver.set_claim_status(&claim_id, DisputeStatus::UnderVote).unwrap();

    // Attempt to resolve - should fail
    let result = resolver.resolve_dispute(claim_id.clone(), 0.70).await;
    
    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Insufficient stake"));
}

#[tokio::test]
async fn test_no_currency_client_configured() {
    let mut resolver = DisputeResolver::new();

    let claim_id = resolver
        .submit_claim(
            DisputeType::ForkDetected,
            "claimant".to_string(),
            "accused".to_string(),
            b"evidence".to_vec(),
        )
        .unwrap();

    // Skip to voting
    resolver.set_claim_status(&claim_id, DisputeStatus::UnderVote).unwrap();

    // Attempt to resolve without client
    let result = resolver.resolve_dispute(claim_id, 0.70).await;

    assert!(result.is_err());
    assert!(result
        .unwrap_err()
        .to_string()
        .contains("Currency chain client not configured"));
}

#[test]
fn test_http_client_creation() {
    let client = HttpCurrencyChainClient::new("http://localhost:8545".to_string());
    // Should not panic
    drop(client);
}

#[test]
fn test_slashing_config_defaults() {
    let config = SlashingConfig::default();
    assert_eq!(config.base_slash_rate, 0.30);
    assert_eq!(config.false_claim_multiplier, 1.5);
    assert_eq!(config.claimant_reward_rate, 0.10);
    assert_eq!(config.min_dispute_stake, 1000);
}
