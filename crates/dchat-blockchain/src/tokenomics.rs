//! Tokenomics System
//!
//! Complete token lifecycle management including:
//! - Token minting (fixed/variable supply)
//! - Token distribution mechanisms
//! - Marketplace liquidity pools
//! - Token rewards and incentives
//! - Supply management and economics

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Token supply configuration
///
/// All amounts are in motes (smallest unit of DCHAT).
/// 1 DCHAT = 100,000,000 motes (8 decimal places).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenSupplyConfig {
    /// Initial token supply at genesis (in motes)
    pub initial_supply: u64,
    /// Maximum supply cap in motes (None = unlimited)
    pub max_supply: Option<u64>,
    /// Inflation rate per block (in basis points, 100 = 1%)
    pub inflation_rate_bps: u16,
    /// Block interval for inflation (seconds)
    pub inflation_interval_seconds: u64,
    /// Deflation burn rate (basis points)
    pub burn_rate_bps: u16,
}

impl Default for TokenSupplyConfig {
    fn default() -> Self {
        // DCHAT uses 8 decimals: 1 DCHAT = 100_000_000 motes
        // These are whole token counts; multiply by MOTES_PER_DCHAT for motes
        Self {
            initial_supply: 100_000_000_000, // 100 billion DCHAT (whole tokens)
            max_supply: Some(1_000_000_000_000), // 1 trillion DCHAT cap (whole tokens)
            inflation_rate_bps: 500,         // 5% annual
            inflation_interval_seconds: 15,  // per block
            burn_rate_bps: 100,              // 1% of transactions burned
        }
    }
}

/// Minting event record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintEvent {
    pub id: Uuid,
    pub amount: u64,
    pub reason: MintReason,
    pub recipient: Option<UserId>,
    pub block_height: u64,
    pub timestamp: DateTime<Utc>,
}

/// Reason for token minting
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MintReason {
    /// Genesis block initial distribution
    Genesis,
    /// Block reward for validators
    BlockReward,
    /// Relay node rewards
    RelayReward,
    /// Inflation per block
    Inflation,
    /// Marketplace liquidity injection
    MarketplaceLiquidity,
    /// Airdrop or distribution event
    Airdrop,
    /// Governance proposal reward
    GovernanceReward,
    /// Faucet drip for testnet/development
    Faucet,
}

/// Token burn event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnEvent {
    pub id: Uuid,
    pub amount: u64,
    pub reason: BurnReason,
    pub burner: UserId,
    pub block_height: u64,
    pub timestamp: DateTime<Utc>,
}

/// Reason for token burning
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BurnReason {
    /// Transaction fee burn
    TransactionFee,
    /// Deflationary burn from circulation
    Deflation,
    /// Penalty/slashing burn
    Slash,
    /// Voluntary burn for scarcity
    VoluntaryBurn,
}

/// Marketplace liquidity pool
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LiquidityPool {
    pub id: Uuid,
    pub name: String,
    pub total_tokens: u64,
    pub available_tokens: u64,
    pub reserved_tokens: u64,
    /// Tokens allocated but not yet distributed
    pub pending_allocations: u64,
    pub created_at: DateTime<Utc>,
    pub last_replenish: DateTime<Utc>,
}

/// Token distribution mechanism
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DistributionSchedule {
    pub id: Uuid,
    pub recipient_type: RecipientType,
    pub amount_per_interval: u64,
    pub interval_blocks: u64,
    pub start_block: u64,
    pub end_block: Option<u64>,
    pub total_distributed: u64,
}

/// Type of recipient for distributions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum RecipientType {
    /// All validators proportional to stake
    Validators,
    /// All relay nodes proportional to uptime
    RelayNodes,
    /// Marketplace liquidity pool
    MarketplaceLiquidity,
    /// Treasury/governance fund
    Treasury,
    /// Development fund
    DevelopmentFund,
}

/// Tokenomics manager - handles all token lifecycle operations
pub struct TokenomicsManager {
    config: TokenSupplyConfig,
    /// Current circulating supply
    circulating_supply: Arc<RwLock<u64>>,
    /// Total burned tokens
    total_burned: Arc<RwLock<u64>>,
    /// Mint events log
    mint_history: Arc<RwLock<Vec<MintEvent>>>,
    /// Burn events log
    burn_history: Arc<RwLock<Vec<BurnEvent>>>,
    /// Liquidity pools for marketplace
    liquidity_pools: Arc<RwLock<HashMap<Uuid, LiquidityPool>>>,
    /// Distribution schedules
    distribution_schedules: Arc<RwLock<Vec<DistributionSchedule>>>,
    /// Current block height
    current_block: Arc<RwLock<u64>>,
}

impl TokenomicsManager {
    /// Create new tokenomics manager
    pub fn new(config: TokenSupplyConfig) -> Self {
        let initial_supply = config.initial_supply;
        Self {
            config,
            circulating_supply: Arc::new(RwLock::new(initial_supply)),
            total_burned: Arc::new(RwLock::new(0)),
            mint_history: Arc::new(RwLock::new(Vec::new())),
            burn_history: Arc::new(RwLock::new(Vec::new())),
            liquidity_pools: Arc::new(RwLock::new(HashMap::new())),
            distribution_schedules: Arc::new(RwLock::new(Vec::new())),
            current_block: Arc::new(RwLock::new(1)),
        }
    }

    /// Initialize genesis distribution
    pub fn genesis_mint(&self, recipients: Vec<(UserId, u64)>) -> Result<Vec<Uuid>> {
        let mut mint_ids = Vec::new();
        let current_block = *self.current_block.read().unwrap();

        for (recipient, amount) in recipients {
            let event = MintEvent {
                id: Uuid::new_v4(),
                amount,
                reason: MintReason::Genesis,
                recipient: Some(recipient),
                block_height: current_block,
                timestamp: Utc::now(),
            };
            mint_ids.push(event.id);
            self.mint_history.write().unwrap().push(event);
        }

        Ok(mint_ids)
    }

    /// Mint tokens (respects max supply cap)
    pub fn mint_tokens(
        &self,
        amount: u64,
        reason: MintReason,
        recipient: Option<UserId>,
    ) -> Result<Uuid> {
        let mut supply = self.circulating_supply.write().unwrap();

        // Check max supply cap
        if let Some(max) = self.config.max_supply {
            if *supply + amount > max {
                return Err(Error::InvalidInput(format!(
                    "Minting {} tokens would exceed max supply of {}",
                    amount, max
                )));
            }
        }

        *supply += amount;
        let current_block = *self.current_block.read().unwrap();

        let event = MintEvent {
            id: Uuid::new_v4(),
            amount,
            reason,
            recipient,
            block_height: current_block,
            timestamp: Utc::now(),
        };

        let event_id = event.id;
        self.mint_history.write().unwrap().push(event);

        Ok(event_id)
    }

    /// Burn tokens (deflationary mechanism)
    pub fn burn_tokens(&self, amount: u64, reason: BurnReason, burner: UserId) -> Result<Uuid> {
        let mut supply = self.circulating_supply.write().unwrap();

        if *supply < amount {
            return Err(Error::InvalidInput(format!(
                "Cannot burn {} tokens, only {} in circulation",
                amount, *supply
            )));
        }

        *supply -= amount;
        let mut burned = self.total_burned.write().unwrap();
        *burned += amount;

        let current_block = *self.current_block.read().unwrap();

        let event = BurnEvent {
            id: Uuid::new_v4(),
            amount,
            reason,
            burner,
            block_height: current_block,
            timestamp: Utc::now(),
        };

        let event_id = event.id;
        self.burn_history.write().unwrap().push(event);

        Ok(event_id)
    }

    /// Create marketplace liquidity pool
    pub fn create_liquidity_pool(&self, name: String, initial_tokens: u64) -> Result<Uuid> {
        // Mint tokens for liquidity pool
        self.mint_tokens(initial_tokens, MintReason::MarketplaceLiquidity, None)?;

        let pool = LiquidityPool {
            id: Uuid::new_v4(),
            name,
            total_tokens: initial_tokens,
            available_tokens: initial_tokens,
            reserved_tokens: 0,
            pending_allocations: 0,
            created_at: Utc::now(),
            last_replenish: Utc::now(),
        };

        let pool_id = pool.id;
        self.liquidity_pools.write().unwrap().insert(pool_id, pool);

        Ok(pool_id)
    }

    /// Allocate tokens from liquidity pool (for marketplace sales)
    pub fn allocate_from_pool(&self, pool_id: &Uuid, amount: u64) -> Result<()> {
        let mut pools = self.liquidity_pools.write().unwrap();
        let pool = pools
            .get_mut(pool_id)
            .ok_or_else(|| Error::NotFound("Liquidity pool not found".to_string()))?;

        if pool.available_tokens < amount {
            return Err(Error::InvalidInput(format!(
                "Insufficient liquidity: need {}, have {}",
                amount, pool.available_tokens
            )));
        }

        pool.available_tokens -= amount;
        pool.reserved_tokens += amount;
        pool.pending_allocations += amount;

        Ok(())
    }

    /// Release allocated tokens (complete sale)
    pub fn release_allocation(&self, pool_id: &Uuid, amount: u64) -> Result<()> {
        let mut pools = self.liquidity_pools.write().unwrap();
        let pool = pools
            .get_mut(pool_id)
            .ok_or_else(|| Error::NotFound("Liquidity pool not found".to_string()))?;

        if pool.pending_allocations < amount {
            return Err(Error::InvalidInput(
                "Invalid allocation release".to_string(),
            ));
        }

        pool.pending_allocations -= amount;
        pool.reserved_tokens -= amount;

        Ok(())
    }

    /// Replenish liquidity pool (from inflation or treasury)
    pub fn replenish_pool(&self, pool_id: &Uuid, amount: u64) -> Result<()> {
        // Mint new tokens for replenishment
        self.mint_tokens(amount, MintReason::MarketplaceLiquidity, None)?;

        let mut pools = self.liquidity_pools.write().unwrap();
        let pool = pools
            .get_mut(pool_id)
            .ok_or_else(|| Error::NotFound("Liquidity pool not found".to_string()))?;

        pool.total_tokens += amount;
        pool.available_tokens += amount;
        pool.last_replenish = Utc::now();

        Ok(())
    }

    /// Create distribution schedule
    pub fn create_distribution_schedule(
        &self,
        recipient_type: RecipientType,
        amount_per_interval: u64,
        interval_blocks: u64,
        duration_blocks: Option<u64>,
    ) -> Result<Uuid> {
        let current_block = *self.current_block.read().unwrap();
        let end_block = duration_blocks.map(|d| current_block + d);

        let schedule = DistributionSchedule {
            id: Uuid::new_v4(),
            recipient_type,
            amount_per_interval,
            interval_blocks,
            start_block: current_block,
            end_block,
            total_distributed: 0,
        };

        let schedule_id = schedule.id;
        self.distribution_schedules.write().unwrap().push(schedule);

        Ok(schedule_id)
    }

    /// Process inflation for current block
    pub fn process_block_inflation(&self) -> Result<Vec<Uuid>> {
        let current_block = *self.current_block.read().unwrap();
        let mut mint_ids = Vec::new();

        // Calculate inflation amount per block
        let supply = *self.circulating_supply.read().unwrap();
        let annual_inflation = (supply as f64 * self.config.inflation_rate_bps as f64) / 10000.0;
        let blocks_per_year = (365 * 24 * 3600) / self.config.inflation_interval_seconds;
        let inflation_per_block = (annual_inflation / blocks_per_year as f64) as u64;

        if inflation_per_block > 0 {
            let mint_id = self.mint_tokens(inflation_per_block, MintReason::Inflation, None)?;
            mint_ids.push(mint_id);
        }

        // Process distribution schedules
        let mut schedules = self.distribution_schedules.write().unwrap();
        for schedule in schedules.iter_mut() {
            if current_block < schedule.start_block {
                continue;
            }
            if let Some(end) = schedule.end_block {
                if current_block > end {
                    continue;
                }
            }

            // Check if this block should trigger distribution
            if (current_block - schedule.start_block).is_multiple_of(schedule.interval_blocks) {
                let mint_id = self.mint_tokens(
                    schedule.amount_per_interval,
                    match schedule.recipient_type {
                        RecipientType::Validators => MintReason::BlockReward,
                        RecipientType::RelayNodes => MintReason::RelayReward,
                        RecipientType::MarketplaceLiquidity => MintReason::MarketplaceLiquidity,
                        _ => MintReason::Inflation,
                    },
                    None,
                )?;
                schedule.total_distributed += schedule.amount_per_interval;
                mint_ids.push(mint_id);
            }
        }

        Ok(mint_ids)
    }

    /// Get current circulating supply
    pub fn get_circulating_supply(&self) -> u64 {
        *self.circulating_supply.read().unwrap()
    }

    /// Get total burned tokens
    pub fn get_total_burned(&self) -> u64 {
        *self.total_burned.read().unwrap()
    }

    /// Get effective supply (circulating - burned)
    pub fn get_effective_supply(&self) -> u64 {
        self.get_circulating_supply()
    }

    /// Get liquidity pool status
    pub fn get_pool(&self, pool_id: &Uuid) -> Option<LiquidityPool> {
        self.liquidity_pools.read().unwrap().get(pool_id).cloned()
    }

    /// Get all liquidity pools
    pub fn get_all_pools(&self) -> Vec<LiquidityPool> {
        self.liquidity_pools
            .read()
            .unwrap()
            .values()
            .cloned()
            .collect()
    }

    /// Get mint history
    pub fn get_mint_history(&self, limit: usize) -> Vec<MintEvent> {
        let history = self.mint_history.read().unwrap();
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Get burn history
    pub fn get_burn_history(&self, limit: usize) -> Vec<BurnEvent> {
        let history = self.burn_history.read().unwrap();
        history.iter().rev().take(limit).cloned().collect()
    }

    /// Record a mint event for accounting purposes
    /// This is used when tokens are minted via other mechanisms (e.g., CurrencyChainClient)
    /// but need to be tracked in tokenomics
    pub fn record_mint(&self, amount: u64, reason: MintReason) -> Result<()> {
        // Update circulating supply
        {
            let mut supply = self.circulating_supply.write().unwrap();
            *supply = supply.saturating_add(amount);
        }

        // Record mint event
        let event = MintEvent {
            id: Uuid::new_v4(),
            amount,
            reason,
            recipient: None, // External mint doesn't have a specific recipient here
            timestamp: chrono::Utc::now(),
            block_height: *self.current_block.read().unwrap(),
        };
        self.mint_history.write().unwrap().push(event);

        Ok(())
    }

    /// Get tokenomics statistics
    pub fn get_statistics(&self) -> TokenomicsStats {
        let supply = *self.circulating_supply.read().unwrap();
        let burned = *self.total_burned.read().unwrap();
        let mint_history = self.mint_history.read().unwrap();
        let _burn_history = self.burn_history.read().unwrap();
        let pools = self.liquidity_pools.read().unwrap();

        let total_minted: u64 = mint_history.iter().map(|e| e.amount).sum();
        let total_pool_liquidity: u64 = pools.values().map(|p| p.total_tokens).sum();

        TokenomicsStats {
            circulating_supply: supply,
            total_minted,
            total_burned: burned,
            effective_supply: supply,
            max_supply: self.config.max_supply,
            inflation_rate_bps: self.config.inflation_rate_bps,
            burn_rate_bps: self.config.burn_rate_bps,
            total_pool_liquidity,
            active_pools: pools.len() as u64,
        }
    }

    /// Advance block (for simulation and testing)
    pub fn advance_block(&self) -> Result<()> {
        let mut block = self.current_block.write().unwrap();
        *block += 1;
        Ok(())
    }

    /// Get current block height
    pub fn get_current_block(&self) -> u64 {
        *self.current_block.read().unwrap()
    }
}

/// Tokenomics statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenomicsStats {
    pub circulating_supply: u64,
    pub total_minted: u64,
    pub total_burned: u64,
    pub effective_supply: u64,
    pub max_supply: Option<u64>,
    pub inflation_rate_bps: u16,
    pub burn_rate_bps: u16,
    pub total_pool_liquidity: u64,
    pub active_pools: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_token_minting() {
        let config = TokenSupplyConfig::default();
        let manager = TokenomicsManager::new(config);

        let initial = manager.get_circulating_supply();
        assert_eq!(initial, 100_000_000_000);

        let user = UserId(Uuid::new_v4());
        let mint_id = manager
            .mint_tokens(1000, MintReason::BlockReward, Some(user))
            .unwrap();
        assert!(!mint_id.is_nil());

        let new_supply = manager.get_circulating_supply();
        assert_eq!(new_supply, initial + 1000);
    }

    #[test]
    fn test_max_supply_cap() {
        let mut config = TokenSupplyConfig::default();
        config.max_supply = Some(100_000_001_000);
        let manager = TokenomicsManager::new(config);

        // Should succeed (within cap)
        manager
            .mint_tokens(500, MintReason::Inflation, None)
            .unwrap();

        // Should fail (exceeds cap)
        let result = manager.mint_tokens(1000, MintReason::Inflation, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_token_burning() {
        let config = TokenSupplyConfig::default();
        let manager = TokenomicsManager::new(config);

        let initial = manager.get_circulating_supply();
        let user = UserId(Uuid::new_v4());

        manager
            .burn_tokens(1000, BurnReason::TransactionFee, user)
            .unwrap();

        let new_supply = manager.get_circulating_supply();
        assert_eq!(new_supply, initial - 1000);

        let burned = manager.get_total_burned();
        assert_eq!(burned, 1000);
    }

    #[test]
    fn test_liquidity_pool() {
        let config = TokenSupplyConfig::default();
        let manager = TokenomicsManager::new(config);

        let pool_id = manager
            .create_liquidity_pool("Marketplace".to_string(), 10_000_000)
            .unwrap();

        let pool = manager.get_pool(&pool_id).unwrap();
        assert_eq!(pool.total_tokens, 10_000_000);
        assert_eq!(pool.available_tokens, 10_000_000);

        // Allocate some tokens
        manager.allocate_from_pool(&pool_id, 1000).unwrap();

        let pool = manager.get_pool(&pool_id).unwrap();
        assert_eq!(pool.available_tokens, 9_999_000);
        assert_eq!(pool.reserved_tokens, 1000);
    }

    #[test]
    fn test_distribution_schedule() {
        let config = TokenSupplyConfig::default();
        let manager = TokenomicsManager::new(config);

        let schedule_id = manager
            .create_distribution_schedule(RecipientType::Validators, 1000, 100, Some(1000))
            .unwrap();

        assert!(!schedule_id.is_nil());
    }
}
