# Ironclad CLI - DPL Smart Contract Framework

**Ironclad** is the Anchor-like development framework for dchat smart contracts.

## Overview

Ironclad provides a complete toolkit for building, testing, and deploying DPL (dchat Program Language) smart contracts:

- 🏗️ **Project scaffolding** - Initialize projects from templates
- 🔨 **Build pipeline** - Compile Rust to WASM with manifest verification
- 🧪 **Testing** - Run unit and integration tests
- 📄 **IDL generation** - Extract Interface Definition Language from WASM
- 🚀 **Deployment** - Deploy to localnet, devnet, testnet, or mainnet
- ✅ **Verification** - Verify manifest integrity and schema hashes

## Installation

```bash
cd crates/ironclad-cli
cargo install --path .
```

Or run directly from the workspace:

```bash
cargo build -p ironclad-cli
# Then use: target/debug/ironclad
```

## Quick Start

### Initialize a New Project

```bash
# Create from template
ironclad init my-program --template counter

cd my-program
```

**Available templates:**

- `counter` - Simple state counter
- `escrow` - Multi-party escrow
- `token` - Fungible token implementation
- `custom` - Empty template

### Build

```bash
# Development build
ironclad build

# Release build with verification
ironclad build --release --verify
```

### Test

```bash
ironclad test

# With verbose output
ironclad test --verbose
```

### Verify

```bash
ironclad verify

# With expected schema hash
ironclad verify --expected-hash <hash>
```

### Deploy

```bash
# Deploy to devnet
ironclad deploy --network devnet

# Deploy to mainnet with custom keypair
ironclad deploy --network mainnet --keypair ~/.dchat/my-keypair.json
```

### IDL Operations

```bash
# Extract IDL from WASM
ironclad idl extract

# Show IDL hash
ironclad idl hash idl/my-program.json

# Validate IDL
ironclad idl validate idl/my-program.json

# Generate TypeScript client (future)
ironclad idl generate idl/my-program.json --language typescript
```

### Program Info

```bash
# Show local project info
ironclad info

# Query deployed program
ironclad info --program <program-id> --network mainnet
```

### Configuration

```bash
# Show current config
ironclad config show

# Set configuration value
ironclad config set project.name my-program
ironclad config set build.generate_idl true

# Get configuration value
ironclad config get project.version
```

### Clean

```bash
# Remove build artifacts
ironclad clean
```

## Project Structure

```
my-program/
├── Cargo.toml
├── Ironclad.toml       # Project configuration
├── build.rs            # DPL build script
├── src/
│   └── lib.rs          # Program code
├── tests/
│   └── integration.rs
├── idl/                # Generated IDL files
│   └── my-program.json
└── target/
    └── wasm32-wasip1/
        └── release/
            └── my_program.wasm
```

## Configuration File (Ironclad.toml)

```toml
[project]
name = "my-program"
version = "0.1.0"
program_id = "DcHaT..."  # Set after first deployment
description = "My awesome smart contract"
authors = ["Your Name"]

[build]
target = "wasm32-wasip1"
generate_idl = true
verify_schema = false

[networks.localnet]
url = "http://localhost:8545"
ws_url = "ws://localhost:8546"
keypair = "~/.dchat/keypair.json"
confirm = true

[networks.devnet]
url = "https://devnet.dchat.network"
ws_url = "wss://devnet.dchat.network/ws"
confirm = true

[networks.mainnet]
url = "https://mainnet.dchat.network"
ws_url = "wss://mainnet.dchat.network/ws"
confirm = true

[test]
timeout = 300
parallel = true

[idl]
output_dir = "idl"
generate_ts = false
ts_output_dir = "ts-client"
```

## Using with Existing Examples

The ironclad CLI works with the existing DPL examples in this repository:

```bash
# Navigate to an example
cd examples/contracts/dpl-escrow-ironclad

# Build
ironclad build --release

# Test
ironclad test

# Verify
ironclad verify

# Show info
ironclad info
```

**Note**: The `dpl-escrow-ironclad` example is already set up as a proper Ironclad project with `Ironclad.toml`.

## Command Reference

| Command                | Description            | Example                                                |
| ---------------------- | ---------------------- | ------------------------------------------------------ |
| `ironclad init <name>` | Initialize new project | `ironclad init my-token --template token`              |
| `ironclad build`       | Build smart contract   | `ironclad build --release --verify`                    |
| `ironclad test`        | Run tests              | `ironclad test --verbose`                              |
| `ironclad verify`      | Verify manifest        | `ironclad verify --expected-hash <hash>`               |
| `ironclad idl extract` | Extract IDL from WASM  | `ironclad idl extract --output idl/program.json`       |
| `ironclad idl hash`    | Show IDL hash          | `ironclad idl hash idl/program.json`                   |
| `ironclad deploy`      | Deploy program         | `ironclad deploy --network devnet`                     |
| `ironclad upgrade`     | Upgrade program        | `ironclad upgrade --program-id <id> --network mainnet` |
| `ironclad info`        | Show program info      | `ironclad info --program <id> --network mainnet`       |
| `ironclad config show` | Show configuration     | `ironclad config show`                                 |
| `ironclad clean`       | Clean build artifacts  | `ironclad clean`                                       |

## Development Workflow

1. **Initialize**: `ironclad init my-program`
2. **Develop**: Edit `src/lib.rs`
3. **Build**: `ironclad build`
4. **Test**: `ironclad test`
5. **Verify**: `ironclad verify`
6. **Deploy**: `ironclad deploy --network devnet`
7. **Upgrade**: `ironclad upgrade --program-id <id>`

## Features

### ✅ Implemented

- Project initialization with templates
- WASM build pipeline
- Test runner
- Manifest verification
- IDL extraction
- Program info display
- Configuration management
- Build artifact cleanup

### 🚧 In Progress

- Network deployment (RPC integration)
- Program upgrades
- IDL-based client generation

### 📋 Planned

- Interactive deployment wizard
- Reproducible builds
- Security auditing
- Gas optimization analysis

## Comparison to Anchor (Solana)

| Feature          | Anchor          | Ironclad          |
| ---------------- | --------------- | ----------------- |
| **Language**     | Rust            | Rust              |
| **Target**       | BPF/eBPF        | WASM              |
| **Init**         | `anchor init`   | `ironclad init`   |
| **Build**        | `anchor build`  | `ironclad build`  |
| **Test**         | `anchor test`   | `ironclad test`   |
| **Deploy**       | `anchor deploy` | `ironclad deploy` |
| **IDL**          | Automatic       | Automatic         |
| **Verification** | `anchor verify` | `ironclad verify` |

## Troubleshooting

### "No Ironclad.toml found"

Run `ironclad config init` in your project root, or run `ironclad init` to create a new project.

### "WASM file not found"

Run `ironclad build --release` before deploying or verifying.

### "Manifest not found in WASM"

Ensure you're using the DPL SDK with proper `build.rs` configuration. See existing examples for reference.

## Examples

See [examples/contracts/dpl-escrow-ironclad/](../../examples/contracts/dpl-escrow-ironclad/) for a complete working example.

## Contributing

Ironclad is part of the dchat project. Contributions are welcome!

## License

MIT OR Apache-2.0

---

**Built with ❤️ for the dchat ecosystem**
