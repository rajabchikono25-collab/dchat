# Counter Contract Example

A simple counter smart contract demonstrating dchat's WebAssembly-based contract system.

## Features

- **Initialize**: Set up a new counter account
- **Increment**: Add to the counter (with overflow protection)
- **Decrement**: Subtract from the counter (with underflow protection)
- **Set**: Set counter to a specific value

## Building

```bash
# Install WASM target (one-time)
rustup target add wasm32-unknown-unknown

# Build the contract
cargo build --target wasm32-unknown-unknown --release
```

The compiled WASM will be at:

```
target/wasm32-unknown-unknown/release/counter_contract.wasm
```

## Contract Size

The optimized contract is only **~0.8 KB** thanks to:

- `no_std` (no standard library)
- LTO (link-time optimization)
- Size optimization (`opt-level = "z"`)
- Symbol stripping

## Account State Layout

The counter stores its state in account data with the following layout:

| Offset | Size | Field     | Description                 |
| ------ | ---- | --------- | --------------------------- |
| 0      | 4    | Magic     | `[0xC0, 0x55, 0x4E, 0x54]`  |
| 4      | 1    | Version   | State version (currently 1) |
| 5      | 8    | Counter   | u64 counter value (LE)      |
| 13     | 32   | Authority | Owner pubkey (reserved)     |

Total: 45 bytes

## Instruction Format

Instructions are simple byte sequences:

| Tag | Instruction | Payload              |
| --- | ----------- | -------------------- |
| 0   | Initialize  | None                 |
| 1   | Increment   | Optional: u64 amount |
| 2   | Decrement   | Optional: u64 amount |
| 3   | Set         | Required: u64 value  |

## Error Codes

| Code | Meaning                 |
| ---- | ----------------------- |
| 0    | Success                 |
| 1    | Invalid instruction     |
| 2    | Account not initialized |
| 3    | Already initialized     |
| 4    | Overflow                |
| 5    | Underflow               |
| 6    | Invalid account count   |
| 7    | Insufficient data       |

## Testing

The contract is tested using the dchat-programs VM:

```bash
# From the dchat root directory
cargo test -p dchat-programs --test counter_contract
```

## Security Notes

This is an **example contract** for educational purposes. Production contracts should:

1. Verify account ownership before modifications
2. Use proper signature verification
3. Implement proper account validation
4. Use the full dchat guest SDK for production features

## License

MIT OR Apache-2.0
