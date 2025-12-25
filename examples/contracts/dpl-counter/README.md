# DPL Counter Contract

A simple counter smart contract built with the **dchat Program Language (DPL)** framework, demonstrating best practices for wasmi/wasip1 runtime execution.

## Overview

This contract implements a simple counter with:

- **Initialize**: Create a new counter with an initial value
- **Increment**: Add 1 to the counter (anyone can call)
- **Decrement**: Subtract 1 from the counter (anyone can call)
- **Set**: Set counter to a specific value (authority only)
- **Reset**: Reset counter to zero (authority only)
- **Transfer Authority**: Change the counter's owner

## Prerequisites

1. **Rust toolchain** with `wasm32-wasip1` target:

   ```bash
   rustup target add wasm32-wasip1
   ```

2. **Ironclad CLI** (optional, for streamlined workflow):
   ```bash
   cargo install --path ../../crates/ironclad-cli
   ```

## Build

### Using Cargo directly (for wasmi/wasip1):

```bash
# Development build
cargo build --target wasm32-wasip1

# Release build (optimized for deployment)
cargo build --target wasm32-wasip1 --release
```

The WASM artifact will be at:

- Debug: `target/wasm32-wasip1/debug/dpl_counter.wasm`
- Release: `target/wasm32-wasip1/release/dpl_counter.wasm`

### Using Ironclad CLI:

```bash
# Development build
ironclad build

# Release build with verification
ironclad build --release --verify
```

## Test

```bash
# Run unit tests
cargo test

# Run with Ironclad
ironclad test
```

## Verify

Verify the contract manifest and schema hash:

```bash
ironclad verify

# Or with expected hash
ironclad verify --expected-hash <hash>
```

## Deploy

```bash
# Deploy to devnet
ironclad deploy --network devnet

# Deploy to testnet
ironclad deploy --network testnet

# Deploy to mainnet (requires keypair)
ironclad deploy --network mainnet --keypair ~/.config/dchat/keypair.json
```

## Contract Architecture

### State

```rust
#[account]
pub struct Counter {
    pub value: u64,           // Current counter value
    pub authority: Pubkey,    // Owner who can set/reset
    pub bump: u8,             // PDA bump seed
    pub total_operations: u64, // Operation count
}
```

### Instructions

| Tag | Instruction         | Description    | Access         |
| --- | ------------------- | -------------- | -------------- |
| 0   | `Initialize`        | Create counter | Anyone         |
| 1   | `Increment`         | Add 1          | Anyone         |
| 2   | `Decrement`         | Subtract 1     | Anyone         |
| 3   | `Set`               | Set value      | Authority only |
| 4   | `Reset`             | Set to zero    | Authority only |
| 5   | `TransferAuthority` | Change owner   | Authority only |

### Events

- `CounterInitialized`: Emitted when counter is created
- `CounterChanged`: Emitted on increment/decrement/set
- `CounterReset`: Emitted when counter is reset
- `AuthorityTransferred`: Emitted when ownership changes

### Errors

| Code | Name           | Description                    |
| ---- | -------------- | ------------------------------ |
| 6000 | `Overflow`     | Counter would exceed max value |
| 6001 | `Underflow`    | Counter cannot go below zero   |
| 6002 | `Unauthorized` | Caller is not the authority    |

## wasmi/wasip1 Compatibility

This contract is designed for the **wasmi** runtime with **WASI preview 1** support:

- **Deterministic execution**: No floating-point operations
- **Safe WASI subset**: Only allowed imports (`fd_write`, `proc_exit`, etc.)
- **Consensus-safe**: All operations are reproducible across nodes

### Allowed WASI Functions

- `fd_write` - Logging to stdout/stderr
- `proc_exit` - Exit the program
- `environ_*` - Environment (returns empty)
- `args_*` - Arguments (returns empty)
- `fd_close` - Close file descriptors

### Forbidden (will fail validation)

- `clock_time_get` - Non-deterministic time
- `random_get` - Non-deterministic RNG
- `fd_read` - File I/O
- `path_*` - Filesystem access
- `sock_*` - Network access

## Example Usage

```typescript
// TypeScript client example
import { Counter } from "./idl/dpl_counter";

// Initialize a new counter
await program.methods
  .initialize(new BN(100))
  .accounts({
    counter: counterPDA,
    authority: wallet.publicKey,
    systemProgram: SystemProgram.programId,
  })
  .rpc();

// Increment
await program.methods.increment().accounts({ counter: counterPDA }).rpc();

// Get current value
const account = await program.account.counter.fetch(counterPDA);
console.log("Counter value:", account.value.toNumber());
```

## License

MIT
