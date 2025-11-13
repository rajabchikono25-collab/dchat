# Slashing Implementation - Production Ready

## Overview

This document describes the complete slashing implementation for dchat's dispute resolution system, integrating cryptographic dispute resolution with currency chain stake enforcement.

## Architecture

### Components

1. **DisputeResolver** (`dchat-chain/src/dispute_resolution.rs`)
   - Core dispute resolution logic with fork verification
   - Integrates with currency chain for stake enforcement
   - Records slashing events for transparency

2. **CurrencyChainClient** (trait)
   - Abstract interface for currency chain operations
   - Operations: query stake, execute slash, transfer rewards
   - Allows mock implementations for testing

3. **HttpCurrencyChainClient** (`dchat-chain/src/currency_chain_client.rs`)
   - HTTP JSON-RPC client implementation
   - Connects to currency chain validators
   - Configurable via `CURRENCY_CHAIN_RPC` environment variable

4. **SlashingConfig**
   - Configurable slashing parameters
   - Default: 30% base rate, 45% for false claims
   - 10% reward to successful claimants

## Slashing Flow

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Dispute Resolution Vote                                  │
│    - Governance votes on dispute claim                      │
│    - Threshold: 66% vote required for slash                 │
└─────────────────┬───────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Stake Query                                              │
│    - Query accused/claimant stake from currency chain       │
│    - Validate minimum stake requirements                    │
└─────────────────┬───────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Slash Calculation                                        │
│    - Calculate slash amount based on offense               │
│    - Accused: base_slash_rate (30%)                        │
│    - False claimant: base_rate * multiplier (45%)          │
└─────────────────┬───────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Execute Slash Transaction                                │
│    - Submit slash transaction to currency chain             │
│    - Reduce validator's staked amount                       │
│    - Record transaction ID                                  │
└─────────────────┬───────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Reward Distribution                                      │
│    - Transfer reward to successful party                    │
│    - Reward: 10% of slashed amount                          │
│    - Remaining: DAO treasury/insurance fund                 │
└─────────────────┬───────────────────────────────────────────┘
                  │
                  ▼
┌─────────────────────────────────────────────────────────────┐
│ 6. Event Recording                                          │
│    - Record SlashingEvent with full details                 │
│    - Transparency: tx ID, amounts, parties, timestamps      │
└─────────────────────────────────────────────────────────────┘
```

## Implementation Details

### Slashing Rates

```rust
SlashingConfig {
    base_slash_rate: 0.30,        // 30% for resolved disputes
    false_claim_multiplier: 1.5,  // 45% for false claims (30% * 1.5)
    claimant_reward_rate: 0.10,   // 10% reward
    min_dispute_stake: 1000,      // Minimum 1000 tokens
}
```

### Vote Thresholds

- **Slash Accused**: ≥66% vote for claimant
- **Slash Claimant**: ≤34% vote for claimant (false claim)
- **Dismiss**: Between 34% and 66% (inconclusive)

### Currency Chain RPC Methods

```json
// Query stake
{
  "method": "currency.query_validator_stake",
  "params": {
    "validator_key": "hex_encoded_key"
  }
}

// Execute slash
{
  "method": "currency.slash_validator_stake",
  "params": {
    "validator_key": "hex_encoded_key",
    "slash_amount": 3000,
    "reason": "Dispute resolved against accused"
  }
}

// Transfer reward
{
  "method": "currency.transfer_reward",
  "params": {
    "recipient_key": "hex_encoded_key",
    "amount": 300,
    "memo": "Dispute resolution reward"
  }
}
```

## Usage Example

```rust
use dchat_chain::{
    DisputeResolver, DisputeType, HttpCurrencyChainClient, 
    SlashingConfig
};
use std::sync::Arc;

// Create currency chain client
let currency_client = Arc::new(
    HttpCurrencyChainClient::from_env()
);

// Create resolver with slashing enabled
let mut resolver = DisputeResolver::new()
    .with_slashing_config(SlashingConfig::default())
    .with_currency_chain_client(currency_client);

// Submit dispute claim
let claim_id = resolver.submit_claim(
    DisputeType::ForkDetected,
    "alice".to_string(),
    "bob".to_string(),
    fork_evidence,
)?;

// ... challenge, respond, vote ...

// Resolve dispute (triggers slashing if vote exceeds threshold)
resolver.resolve_dispute(claim_id, 0.70).await?;

// Query slashing events
let events = resolver.get_slashing_events();
for event in events {
    println!(
        "Slashed {} tokens from {} (tx: {})",
        event.slash_amount,
        event.slashed_party,
        event.transaction_id
    );
}
```

## Security Considerations

### 1. Minimum Stake Enforcement

Participants must maintain minimum stake to prevent frivolous claims:
```rust
if original_stake < min_dispute_stake {
    return Err("Insufficient stake");
}
```

### 2. False Claim Deterrent

Higher penalty for false claims (45% vs 30%) creates skin-in-the-game:
```rust
let false_claim_rate = base_slash_rate * false_claim_multiplier;
```

### 3. Transaction Atomicity

Slash execution records transaction ID for audit trail:
- If slash fails, no reward is distributed
- Events contain full transaction details
- Replay protection via currency chain

### 4. Reward Distribution

Rewards incentivize honest reporting:
- 10% of slashed amount goes to successful party
- Remaining 90% to DAO treasury/insurance fund
- Prevents reward farming (only on valid claims)

## Testing

### Unit Tests

Located in `dchat-chain/tests/slashing_integration_test.rs`:

1. **test_slash_accused_after_vote**
   - Verifies slash execution when vote ≥66%
   - Checks stake reduction and reward distribution
   - Validates event recording

2. **test_slash_claimant_for_false_claim**
   - Verifies false claim penalty (45%)
   - Checks reward to accused party
   - Validates higher penalty rate

3. **test_inconclusive_dispute_no_slash**
   - Verifies no slash on 50% vote
   - Checks claim dismissed status
   - Ensures no transactions

4. **test_insufficient_stake_error**
   - Verifies minimum stake enforcement
   - Checks error handling
   - Prevents low-stake manipulation

5. **test_no_currency_client_configured**
   - Verifies error when client missing
   - Tests graceful failure mode

### Running Tests

```bash
# All tests
cargo test --package dchat-chain

# Slashing tests only
cargo test --package dchat-chain slashing_integration

# With logging
RUST_LOG=debug cargo test --package dchat-chain slashing_integration -- --nocapture
```

## Configuration

### Environment Variables

```bash
# Currency chain RPC endpoint
export CURRENCY_CHAIN_RPC="http://localhost:8545"

# For production
export CURRENCY_CHAIN_RPC="https://currency-chain.dchat.network:8545"
```

### Custom Configuration

```rust
let config = SlashingConfig {
    base_slash_rate: 0.25,        // 25% instead of 30%
    false_claim_multiplier: 2.0,  // 50% for false claims
    claimant_reward_rate: 0.15,   // 15% reward
    min_dispute_stake: 5000,      // Higher minimum
};

let resolver = DisputeResolver::new()
    .with_slashing_config(config);
```

## Integration Points

### 1. Governance Module

```rust
// After governance vote completes
let vote_result = governance.get_vote_result(claim_id)?;
resolver.resolve_dispute(claim_id, vote_result.approval_rate).await?;
```

### 2. Reputation System

```rust
// Update reputation after slashing
for event in resolver.get_slashing_events() {
    reputation_manager.apply_penalty(
        &event.slashed_party,
        event.slash_amount
    )?;
}
```

### 3. Event Broadcasting

```rust
// Broadcast slashing events to network
for event in resolver.get_slashing_events() {
    event_bus.publish(
        "slashing.executed",
        serde_json::to_vec(&event)?
    )?;
}
```

## Error Handling

### Common Errors

1. **Currency chain client not configured**
   ```rust
   Error: "Currency chain client not configured"
   Solution: Call .with_currency_chain_client()
   ```

2. **Insufficient stake**
   ```rust
   Error: "Insufficient stake: 500 < 1000"
   Solution: Ensure validators meet minimum stake
   ```

3. **RPC connection failure**
   ```rust
   Error: "RPC request failed: connection refused"
   Solution: Check CURRENCY_CHAIN_RPC endpoint
   ```

4. **Transaction failed**
   ```rust
   Error: "RPC error -32000: insufficient funds"
   Solution: Ensure validator has sufficient stake
   ```

## Performance Considerations

### 1. RPC Call Optimization

Batch operations when possible:
```rust
// Query multiple stakes
let stakes = futures::future::try_join_all(
    validators.iter().map(|v| client.get_validator_stake(v))
).await?;
```

### 2. Event Storage

Limit in-memory event storage:
```rust
// Prune old events
if self.slashing_events.len() > MAX_EVENTS {
    self.slashing_events.drain(0..100);
}
```

### 3. Async Operations

All currency chain operations are async:
- Non-blocking slash execution
- Concurrent stake queries
- Parallel reward distribution

## Monitoring

### Key Metrics

1. **Slash Rate**: Track slashes per day
2. **False Claim Rate**: Monitor frivolous claims
3. **Average Slash Amount**: Detect anomalies
4. **Transaction Success Rate**: Currency chain health
5. **Event Processing Lag**: System performance

### Prometheus Metrics

```rust
// Track slash executions
slash_executions_total.inc();
slash_amount_total.add(slash_amount as f64);

// Track false claims
false_claim_penalties_total.inc();

// Track rewards
rewards_distributed_total.add(reward_amount as f64);
```

## Production Checklist

- [x] Currency chain client trait defined
- [x] HTTP RPC client implemented
- [x] Slashing calculation logic complete
- [x] Reward distribution implemented
- [x] Event recording and transparency
- [x] Comprehensive unit tests
- [x] Integration tests with mock client
- [x] Error handling and validation
- [x] Async/await support
- [x] Configuration via environment variables
- [x] Documentation complete

## Future Enhancements

1. **Multi-Currency Support**: Slash in different tokens
2. **Gradual Slashing**: Time-locked stake reduction
3. **Insurance Integration**: Direct fund transfers
4. **Appeal Mechanism**: Dispute resolution appeals
5. **Batch Slashing**: Execute multiple slashes atomically

## References

- **ARCHITECTURE.md**: Section 34 - Dispute Resolution
- **probus.md**: Task 4 - Slashing Implementation
- **dchat-chain/chain/slashing/penalty.rs**: Penalty calculation
- **dchat-chain/chain/currency_chain/staking.rs**: Original staking RPC

## Related Issues

- Onion Routing Integration (Task 1) - COMPLETE
- Fork Signature Verification (Task 3) - COMPLETE
- Dispute Resolution (Section 34) - NOW COMPLETE

---

**Status**: Production Ready ✅  
**Implementation Date**: 2025-01-26  
**Integration**: Currency chain, governance, reputation system
