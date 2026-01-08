# 🔬 DCHAT - Decentralized Chat Network

## Executive Summary

dchat is a **cohesively integrated Rust monorepo** containing 26 crates that form a proper dependency DAG (Directed Acyclic Graph) with well-defined responsibilities. The system implements a decentralized, end-to-end encrypted chat platform with blockchain-enforced message ordering, sovereign identity, and novel cryptographic protocols.

---

## 📊 Architecture Overview

```
┌─────────────────────────────────────────────────────────────────────────────┐
│                           LAYER 7: APPLICATIONS                              │
│  ┌─────────────┐  ┌────────────────┐  ┌─────────────────┐                   │
│  │ ironclad-cli│  │ dchat-sdk-rust │  │  src/main.rs    │                   │
│  │  (Anchor-   │  │ (High-level    │  │  (Node Binary)  │                   │
│  │   style CLI)│  │  SDK wrapper)  │  │                 │                   │
│  └─────────────┘  └────────────────┘  └─────────────────┘                   │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 6: PLATFORM EXTENSIONS                          │
│  ┌─────────────┐  ┌──────────────┐  ┌──────────────┐  ┌───────────────────┐ │
│  │ dchat-bots  │  │dchat-miniapps│  │dchat-market- │  │dchat-accessibility│ │
│  │ (Bot API)   │  │ (Mini-app    │  │   place      │  │   (WCAG 2.1)      │ │
│  │             │  │  platform)   │  │ (NFT/goods)  │  │                   │ │
│  └─────────────┘  └──────────────┘  └──────────────┘  └───────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                     LAYER 5: BLOCKCHAIN & GOVERNANCE                         │
│  ┌──────────────┐  ┌─────────────┐  ┌────────────┐  ┌─────────────────────┐ │
│  │ dchat-       │  │dchat-chain  │  │dchat-      │  │ dchat-programs      │ │
│  │   blockchain │  │(Tx types)   │  │ governance │  │ (Wasm VM runtime)   │ │
│  │ (Dual-chain) │  │             │  │ (DAO/Vote) │  │                     │ │
│  └──────────────┘  └─────────────┘  └────────────┘  └─────────────────────┘ │
│  ┌──────────────┐  ┌─────────────┐  ┌────────────────────────────────────┐  │
│  │ dchat-dpl    │  │dchat-dpl-   │  │         dchat-bridge               │  │
│  │ (Anchor-like │  │   macros    │  │    (Cross-chain: Chat↔Currency     │  │
│  │  SDK)        │  │             │  │               ↔Solana)             │  │
│  └──────────────┘  └─────────────┘  └────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 4: MESSAGING & NETWORK                          │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-messaging                                │ │
│  │  (Message types, delivery proofs, ordering, rate limiting, QGE)        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-network                                  │ │
│  │  (libp2p: Kademlia DHT, Gossipsub, NAT traversal, onion routing,       │ │
│  │   epoch token issuance, relay incentives)                              │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                      LAYER 3: STORAGE & PERSISTENCE                          │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-storage                                  │ │
│  │  (SQLite local + distributed: CockroachDB, Redis, MinIO, TiKV, IPFS)   │ │
│  │  (Tiered storage, lifecycle management, backup, compression)           │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│  ┌─────────────────────┐  ┌─────────────────────────────────────────────┐  │
│  │      dchat-data     │  │              dchat-deployment               │  │
│  │ (TTL, content-ID,   │  │ (Multi-region config, relay network,        │  │
│  │  dedup, serializ.)  │  │  backup system, health monitoring)          │  │
│  └─────────────────────┘  └─────────────────────────────────────────────┘  │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 2: IDENTITY & PRIVACY                           │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-identity                                 │ │
│  │  (PoW registration, homoglyph protection, FROST threshold signing,     │ │
│  │   biometric, burner, device management, guardian recovery, MPC)        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-privacy                                  │ │
│  │  (Groth16 ZK proofs, RSA blind signatures, stealth addresses,          │ │
│  │   membership proofs, trusted setup ceremony)                           │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                         dchat-distribution                              │ │
│  │  (Package manager, gossip-based updates, version announcements)        │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                     LAYER 1: CRYPTOGRAPHIC FOUNDATION                        │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                          dchat-crypto                                   │ │
│  │  Ed25519 • X25519 • Noise Protocol (XX/XK/IK/NK) • Double Ratchet      │ │
│  │  X3DH • ML-KEM-768 (Post-Quantum) • Falcon512 • AES-256-GCM            │ │
│  │  QGE (Quorum-Gated Encryption) • Group Sender Keys • HKDF derivation   │ │
│  │  SUK (Storage Unlock Key) • BIP-39 mnemonic • Device attestation       │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
└─────────────────────────────────────────────────────────────────────────────┘
                                      │
                                      ▼
┌─────────────────────────────────────────────────────────────────────────────┐
│                        LAYER 0: CORE FOUNDATION                              │
│  ┌────────────────────────────────────────────────────────────────────────┐ │
│  │                           dchat-core                                    │ │
│  │  UserId • ChannelId • MessageId • PublicKey • Signature • Error        │ │
│  │  Motes (100M/DCHAT) • Config • Constants • EventBus                    │ │
│  └────────────────────────────────────────────────────────────────────────┘ │
│  ┌─────────────────────────────┐  ┌─────────────────────────────────────┐  │
│  │     dchat-observability     │  │          dchat-validator            │  │
│  │  (Prometheus metrics,       │  │  (Multi-region coordination,        │  │
│  │   distributed tracing,      │  │   BFT config, health probes)        │  │
│  │   alerting)                 │  │  [STANDALONE - no dchat deps]       │  │
│  └─────────────────────────────┘  └─────────────────────────────────────┘  │
│  ┌─────────────────────────────────────────────────────────────────────┐   │
│  │                          dchat-testing                              │   │
│  │  (Chaos engineering: network simulation, fault injection, recovery) │   │
│  └─────────────────────────────────────────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────────────────┘
```

---

## 🔗 Key Integration Patterns

### 1. Dual-Chain Architecture

dchat operates two parallel blockchains that work together:

| Chain | Crate | Purpose |
|-------|-------|---------|
| **Chat Chain** | `dchat-blockchain::ChatChainClient` | Identity registration, message ordering, channels, governance, reputation |
| **Currency Chain** | `dchat-blockchain::CurrencyChainClient` | Staking, transfers, fees, rewards, tokenomics |

**Cross-Chain Bridge** (`dchat-bridge`): Atomic swaps with BLS-aggregated finality proofs, Solana wDCHAT bridging, M-of-N multi-sig validation.

---

### 2. Quorum-Gated Encryption (QGE) - Novel System

**Location**: `dchat-crypto/src/qge.rs` + `dchat-network/src/epoch_tokens.rs`

```
┌───────────────────────────────────────────────────────────────────────────┐
│                           QGE MESSAGE FLOW                                 │
├───────────────────────────────────────────────────────────────────────────┤
│                                                                           │
│  1. SENDER                                                                │
│     ├── Has valid Epoch Token (from relay committee)                     │
│     ├── Derives message key from Double Ratchet                          │
│     └── Encrypts with QGE envelope                                       │
│                                                                           │
│  2. RELAY NETWORK                                                         │
│     ├── Routes encrypted message                                         │
│     ├── Issues epoch tokens (FROST threshold: 4/7 DM, 5/9 small, 7/11)  │
│     └── Earns incentives for delivery                                    │
│                                                                           │
│  3. RECIPIENT                                                             │
│     ├── Presents epoch token to decrypt                                  │
│     ├── Token expires → message becomes undecryptable                    │
│     └── Revocation = token withholding (no re-keying needed!)           │
│                                                                           │
└───────────────────────────────────────────────────────────────────────────┘
```

**Key Innovation**: Unlike Signal where revocation requires key rotation, QGE simply withholds future epoch tokens. The relay committee acts as a distributed access control layer.

---

### 3. Smart Contract Platform (DPL)

dchat has its own Solana-style smart contract runtime:

| Crate | Role |
|-------|------|
| `dchat-programs` | Host-side VM, metering, syscalls, parallel scheduler |
| `dchat-dpl` | Guest-side SDK (Anchor-style macros) |
| `dchat-dpl-macros` | `#[program]`, `#[account]`, `#[derive(Accounts)]` |
| `ironclad-cli` | CLI toolchain (like Anchor CLI) |

**Features**:
- WASM-based deterministic VM
- Compute unit metering
- Cross-Program Invocation (CPI)
- Program Derived Addresses (PDAs)
- Native programs: System, Token, Loader, Capability, Privacy, Marketplace, Bot Registry, Confidential Token

---

### 4. Cryptographic Stack Integration

```
dchat-crypto
    ├── keys.rs          → Used by ALL crates for keypairs
    ├── noise.rs         → dchat-network (transport encryption)
    ├── ratchet/         → dchat-messaging (E2E encryption)
    ├── x3dh.rs          → dchat-identity (session establishment)
    ├── qge.rs           → dchat-network + dchat-messaging
    ├── post_quantum.rs  → Future-proofing (ML-KEM-768)
    └── group_key.rs     → dchat-messaging (channel encryption)
```

---

### 5. Identity & Privacy Integration

```
dchat-identity
    ├── Uses dchat-crypto for:
    │   ├── FROST threshold signatures (guardian recovery)
    │   ├── Key derivation (device keys)
    │   └── X3DH (session establishment)
    │
    └── Uses dchat-privacy for:
        ├── ZK membership proofs
        └── Blind tokens (anonymous operations)
```

---

## 📦 Detailed Crate Breakdown

### Layer 0: Foundation

#### `dchat-core` (Zero dependencies on other dchat crates)
- **Types**: `UserId`, `ChannelId`, `MessageId`, `PublicKey`, `Signature`, `Message`, `Channel`, `UserProfile`, `NodeInfo`
- **Motes**: 1 DCHAT = 100,000,000 motes (8 decimals), MAX_SUPPLY = 100B DCHAT
- **EventBus**: Broadcast-based async pub/sub
- **Constants**: Validator thresholds (MIN=4, MAX=100, MIN_STAKE=10M), BFT thresholds, slashing rates

#### `dchat-observability` (Depends only on dchat-core)
- Prometheus-style metrics (Counter, Gauge, Histogram)
- Distributed tracing (OpenTelemetry-compatible)
- Alerting rules with routing

#### `dchat-validator` (STANDALONE - no dchat deps)
- Multi-region coordinator
- BFT configuration
- Health probes

#### `dchat-testing` (Depends only on dchat-core)
- Chaos engineering: network partition, packet loss, latency injection
- Fault injection and recovery testing

---

### Layer 1: Cryptography

#### `dchat-crypto`

| Module | Algorithm | Usage |
|--------|-----------|-------|
| `keys.rs` | Ed25519, X25519, HKDF | Universal keypair + derivation |
| `noise.rs` | Noise Protocol (XX/XK/IK/NK) | Transport encryption |
| `signatures.rs` | Ed25519 | Message/transaction signing |
| `ratchet/` | Double Ratchet + X3DH | Signal-style E2E |
| `post_quantum.rs` | ML-KEM-768 + Falcon512 | Hybrid post-quantum |
| `qge.rs` | Custom protocol | Quorum-gated encryption |
| `group_key.rs` | Sender Keys | Scalable group encryption |
| `encryption.rs` | AES-256-GCM, ChaCha20Poly1305 | AEAD |

---

### Layer 2: Identity & Privacy

#### `dchat-identity`

| Module | Feature |
|--------|---------|
| `identity.rs` | PoW registration (20-bit ~2-10s), homoglyph protection (NFKC) |
| `mpc/` | FROST threshold signing (2-of-3 guardian recovery) |
| `biometric/` | Platform biometric enrollment |
| `device/` | Multi-device management |
| `burner/` | Ephemeral identities |
| `enclave/` | Secure enclave key storage |

#### `dchat-privacy`

| Module | Technology |
|--------|------------|
| `zk_proofs/` | Groth16 (ark-*) |
| `blind_tokens/` | RSA blind signatures |
| `stealth/` | Stealth addresses |
| `membership_proof/` | Merkle membership |
| `ceremony_gen/` | Trusted setup generation |

---

### Layer 3: Storage

#### `dchat-storage`
- **Local**: SQLite with async (sqlx)
- **Distributed**: CockroachDB, Redis Cluster, MinIO S3, TiKV, IPFS
- **Features**: Tiered storage, lifecycle/TTL, compression (zstd/brotli/lz4), deduplication
- **Economics**: Storage bonds, micropayments, provider marketplace

#### `dchat-deployment`
- Multi-region validator configuration
- Relay network setup
- Backup system (S3, GCS, IPFS, local replicas)
- Health monitoring (Prometheus, Grafana)
- Auto-scaling and failover

---

### Layer 4: Network & Messaging

#### `dchat-network`
- **libp2p Stack**: Kademlia DHT, Gossipsub, mDNS, QUIC
- **NAT Traversal**: UPnP, STUN, TURN, DCUtR
- **Onion Routing**: 3-5 hop paths
- **Epoch Tokens**: FROST-based issuance
- **Rate Limiting**: Connection and message rate enforcement

#### `dchat-messaging`

| Feature | Implementation |
|---------|----------------|
| Ordering | HLC-based with blockchain anchoring |
| Delivery Proofs | Cryptographic receipts |
| Delta Sync | Bloom filter-based |
| Expiration | TTL with crypto tombstones |
| Rate Limiting | Per-user, per-channel quotas |

---

### Layer 5: Blockchain

#### `dchat-blockchain`
- Dual chain clients (Chat + Currency)
- Block hierarchy (miniblocks → subblocks → finality blocks)
- Consensus: PoRW, PoT, TSC
- Payment channels (off-chain scaling)
- Tokenomics: supply, inflation, fee distribution
- Hardened consensus: VRF committees, admission control

#### `dchat-chain`
- Transaction types for both chains
- Dispute resolution
- Insurance fund
- Merkle-based pruning
- Sharding support

#### `dchat-governance`
- Protocol DAO
- Commit-reveal voting
- Moderation system
- Abuse reporting (with ZK)
- Upgrade management

#### `dchat-bridge`
- Cross-chain synchronization
- BLS-aggregated finality proofs
- M-of-N multi-sig
- Solana SPL token bridging
- Slashing for misbehavior

#### `dchat-programs` + `dchat-dpl` + `dchat-dpl-macros`
Complete Solana-style smart contract platform with Anchor-like developer experience.

---

### Layer 6: Platform Extensions

#### `dchat-bots` (Telegram-style)
- Bot creation and registration
- Webhook management
- Command handling
- Inline queries
- Callback queries
- Permission management

#### `dchat-miniapps`
- On-chain registry with developer verification
- Client-side sandbox (iframe/WebView)
- Permission model (deny-by-default)
- Wallet integration (Intent→Execute→Receipt)
- Cross-chain UX

#### `dchat-marketplace`
- Digital goods (stickers, themes, bots, NFTs)
- Creator economy (tips, subscriptions)
- Escrow with dispute resolution
- Bot/channel ownership transfer
- Channel memberships

#### `dchat-accessibility`
- WCAG 2.1 AA+ compliance
- Screen reader support
- Keyboard navigation
- Color contrast validation
- TTS integration

---

### Layer 7: Applications

#### `ironclad-cli`
The "Anchor CLI" for dchat programs:
- `ironclad init` - scaffold project
- `ironclad build` - compile to WASM
- `ironclad test` - run tests
- `ironclad deploy` - deploy to network
- `ironclad verify` - manifest verification
- `ironclad idl` - IDL generation

#### `dchat-sdk-rust`
High-level SDK for building dchat applications:
```rust
let client = Client::builder()
    .name("Alice")
    .build()
    .await?;
client.send_message("Hello!").await?;
```

#### `src/main.rs` (Main Binary)
Complete node implementation supporting:
- **Relay mode**: Message routing with incentives
- **User mode**: Interactive chat client
- **Validator mode**: Consensus participation
- **Testnet mode**: Launch full local network

---

## 🚀 Quick Start

### Run as Relay Node
```bash
cargo run --release -- relay --listen 0.0.0.0:7070 --bootstrap /ip4/...
```

### Run as User Client
```bash
cargo run --release -- user --username alice --bootstrap /ip4/...
```

### Run as Validator
```bash
cargo run --release -- validator --key validator.json --chain-rpc http://localhost:8545
```

### Launch Local Testnet
```bash
cargo run --release -- testnet --validators 3 --relays 3 --clients 5
```

### Generate Identity
```bash
cargo run --release -- keygen --output identity.json
```

---

## 🔍 Integration Quality Assessment

### ✅ Well-Integrated Patterns

1. **Consistent Type Sharing**: All crates properly use `dchat-core` types
2. **Layered Dependencies**: Clear DAG with no circular dependencies
3. **Crypto Centralization**: All crypto flows through `dchat-crypto`
4. **Event-Driven Architecture**: `EventBus` enables loose coupling
5. **Feature Flags**: Conditional compilation for optional dependencies

### ⚠️ Potential Concerns

1. **dchat-validator is Standalone**: Has zero dchat-* dependencies, which means it can't directly use shared types. This is intentional for modularity but requires serialization bridges.

2. **Heavy dchat-blockchain**: This crate depends on 8+ other dchat crates and has extensive feature flags. Consider splitting into:
   - `dchat-blockchain-core`
   - `dchat-blockchain-client`
   - `dchat-blockchain-consensus`

3. **Main Binary Complexity**: `src/main.rs` is 15,000+ lines. Consider splitting commands into separate crate modules.

---

## 📊 Project Statistics

| Metric | Value |
|--------|-------|
| Total Crates | 27 |
| Architecture Layers | 8 |
| Cryptographic Algorithms | 12+ |
| Supported Platforms | Desktop, Mobile, VR/AR |
| Blockchain Chains | 2 (Chat + Currency) |
| External Bridge | Solana |

---

## 🎯 Conclusion

dchat is a **sophisticated, well-architected system** with clear separation of concerns:

| Aspect | Rating | Notes |
|--------|--------|-------|
| **Modularity** | ⭐⭐⭐⭐⭐ | 27 crates with clear boundaries |
| **Integration** | ⭐⭐⭐⭐ | Consistent patterns, some room for improvement |
| **Crypto Design** | ⭐⭐⭐⭐⭐ | Novel QGE system, proper post-quantum prep |
| **Scalability** | ⭐⭐⭐⭐ | Dual-chain, sharding, payment channels |
| **Documentation** | ⭐⭐⭐⭐ | Good module docs, architecture documented |

The crates form a cohesive whole where data flows naturally from core types through crypto, identity, network, messaging, storage, and finally to blockchain for ordering/finality.

---

## License

See [LICENSE](LICENSE) for details.
