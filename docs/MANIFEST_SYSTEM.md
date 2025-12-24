# DPL Manifest System Documentation

The DPL Manifest System provides cryptographic integrity guarantees for dchat programs, enabling on-chain schema verification and protecting against ABI drift attacks.

## Table of Contents

1. [Overview](#overview)
2. [Manifest Structure](#manifest-structure)
3. [IDL and Schema Hash](#idl-and-schema-hash)
4. [Capabilities](#capabilities)
5. [CLI Commands](#cli-commands)
6. [Programmatic API](#programmatic-api)
7. [Validation](#validation)
8. [Security Model](#security-model)
9. [Best Practices](#best-practices)

## Overview

Every DPL program built with the Edition 2025 SDK contains an embedded **manifest** - a 64-byte custom WASM section that provides:

- **Version Information**: SDK version and ABI compatibility
- **Schema Hash**: Cryptographic hash of the program's interface definition
- **Capabilities**: Declared features the program uses (CPI, PDAs, tokens, etc.)
- **Import Profile**: Whether the program uses WASI or legacy imports

### Key Benefits

| Benefit           | Description                              |
| ----------------- | ---------------------------------------- |
| **Integrity**     | Verify program hasn't been tampered with |
| **Compatibility** | Detect ABI changes before deployment     |
| **Introspection** | Query program interface on-chain         |
| **Security**      | Prevent instruction confusion attacks    |

## Manifest Structure

The manifest is stored in a WASM custom section named `dpl_manifest`:

```
┌──────────────────────────────────────────────────────────────────┐
│                        DPL MANIFEST (64 bytes)                   │
├────────┬──────┬───────────────────────────────────────────────────┤
│ Offset │ Size │ Field                                             │
├────────┼──────┼───────────────────────────────────────────────────┤
│ 0      │ 4    │ Magic: "DPLM" (0x44 0x50 0x4C 0x4D)               │
│ 4      │ 2    │ SDK Major Version (little-endian u16)             │
│ 6      │ 2    │ SDK Minor Version (little-endian u16)             │
│ 8      │ 2    │ SDK Patch Version (little-endian u16)             │
│ 10     │ 2    │ Edition Year (e.g., 2025 = 0xE9 0x07)             │
│ 12     │ 1    │ ABI Version (current: 1)                          │
│ 13     │ 1    │ Import Profile (0=Legacy, 1=WASI, 2=Hybrid)       │
│ 14     │ 2    │ Reserved (must be 0x00 0x00)                      │
│ 16     │ 32   │ Schema Hash (BLAKE3 of canonical IDL)             │
│ 48     │ 8    │ Capabilities (bitflags, little-endian u64)        │
│ 56     │ 8    │ Reserved (must be all zeros)                      │
└────────┴──────┴───────────────────────────────────────────────────┘
```

### Example Hex Dump

```
44 50 4C 4D  # Magic: "DPLM"
00 00        # SDK Major: 0
01 00        # SDK Minor: 1
00 00        # SDK Patch: 0
E9 07        # Edition: 2025
01           # ABI Version: 1
01           # Import Profile: WASI
00 00        # Reserved
a1 b2 c3...  # 32-byte Schema Hash
07 00 00 00 00 00 00 00  # Capabilities: EMITS_EVENTS | USES_CPI | USES_PDAS
00 00 00 00 00 00 00 00  # Reserved
```

## IDL and Schema Hash

### Interface Definition Language (IDL)

The IDL describes the program's public interface:

```json
{
  "version": "0.1.0",
  "name": "my_token_program",
  "instructions": [
    {
      "name": "initialize",
      "discriminator": [0xaf, 0x07, 0x1d, 0x22, 0x8b, 0x4c, 0x9e, 0x5f],
      "args": [
        { "name": "decimals", "type": "u8" },
        { "name": "name", "type": "string" }
      ],
      "accounts": [
        { "name": "mint", "is_mut": true, "is_signer": false },
        { "name": "authority", "is_mut": false, "is_signer": true }
      ]
    }
  ],
  "accounts": [
    {
      "name": "Mint",
      "discriminator": [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
      "fields": [
        { "name": "authority", "type": "pubkey" },
        { "name": "supply", "type": "u64" },
        { "name": "decimals", "type": "u8" }
      ]
    }
  ],
  "events": [],
  "errors": [
    { "code": 6000, "name": "Unauthorized", "msg": "Signer is not authorized" }
  ]
}
```

### Schema Hash Computation

The schema hash is computed as:

```rust
// Canonical JSON serialization (sorted keys, no whitespace)
let canonical_json = serde_json::to_string(&idl)?;

// BLAKE3 hash
let hash = blake3::hash(canonical_json.as_bytes());
```

### Instruction Discriminators

Each instruction has an 8-byte discriminator computed from its name:

```rust
let discriminator = blake3::hash(b"instruction:initialize")[..8];
// Result: [0xaf, 0x07, 0x1d, 0x22, 0x8b, 0x4c, 0x9e, 0x5f]
```

This ensures stable instruction routing regardless of source code order.

## Capabilities

Capabilities are bitflags declaring what features a program uses:

```rust
bitflags! {
    pub struct Capabilities: u64 {
        const EMITS_EVENTS      = 1 << 0;  // Program emits events
        const USES_CPI          = 1 << 1;  // Cross-program invocation
        const USES_PDAS         = 1 << 2;  // Program derived addresses
        const REQUIRES_SIGNERS  = 1 << 3;  // Requires signer verification
        const USES_TOKENS       = 1 << 4;  // Token operations
        const USES_PRIVACY      = 1 << 5;  // Privacy features
        const USES_CAPABILITIES = 1 << 6;  // Capability tokens
        const UPGRADEABLE       = 1 << 7;  // Has upgrade authority
        const USES_GOVERNANCE   = 1 << 8;  // Governance features
        const USES_STAKING      = 1 << 9;  // Staking features
    }
}
```

### Why Capabilities Matter

1. **Static Analysis**: Tools can analyze program behavior without execution
2. **Permission Checks**: Runtime can verify declared vs actual behavior
3. **Optimization**: Runtime can optimize based on declared capabilities
4. **Auditing**: Auditors know what features to review

## CLI Commands

### Extract Manifest

```bash
# From WASM file
dchat program manifest ./my_program.wasm

# From deployed program
dchat program manifest <program_address> --rpc-url https://api.dchat.network

# Output as JSON
dchat program manifest ./my_program.wasm --format json

# Raw binary output
dchat program manifest ./my_program.wasm --raw > manifest.bin
```

**Example Output**:

```
DPL Manifest
════════════════════════════════════════════════════════════════════
SDK Version:     0.1.0
Edition:         2025
ABI Version:     1
Import Profile:  WASI

Schema Hash:     a1b2c3d4e5f6...
Capabilities:    EMITS_EVENTS | USES_CPI | USES_PDAS
```

### Verify Manifest

```bash
# Verify schema hash matches expected value
dchat program verify-manifest \
    --program ./my_program.wasm \
    --expected-hash a1b2c3d4e5f6...

# Verify against IDL file
dchat program verify-manifest \
    --program ./my_program.wasm \
    --idl ./idl.json
```

### Validate Program

```bash
# Basic validation
dchat program validate ./my_program.wasm

# Strict validation (requires manifest)
dchat program validate ./my_program.wasm --require-manifest

# Reject zero schema hash
dchat program validate ./my_program.wasm --reject-zero-hash
```

## Programmatic API

### Extracting Manifest (Rust)

```rust
use dchat_programs::manifest::{extract_manifest, DplManifest};

fn inspect_program(wasm_bytes: &[u8]) -> Result<(), Box<dyn Error>> {
    match extract_manifest(wasm_bytes)? {
        Some(manifest) => {
            println!("SDK Version: {}", manifest.sdk_version_string());
            println!("Schema Hash: {}", hex::encode(&manifest.schema_hash));
            println!("Capabilities: {:?}", manifest.capabilities);
        }
        None => {
            println!("Legacy program (no manifest)");
        }
    }
    Ok(())
}
```

### Computing Schema Hash

```rust
use dchat_dpl::idl::{Idl, IdlInstruction, IdlAccountMeta};

fn generate_idl_hash() -> [u8; 32] {
    let idl = Idl::new("my_program", "0.1.0")
        .with_instruction(
            IdlInstruction::new("initialize")
                .with_arg("amount", IdlType::U64)
                .with_account(IdlAccountMeta::new("mint", true, false))
        );

    idl.schema_hash()
}
```

### On-Chain Schema Query (Syscall)

Programs can query other programs' schemas:

```rust
use dchat_dpl::syscall::{get_program_manifest, verify_program_schema};

pub fn verify_target_program(target_program: &Pubkey) -> Result<()> {
    // Get manifest from target program
    let manifest = get_program_manifest(target_program)?;

    // Verify expected schema hash
    let expected_hash = [0xa1, 0xb2, 0xc3, /* ... */];
    if manifest.schema_hash != expected_hash {
        return Err(Error::SchemaMismatch);
    }

    // Or use built-in verification
    verify_program_schema(target_program, &expected_hash)?;

    Ok(())
}
```

## Validation

### Validation Levels

| Level      | require_manifest | reject_zero_hash | Use Case             |
| ---------- | ---------------- | ---------------- | -------------------- |
| Permissive | false            | false            | Legacy compatibility |
| Standard   | true             | false            | New deploys          |
| Strict     | true             | true             | High-security        |

### Validation Flow

```
┌─────────────────────┐
│ Load WASM Bytecode  │
└─────────┬───────────┘
          │
          ▼
┌─────────────────────┐     ┌────────────────────┐
│ Check WASM Magic    │────▶│ Reject: Not WASM   │
│ 0x00 0x61 0x73 0x6D │     └────────────────────┘
└─────────┬───────────┘
          │ Valid
          ▼
┌─────────────────────┐     ┌────────────────────┐
│ Parse Sections      │────▶│ Reject: Malformed  │
└─────────┬───────────┘     └────────────────────┘
          │ Valid
          ▼
┌─────────────────────┐
│ Find dpl_manifest   │
└─────────┬───────────┘
          │
    ┌─────┴─────┐
    │           │
    ▼           ▼
┌────────┐  ┌────────────┐
│ Found  │  │ Not Found  │
└────┬───┘  └─────┬──────┘
     │            │
     ▼            ▼
┌─────────┐  ┌───────────────────┐     ┌────────────────────┐
│ Parse   │  │ require_manifest? │────▶│ Reject: No manifest│
│ Manifest│  └─────────┬─────────┘     └────────────────────┘
└────┬────┘            │ No
     │                 ▼
     │           ┌────────────┐
     │           │ Legacy OK  │
     │           └────────────┘
     ▼
┌─────────────────────┐     ┌────────────────────┐
│ Check Magic "DPLM"  │────▶│ Reject: Bad magic  │
└─────────┬───────────┘     └────────────────────┘
          │ Valid
          ▼
┌─────────────────────┐     ┌────────────────────┐
│ reject_zero_hash?   │────▶│ Reject: Zero hash  │
│ && hash == zeros    │     └────────────────────┘
└─────────┬───────────┘
          │ Pass
          ▼
┌─────────────────────┐
│ ✓ Validation OK     │
└─────────────────────┘
```

## Security Model

### Threat Model

| Threat                    | Protection                                |
| ------------------------- | ----------------------------------------- |
| **ABI Drift**             | Schema hash detects interface changes     |
| **Instruction Confusion** | Stable discriminators from names          |
| **Replay Attacks**        | Version + timestamp in build metadata     |
| **Supply Chain**          | Reproducible builds + schema verification |
| **Capability Escalation** | Declared capabilities enforced at runtime |

### Security Properties

1. **Immutability**: Manifest is embedded in WASM, cannot be modified without changing bytecode hash
2. **Binding**: Schema hash cryptographically binds program to its interface
3. **Verifiability**: Anyone can recompute schema hash from IDL
4. **Non-Repudiation**: Deployed manifest proves original interface

### Attack Prevention

#### ABI Drift Attack

**Attack**: Attacker deploys program upgrade with changed instruction arguments.
**Protection**: Schema hash change detected, deployment rejected or flagged.

#### Instruction Confusion Attack

**Attack**: Attacker crafts transaction that calls wrong instruction.
**Protection**: BLAKE3 discriminators have collision resistance.

#### Capability Abuse

**Attack**: Program claims minimal capabilities but uses privileged features.
**Protection**: Runtime validates actual behavior against declared capabilities.

## Best Practices

### Development

1. **Generate IDL on Every Build**

   ```bash
   # In CI/CD
   dchat program manifest ./target/*.wasm --format json > idl.json
   ```

2. **Commit IDL to Version Control**
   - Track schema changes in git history
   - Review IDL changes in PRs

3. **Use Strict Validation in CI**

   ```bash
   dchat program validate ./program.wasm --require-manifest --reject-zero-hash
   ```

4. **Pin Schema Hash for Releases**
   ```bash
   DPL_SCHEMA_HASH=a1b2c3... cargo build --release
   ```

### Deployment

1. **Verify Before Deploy**

   ```bash
   dchat program verify-manifest --program ./program.wasm --idl ./idl.json
   ```

2. **Document Schema Hash in Release Notes**

   ```markdown
   ## v1.2.0

   - Schema Hash: a1b2c3d4e5f6...
   - IDL: [link to idl.json]
   ```

3. **Monitor Schema Changes**
   - Alert on unexpected schema hash changes
   - Require approval for interface changes

### Client Development

1. **Fetch IDL from Chain**

   ```typescript
   const idl = await program.fetchIdl(programId);
   const expectedHash = computeSchemaHash(idl);
   ```

2. **Validate Before Transactions**

   ```typescript
   if (manifest.schemaHash !== expectedHash) {
     throw new Error("Program schema changed!");
   }
   ```

3. **Cache IDL with Version Check**
   ```typescript
   const cachedIdl = loadFromCache(programId);
   if (cachedIdl.schemaHash !== onChainHash) {
     refreshCache(programId);
   }
   ```

## See Also

- [Reproducible Builds Guide](./REPRODUCIBLE_BUILDS.md)
- [Migration Guide](./MIGRATION_GUIDE.md)
- [DPL SDK Reference](../crates/dchat-dpl/README.md)
