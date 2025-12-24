# DPL Escrow Ironclad Example

A complete escrow contract demonstrating the DPL ironclad manifest system.

## Overview

This example shows how to build a production-ready DPL program with:

- **Ironclad Manifest**: 64-byte manifest embedded in WASM
- **Schema Hash**: Deterministic hash of the IDL for ABI verification
- **Build-time Verification**: build.rs integration for CI/CD
- **Stable Discriminators**: BLAKE3-based instruction routing
- **Capability Declaration**: Events, PDAs, signers

## Contract Features

### Escrow Flow

```
┌─────────────┐     ┌─────────────┐     ┌─────────────┐
│  Initialize │────▶│   Active    │────▶│  Released   │
│  (deposit)  │     │             │     │ (to recip.) │
└─────────────┘     └──────┬──────┘     └─────────────┘
                           │
                    ┌──────┴──────┐
                    │             │
                    ▼             ▼
             ┌───────────┐  ┌───────────┐
             │ Cancelled │  │ Disputed  │
             │ (refund)  │  │           │
             └───────────┘  └─────┬─────┘
                                  │
                           ┌──────┴──────┐
                           │             │
                           ▼             ▼
                    ┌───────────┐  ┌───────────┐
                    │ Released  │  │ Refunded  │
                    │ (arbiter) │  │ (arbiter) │
                    └───────────┘  └───────────┘
```

### Instructions

| Instruction  | Description                    | Signer                  |
| ------------ | ------------------------------ | ----------------------- |
| `initialize` | Create escrow with tokens      | Depositor               |
| `release`    | Release tokens to recipient    | Recipient (if required) |
| `cancel`     | Cancel and refund to depositor | Depositor               |
| `dispute`    | Raise a dispute                | Depositor or Recipient  |
| `resolve`    | Resolve dispute                | Arbiter                 |

## Building

### Development Build

```bash
cargo build --target wasm32-wasi --release
```

### Reproducible Build

```bash
# Set reproducible timestamp
export SOURCE_DATE_EPOCH=$(date +%s)
export DPL_BUILD_HOST="release"

# Build
cargo build --target wasm32-wasi --release
```

### With Schema Verification

```bash
# First, generate the IDL
dchat program manifest target/wasm32-wasi/release/dpl_escrow_ironclad.wasm \
    --format json > idl.json

# Then build with verification
DPL_SCHEMA_HASH=$(cat idl.json | dchat program schema-hash) \
    cargo build --target wasm32-wasi --release --features release-verify
```

## Manifest Inspection

### View Manifest

```bash
dchat program manifest target/wasm32-wasi/release/dpl_escrow_ironclad.wasm
```

Expected output:

```
DPL Manifest
════════════════════════════════════════════════════════════════════
SDK Version:     0.1.0
Edition:         2025
ABI Version:     1
Import Profile:  WASI

Schema Hash:     a1b2c3d4e5f6...
Capabilities:    EMITS_EVENTS | USES_PDAS | REQUIRES_SIGNERS
```

### Extract IDL

```bash
dchat program manifest target/wasm32-wasi/release/dpl_escrow_ironclad.wasm \
    --format json > idl.json
```

### Verify Manifest

```bash
dchat program verify-manifest \
    --program target/wasm32-wasi/release/dpl_escrow_ironclad.wasm \
    --idl idl.json
```

## IDL Schema

The generated IDL includes:

```json
{
  "version": "0.1.0",
  "name": "escrow",
  "instructions": [
    {
      "name": "initialize",
      "discriminator": [...],
      "args": [
        { "name": "amount", "type": "u64" },
        { "name": "unlock_time", "type": "i64" },
        { "name": "require_recipient_signature", "type": "bool" }
      ],
      "accounts": [
        { "name": "escrow", "is_mut": true, "is_signer": false },
        { "name": "depositor", "is_mut": true, "is_signer": true },
        { "name": "recipient", "is_mut": false, "is_signer": false },
        { "name": "vault", "is_mut": true, "is_signer": false },
        { "name": "system", "is_mut": false, "is_signer": false }
      ]
    },
    ...
  ],
  "accounts": [
    {
      "name": "Escrow",
      "discriminator": [...],
      "fields": [
        { "name": "depositor", "type": "pubkey" },
        { "name": "recipient", "type": "pubkey" },
        { "name": "amount", "type": "u64" },
        ...
      ]
    }
  ],
  "events": [
    { "name": "EscrowCreated", "discriminator": [...], "fields": [...] },
    { "name": "EscrowReleased", "discriminator": [...], "fields": [...] },
    ...
  ],
  "errors": [
    { "code": 6000, "name": "ZeroAmount", "msg": "Amount must be greater than zero" },
    { "code": 6001, "name": "InvalidState", "msg": "Invalid escrow state..." },
    ...
  ]
}
```

## CI/CD Integration

### GitHub Actions

```yaml
name: Build & Verify

on: [push, pull_request]

env:
  SOURCE_DATE_EPOCH: "1704067200"

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable
        with:
          targets: wasm32-wasi

      - name: Build
        working-directory: examples/contracts/dpl-escrow-ironclad
        run: cargo build --target wasm32-wasi --release

      - name: Verify Manifest
        run: |
          dchat program verify-manifest \
            --program examples/contracts/dpl-escrow-ironclad/target/wasm32-wasi/release/dpl_escrow_ironclad.wasm \
            --expected-hash ${{ vars.EXPECTED_SCHEMA_HASH }}
```

## Security Considerations

### Schema Hash Verification

The schema hash ensures:

1. Instruction signatures haven't changed
2. Account layouts are consistent
3. Event formats are stable
4. Error codes are fixed

### Discriminator Stability

Discriminators are computed from names:

```rust
// "initialize" -> BLAKE3("instruction:initialize")[..8]
// Result: stable across builds
```

Not from enum positions:

```rust
// enum { A, B, C } -> A=0, B=1, C=2
// Problem: inserting X before A changes all discriminators!
```

## Testing

```bash
# Run unit tests
cargo test

# Check compilation
cargo check
```

## Files

| File         | Description                          |
| ------------ | ------------------------------------ |
| `Cargo.toml` | Dependencies and build configuration |
| `build.rs`   | Build-time manifest verification     |
| `src/lib.rs` | Contract implementation              |
| `idl.json`   | Generated IDL (after build)          |

## Related Documentation

- [Manifest System](../../../docs/MANIFEST_SYSTEM.md)
- [Reproducible Builds](../../../docs/REPRODUCIBLE_BUILDS.md)
- [Migration Guide](../../../docs/MIGRATION_GUIDE.md)
