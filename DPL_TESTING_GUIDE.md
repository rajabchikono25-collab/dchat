# DPL Testing & Validation Suite

This directory contains comprehensive tests and examples for the Dchat Program Language (DPL) platform.

## 📦 New Additions

### 1. Token Mint Example Contract (`examples/contracts/dpl-token-mint/`)

A full-featured SPL-style token program demonstrating:

**Features:**

- Mint initialization with configurable decimals and supply caps
- Token account creation
- Minting new tokens (authority-controlled)
- Burning tokens (holder-controlled)
- Token transfers with balance checks
- Freeze/thaw authority
- Multiple validation patterns

**Build:**

```bash
cd examples/contracts/dpl-token-mint
cargo build --target wasm32-wasi --release
```

**Key Patterns Demonstrated:**

- Complex account constraints (`has_one`, `seeds`, mint constraints)
- Multiple authority roles (mint authority, freeze authority)
- Capability-based permissions
- Overflow/underflow protection
- Event emission with rich metadata
- Custom error codes

---

## 🔒 Validator Security Tests (`tests/validator_security.rs`)

Integration tests ensuring the validator properly rejects forbidden WASI calls and enforces deterministic execution.

### Forbidden WASI Call Tests

Tests that verify rejection of non-deterministic operations:

```rust
#[test]
fn test_validator_rejects_clock_time_get() { ... }

#[test]
fn test_validator_rejects_random_get() { ... }

#[test]
fn test_validator_rejects_fd_read() { ... }

#[test]
fn test_validator_rejects_socket_calls() { ... }

#[test]
fn test_validator_rejects_path_operations() { ... }
```

**What's Tested:**

- ❌ `clock_time_get` - Non-deterministic (time)
- ❌ `random_get` - Non-deterministic (RNG)
- ❌ `fd_read` - File I/O
- ❌ `sock_recv`, `sock_send` - Network access
- ❌ `path_open`, `path_*` - Filesystem operations

### Allowed WASI Call Tests

Verifies allowed deterministic operations pass validation:

```rust
#[test]
fn test_validator_allows_fd_write() { ... }

#[test]
fn test_validator_allows_proc_exit() { ... }

#[test]
fn test_validator_allows_environ_functions() { ... }
```

**What's Allowed:**

- ✅ `fd_write` - Stdout/stderr (logged deterministically)
- ✅ `proc_exit` - Exit with code
- ✅ `environ_sizes_get`, `environ_get` - Empty environment
- ✅ `args_sizes_get`, `args_get` - No arguments
- ✅ `fd_close` - Close file descriptors

### Determinism Enforcement Tests

Ensures float operations are rejected:

```rust
#[test]
fn test_validator_rejects_floats() { ... }

#[test]
fn test_validator_rejects_f64_operations() { ... }

#[test]
fn test_validator_allows_deterministic_math() { ... }
```

**Float Enforcement:**

- ❌ `f32.const`, `f32.add`, `f32.mul`, etc. - Non-deterministic IEEE-754
- ❌ `f64.const`, `f64.div`, etc. - Non-deterministic
- ✅ Integer operations (`i32`, `i64`) - Fully deterministic

### Size Limit Tests

Validates bytecode size constraints:

```rust
#[test]
fn test_validator_rejects_oversized_module() { ... }

#[test]
fn test_validator_rejects_empty_bytecode() { ... }

#[test]
fn test_validator_rejects_invalid_magic() { ... }
```

---

## 🔄 End-to-End Dispatcher Tests (`tests/dispatcher_e2e.rs`)

Tests that wire up serialized `AccountsBlob` and `instruction_data` to drive the dispatcher in unit tests.

### Test Architecture

```
┌─────────────────────────────────────────────────────────────┐
│ 1. Create SerializableAccount structs                       │
│    - Define pubkeys, balances, data, permissions            │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 2. Serialize to AccountsBlob                                │
│    - AccountsBlob::new(&accounts)                           │
│    - blob.encode() → Vec<u8>                                │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 3. Create instruction envelope                              │
│    - IxEnvelope::new(tag, payload)                          │
│    - envelope.encode() → Vec<u8>                            │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 4. Deserialize in handler                                   │
│    - AccountsBlob::decode(&bytes)                           │
│    - IxEnvelope::decode(&bytes)                             │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 5. Execute instruction with AccountsCursor                  │
│    - cursor.get_mut(0) → modify accounts                    │
│    - Enforce is_signer, is_writable constraints             │
└─────────────────────────────┬───────────────────────────────┘
                              │
                              ▼
┌─────────────────────────────────────────────────────────────┐
│ 6. Verify results                                           │
│    - Check account data changes                             │
│    - Verify balance transfers                               │
│    - Confirm error handling                                 │
└─────────────────────────────────────────────────────────────┘
```

### Mock Instruction Handler

Simulates a simple counter program with three instructions:

```rust
enum MockInstruction {
    Initialize = 0,  // Set initial counter value
    Transfer = 1,    // Transfer motes between accounts
    Increment = 2,   // Increment counter by 1
}
```

### Test Cases

#### ✅ Successful Operations

```rust
#[test]
fn test_e2e_initialize_instruction()
// - Creates counter account and authority signer
// - Serializes accounts → AccountsBlob
// - Creates Initialize instruction envelope
// - Executes and verifies counter initialized with correct value

#[test]
fn test_e2e_transfer_instruction()
// - Creates source, destination, and authority accounts
// - Transfers 25 DCHAT from source to destination
// - Verifies final balances: source 75 DCHAT, dest 75 DCHAT

#[test]
fn test_e2e_increment_instruction()
// - Counter starts at 10
// - Increments to 11
// - Verifies return value and stored value match

#[test]
fn test_e2e_multiple_instructions_sequence()
// - Initialize counter to 100
// - Increment twice (100 → 101 → 102)
// - Verifies state persistence across instructions
```

#### ❌ Error Handling

```rust
#[test]
fn test_e2e_missing_signer_fails()
// Expected: ProgramError::MissingRequiredSignature

#[test]
fn test_e2e_insufficient_funds_fails()
// Expected: ProgramError::InsufficientFunds

#[test]
fn test_e2e_readonly_account_fails()
// Expected: ProgramError::InvalidAccountData
```

---

## 🏃 Running Tests

### Run All Tests

```bash
cargo test -p dchat-programs
```

### Run Specific Test Suites

**Validator Security:**

```bash
cargo test -p dchat-programs --test validator_security
```

**Dispatcher E2E:**

```bash
cargo test -p dchat-programs --test dispatcher_e2e
```

**ABI Tests:**

```bash
cargo test -p dchat-programs --test abi_v1
```

**DPL Integration:**

```bash
cargo test -p dchat-programs --test dpl_tests
```

### Run with Logging

```bash
RUST_LOG=debug cargo test -p dchat-programs -- --nocapture
```

---

## 📊 Test Coverage

| Category                | Tests | Status  |
| ----------------------- | ----- | ------- |
| **Validator Security**  | 15+   | ✅ Pass |
| - Forbidden WASI calls  | 5     | ✅ Pass |
| - Allowed WASI calls    | 3     | ✅ Pass |
| - Float enforcement     | 3     | ✅ Pass |
| - Size limits           | 3     | ✅ Pass |
| - Import classification | 1     | ✅ Pass |
| **Dispatcher E2E**      | 7     | ✅ Pass |
| - Successful operations | 4     | ✅ Pass |
| - Error handling        | 3     | ✅ Pass |
| **ABI Parsing**         | 30+   | ✅ Pass |
| **DPL Integration**     | 20+   | ✅ Pass |

---

## 🔧 Implementation Details

### Validator Architecture

```rust
pub struct BytecodeValidator {
    config: ValidationConfig,
}

impl BytecodeValidator {
    pub fn validate(&self, bytecode: &[u8]) -> Result<ValidatedBytecode> {
        // 1. Check size limits
        // 2. Parse WASM module
        // 3. Validate imports (WASI whitelist)
        // 4. Check for forbidden opcodes (floats)
        // 5. Verify function/global counts
        // 6. Return validated bytecode
    }
}
```

### WASI Import Enforcement

```rust
pub const ALLOWED_WASI_FUNCTIONS: &[&str] = &[
    "fd_write",
    "proc_exit",
    "environ_sizes_get",
    "environ_get",
    "args_sizes_get",
    "args_get",
    "fd_prestat_get",
    "fd_prestat_dir_name",
    "fd_close",
];

pub const FORBIDDEN_WASI_IMPORTS: &[&str] = &[
    "clock_time_get",     // Non-deterministic (time)
    "random_get",         // Non-deterministic (RNG)
    "fd_read",            // File I/O
    "sock_recv",          // Network I/O
    "path_open",          // Filesystem access
    // ... 30+ forbidden functions
];
```

### AccountsBlob Wire Format

```
┌─────────────────────────────────────────────────────────────┐
│ AccountsBlob Wire Format                                     │
├─────────────────────────────────────────────────────────────┤
│ Magic (4 bytes): 0x4443484D "DCHM"                          │
│ Version (2 bytes): 0x0001                                    │
│ Account Count (2 bytes)                                      │
│ Reserved (8 bytes)                                           │
├─────────────────────────────────────────────────────────────┤
│ Table of Contents (TOC)                                      │
│   For each account (112 bytes):                              │
│     - pubkey (32 bytes)                                      │
│     - owner (32 bytes)                                       │
│     - motes (8 bytes)                                        │
│     - data_offset (4 bytes)                                  │
│     - data_len (4 bytes)                                     │
│     - flags (4 bytes): is_signer | is_writable | executable │
│     - rent_epoch (8 bytes)                                   │
│     - padding (20 bytes)                                     │
├─────────────────────────────────────────────────────────────┤
│ Account Data Payload                                         │
│   Concatenated account.data blobs                            │
└─────────────────────────────────────────────────────────────┘
```

---

## 🎯 Key Takeaways

### 1. Token Mint Example

- Demonstrates **production-ready** token program patterns
- Shows how to use DPL macros for clean, type-safe code
- Validates multiple authority roles and constraints

### 2. Validator Security Tests

- **Comprehensive WASI enforcement** - all forbidden calls rejected
- **Float operation blocking** - IEEE-754 non-determinism prevented
- **Size limit enforcement** - prevents resource exhaustion

### 3. Dispatcher E2E Tests

- **Full serialization pipeline** tested
- **Error handling validated** (missing signer, insufficient funds, readonly violations)
- **State persistence** verified across instruction sequences

---

## 📚 Related Documentation

- [DPL Architecture](../../crates/dchat-dpl/README.md)
- [ABI Specification](../../crates/dchat-programs/src/abi.rs)
- [Validation Rules](../../crates/dchat-programs/src/validation.rs)
- [WASI Shim](../../crates/dchat-programs/src/wasi_shim.rs)
- [Counter Example](../contracts/dpl-counter/src/lib.rs)

---

## 🚀 Next Steps

1. **Build token mint contract:**

   ```bash
   cd examples/contracts/dpl-token-mint
   cargo build --target wasm32-wasi --release
   ```

2. **Run validator security tests:**

   ```bash
   cargo test -p dchat-programs --test validator_security -- --nocapture
   ```

3. **Run dispatcher E2E tests:**

   ```bash
   cargo test -p dchat-programs --test dispatcher_e2e -- --nocapture
   ```

4. **Deploy to testnet:**
   ```bash
   dchat-cli deploy-program \
     --bytecode target/wasm32-wasi/release/dpl_token_mint.wasm \
     --network testnet \
     --keypair ~/.dchat/keypair.json
   ```

---

**Status**: ✅ All tests passing  
**Last Updated**: December 24, 2025  
**Maintainer**: dchat contributors
