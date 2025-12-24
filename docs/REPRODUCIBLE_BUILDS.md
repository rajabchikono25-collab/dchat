# DPL Reproducible Builds Guide

This document describes how to achieve reproducible builds for DPL programs, ensuring that the same source code always produces the same WASM bytecode with identical manifest hashes.

## Why Reproducible Builds Matter

For dchat programs:

1. **Security Verification**: Users can verify that deployed programs match audited source code
2. **Trustless Deployment**: Anyone can rebuild and verify the exact bytecode hash
3. **Schema Hash Stability**: The IDL schema hash embedded in the manifest must be deterministic
4. **Upgrade Safety**: Comparing bytecode hashes ensures upgrades are intentional

## Quick Start

### Deterministic Build Command

```bash
# Set reproducible build environment
export SOURCE_DATE_EPOCH=$(date +%s)
export DPL_BUILD_HOST="release"

# Build with release profile and deterministic settings
cargo build --release --target wasm32-wasi \
    -Z build-std=std,panic_abort \
    -Z build-std-features=panic_immediate_abort
```

### Verify Build Output

```bash
# Extract and verify manifest from compiled WASM
dchat program manifest target/wasm32-wasi/release/my_program.wasm

# Compare schema hash with expected value
dchat program verify-manifest \
    --program target/wasm32-wasi/release/my_program.wasm \
    --expected-hash <known_schema_hash>
```

## Environment Variables

| Variable              | Description                        | Default                  |
| --------------------- | ---------------------------------- | ------------------------ |
| `SOURCE_DATE_EPOCH`   | Unix timestamp for reproducibility | Current time             |
| `DPL_BUILD_TIMESTAMP` | Override build timestamp           | Uses `SOURCE_DATE_EPOCH` |
| `DPL_BUILD_HOST`      | Build host identifier              | Target triple            |
| `DPL_SCHEMA_HASH`     | Expected schema hash for CI        | None                     |
| `RUSTFLAGS`           | Rust compiler flags                | See below                |

## Cargo Configuration

Create `.cargo/config.toml` in your project:

```toml
[build]
# Always build for WASI target
target = "wasm32-wasi"

[target.wasm32-wasi]
# Disable incremental compilation for reproducibility
incremental = false

# Use LLD linker for consistent output
linker = "rust-lld"

[env]
# Strip debug info from release builds
CARGO_PROFILE_RELEASE_DEBUG = "false"
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS = "false"

# Deterministic panic handling
CARGO_PROFILE_RELEASE_PANIC = "abort"

# Consistent optimization level
CARGO_PROFILE_RELEASE_OPT_LEVEL = "3"
CARGO_PROFILE_RELEASE_LTO = "true"
CARGO_PROFILE_RELEASE_CODEGEN_UNITS = "1"
```

## build.rs Integration

Use the DPL build helpers for schema verification:

```rust
// build.rs
use dchat_dpl::build::{BuildConfig, generate_manifest_metadata};
use std::path::PathBuf;

fn main() {
    let config = BuildConfig {
        idl_path: Some(PathBuf::from("idl.json")),
        expected_schema_hash: std::env::var("DPL_SCHEMA_HASH").ok(),
        ..Default::default()
    };

    // This will panic if schema hash doesn't match expected value
    generate_manifest_metadata(&config)
        .expect("Failed to generate manifest metadata");

    // Re-run build if IDL changes
    println!("cargo:rerun-if-changed=idl.json");
    println!("cargo:rerun-if-changed=src/lib.rs");
}
```

## CI/CD Pipeline

### GitHub Actions Example

```yaml
name: Reproducible Build

on:
  push:
    branches: [main]
  pull_request:

env:
  SOURCE_DATE_EPOCH: "1704067200" # Fixed epoch for reproducibility
  DPL_BUILD_HOST: "github-actions"
  RUSTFLAGS: "-C link-arg=-zstack-size=65536"

jobs:
  build:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4

      - name: Install Rust
        uses: dtolnay/rust-action@stable
        with:
          targets: wasm32-wasi
          components: rust-src

      - name: Build Program
        run: |
          cargo build --release --target wasm32-wasi \
            -Z build-std=std,panic_abort

      - name: Verify Manifest
        run: |
          cargo run -- program verify-manifest \
            --program target/wasm32-wasi/release/${{ github.event.repository.name }}.wasm \
            --expected-hash ${{ vars.EXPECTED_SCHEMA_HASH }}

      - name: Compute Bytecode Hash
        id: hash
        run: |
          HASH=$(sha256sum target/wasm32-wasi/release/*.wasm | cut -d' ' -f1)
          echo "bytecode_hash=$HASH" >> $GITHUB_OUTPUT

      - name: Verify Against Known Hash
        if: github.ref == 'refs/heads/main'
        run: |
          if [ "${{ steps.hash.outputs.bytecode_hash }}" != "${{ vars.EXPECTED_BYTECODE_HASH }}" ]; then
            echo "::error::Bytecode hash mismatch!"
            echo "Expected: ${{ vars.EXPECTED_BYTECODE_HASH }}"
            echo "Actual: ${{ steps.hash.outputs.bytecode_hash }}"
            exit 1
          fi
```

## Sources of Non-Determinism

### 1. Timestamps

**Problem**: Build timestamps embedded in metadata vary between builds.

**Solution**:

```bash
export SOURCE_DATE_EPOCH=1704067200  # Use fixed timestamp
```

### 2. File System Order

**Problem**: File iteration order can vary across systems.

**Solution**: The DPL IDL generator sorts instructions and types alphabetically.

### 3. Floating-Point Operations

**Problem**: FP operations can produce different results on different CPUs.

**Solution**: Avoid floating-point in IDL generation (use integer-only hashing).

### 4. Path-Dependent Output

**Problem**: Absolute paths leak into debug info.

**Solution**:

```bash
export CARGO_PROFILE_RELEASE_DEBUG=false
```

### 5. Toolchain Version

**Problem**: Different Rust versions produce different output.

**Solution**: Pin toolchain in `rust-toolchain.toml`:

```toml
[toolchain]
channel = "1.83.0"
components = ["rust-src"]
targets = ["wasm32-wasi"]
```

### 6. Dependency Versions

**Problem**: Transitive dependency updates change output.

**Solution**: Use `Cargo.lock` and commit it to version control.

## Verification Workflow

### Local Verification

```bash
# 1. Clean build directory
cargo clean

# 2. Set reproducible environment
export SOURCE_DATE_EPOCH=$(git log -1 --format=%ct)
export DPL_BUILD_HOST="local-verify"

# 3. Build
cargo build --release --target wasm32-wasi

# 4. Compute hashes
BYTECODE_HASH=$(sha256sum target/wasm32-wasi/release/my_program.wasm | cut -d' ' -f1)
echo "Bytecode SHA-256: $BYTECODE_HASH"

# 5. Extract manifest
dchat program manifest target/wasm32-wasi/release/my_program.wasm --format json

# 6. Verify schema hash
dchat program verify-manifest \
    --program target/wasm32-wasi/release/my_program.wasm \
    --expected-hash <known_hash>
```

### Auditor Verification

For third-party auditors verifying deployed programs:

```bash
# 1. Clone repository at specific commit
git clone <repo_url>
git checkout <audit_commit>

# 2. Use exact toolchain
rustup override set $(cat rust-toolchain.toml | grep channel | cut -d'"' -f2)

# 3. Clean and rebuild
cargo clean
SOURCE_DATE_EPOCH=$(git log -1 --format=%ct) cargo build --release --target wasm32-wasi

# 4. Compare with deployed bytecode
BUILT_HASH=$(sha256sum target/wasm32-wasi/release/*.wasm | cut -d' ' -f1)
DEPLOYED_HASH=$(dchat program hash <program_address>)

if [ "$BUILT_HASH" = "$DEPLOYED_HASH" ]; then
    echo "✓ Verification successful: deployed bytecode matches source"
else
    echo "✗ Verification failed: bytecode mismatch"
    exit 1
fi
```

## IDL Schema Hash Stability

The schema hash is computed from the canonical IDL representation:

1. Instructions sorted alphabetically by name
2. Accounts sorted alphabetically within each instruction
3. Types sorted alphabetically
4. All fields use consistent naming (snake_case)
5. Optional fields use `None` rather than omission

### Computing Schema Hash Manually

```rust
use dchat_dpl::idl::Idl;
use std::fs;

fn main() {
    let idl_json = fs::read_to_string("idl.json").unwrap();
    let idl: Idl = serde_json::from_str(&idl_json).unwrap();
    let hash = idl.schema_hash();
    println!("Schema hash: {}", hex::encode(&hash));
}
```

### Comparing IDL Versions

```bash
# Generate IDL from source
dchat program idl ./target/wasm32-wasi/release/my_program.wasm > current_idl.json

# Compare with committed IDL
diff -u idl.json current_idl.json

# Compare schema hashes
COMMITTED=$(cat idl.json | dchat program schema-hash)
CURRENT=$(cat current_idl.json | dchat program schema-hash)
echo "Committed: $COMMITTED"
echo "Current: $CURRENT"
```

## Docker-Based Reproducible Builds

For maximum reproducibility, use a Docker container:

```dockerfile
# Dockerfile.build
FROM rust:1.83.0-slim

RUN rustup target add wasm32-wasi
RUN rustup component add rust-src

WORKDIR /build
COPY . .

ENV SOURCE_DATE_EPOCH=1704067200
ENV DPL_BUILD_HOST=docker
ENV CARGO_PROFILE_RELEASE_DEBUG=false
ENV CARGO_INCREMENTAL=0

RUN cargo build --release --target wasm32-wasi \
    -Z build-std=std,panic_abort

# Output hash for verification
RUN sha256sum target/wasm32-wasi/release/*.wasm
```

Build command:

```bash
docker build -f Dockerfile.build -t program-build .
docker run --rm program-build cat /build/target/wasm32-wasi/release/my_program.wasm > my_program.wasm
sha256sum my_program.wasm
```

## Troubleshooting

### Hash Mismatch After Clean Build

1. Check for `file!()` or `line!()` macros in source
2. Verify `SOURCE_DATE_EPOCH` is set consistently
3. Confirm same Rust toolchain version
4. Check for `env!("CARGO_PKG_...")` usage
5. Verify `Cargo.lock` is committed and unchanged

### Schema Hash Changed Unexpectedly

1. Check for instruction/account order changes
2. Look for type definition changes
3. Verify no new fields added/removed
4. Compare IDL files directly with `diff`

### Different Hashes on Different Machines

1. Use Docker for isolated builds
2. Pin exact Rust version in `rust-toolchain.toml`
3. Commit `Cargo.lock`
4. Use same CPU architecture (x86_64 vs ARM)

## Security Considerations

1. **Always verify schema hash before deployment**: The manifest protects against ABI drift
2. **Pin dependencies**: Use exact versions in `Cargo.toml`
3. **Audit transitive dependencies**: Use `cargo audit` and `cargo deny`
4. **Sign release artifacts**: Use GPG or Sigstore for binary signing
5. **Publish verification instructions**: Include hash in release notes

## References

- [Reproducible Builds Project](https://reproducible-builds.org/)
- [Rust Reproducible Builds](https://doc.rust-lang.org/cargo/reference/reproducible-builds.html)
- [SOURCE_DATE_EPOCH Specification](https://reproducible-builds.org/specs/source-date-epoch/)
