# Pre-Stake Genesis: Mainnet Launch Guide

## Overview

This guide explains how to launch the dchat mainnet using pre-stake genesis, which solves the "chicken and egg" problem of mainnet launch where:

- Validators need RPC to stake
- RPC needs validators to be running
- Validators need genesis to start

## Solution: Pre-Stake Genesis

Pre-stake genesis allows validators to commit to staking **before** the chain is live by:

1. Creating cryptographically signed bond commitments offline
2. Collecting all commitments into a manifest
3. Generating genesis files with pre-staked validators
4. All validators start with the same genesis file

## Workflow

### Phase 1: Preparation (Coordinator)

```bash
# 1. Create the pre-stake manifest
dchat prestake-genesis init-manifest \
  --chain-id dchat-mainnet-1 \
  --initial-supply 1000000000 \
  --min-stake 10000 \
  --output ./prestake-manifest.json
```

### Phase 2: Validator Commitments (Each Validator)

```bash
# 2. Each validator generates their keys (if not already done)
dchat keygen --output ./validator-key.json --validator

# 3. Each validator creates their bond commitment
dchat prestake-genesis create-commitment \
  --key-file ./validator-key.json \
  --name "validator-us-east-1" \
  --stake 50000 \
  --address "validator1.dchat.network:26656" \
  --region "us-east" \
  --lockup-days 30 \
  --chain-id dchat-mainnet-1 \
  --output ./my-commitment.json
```

Validators send their commitment files to the coordinator.

### Phase 3: Collect Commitments (Coordinator)

```bash
# 4. Add each validator's commitment to the manifest
dchat prestake-genesis add-commitment \
  --manifest ./prestake-manifest.json \
  --commitment ./validator1-commitment.json

dchat prestake-genesis add-commitment \
  --manifest ./prestake-manifest.json \
  --commitment ./validator2-commitment.json

# ... repeat for all validators
```

### Phase 4: Validate & Generate (Coordinator)

```bash
# 5. Validate the manifest is ready
dchat prestake-genesis validate-manifest \
  --manifest ./prestake-manifest.json

# 6. Generate genesis files
dchat prestake-genesis generate-genesis \
  --manifest ./prestake-manifest.json \
  --coordinator-key ./coordinator-key.json \
  --output ./genesis
```

### Phase 5: Launch (All Validators)

```bash
# 7. Distribute genesis files to all validators
# Each validator receives: genesis/chat_chain_genesis.json
#                          genesis/currency_chain_genesis.json
#                          genesis/genesis.json

# 8. Each validator starts with the genesis files
dchat --role validator --genesis-dir ./genesis
```

## Requirements

### Minimum Validators

- **4 validators** required (Byzantine fault tolerance)

### Geographic Diversity

- **3 distinct regions** required (prevents regional centralization)

### Stake Requirements

- **Minimum stake**: 10,000 DCHAT per validator
- **Lockup period**: Minimum 7 days

## Security

### Bond Commitments

- Each commitment is cryptographically signed with the validator's Ed25519 key
- Commitments cannot be repudiated after signing
- Chain ID hash prevents cross-chain replay attacks

### Genesis Signing

- Coordinator signs the final genesis blocks
- All validators can verify the genesis signature
- Stakes are locked from block 0

## Troubleshooting

### "Insufficient validators"

Add more validators until you have at least 4.

### "Insufficient regions"

Ensure validators are distributed across at least 3 geographic regions.

### "Stake below minimum"

Each validator must stake at least the minimum amount (default: 10,000 DCHAT).

### "Commitment signature verification failed"

The commitment file may be corrupted or the validator used a different key.

## CLI Reference

```
dchat prestake-genesis init-manifest     # Create new manifest
dchat prestake-genesis create-commitment # Sign bond commitment
dchat prestake-genesis add-commitment    # Add commitment to manifest
dchat prestake-genesis validate-manifest # Validate manifest
dchat prestake-genesis generate-genesis  # Generate genesis files
```

Run `dchat prestake-genesis --help` for full options.
