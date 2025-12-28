# Signed Transaction Envelope Implementation

This document describes the unified signed transaction envelope system for dchat's blockchain integration.

## Overview

dchat now uses a **unified signed transaction envelope** format for both chat-chain and currency-chain transactions. This provides:

1. **Cryptographic authentication** - Ed25519 signatures prove transaction origin
2. **Replay protection** - Nonce-based replay prevention
3. **Chain isolation** - Chain ID prevents cross-chain replay attacks
4. **UserId binding** - Public key → UserId derivation is deterministic and verifiable

## Architecture

### Key Components

| Component                                   | Location                                   | Purpose                                      |
| ------------------------------------------- | ------------------------------------------ | -------------------------------------------- |
| `SignedTransactionEnvelope`                 | `dchat-chain/src/signed_envelope.rs`       | Wire format for signed transactions          |
| `EnvelopeVerifier`                          | `dchat-chain/src/signed_envelope.rs`       | Server-side verification with nonce tracking |
| `EnvelopeBuilder`                           | `dchat-chain/src/signed_envelope.rs`       | Client-side envelope construction            |
| `SignedTxClient`                            | `dchat-blockchain/src/signed_tx_client.rs` | High-level client combining wallet + signing |
| `ChatChainClient::submit_signed_envelope()` | `dchat-blockchain/src/chat_chain.rs`       | Server-side submission endpoint              |

### Envelope Format

```rust
pub struct SignedTransactionEnvelope {
    pub version: u8,           // Currently 1
    pub chain_id: u32,         // Chain identifier
    pub sender: [u8; 16],      // UserId (derived from pubkey)
    pub nonce: u64,            // Monotonically increasing
    pub domain: EnvelopeDomain, // Chat or Currency
    pub tx_type: UnifiedTransactionType,
    pub payload: Vec<u8>,      // Transaction-specific data
    pub payload_hash: [u8; 32], // BLAKE3 hash of payload
    pub public_key: [u8; 32],  // Ed25519 public key
    pub signature: [u8; 64],   // Ed25519 signature
}
```

### Signing Bytes (64 bytes fixed)

The signature is computed over a canonical 64-byte message:

| Offset | Size | Field                            |
| ------ | ---- | -------------------------------- |
| 0      | 1    | version                          |
| 1      | 4    | chain_id (LE)                    |
| 5      | 16   | sender                           |
| 21     | 8    | nonce (LE)                       |
| 29     | 1    | domain (0=Chat, 1=Currency)      |
| 30     | 2    | tx_type (discriminant + variant) |
| 32     | 32   | payload_hash                     |

### Chain IDs

```rust
pub mod chain_ids {
    pub const MAINNET_CHAT: u32 = 1337;
    pub const MAINNET_CURRENCY: u32 = 1338;
    pub const TESTNET_CHAT: u32 = 13370;
    pub const TESTNET_CURRENCY: u32 = 13380;
}
```

### UserId Derivation

```rust
/// Derive canonical UserId from Ed25519 public key
pub fn address_from_public_key(public_key: &[u8; 32]) -> Uuid {
    let hash = blake3::hash(public_key);
    let bytes: [u8; 16] = hash.as_bytes()[..16].try_into().unwrap();
    Uuid::from_bytes(bytes)
}
```

## Usage

### Client-Side (Creating and Signing)

```rust
use dchat_blockchain::wallet::{Wallet, WalletConfig};
use dchat_chain::signed_envelope::{chain_ids, EnvelopeBuilder};
use dchat_chain::TransactionType;
use dchat_crypto::MnemonicLength;

// Create wallet
let (wallet, phrase) = Wallet::create(WalletConfig::default(), MnemonicLength::Words12, None)?;

// Get keys
let public_key: [u8; 32] = *wallet.public_key().unwrap().as_bytes();
let signing_key = wallet.signing_key().unwrap();

// Build and sign envelope
let envelope = EnvelopeBuilder::new(chain_ids::TESTNET_CHAT, public_key)
    .nonce(1)
    .chat_tx(TransactionType::RegisterUser)
    .payload(serde_json::to_vec(&payload)?)
    .build_and_sign(signing_key)?;
```

### Using SignedTxClient (Recommended)

```rust
use dchat_blockchain::signed_tx_client::{SignedTxClient, SignedTxClientConfig};

// Create client with wallet
let config = SignedTxClientConfig::testnet("http://localhost:8545");
let client = SignedTxClient::new(config, wallet)?;

// Submit signed transaction
let tx_id = client.register_user("alice").await?;
```

### Server-Side (Verification)

```rust
use dchat_chain::signed_envelope::{chain_ids, EnvelopeVerifier};

let mut verifier = EnvelopeVerifier::new(chain_ids::TESTNET_CHAT);

// Verify and accept (updates nonce state)
verifier.verify_and_accept(&envelope)?;

// Or verify without accepting (idempotent)
verifier.verify(&envelope)?;
```

### ChatChainClient Integration

```rust
use dchat_blockchain::chat_chain::{ChatChainClient, ChatChainConfig};

let client = ChatChainClient::new_testnet(ChatChainConfig::default())?;

// Submit signed envelope
let tx_id = client.submit_signed_envelope(envelope).await?;
```

## Verification Checks

The `EnvelopeVerifier` performs these checks in order:

1. **Version** - Must be ENVELOPE_VERSION (1)
2. **Chain ID** - Must match verifier's chain ID
3. **Domain** - Must be valid (Chat or Currency)
4. **Payload hash** - BLAKE3(payload) must match envelope.payload_hash
5. **UserId binding** - address_from_public_key(pubkey) must match sender
6. **Nonce** - Must be greater than last seen nonce for this sender
7. **Signature** - Ed25519 verify(signing_bytes, signature, pubkey)

## Migration Path

### Backward Compatibility

Existing unsigned transaction methods remain functional:

- `ChatChainClient::register_user(user_id, public_key)` - Still works (unsigned)
- `ChatChainClient::submit_signed_envelope(envelope)` - New signed path

### Recommended Migration

1. **Phase 1**: Add signed envelope support (current)
2. **Phase 2**: Deprecate unsigned methods with warnings
3. **Phase 3**: Require signatures for all transactions
4. **Phase 4**: Remove unsigned code paths

## Testing

Integration tests are in `crates/dchat-blockchain/tests/signed_envelope_integration.rs`:

```bash
cargo test --package dchat-blockchain --test signed_envelope_integration --features test-mocks
```

Test coverage:

- ✅ Address derivation determinism
- ✅ Address uniqueness
- ✅ Wallet key extraction
- ✅ Envelope creation and signing
- ✅ Envelope verification
- ✅ Nonce replay protection
- ✅ Wrong chain ID rejection
- ✅ ChatChainClient integration

## Security Considerations

1. **Nonce Storage**: In production, nonces should be persisted to survive restarts
2. **Clock Skew**: Consider adding timestamp validation to prevent old envelope replay
3. **Key Rotation**: Implement key rotation protocol for long-lived identities
4. **Rate Limiting**: Apply per-sender rate limits based on nonce velocity

## Files Changed

| File                                                           | Change                                         |
| -------------------------------------------------------------- | ---------------------------------------------- |
| `crates/dchat-chain/src/signed_envelope.rs`                    | NEW - Core envelope types and verification     |
| `crates/dchat-chain/src/lib.rs`                                | Export signed_envelope module                  |
| `crates/dchat-blockchain/src/signed_tx_client.rs`              | NEW - Client-side signing helper               |
| `crates/dchat-blockchain/src/chat_chain.rs`                    | Add submit_signed_envelope(), chain_id support |
| `crates/dchat-blockchain/src/lib.rs`                           | Export signed_tx_client module                 |
| `crates/dchat-blockchain/tests/signed_envelope_integration.rs` | NEW - Integration tests                        |
