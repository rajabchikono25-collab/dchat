# Fee Distribution and Reward Accounting - Mainnet Implementation

## Summary

This document describes the mainnet-ready implementation of fee sinks, reward distribution, and burn accounting for dchat.

**Implementation Date**: December 2025  
**Status**: Ready for mainnet launch

---

## 1. Architecture Overview

### Fee Flow Diagram

```
                          ┌──────────────┐
                          │   User Fee   │
                          │   Payment    │
                          └──────┬───────┘
                                 │
                    ┌────────────┼────────────┐
                    │            │            │
              ┌─────▼─────┐ ┌────▼────┐ ┌─────▼─────┐
              │  Message  │ │Transfer │ │ Channel   │
              │   Fees    │ │  Fees   │ │   Fees    │
              └─────┬─────┘ └────┬────┘ └─────┬─────┘
                    │            │            │
                    │      ┌─────▼─────┐      │
                    │      │ 1% Burn   │      │
                    │      └─────┬─────┘      │
                    │            │            │
              ┌─────▼─────┐ ┌────▼────┐ ┌─────▼─────┐
              │  100% to  │ │  99%    │ │ 70/20/10  │
              │   Relay   │ │Recipient│ │   Split   │
              └───────────┘ └─────────┘ └─────┬─────┘
                                              │
                         ┌────────────────────┼────────────────────┐
                         │                    │                    │
                   ┌─────▼─────┐        ┌─────▼─────┐        ┌─────▼─────┐
                   │ Validator │        │   Relay   │        │ Treasury  │
                   │   Pool    │        │   Pool    │        │   Pool    │
                   │   (70%)   │        │   (20%)   │        │   (10%)   │
                   └───────────┘        └───────────┘        └───────────┘
```

### Key Principles

1. **Conservation of Value**: Every fee collected equals the sum of all distributions
2. **Deterministic Routing**: Fee destinations are algorithmically determined, not discretionary
3. **Consensus-Verifiable**: All fee distributions are committed to block state root
4. **Burn Isolation**: Burns only apply to user-to-user transfers, not service fees or rewards

---

## 2. Fee Types and Routing

| Fee Type             | Burn?    | Recipient                    | Notes                                   |
| -------------------- | -------- | ---------------------------- | --------------------------------------- |
| **Transfer Fee**     | Yes (1%) | 99% to recipient             | Standard user-to-user transfers         |
| **Message Fee**      | No       | 100% to relay                | Service fee for message delivery        |
| **Channel Creation** | No       | 70/20/10 split               | Split to validator/relay/treasury pools |
| **Marketplace**      | Yes (1%) | 90% to creator, 10% to pools | Creator economy support                 |
| **Storage**          | No       | 70/20/10 split               | Distributed storage micropayments       |
| **Premium Features** | No       | 70/20/10 split               | Premium tier subscriptions              |

### Split Ratios (from PRODUCTION_IMPROVEMENTS_ROADMAP.md)

- **Validators**: 70% (7000 basis points)
- **Relays**: 20% (2000 basis points)
- **Treasury**: 10% (1000 basis points)

---

## 3. Implementation Files

### New Files Created

| File                                                                                                       | Purpose                                                          |
| ---------------------------------------------------------------------------------------------------------- | ---------------------------------------------------------------- |
| [crates/dchat-blockchain/src/fee_distribution.rs](crates/dchat-blockchain/src/fee_distribution.rs)         | Core fee distribution logic, pool accounting, block fee tracking |
| [crates/dchat-blockchain/tests/conservation_tests.rs](crates/dchat-blockchain/tests/conservation_tests.rs) | Comprehensive conservation law tests                             |

### Modified Files

| File                                                                                                | Changes                                                                  |
| --------------------------------------------------------------------------------------------------- | ------------------------------------------------------------------------ |
| [crates/dchat-blockchain/src/lib.rs](crates/dchat-blockchain/src/lib.rs#L28)                        | Added fee_distribution module and exports                                |
| [crates/dchat-blockchain/src/currency_chain.rs](crates/dchat-blockchain/src/currency_chain.rs)      | Added FeeDistributionManager, `transfer_internal`, `collect_message_fee` |
| [crates/dchat-messaging/src/message_service.rs](crates/dchat-messaging/src/message_service.rs#L394) | Changed `deduct_fee` to use `collect_message_fee`                        |

---

## 4. API Reference

### FeeDistributionManager

```rust
/// Create fee distribution manager
pub fn new(config: FeeDistributionConfig) -> Self

/// Start new block for fee accounting
pub fn start_block(&self, block_height: u64, current_supply: u64)

/// Collect fee with proper distribution
pub fn collect_fee(
    &self,
    fee_type: FeeType,
    gross_amount: u64,
    payer: UserId,
    direct_recipient: Option<UserId>,
    tx_id: Uuid,
) -> Result<FeeCollectionRecord>

/// Get pool balance
pub fn get_pool_balance(&self, pool: &UserId) -> u64

/// Withdraw from pool for distribution
pub fn withdraw_from_pool(&self, pool: &UserId, amount: u64) -> Result<u64>

/// Verify conservation for current block
pub fn verify_current_block(&self) -> Result<()>
```

### CurrencyChainClient (New Methods)

```rust
/// Transfer tokens without burn (for rewards, refunds)
pub fn transfer_internal(
    &self,
    from: &UserId,
    to: &UserId,
    amount: u64,
    reason: &str,
) -> Result<Uuid>

/// Collect message fee (100% to relay, no burn)
pub fn collect_message_fee(
    &self,
    sender: &UserId,
    relay: &UserId,
    fee_amount: u64,
) -> Result<Uuid>

/// Get fee distribution manager
pub fn get_fee_distribution(&self) -> Option<&Arc<FeeDistributionManager>>
```

---

## 5. Protocol Sink Addresses

Deterministic well-known addresses for protocol accounting:

| Sink           | UUID                                   | Purpose                          |
| -------------- | -------------------------------------- | -------------------------------- |
| Treasury       | `00000000-0000-0000-0000-000000000001` | Protocol development, operations |
| Validator Pool | `00000000-0000-0000-0000-000000000002` | Block producer rewards           |
| Relay Pool     | `00000000-0000-0000-0000-000000000003` | Message relay rewards            |
| Burn Sink      | `00000000-0000-0000-0000-000000000004` | Burned token accounting          |

---

## 6. Consensus Verification

### BlockFeeAccounting Structure

Every block commits fee accounting data to state root:

```rust
pub struct BlockFeeAccounting {
    pub block_height: u64,
    pub timestamp: DateTime<Utc>,

    // Fee Totals
    pub total_fees_collected: u64,
    pub total_burned: u64,
    pub total_to_validator_pool: u64,
    pub total_to_relay_pool: u64,
    pub total_to_treasury: u64,
    pub total_direct_to_relays: u64,

    // Individual Records (for audit)
    pub fee_records: Vec<FeeCollectionRecord>,

    // Conservation Checks
    pub pre_block_supply: u64,
    pub post_block_supply: u64,

    // Merkle Commitment
    pub fee_records_merkle_root: [u8; 32],
}
```

### Verification

```rust
// Validators verify conservation at block end
fee_accounting.verify_conservation()?;

// Light clients verify via merkle proof
assert!(verify_merkle_proof(
    fee_record,
    fee_records_merkle_root,
    proof
));
```

---

## 7. Conservation Tests

The following invariants are tested:

1. **Fee Share Conservation**: `VALIDATOR_SHARE + RELAY_SHARE + TREASURY_SHARE = 10000 bps (100%)`
2. **Distribution Conservation**: For every fee, `burn + validator + relay + treasury + direct = gross_amount`
3. **Supply Conservation**: `post_block_supply = pre_block_supply - total_burned`
4. **Message Fee No-Burn**: Message fees have zero burn, 100% to relay
5. **Internal Transfer No-Burn**: Reward distributions have zero burn
6. **Pool Balance Accounting**: Pool credits match recorded amounts
7. **Withdrawal Accounting**: Pool debits are tracked correctly
8. **Minimum Amount Handling**: No rounding errors for small amounts
9. **Maximum Amount Handling**: No overflow for large amounts
10. **Merkle Root Determinism**: Same records produce same root

---

## 8. Migration Notes

### Breaking Changes

None. The new fee distribution is additive:

- Existing `transfer()` continues to work with burn
- New `collect_message_fee()` is now used by message_service
- New `transfer_internal()` available for reward payouts

### Upgrade Procedure

1. Deploy new binaries to all validators
2. Validators will automatically initialize FeeDistributionManager
3. Fee accounting begins from next block
4. No state migration required

---

## 9. Operational Considerations

### Pool Withdrawal for Distributions

Validators and relays claim rewards from respective pools:

```rust
// Validator claims from validator pool
let amount = fee_distribution.withdraw_from_pool(&sinks.validator_pool, claim_amount)?;
currency_chain.transfer_internal(&sinks.validator_pool, &validator_id, amount, "validator_reward")?;

// Relay claims from relay pool
let amount = fee_distribution.withdraw_from_pool(&sinks.relay_pool, claim_amount)?;
currency_chain.transfer_internal(&sinks.relay_pool, &relay_id, amount, "relay_reward")?;
```

### Monitoring

Key metrics to monitor:

- `dchat_fee_collected_total{type="message|transfer|channel"}` - Total fees collected
- `dchat_fee_burned_total` - Total tokens burned
- `dchat_pool_balance{pool="validator|relay|treasury"}` - Current pool balances
- `dchat_fee_conservation_violations` - Should always be 0

---

## 10. Security Considerations

1. **No External Access to Pools**: Pool sinks have deterministic UUIDs but no private keys; only protocol can withdraw
2. **Conservation Verified Per-Block**: Any conservation violation halts consensus
3. **Merkle Commitment**: Light clients can verify fee routing without full state
4. **No Burn on Service Fees**: Prevents economic attacks on relay operators
5. **Rounding to Treasury**: Any dust from rounding goes to treasury, never lost

---

## 11. Future Enhancements

1. **Dynamic Fee Adjustment**: Governance can adjust split ratios via proposal
2. **Burn Rate Adjustment**: Governance can adjust burn rate for economic tuning
3. **Pool Investment**: Treasury pool could earn yield via DeFi integrations
4. **Fee Delegation**: Users could delegate fee payments to sponsors

---

## Document History

| Date       | Version | Changes                        |
| ---------- | ------- | ------------------------------ |
| 2025-12-XX | 1.0     | Initial mainnet implementation |
