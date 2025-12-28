# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [Unreleased]

## [0.1.1] - 2025-01-03

### Fixed

- **IDL Generation**: Complete rewrite of IDL extraction to parse Rust source files instead of attempting to extract from WASM manifest
  - Now properly extracts instructions, accounts, types, events, and errors from `src/lib.rs`
  - Parses `#[program]` module to extract instruction functions and their arguments
  - Parses `#[derive(Accounts)]` structs to extract account context definitions
  - Parses `#[account]` structs to extract state type definitions with discriminators
  - Parses `#[event]` structs to extract event definitions with fields and discriminators
  - Parses `#[error_code]` enums to extract error codes and messages
  - Parses borsh-serializable enums as type definitions
  - Computes FNV-1a discriminators matching the DPL macro implementation

### Added

- Dependencies: `syn`, `quote`, `proc-macro2` for Rust source parsing

## [0.1.0] - 2025-12-28

### Added

- Initial release of Ironclad CLI
- `ironclad init` - Initialize new DPL smart contract projects with templates
  - Counter template for basic state management
  - Escrow template for multi-party conditional transfers
  - Token template for fungible token implementation
  - Custom/empty template for starting from scratch
- `ironclad build` - Build smart contracts to WASM
  - Support for debug and release builds
  - Automatic IDL generation
  - Schema verification
- `ironclad test` - Run unit and integration tests
- `ironclad verify` - Verify program manifest and schema hashes
- `ironclad deploy` - Deploy programs to dchat networks
  - Support for localnet, devnet, testnet, and mainnet
- `ironclad upgrade` - Upgrade existing deployed programs
- `ironclad idl` - IDL management commands
  - Extract IDL from WASM binaries
  - Compute IDL schema hashes
  - Validate IDL files
  - Generate TypeScript clients (experimental)
- `ironclad info` - Display program information
- `ironclad config` - Configuration management
- `ironclad clean` - Clean build artifacts
- `ironclad completions` - Generate shell completions for bash, zsh, fish, PowerShell
- Project configuration via `Ironclad.toml`
- Multi-network configuration support
- Colored terminal output with progress indicators

### Changed

- Default WASM target changed to `wasm32-wasip1` for better WASI support

### Security

- All network communications use HTTPS
- Keypair files are validated before use
- Manifest verification prevents deployment of tampered programs

[Unreleased]: https://github.com/dchat/dchat/compare/ironclad-cli-v0.1.0...HEAD
[0.1.0]: https://github.com/dchat/dchat/releases/tag/ironclad-cli-v0.1.0
