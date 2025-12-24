# DPL Program Migration Guide

This document describes how to migrate legacy programs to the new manifest-enabled DPL format, ensuring backward compatibility while gaining the security benefits of the ironclad manifest system.

## Overview

### What Changed

DPL Edition 2025 introduces the **ironclad manifest** - a 64-byte custom section embedded in WASM programs that contains:

- SDK version information
- ABI compatibility version
- Import profile (WASI/legacy)
- Schema hash (SHA-256 of the IDL)
- Capability flags

### Why Migrate

1. **Security**: Schema hash prevents ABI drift attacks
2. **Verification**: Users can verify program integrity
3. **Introspection**: On-chain IDL queries via syscall
4. **Future-Proofing**: Required for dchat network upgrades

### Compatibility Mode

Legacy programs (without manifest) are still supported but with restrictions:

| Feature                 | Legacy Mode | Manifest Mode |
| ----------------------- | ----------- | ------------- |
| Basic execution         | ✅          | ✅            |
| CPI to other programs   | ✅          | ✅            |
| On-chain IDL query      | ❌          | ✅            |
| Schema verification     | ❌          | ✅            |
| Strict validation       | Blocked     | ✅            |
| Future network upgrades | May break   | ✅ Guaranteed |

## Migration Path

### Step 1: Assess Current Program

Determine your program's current state:

```bash
# Check if program has manifest
dchat program manifest <program_address_or_wasm_file>

# Possible outputs:
# 1. "DPL Manifest found: ..." - Already migrated
# 2. "No manifest found (legacy program)" - Needs migration
```

### Step 2: Update Dependencies

Update `Cargo.toml` to use DPL Edition 2025:

```toml
[dependencies]
dchat-dpl = { version = "0.1.0", features = ["std"] }

# For build.rs schema verification
[build-dependencies]
dchat-dpl = { version = "0.1.0", features = ["build"] }
```

### Step 3: Update Program Structure

#### Before (Legacy)

```rust
// Old style without DPL macros
use borsh::{BorshDeserialize, BorshSerialize};

#[derive(BorshDeserialize)]
pub enum Instruction {
    Initialize { value: u64 },
    Update { new_value: u64 },
}

pub fn process(instruction: &[u8]) -> Result<(), ProgramError> {
    let ix = Instruction::try_from_slice(instruction)?;
    match ix {
        Instruction::Initialize { value } => { /* ... */ }
        Instruction::Update { new_value } => { /* ... */ }
    }
    Ok(())
}
```

#### After (DPL Edition 2025)

```rust
use dchat_dpl::prelude::*;

#[program]
pub mod my_program {
    use super::*;

    pub fn initialize(ctx: Context<Initialize>, value: u64) -> Result<()> {
        // Implementation
        Ok(())
    }

    pub fn update(ctx: Context<Update>, new_value: u64) -> Result<()> {
        // Implementation
        Ok(())
    }
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, space = 8 + 8)]
    pub data: Account<'info, MyData>,
    #[account(signer)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct Update<'info> {
    #[account(mut)]
    pub data: Account<'info, MyData>,
    #[account(signer)]
    pub authority: Signer<'info>,
}

#[account]
pub struct MyData {
    pub value: u64,
}
```

### Step 4: Generate IDL

The `#[program]` macro automatically generates the IDL and embeds the manifest. After building:

```bash
# Build the program
cargo build --release --target wasm32-wasi

# Extract the generated IDL
dchat program manifest target/wasm32-wasi/release/my_program.wasm --format json > idl.json
```

### Step 5: Add build.rs Verification

Create or update `build.rs`:

```rust
use dchat_dpl::build::{BuildConfig, generate_manifest_metadata};
use std::path::PathBuf;

fn main() {
    // For development builds, just generate metadata
    #[cfg(not(feature = "release-verify"))]
    {
        generate_manifest_metadata(&BuildConfig::default())
            .expect("Failed to generate metadata");
    }

    // For release builds, verify against committed IDL
    #[cfg(feature = "release-verify")]
    {
        let config = BuildConfig {
            idl_path: Some(PathBuf::from("idl.json")),
            expected_schema_hash: std::env::var("DPL_SCHEMA_HASH").ok(),
            verbose: true,
            ..Default::default()
        };
        generate_manifest_metadata(&config)
            .expect("Schema verification failed!");
    }

    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=idl.json");
}
```

### Step 6: Deploy Upgrade

Deploy the upgraded program:

```bash
# Deploy with upgrade authority
dchat program deploy \
    --program-id <existing_program_id> \
    --bytecode target/wasm32-wasi/release/my_program.wasm \
    --upgrade-authority <authority_keypair>

# Verify manifest is present
dchat program manifest <program_id>
```

## Breaking Change Handling

### Instruction Discriminator Changes

DPL uses BLAKE3 hash of instruction names for discriminators, which differs from positional enum variants.

**Problem**: Existing clients send old discriminators.

**Solution**: Implement dual-dispatch during transition:

```rust
#[program]
pub mod my_program {
    use super::*;

    // New style instruction
    pub fn initialize(ctx: Context<Initialize>, value: u64) -> Result<()> {
        initialize_impl(ctx, value)
    }

    // Legacy compatibility shim (accepts old discriminator)
    #[instruction(discriminator = [0, 0, 0, 0, 0, 0, 0, 0])]  // Old variant 0
    pub fn initialize_legacy(ctx: Context<Initialize>, value: u64) -> Result<()> {
        initialize_impl(ctx, value)
    }

    fn initialize_impl(ctx: Context<Initialize>, value: u64) -> Result<()> {
        // Actual implementation
        Ok(())
    }
}
```

### Account Layout Changes

If account data layout changed:

**Solution**: Version-aware deserialization:

```rust
#[account]
pub struct MyDataV2 {
    pub version: u8,         // New: version byte
    pub value: u64,
    pub additional: [u8; 32], // New: additional field
}

impl MyDataV2 {
    pub fn from_legacy(legacy_data: &[u8]) -> Result<Self> {
        if legacy_data.len() == 8 {
            // V1 format: just u64
            let value = u64::from_le_bytes(legacy_data.try_into()?);
            Ok(Self {
                version: 2,
                value,
                additional: [0u8; 32],
            })
        } else {
            // V2 format
            Self::try_from_slice(legacy_data)
        }
    }
}
```

## Gradual Migration Strategy

For complex programs, migrate gradually:

### Phase 1: Add Manifest (Non-Breaking)

1. Update to DPL SDK
2. Keep existing instruction handlers
3. Add `#[program]` wrapper that delegates to legacy code
4. Deploy - manifest is added, behavior unchanged

### Phase 2: Update Clients

1. Generate new IDL from deployed program
2. Update client SDKs to use new discriminators
3. Test with both old and new clients

### Phase 3: Remove Legacy Support

1. Remove legacy compatibility shims
2. Update program to pure DPL style
3. Deploy final version

## Runtime Behavior Differences

### Import Profile Detection

Programs are classified by their import profile:

| Profile  | Module                   | Description                         |
| -------- | ------------------------ | ----------------------------------- |
| `Legacy` | `env` only               | Old wasm32-unknown-unknown programs |
| `Wasi`   | `wasi_snapshot_preview1` | New wasm32-wasi programs            |
| `Hybrid` | Both                     | Transition programs                 |

The runtime automatically detects the profile and provides appropriate host functions.

### WASI Shim

For WASI programs, dchat provides a deterministic WASI shim:

- `fd_write` → Logs to program output
- `clock_time_get` → Returns deterministic slot-based time
- `random_get` → Deterministic PRNG seeded by slot + program ID
- `environ_get` → Empty environment
- `args_get` → Empty args

## Validation Mode Configuration

### Default Mode (Permissive)

```rust
let config = ValidationConfig::default();
// Allows legacy programs without manifest
// Allows zero schema hash
```

### Strict Mode (Recommended for New Deploys)

```rust
let config = ValidationConfig {
    require_manifest: true,
    reject_zero_schema_hash: true,
    ..Default::default()
};
// Rejects programs without valid manifest
```

### CLI Flags

```bash
# Validate with strict mode
dchat program validate my_program.wasm --require-manifest --reject-zero-hash

# Deploy with strict validation
dchat program deploy my_program.wasm --strict
```

## Network Upgrade Timeline

### Current: Transitional Period

- Legacy programs continue to work
- Manifest optional but recommended
- Strict validation opt-in

### Future: Manifest Required

A future network upgrade will require manifests for:

1. New program deployments
2. Program upgrades
3. Certain CPI targets

**Recommendation**: Migrate now to avoid disruption.

## Troubleshooting

### "Invalid manifest magic bytes"

**Cause**: Corrupt or incorrectly formatted manifest section.

**Fix**: Rebuild with latest DPL SDK.

### "Schema hash mismatch"

**Cause**: IDL generated from source doesn't match embedded manifest.

**Fix**:

1. Regenerate IDL: `dchat program manifest <wasm> --format json > idl.json`
2. Update expected hash in build.rs
3. Rebuild

### "Legacy program blocked by strict validation"

**Cause**: Program lacks manifest but strict mode is enabled.

**Fix**: Either migrate the program or disable strict validation.

### "Unknown import profile"

**Cause**: Program has imports from unexpected modules.

**Fix**: Ensure program compiles for `wasm32-wasi` target with DPL SDK.

## Reference: Manifest Structure

```
Offset | Size | Field              | Description
-------|------|--------------------|---------------------------------
0      | 4    | Magic              | "DPLM" (0x44, 0x50, 0x4C, 0x4D)
4      | 2    | SDK Major          | SDK major version
6      | 2    | SDK Minor          | SDK minor version
8      | 2    | SDK Patch          | SDK patch version
10     | 2    | Edition            | DPL edition (2025 = 0x07E9)
12     | 1    | ABI Version        | ABI compatibility version
13     | 1    | Import Profile     | 0=Legacy, 1=WASI, 2=Hybrid
14     | 2    | Reserved           | Must be zero
16     | 32   | Schema Hash        | BLAKE3 of canonical IDL
48     | 8    | Capabilities       | Bitflags (events, CPI, etc.)
56     | 8    | Reserved           | Must be zero
```

## Support

For migration assistance:

- Open an issue on the dchat repository
- Check the DPL SDK documentation
- Review the manifest validation test suite
