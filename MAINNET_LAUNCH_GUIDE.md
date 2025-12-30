# dchat Mainnet Launch Guide

## Overview

This guide walks through launching dchat mainnet using the **pre-stake genesis** approach, which solves the "chicken and egg" problem of needing validators to be staked before the chain exists.

## The Problem

Traditional blockchain launches face a bootstrapping dilemma:

- Validators need to stake tokens to participate
- Staking requires RPC calls to the chain
- The chain doesn't exist until validators are running
- Validators can't run without the genesis file

## The Solution: Pre-Stake Genesis

Pre-stake genesis uses **cryptographic bond commitments** to solve this:

1. Validators sign commitments offline (no chain needed)
2. Commitments are collected into a manifest
3. Genesis is generated from the manifest with pre-staked validators
4. Chain starts with validators already staked at block 0

## Prerequisites

- dchat binary built and in PATH
- Ed25519 key pair for each validator
- Minimum 4 validators across 3+ geographic regions
- Minimum 10,000 DCHAT stake per validator (in production allocation)

## Launch Process

### Phase 1: Preparation (Coordinator)

The genesis coordinator initializes the process:

```bash
# Initialize the pre-stake manifest
dchat pre-stake-genesis init-manifest \
    --chain-id dchat-mainnet-1 \
    --output ./prestake-manifest.json \
    --initial-supply 1000000000 \
    --min-stake 10000
```

This creates a manifest file that will collect validator commitments.

### Phase 2: Validator Commitments

Each validator generates a signed bond commitment:

```bash
# Generate a new key (if needed)
dchat keygen --output ./validator.key

# Create and sign the bond commitment
dchat pre-stake-genesis create-commitment \
    --key-file ./validator.key \
    --name "Validator-US-East" \
    --stake 50000 \
    --address "validator1.example.com:26656" \
    --region us-east \
    --lockup-days 30 \
    --chain-id dchat-mainnet-1 \
    --output ./my-commitment.json
```

**Important**: The commitment is cryptographically signed and binding. By sharing it, the validator irrevocably commits to staking the specified amount.

### Phase 3: Collect Commitments (Coordinator)

The coordinator adds each validator's commitment to the manifest:

```bash
# Add each commitment
dchat pre-stake-genesis add-commitment \
    --manifest ./prestake-manifest.json \
    --commitment ./validator1-commitment.json

dchat pre-stake-genesis add-commitment \
    --manifest ./prestake-manifest.json \
    --commitment ./validator2-commitment.json

# ... repeat for all validators
```

### Phase 4: Validate Manifest

Before generating genesis, validate the manifest meets all requirements:

```bash
dchat pre-stake-genesis validate-manifest \
    --manifest ./prestake-manifest.json
```

Requirements checked:

- Minimum 4 validators
- Minimum 3 geographic regions
- All signatures valid
- All stakes meet minimum
- Total stake within allocation limit

### Phase 5: Generate Genesis

Generate the genesis files:

```bash
dchat pre-stake-genesis generate-genesis \
    --manifest ./prestake-manifest.json \
    --coordinator-key ./coordinator.key \
    --output ./genesis/
```

This creates:

- `genesis/currency_chain_genesis.json` - Currency chain genesis
- `genesis/chat_chain_genesis.json` - Chat chain genesis
- `genesis/genesis.json` - Combined genesis summary

### Phase 6: Distribute and Launch

1. **Distribute genesis files** to ALL validators
2. **Verify file hashes** match on every validator
3. **Coordinate launch time** (all must start within minutes)
4. **Start validators**:

```bash
dchat --role validator \
    --genesis-dir ./genesis/ \
    --key-file ./validator.key \
    --data-dir ./data/
```

## Verification Checklist

Before launch, verify:

- [ ] All validators have identical genesis files
- [ ] Genesis file hashes match across all validators
- [ ] Each validator's key matches their commitment
- [ ] Network addresses are reachable
- [ ] Clocks are synchronized (NTP)
- [ ] Firewall ports open (26656 P2P, 26657 RPC)

## Security Considerations

### Bond Commitments

- Commitments are Ed25519 signed and cannot be forged
- Chain ID is included to prevent replay across networks
- Lockup period enforced from block 0

### Genesis Integrity

- Genesis files are deterministically generated from manifest
- All validators must have identical files
- Any mismatch will prevent consensus

### Key Security

- Validator keys must be backed up securely
- Never share private keys
- Use hardware security modules (HSM) for production

## Troubleshooting

### "Minimum validators required" Error

Collect at least 4 validator commitments before generating genesis.

### "Minimum regions required" Error

Ensure validators are distributed across at least 3 geographic regions.

### "Chain ID mismatch" Error

All validators must use the same chain ID when creating commitments.

### Validators Not Connecting

- Verify network addresses are reachable
- Check firewall rules
- Ensure all validators started with same genesis

## Helper Scripts

Launch scripts are provided in `scripts/`:

- `mainnet_launch.sh` - Bash script for Linux/macOS
- `mainnet_launch.ps1` - PowerShell script for Windows

Interactive mode:

```bash
./scripts/mainnet_launch.sh full-flow
```

## Token Economics at Genesis

Default allocation (1 billion DCHAT):

- 20% Foundation treasury
- 24% Validator staking pool
- 56% Community rewards pool

Validators stake from the validator pool allocation. The actual token balance is credited at genesis.

## Post-Launch

After successful launch:

1. **Monitor block production** - Blocks should be produced every 3-5 seconds
2. **Verify validator participation** - All genesis validators should be signing
3. **Enable public staking** - Once stable, open staking to additional validators
4. **Enable governance** - Unlock DAO voting features

## Related Documentation

- [ARCHITECTURE-2.0.md](./ARCHITECTURE-2.0.md) - System architecture
- [MAINNET_QUICK_START.md](./MAINNET_QUICK_START.md) - Quick start guide
- [SECURITY_MODEL.md](./SECURITY_MODEL.md) - Security model
