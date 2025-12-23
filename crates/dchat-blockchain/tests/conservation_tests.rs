//! Conservation tests for fee sinks and reward distribution
//!
//! These tests verify that:
//! 1. Token supply is conserved across all operations
//! 2. Fees are routed to correct sinks
//! 3. Burns are properly accounted for
//! 4. Reward distributions don't create/destroy tokens unexpectedly

use dchat_blockchain::fee_distribution::{
    FeeDistributionConfig, FeeDistributionManager, FeeType, ProtocolSinks, DEFAULT_BURN_RATE_BPS,
    INSURANCE_FUND_ALLOCATION_BPS, RELAY_FEE_SHARE_BPS, TREASURY_FEE_SHARE_BPS,
    VALIDATOR_FEE_SHARE_BPS,
};
use dchat_core::types::UserId;
use uuid::Uuid;

// =============================================================================
// CONSERVATION LAW TESTS
// =============================================================================

/// Test that fee distribution shares sum to 100%
#[test]
fn test_fee_share_conservation() {
    // All four shares must sum to exactly 100% (10000 bps)
    // Validator: 68% + Relay: 20% + Treasury: 10% + Insurance: 2% = 100%
    let total = VALIDATOR_FEE_SHARE_BPS
        + RELAY_FEE_SHARE_BPS
        + TREASURY_FEE_SHARE_BPS
        + INSURANCE_FUND_ALLOCATION_BPS;
    assert_eq!(
        total, 10000,
        "Fee shares must sum to 100% (10000 bps), got {}",
        total
    );
}

/// Test that fee distribution calculation conserves value
#[test]
fn test_fee_distribution_calculation_conservation() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

    // Test with various amounts
    for amount in [100, 1000, 10000, 100000, 1000000, 100000000u64] {
        // With burn - now returns 6-tuple (burn, validator, relay, treasury, insurance, direct)
        let (burn, validator, relay, treasury, insurance, direct) =
            manager.calculate_distribution(amount, true, None);

        let total = burn + validator + relay + treasury + insurance + direct;
        assert_eq!(
            total, amount,
            "Conservation violated for amount {} with burn: {} + {} + {} + {} + {} + {} = {}",
            amount, burn, validator, relay, treasury, insurance, direct, total
        );

        // Without burn
        let (burn_nb, validator_nb, relay_nb, treasury_nb, insurance_nb, direct_nb) =
            manager.calculate_distribution(amount, false, None);

        let total_nb = burn_nb + validator_nb + relay_nb + treasury_nb + insurance_nb + direct_nb;
        assert_eq!(
            total_nb, amount,
            "Conservation violated for amount {} without burn: {} + {} + {} + {} + {} + {} = {}",
            amount, burn_nb, validator_nb, relay_nb, treasury_nb, insurance_nb, direct_nb, total_nb
        );

        // With direct recipient (90%)
        let (burn_dr, validator_dr, relay_dr, treasury_dr, insurance_dr, direct_dr) =
            manager.calculate_distribution(amount, false, Some(9000));

        let total_dr = burn_dr + validator_dr + relay_dr + treasury_dr + insurance_dr + direct_dr;
        assert_eq!(
            total_dr, amount,
            "Conservation violated for amount {} with 90% direct: {} + {} + {} + {} + {} + {} = {}",
            amount, burn_dr, validator_dr, relay_dr, treasury_dr, insurance_dr, direct_dr, total_dr
        );
    }
}

/// Test that message fees are 100% to relay with no burn
#[test]
fn test_message_fee_no_burn() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    manager.start_block(1, 1_000_000);

    let payer = UserId(Uuid::new_v4());
    let relay = UserId(Uuid::new_v4());
    let fee_amount = 1000u64;

    let record = manager
        .collect_fee(
            FeeType::MessageFee,
            fee_amount,
            payer,
            Some(relay),
            Uuid::new_v4(),
        )
        .unwrap();

    assert_eq!(record.burn_amount, 0, "Message fees should NOT be burned");
    assert_eq!(
        record.direct_recipient_amount, fee_amount,
        "Relay should receive 100% of message fee"
    );
    assert_eq!(record.validator_share, 0);
    assert_eq!(record.relay_share, 0);
    assert_eq!(record.treasury_share, 0);
    assert_eq!(record.insurance_share, 0);

    // Verify conservation
    let total = record.burn_amount
        + record.validator_share
        + record.relay_share
        + record.treasury_share
        + record.insurance_share
        + record.direct_recipient_amount;
    assert_eq!(total, fee_amount);
}

/// Test that transfer fees have 1% burn and 70/20/10 split (with 2% insurance)
#[test]
fn test_transfer_fee_burn_and_split() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    manager.start_block(1, 1_000_000);

    let payer = UserId(Uuid::new_v4());
    let amount = 10000u64; // 10000 motes

    let record = manager
        .collect_fee(FeeType::TransferFee, amount, payer, None, Uuid::new_v4())
        .unwrap();

    // Expected: 1% burn = 100 motes
    let expected_burn = (amount * DEFAULT_BURN_RATE_BPS as u64) / 10000;
    assert_eq!(record.burn_amount, expected_burn, "Should burn 1%");

    let after_burn = amount - expected_burn; // 9900 motes

    // All shares calculated from after_burn (68% + 20% + 10% + 2% = 100%)
    let expected_validator = (after_burn * VALIDATOR_FEE_SHARE_BPS as u64) / 10000; // 68% = 6732
    let expected_relay = (after_burn * RELAY_FEE_SHARE_BPS as u64) / 10000; // 20% = 1980
    let expected_insurance = (after_burn * INSURANCE_FUND_ALLOCATION_BPS as u64) / 10000; // 2% = 198
                                                                                          // Treasury gets remainder: 10% = 990 (absorbs rounding dust)
    let expected_treasury = after_burn - expected_validator - expected_relay - expected_insurance;

    assert_eq!(
        record.validator_share, expected_validator,
        "Validator share mismatch"
    );
    assert_eq!(record.relay_share, expected_relay, "Relay share mismatch");
    assert_eq!(
        record.treasury_share, expected_treasury,
        "Treasury share mismatch"
    );
    assert_eq!(
        record.insurance_share, expected_insurance,
        "Insurance share mismatch"
    );
    assert_eq!(record.direct_recipient_amount, 0);

    // Verify conservation: all shares must sum to original amount
    let total = record.burn_amount
        + record.validator_share
        + record.relay_share
        + record.treasury_share
        + record.insurance_share
        + record.direct_recipient_amount;
    assert_eq!(total, amount, "Conservation violated");
}

/// Test pool balance accounting
#[test]
fn test_pool_balance_accounting() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    manager.start_block(1, 1_000_000);

    let sinks = manager.sinks().clone();
    let payer = UserId(Uuid::new_v4());

    // Initial balances should be zero
    assert_eq!(manager.get_pool_balance(&sinks.validator_pool), 0);
    assert_eq!(manager.get_pool_balance(&sinks.relay_pool), 0);
    assert_eq!(manager.get_pool_balance(&sinks.treasury), 0);
    assert_eq!(manager.get_pool_balance(&sinks.burn_sink), 0);

    // Collect a fee
    let amount = 10000u64;
    let record = manager
        .collect_fee(FeeType::TransferFee, amount, payer, None, Uuid::new_v4())
        .unwrap();

    // Verify pools were credited correctly
    assert_eq!(
        manager.get_pool_balance(&sinks.validator_pool),
        record.validator_share
    );
    assert_eq!(
        manager.get_pool_balance(&sinks.relay_pool),
        record.relay_share
    );
    assert_eq!(
        manager.get_pool_balance(&sinks.treasury),
        record.treasury_share
    );
    assert_eq!(
        manager.get_pool_balance(&sinks.burn_sink),
        record.burn_amount
    );
}

/// Test pool withdrawal
#[test]
fn test_pool_withdrawal() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    manager.start_block(1, 1_000_000);

    let sinks = manager.sinks().clone();
    let payer = UserId(Uuid::new_v4());

    // Collect several fees
    for _ in 0..10 {
        manager
            .collect_fee(
                FeeType::TransferFee,
                10000,
                payer.clone(),
                None,
                Uuid::new_v4(),
            )
            .unwrap();
    }

    let validator_balance = manager.get_pool_balance(&sinks.validator_pool);
    assert!(validator_balance > 0);

    // Withdraw half
    let withdraw_amount = validator_balance / 2;
    let withdrawn = manager
        .withdraw_from_pool(&sinks.validator_pool, withdraw_amount)
        .unwrap();

    assert_eq!(withdrawn, withdraw_amount);
    assert_eq!(
        manager.get_pool_balance(&sinks.validator_pool),
        validator_balance - withdraw_amount
    );

    // Try to withdraw more than available
    let result = manager.withdraw_from_pool(&sinks.validator_pool, validator_balance);
    assert!(
        result.is_err(),
        "Should fail when withdrawing more than balance"
    );
}

/// Test block fee accounting conservation
#[test]
fn test_block_fee_accounting_conservation() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    let initial_supply = 100_000_000_000u64;
    manager.start_block(1, initial_supply);

    let payer = UserId(Uuid::new_v4());

    // Collect various fees
    for i in 0..5 {
        let fee_type = match i % 3 {
            0 => FeeType::TransferFee,
            1 => FeeType::ChannelCreationFee,
            _ => FeeType::StorageFee,
        };
        manager
            .collect_fee(fee_type, 1000, payer.clone(), None, Uuid::new_v4())
            .unwrap();
    }

    // Verify block accounting conservation
    let result = manager.verify_current_block();
    assert!(
        result.is_ok(),
        "Block conservation check failed: {:?}",
        result
    );

    let accounting = manager.get_current_block_accounting();

    // Each record should be internally consistent
    for record in &accounting.fee_records {
        let sum = record.burn_amount
            + record.validator_share
            + record.relay_share
            + record.treasury_share
            + record.insurance_share
            + record.direct_recipient_amount;
        assert_eq!(
            sum, record.gross_amount,
            "Record {} violates conservation",
            record.id
        );
    }

    // Post-block supply should be pre - burned
    assert_eq!(
        accounting.post_block_supply,
        initial_supply.saturating_sub(accounting.total_burned)
    );
}

/// Test protocol sink address determinism
#[test]
fn test_protocol_sink_determinism() {
    let sinks1 = ProtocolSinks::default();
    let sinks2 = ProtocolSinks::default();

    // Sinks must be deterministic across instances
    assert_eq!(sinks1.treasury, sinks2.treasury);
    assert_eq!(sinks1.validator_pool, sinks2.validator_pool);
    assert_eq!(sinks1.relay_pool, sinks2.relay_pool);
    assert_eq!(sinks1.burn_sink, sinks2.burn_sink);
    assert_eq!(sinks1.insurance_fund, sinks2.insurance_fund);
    assert_eq!(sinks1.storage_bonds, sinks2.storage_bonds);

    // All sinks must be unique
    let mut all_sinks = std::collections::HashSet::new();
    assert!(all_sinks.insert(sinks1.treasury.clone()));
    assert!(all_sinks.insert(sinks1.validator_pool.clone()));
    assert!(all_sinks.insert(sinks1.relay_pool.clone()));
    assert!(all_sinks.insert(sinks1.burn_sink.clone()));
    assert!(all_sinks.insert(sinks1.insurance_fund.clone()));
    assert!(all_sinks.insert(sinks1.storage_bonds.clone()));
}

/// Test that sinks are recognized as protocol addresses
#[test]
fn test_is_protocol_sink() {
    let sinks = ProtocolSinks::default();
    let regular_user = UserId(Uuid::new_v4());

    assert!(sinks.is_protocol_sink(&sinks.treasury));
    assert!(sinks.is_protocol_sink(&sinks.validator_pool));
    assert!(sinks.is_protocol_sink(&sinks.relay_pool));
    assert!(sinks.is_protocol_sink(&sinks.burn_sink));
    assert!(sinks.is_protocol_sink(&sinks.insurance_fund));
    assert!(sinks.is_protocol_sink(&sinks.storage_bonds));
    assert!(!sinks.is_protocol_sink(&regular_user));
}

// =============================================================================
// INTEGRATION TESTS (require test-mocks feature)
// =============================================================================

#[cfg(feature = "test-mocks")]
mod integration_tests {
    use super::*;
    use dchat_blockchain::{CurrencyChainClient, CurrencyChainConfig};

    /// Test currency chain transfer with burn
    #[test]
    fn test_currency_chain_transfer_conservation() {
        let config = CurrencyChainConfig::default();
        let client = CurrencyChainClient::new_mock(config);

        let sender = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());
        let amount = 10000u64;

        // Credit sender with funds
        client.credit_for_testing(&sender, amount * 2);

        let sender_balance_before = client.get_balance(&sender).unwrap();
        let recipient_balance_before = client.get_balance(&recipient).unwrap();

        // Perform transfer
        let _tx_id = client.transfer(&sender, &recipient, amount).unwrap();

        let sender_balance_after = client.get_balance(&sender).unwrap();
        let recipient_balance_after = client.get_balance(&recipient).unwrap();

        // Sender should have lost exactly `amount`
        assert_eq!(sender_balance_after, sender_balance_before - amount);

        // Recipient should have received amount minus 1% burn
        let expected_burn = (amount * DEFAULT_BURN_RATE_BPS as u64) / 10000;
        let expected_received = amount - expected_burn;
        assert_eq!(
            recipient_balance_after,
            recipient_balance_before + expected_received
        );

        // The difference is the burned amount
        let total_before = sender_balance_before + recipient_balance_before;
        let total_after = sender_balance_after + recipient_balance_after;
        assert_eq!(total_before - total_after, expected_burn);
    }

    /// Test message fee collection with no burn
    #[test]
    fn test_message_fee_collection_no_burn() {
        let config = CurrencyChainConfig::default();
        let client = CurrencyChainClient::new_mock(config);

        let sender = UserId(Uuid::new_v4());
        let relay = UserId(Uuid::new_v4());
        let fee = 1000u64;

        // Credit sender with funds
        client.credit_for_testing(&sender, fee * 2);

        let sender_balance_before = client.get_balance(&sender).unwrap();
        let relay_balance_before = client.get_balance(&relay).unwrap();

        // Collect message fee
        let _tx_id = client.collect_message_fee(&sender, &relay, fee).unwrap();

        let sender_balance_after = client.get_balance(&sender).unwrap();
        let relay_balance_after = client.get_balance(&relay).unwrap();

        // Sender loses exactly fee
        assert_eq!(sender_balance_after, sender_balance_before - fee);

        // Relay receives exactly fee (NO BURN)
        assert_eq!(relay_balance_after, relay_balance_before + fee);

        // Total unchanged (no burn)
        let total_before = sender_balance_before + relay_balance_before;
        let total_after = sender_balance_after + relay_balance_after;
        assert_eq!(total_before, total_after, "Message fees should not burn");
    }

    /// Test internal transfer (rewards) with no burn
    #[test]
    fn test_internal_transfer_no_burn() {
        let config = CurrencyChainConfig::default();
        let client = CurrencyChainClient::new_mock(config);

        let pool = UserId(Uuid::new_v4());
        let recipient = UserId(Uuid::new_v4());
        let amount = 5000u64;

        // Credit pool with funds
        client.credit_for_testing(&pool, amount * 2);

        let pool_balance_before = client.get_balance(&pool).unwrap();
        let recipient_balance_before = client.get_balance(&recipient).unwrap();

        // Internal transfer (reward distribution)
        let _tx_id = client
            .transfer_internal(&pool, &recipient, amount, "test_reward")
            .unwrap();

        let pool_balance_after = client.get_balance(&pool).unwrap();
        let recipient_balance_after = client.get_balance(&recipient).unwrap();

        // Pool loses exactly amount
        assert_eq!(pool_balance_after, pool_balance_before - amount);

        // Recipient receives exactly amount (NO BURN)
        assert_eq!(recipient_balance_after, recipient_balance_before + amount);

        // Total unchanged (no burn)
        let total_before = pool_balance_before + recipient_balance_before;
        let total_after = pool_balance_after + recipient_balance_after;
        assert_eq!(
            total_before, total_after,
            "Internal transfers should not burn"
        );
    }
}

// =============================================================================
// EDGE CASE TESTS
// =============================================================================

/// Test with minimum amounts (rounding edge cases)
#[test]
fn test_minimum_amount_conservation() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

    // Test with amounts that might cause rounding issues
    for amount in [1u64, 2, 3, 7, 11, 13, 97, 99, 100, 101] {
        let (burn, validator, relay, treasury, insurance, direct) =
            manager.calculate_distribution(amount, true, None);

        let total = burn + validator + relay + treasury + insurance + direct;
        assert_eq!(
            total, amount,
            "Conservation violated for small amount {}: total={}",
            amount, total
        );
    }
}

/// Test with maximum amounts (overflow protection)
#[test]
fn test_maximum_amount_conservation() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());

    // Test with large amounts
    let large_amounts = [
        u64::MAX / 10000,        // Safe for bps multiplication
        1_000_000_000_000,       // 1 trillion motes
        100_000_000_000_000_000, // Large but safe
    ];

    for amount in large_amounts {
        let (burn, validator, relay, treasury, insurance, direct) =
            manager.calculate_distribution(amount, true, None);

        let total = burn + validator + relay + treasury + insurance + direct;
        assert_eq!(
            total, amount,
            "Conservation violated for large amount {}: total={}",
            amount, total
        );
    }
}

/// Test merkle root computation
#[test]
fn test_merkle_root_determinism() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    manager.start_block(1, 1_000_000);

    let payer = UserId(Uuid::new_v4());

    // Collect some fees
    for _ in 0..5 {
        manager
            .collect_fee(
                FeeType::TransferFee,
                1000,
                payer.clone(),
                None,
                Uuid::new_v4(),
            )
            .unwrap();
    }

    // Get accounting and compute merkle root
    let mut accounting = manager.get_current_block_accounting();
    accounting.compute_merkle_root();

    // Merkle root should be non-zero for non-empty records
    assert_ne!(accounting.fee_records_merkle_root, [0u8; 32]);

    // Recompute should give same result
    let merkle1 = accounting.fee_records_merkle_root;
    accounting.compute_merkle_root();
    let merkle2 = accounting.fee_records_merkle_root;
    assert_eq!(merkle1, merkle2, "Merkle root should be deterministic");
}

/// Test empty block accounting
#[test]
fn test_empty_block_accounting() {
    let manager = FeeDistributionManager::new(FeeDistributionConfig::default());
    let initial_supply = 100_000_000_000u64;
    manager.start_block(1, initial_supply);

    // No fees collected

    // Conservation should still hold
    let result = manager.verify_current_block();
    assert!(result.is_ok());

    let accounting = manager.get_current_block_accounting();
    assert_eq!(accounting.total_fees_collected, 0);
    assert_eq!(accounting.total_burned, 0);
    assert_eq!(accounting.pre_block_supply, initial_supply);
    assert_eq!(accounting.post_block_supply, initial_supply);
}
