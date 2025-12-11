# Copilot Instructions for dchat

## Project Status: PRODUCTION

**dchat is now in PRODUCTION. All code must be production-ready.**

- Do NOT write placeholder/stub code
- Do NOT add "In production:" or "TODO:" comments for future implementation
- Do NOT use simulated/mock implementations
- All features must be fully implemented with real integrations
- All error handling must be comprehensive
- All security measures must be active

## Project Overview

**dchat** is a Rust-based decentralized chat application combining end-to-end encryption, sovereign identity, and blockchain-enforced message ordering. It runs on a parallel chain (chat chain) alongside a currency chain for economics. Key differentiators: wallet-invisible UX, zero-knowledge metadata protection, relay incentives, and decentralized governance via DAO.

## Critical Reference Documents

Always consult these documents when working on dchat:

- **`ARCHITECTURE-2.0.md`**: The authoritative architecture specification. Contains detailed component designs, security requirements, and integration patterns. Use this as the primary reference for system design decisions.
- **`plan.md`**: The implementation plan capturing remaining work items, stubbed/placeholder code, and phased delivery. Check this before implementing any feature to understand current state and gaps.
- **`plan2.md`**: Economic infrastructure implementation plan covering critical gaps in staking, payment channels, fee collection, and reward distribution. Contains prioritized work items (P0/P1/P2) with time estimates and specific code changes needed.
- **`plan4.md`**: **🚨 CRITICAL FOR MAINNET LAUNCH** - Production readiness plan identifying ~146 items requiring implementation before mainnet. Covers:
  - 83 "In production..." placeholder comments that MUST be replaced
  - Mock/simulation code that MUST be removed or guarded
  - Debug-only code paths that MUST NOT reach production
  - Hardcoded values requiring configuration
  - Feature flags requiring production review
  - Build verification checklist

  **⚠️ HANDLE WITH EXTREME CARE**: When implementing items from plan4.md:
  1. Never introduce new placeholder code
  2. Always verify mock code is properly `#[cfg(test)]` or `#[cfg(debug_assertions)]` guarded
  3. Test that release builds fail if test-mocks feature is enabled
  4. Ensure HTTPS enforcement is active in all production RPC calls
  5. Verify all cryptographic operations use production keys, not test keys
  6. Run the Production Build Verification Checklist before any release

- **`ARCHITECTURE.md`**: Legacy architecture document with component breakdown and threat model.
- **`SECURITY_AUDIT_2025-12-08.md`**: Comprehensive security audit report identifying 16 vulnerabilities (2 critical, 4 high, 6 medium, 4 low) with detailed patches, anti-bot protection gaps, and missing CLI commands. Must be consulted for all security-related changes.

**Priority**: When there are discrepancies, `ARCHITECTURE-2.0.md` takes precedence over `ARCHITECTURE.md`.

## Architecture (Summary)

The system has two interdependent chains:

- **Chat Chain**: Identity, messaging, channels, permissions, governance, reputation, account recovery
- **Currency Chain**: Payments, staking, rewards, economics

Core components (34 architectural subsystems):

- **Crypto**: Noise Protocol (rotating keys), Ed25519 identity, ZK proofs, blind tokens
- **Identity Management**: Hierarchical key derivation (BIP-32/44), multi-device sync, linkability control, Sybil resistance
- **Messaging**: Delay-tolerant delivery, DHT routing (libp2p/Kademlia), proof-of-delivery rewards
- **Channels**: On-chain creation, token-gated access, creator economy, staking moderation
- **Privacy & Metadata Resistance**: Stealth payloads, contact graph hiding, onion routing, Sphinx packets, cover traffic
- **Governance**: DAO voting, decentralized moderation, slashing mechanisms, anonymous abuse reporting
- **Relay Network**: Incentivized nodes with uptime scoring and staking rewards
- **Account Recovery**: Multi-signature guardian system with timelocked recovery and social recovery backup
- **Network Resilience**: NAT traversal (UPnP/TURN), eclipse attack prevention, multi-path routing, BGP hijack defense
- **Scalability**: Channel-scoped sharding, state channels, BLS signature aggregation, rollup-style batching
- **Rate Limiting**: Reputation-based QoS, adaptive traffic control, congestion pricing
- **Dispute Resolution**: Cryptographic fork arbitration, message integrity verification, claim-challenge-respond
- **Cross-Chain Bridge**: Atomic swaps, dual-chain state synchronization, finality tracking
- **Observability**: Distributed tracing, Prometheus metrics, network health dashboards, chaos testing, fuzz testing
- **Accessibility**: WCAG 2.1 AA+ compliance, screen readers, keyless UX (enclave/MPC), optional VR/AR
- **Keyless UX**: Biometric authentication, secure enclave (TEE), MPC signers, no password required
- **Privacy-First Accessibility**: Local processing for transcription/OCR, zero telemetry, neurodivergence support, RTL/CJK support
- **Regulatory Compliance**: Client-side encrypted analysis, Bloom filters, decentralized moderation, law enforcement warrants
- **Data Lifecycle**: Message expiration policies, deduplication, storage economics, encrypted backup
- **Protocol Upgrades**: Semantic versioning, cryptographic agility, staged rollouts, post-quantum migration
- **User Safety & Trust**: Proof-of-device, verified badges, context-aware warnings, reputation display
- **Developer Ecosystem**: Plugin API (WebAssembly sandbox), open SDKs (Rust/TS/Go/Python), testnet infrastructure
- **Economic Security**: Game-theoretic relay fairness, token-draining protection, insurance fund, sustainability modeling
- **Post-Quantum Cryptography**: Hybrid Curve25519+Kyber768 now, full PQ transition by 2030, harvest-now-decrypt-later defense
- **Censorship Resistance**: F-Droid/IPFS/Bittorrent distribution, decentralized bootstrap, gossip-based updates
- **Disaster Recovery**: Full chain replay, snapshot checkpoints, erasure coding (Reed-Solomon), distributed backups
- **Progressive Decentralization**: Centralized entry point, feature unlock progression, trust bridge, reputation migration
- **Formal Verification**: TLA+ consensus specs, Coq crypto proofs, continuous fuzzing (Libfuzzer, AFL++), runtime monitors
- **Ethical Governance**: Voting power caps (5%), term limits, diversity requirements, immutable action logs, appeal rights

## Development Workflow

### Setup

```bash
cargo build
cargo test
```

### Running the Project

```bash
# Start a relay node
cargo run --release -- --role relay

# Start a user node (interactive chat)
cargo run -- --role user
```

### Testing

```bash
# Run all tests
cargo test

# Run tests with logging
RUST_LOG=debug cargo test -- --nocapture

# Integration tests (requires local chain)
cargo test --test integration_tests
```

## Code Conventions

### Cryptography

- All inter-node encryption uses Noise Protocol via `snow` crate
- Ed25519 keys for identity; Curve25519 for DH
- Key rotation: new keys after N messages or T time units (see `src/crypto/rotation.rs`)
- Never log plaintext keys; use `std::fmt::Debug` guards

### Messaging

- Message ordering enforced by chain sequence numbers in `src/messaging/order.rs`
- Relay nodes call `deliver_proof::submit_on_chain()` after successful delivery
- Offline messages queued in local SQLite; sync on reconnect via gossip

### Chains

- Chat chain calls encapsulated in `src/chain/chat_chain/`
- Currency chain calls encapsulated in `src/chain/currency_chain/`
- Cross-chain calls use bridge layer in `src/bridge/` with atomicity guarantees

### Identity & Reputation

- One user = multiple potential identities (main + burners)
- Reputation stored on-chain but derived locally via `reputation::Score::from_chain_data()`
- Burner identities have zero persistent reputation

## Key Files & Directories

- **`ARCHITECTURE.md`**: Complete system specification and design rationale (34 components, threat model, roadmap)
- **`src/crypto/`**: Noise Protocol integration, key derivation, rotating keys
- **`src/crypto/post_quantum/`**: Kyber768, FALCON, Dilithium hybrid schemes
- **`src/crypto/hybrid/`**: Classical + post-quantum hybrid implementation
- **`src/identity/`**: Hierarchical key derivation (BIP-32/44), multi-device sync, device attestation, Sybil resistance
- **`src/identity/verification/`**: Trust proofs and verified identity badges
- **`src/chain/chat_chain/identity/`**: Identity registration, on-chain verification, reputation tracking
- **`src/chain/guardians/`**: Multi-signature account recovery system, timelocked reversals
- **`src/chain/snapshots/`**: State snapshots and Merkle proof verification
- **`src/chain/recovery/`**: Leader election and consensus recovery
- **`src/messaging/`**: Message creation, ordering, proof-of-delivery
- **`src/channels/`**: Channel ownership, governance, access control
- **`src/relay/`**: Relay node logic, incentive tracking, uptime scoring
- **`src/governance/abuse_reporting/`**: Decentralized abuse moderation, ZK proof encryption
- **`src/governance/moderation/`**: Governance voting on moderation
- **`src/governance/ethics/`**: Ethical constraints, term limits, diversity, sortition
- **`src/governance/transparency/`**: Immutable action logs, slashing logs
- **`src/governance/constraints/`**: Voting power caps and anti-centralization
- **`src/compliance/`**: Regulatory compliance and hash-proof systems
- **`src/compliance/hash_proofs/`**: CSAM detection, Bloom filters
- **`src/network/nat/`**: UPnP and TURN NAT traversal, hole punching
- **`src/network/onion_routing/`**: Metadata-resistant multi-hop routing, Sphinx packets, circuit management
- **`src/network/resilience/`**: Automatic failover, partition detection, bridge activation
- **`src/network/rate_limiting/`**: Reputation-based QoS and congestion control
- **`src/network/eclipse_prevention/`**: Multi-path routing, sybil guards, ASN diversity
- **`src/network/bootstrap/`**: Decentralized bootstrap and seed node diversity
- **`src/chain/sharding/`**: Channel-scoped subnetworks and state partitioning
- **`src/chain/dispute_resolution/`**: Cryptographic fork arbitration and evidence
- **`src/privacy/`**: ZK proofs, blind tokens, contact graph hiding, metadata obfuscation
- **`src/privacy/encrypted_analysis/`**: Encrypted metadata analysis (SMPC)
- **`src/storage/`**: SQLite/RocksDB backends, local caching, backup sync
- **`src/storage/lifecycle/`**: Message TTL, expiration, deduplication
- **`src/storage/economics/`**: Storage bonds and micropayments
- **`src/storage/backup/`**: Encrypted cloud backup with zero-knowledge
- **`src/recovery/`**: Account recovery and disaster recovery procedures
- **`src/recovery/chain_replay/`**: Full chain replay from genesis
- **`src/recovery/distributed_backup/`**: Distributed backup coordination
- **`src/recovery/erasure_coding/`**: Reed-Solomon erasure coding
- **`src/bridge/`**: Cross-chain atomic transactions and state synchronization
- **`src/observability/`**: Prometheus metrics, distributed tracing, health dashboards, chaos tests
- **`src/accessibility/`**: WCAG compliance, screen readers, VR/AR interfaces, neurodivergence modes
- **`src/onboarding/keyless/`**: Biometric flows, enclave integration, MPC signers
- **`src/onboarding/progressive/`**: Progressive decentralization and feature gates
- **`src/ui/`**: Frontend interfaces (embedded or separate)
- **`src/ui/trust/`**: Trust proofs and verification badges
- **`src/ui/safety_warnings/`**: Context-aware phishing and scam detection
- **`src/ui/education/`**: In-app education on privacy and governance
- **`src/i18n/`**: RTL language support, CJK input methods, cultural calendars
- **`src/plugins/`**: Third-party plugin system and API
- **`src/plugins/sandbox/`**: WebAssembly/JVM plugin sandboxing
- **`src/upgrades/`**: Protocol versioning and cryptographic agility
- **`src/upgrades/versioning/`**: Semantic versioning and negotiation
- **`src/upgrades/pq_migration/`**: Post-quantum migration roadmap
- **`src/distribution/`**: Censorship-resistant app distribution
- **`src/distribution/package_hosting/`**: IPFS, Bittorrent, APK repos
- **`src/distribution/mirrors/`**: Mirror network synchronization
- **`src/economics/relay/`**: Relay payment fairness and incentives
- **`src/economics/security/`**: Game-theoretic economic analysis
- **`src/verification/`**: Formal verification and continuous security
- **`src/verification/formal/`**: TLA+/Coq formal specifications
- **`src/verification/continuous/`**: Continuous fuzzing infrastructure
- **`sdk/rust/`**: Rust SDK for client and relay implementation
- **`sdk/typescript/`**: TypeScript/JavaScript SDK for web clients
- **`tests/testnet/`**: Public and chaos testnet infrastructure
- **`tests/game_theory/`**: Economic and game-theoretic simulations
- **`tests/disaster_recovery/`**: Disaster recovery scenario testing
- **`docs/verification/`**: Formal verification specifications (TLA+, Coq)

## Integration Points

- **Blockchain RPC**: Connects to both chat and currency chain validators
- **libp2p DHT**: Peer discovery; seeded with well-known relay node addresses
- **IPFS**: Optional media hosting (sticker packs, digital goods)
- **WebRTC**: Encrypted P2P voice via data channels
- **External Wallets**: Optional integration; users can sign via Ledger-style signers

## Common Tasks

### Adding a New Message Type

1. Define struct in `src/messaging/types.rs`
2. Add encryption/decryption in `src/crypto/handshake.rs`
3. Add chain ordering entry in `src/chain/chat_chain/ordering.rs`
4. Add relay handler in `src/relay/handlers.rs`
5. Add tests in `tests/messaging_*`

### Implementing a New Governance Vote Type

1. Define vote struct in `src/governance/voting.rs`
2. Add vote validation in `src/governance/validators.rs`
3. Add execution logic in `src/governance/execute.rs`
4. Add UI integration point (document in `src/ui/`)

### Adding Relay Reward Logic

1. Add reward calculation in `src/relay/rewards/calculator.rs`
2. Update staking contract in `src/chain/currency_chain/staking.rs`
3. Add proof-of-delivery verification in `src/relay/proof.rs`
4. Test with local chain simulator in `tests/relay_incentives.rs`

### Cross-Chain Transaction

1. Initiate on source chain via bridge in `src/bridge/initiate.rs`
2. Wait for finality (see `src/bridge/finality.rs`)
3. Execute on destination chain via `src/bridge/execute.rs`
4. Verify atomicity: if either fails, both rollback

### Implementing Account Recovery via Guardians

1. Define guardian struct in `src/recovery/types.rs`
2. Add on-chain guardian registration in `src/chain/guardians/register.rs`
3. Implement recovery initiation in `src/recovery/initiate.rs`
4. Add timelocked verification in `src/recovery/timelock.rs`
5. Create ZK proof verification in `src/privacy/guardian_proofs.rs`
6. Test social recovery fallback path

### Adding NAT Traversal Support

1. Integrate UPnP in `src/network/nat/upnp.rs`
2. Add TURN fallback in `src/network/nat/turn.rs`
3. Implement hole punching in libp2p config
4. Add eclipse attack prevention in `src/network/relay_selection/diversity.rs`
5. Test with restricted firewall simulation

### Implementing Rate Limiting

1. Define reputation scoring in `src/network/rate_limiting/peer_score.rs`
2. Add token bucket algorithm in `src/network/rate_limiting/bucket.rs`
3. Implement backpressure signaling in `src/network/congestion/signals.rs`
4. Add spam detection in `src/network/congestion/anomaly.rs`
5. Integrate with message prioritization in `src/relay/queue.rs`

### Adding Channel Sharding

1. Define shard configuration in `src/chain/sharding/config.rs`
2. Implement state partitioning in `src/chain/sharding/partition.rs`
3. Add cross-shard gossip in `src/chain/sharding/gossip.rs`
4. Create light client mode in `src/chain/sharding/light_client.rs`
5. Test with high-activity threshold simulation

### Implementing Cryptographic Dispute Resolution

1. Define claim struct in `src/chain/dispute_resolution/claim.rs`
2. Add challenge logic in `src/chain/dispute_resolution/challenge.rs`
3. Implement respond mechanism in `src/chain/dispute_resolution/respond.rs`
4. Add slashing vote in `src/chain/dispute_resolution/slash.rs`
5. Create fork recovery in `src/chain/fork_recovery/canonical.rs`

### Setting Up Observability & Monitoring

1. Initialize Prometheus metrics in `src/observability/metrics.rs`
2. Add distributed tracing with opentelemetry in `src/observability/tracing.rs`
3. Create health check endpoint in `src/observability/health.rs`
4. Build dashboard configuration in `docs/monitoring/`
5. Set up chaos testing suite in `tests/chaos/`

### Implementing Hierarchical Key Derivation

1. Define BIP-32/44 paths in `src/identity/derivation/paths.rs`
2. Implement key derivation in `src/identity/derivation/keys.rs`
3. Add device key generation in `src/identity/derivation/device_keys.rs`
4. Create recovery from backup in `src/identity/derivation/recovery.rs`
5. Test against vector test suite in `tests/key_derivation.rs`

### Setting Up Multi-Device Synchronization

1. Define sync messages in `src/identity/sync/messages.rs`
2. Implement gossip protocol in `src/identity/sync/gossip.rs`
3. Add conflict resolution in `src/identity/sync/conflict_resolution.rs`
4. Create device registration in `src/identity/sync/device_registration.rs`
5. Test with 3-device simulation in `tests/multi_device.rs`

### Implementing Onion Routing for Metadata Resistance

1. Define Sphinx packet format in `src/network/onion_routing/sphinx.rs`
2. Implement layered encryption in `src/network/onion_routing/encryption.rs`
3. Create circuit management in `src/network/onion_routing/circuits.rs`
4. Add relay path selection in `src/network/onion_routing/path_selection.rs`
5. Test with traffic analysis simulation in `tests/metadata_resistance.rs`

### Adding Keyless UX with Secure Enclave

1. Initialize enclave SDK in `src/onboarding/enclave/init.rs`
2. Implement biometric unlock in `src/onboarding/enclave/biometric.rs`
3. Add attestation verification in `src/identity/attestation/verify.rs`
4. Create fallback MPC flow in `src/onboarding/mpc/setup.rs`
5. Test enclave signing in `tests/enclave_signing.rs`

### Implementing Automatic Failover Routing

1. Create fallback mechanism in `src/network/resilience/fallback.rs`
2. Add timeout detection in `src/network/resilience/timeouts.rs`
3. Implement peer diversity checks in `src/network/eclipse_prevention/diversity.rs`
4. Add BGP hijack resistance in `src/network/fallback_routing/bgp_resistant.rs`
5. Test with network partition simulation in `tests/chaos/partitions.rs`

### Building Accessibility Compliance (WCAG 2.1 AA+)

1. Create semantic HTML in `src/ui/accessibility/semantic.rs`
2. Add ARIA labels in `src/ui/accessibility/aria.rs`
3. Implement keyboard navigation in `src/ui/accessibility/keyboard.rs`
4. Add screen reader support in `src/accessibility/screen_readers.rs`
5. Test with WAVE, Axe, or NVDA in `tests/accessibility/wcag.rs`

### Setting Up Privacy-Preserving Metadata Hiding

1. Implement ZK contact graph proofs in `src/privacy/zk_proofs/contact_graph.rs`
2. Add cover traffic generation in `src/privacy/cover_traffic/generator.rs`
3. Implement timing obfuscation in `src/privacy/metadata_hiding/timing.rs`
4. Create message padding in `src/privacy/metadata_hiding/padding.rs`
5. Test traffic analysis resistance in `tests/privacy/traffic_analysis.rs`

### Implementing Regulatory Compliance (Section 22)

1. Define hash-proof system in `src/compliance/hash_proofs/mod.rs` (SHA-256/BLAKE3 hashing)
2. Implement probabilistic Bloom filters in `src/compliance/hash_proofs/bloom.rs`
3. Create ZK proof interface in `src/privacy/encrypted_analysis/zk_verify.rs`
4. Add decentralized jury voting in `src/governance/moderation/jury.rs`
5. Implement law enforcement warrant API in `src/compliance/law_enforcement/warrant_api.rs`
6. Create transparency reporting in `src/governance/transparency/reports.rs`
7. Test with encrypted analysis scenarios in `tests/compliance/`

### Adding Data Lifecycle & Storage Economics (Section 23)

1. Define TTL configuration in `src/storage/lifecycle/config.rs`
2. Implement message expiration in `src/storage/lifecycle/expiration.rs`
3. Create deduplication in `src/storage/deduplication/content_addressable.rs`
4. Implement delta encoding in `src/storage/deduplication/delta.rs`
5. Add storage bond contracts in `src/economics/storage_bonds.rs`
6. Create cold/hot tier management in `src/storage/lifecycle/tiering.rs`
7. Implement encrypted backup in `src/storage/backup/encrypted.rs`
8. Test with various TTL scenarios in `tests/storage/lifecycle.rs`

### Implementing Protocol Upgrades & Cryptographic Agility (Section 24)

1. Define semantic versioning in `src/upgrades/versioning/semver.rs`
2. Implement version negotiation in `src/upgrades/versioning/negotiation.rs`
3. Create algorithm suite definitions in `src/crypto/agility/suites.rs`
4. Implement staged rollout logic in `src/upgrades/rollout/staged.rs`
5. Add post-quantum hybrid scheme support in `src/crypto/post_quantum/hybrid.rs`
6. Implement emergency rotation trigger in `src/governance/emergency/crypto_rotation.rs`
7. Test upgrade paths in `tests/upgrades/`

### Building User Safety & Trust Infrastructure (Section 25)

1. Implement proof-of-device in `src/identity/attestation/device_proof.rs`
2. Add verified identity badges in `src/identity/verification/badges.rs`
3. Create context-aware warnings in `src/ui/safety_warnings/context.rs`
4. Implement phishing detection in `src/ui/safety_warnings/phishing.rs`
5. Add reputation display in `src/reputation/proofs/display.rs`
6. Create moderation history viewer in `src/governance/transparency/moderation_history.rs`
7. Test trust signal accuracy in `tests/trust_infrastructure/`

### Setting Up Developer Ecosystem & Plugins (Section 26)

1. Define plugin API in `src/plugins/api/mod.rs`
2. Create WebAssembly sandbox in `src/plugins/sandbox/wasm.rs`
3. Implement message hooks in `src/plugins/hooks/message.rs`
4. Add UI extension capabilities in `src/plugins/hooks/ui_extensions.rs`
5. Create plugin marketplace schema in `src/plugins/marketplace/schema.rs`
6. Build Rust SDK in `sdk/rust/` with crypto, networking, storage wrappers
7. Scaffold TypeScript SDK in `sdk/typescript/`
8. Test plugin loading and isolation in `tests/plugins/`

### Implementing Economic Security & Game Theory (Section 27)

1. Define relay payment fairness in `src/economics/relay/fairness.rs`
2. Add uptime reward calculations in `src/economics/relay/uptime_rewards.rs`
3. Implement geographic distribution bonuses in `src/economics/relay/geographic_bonus.rs`
4. Create token-draining protection in `src/economics/security/token_draining.rs`
5. Add slashing for false proofs in `src/governance/slashing/false_proofs.rs`
6. Implement insurance fund in `src/economics/insurance_fund.rs`
7. Create game theory simulations in `tests/game_theory/economic_models.rs`
8. Run long-term sustainability modeling in `tests/game_theory/sustainability.rs`

### Setting Up Post-Quantum Cryptography (Section 28)

1. Integrate Kyber768 in `src/crypto/post_quantum/kyber.rs`
2. Implement hybrid Curve25519+Kyber768 in `src/crypto/hybrid/combined.rs`
3. Add FALCON or Dilithium signatures in `src/crypto/post_quantum/signatures.rs`
4. Implement dual ciphertext encryption in `src/crypto/post_quantum/dual_ciphertext.rs`
5. Create backward compatibility layer in `src/crypto/post_quantum/compat.rs`
6. Define PQ migration roadmap in `src/upgrades/pq_migration/roadmap.rs`
7. Implement harvest-now-decrypt-later defense in `src/crypto/post_quantum/forward_secrecy.rs`
8. Test PQ schemes against test vectors in `tests/post_quantum/`

### Implementing Censorship-Resistant Distribution (Section 29)

1. Create F-Droid distribution in `src/distribution/f_droid/manifest.rs`
2. Add sideloading support in `src/distribution/sideload/apk_support.rs`
3. Implement IPFS hosting in `src/distribution/package_hosting/ipfs.rs`
4. Add Bittorrent distribution in `src/distribution/package_hosting/bittorrent.rs`
5. Create mirror network sync in `src/distribution/mirrors/sync.rs`
6. Implement package signing verification in `src/distribution/signing/verify.rs`
7. Add gossip-based update discovery in `src/upgrades/gossip_discovery.rs`
8. Test distribution paths in `tests/distribution/`

### Implementing Full-Network Disaster Recovery (Section 30)

1. Create chain replay logic in `src/recovery/chain_replay/replay.rs`
2. Implement snapshot checkpoints in `src/chain/snapshots/checkpoint.rs`
3. Add Merkle proof verification in `src/chain/snapshots/merkle_verify.rs`
4. Create distributed backup coordination in `src/recovery/distributed_backup.rs`
5. Implement leader election for stalled consensus in `src/chain/recovery/leader_election.rs`
6. Add erasure coding with Reed-Solomon in `src/recovery/erasure_coding.rs`
7. Implement fork finalization in `src/chain/fork_recovery/finalization.rs`
8. Test recovery scenarios in `tests/disaster_recovery/`

### Setting Up Progressive Decentralization UX (Section 31)

1. Create centralized entry portal in `src/onboarding/progressive/web_portal.rs`
2. Implement feature unlock progression in `src/onboarding/progressive/feature_gates.rs`
3. Add in-app education system in `src/ui/education/tutorial.rs`
4. Create trust bridge infrastructure in `src/onboarding/progressive/trust_bridge.rs`
5. Implement gradual reputation carryover in `src/reputation/migration/progressive.rs`
6. Build privacy disclosure UI in `src/ui/education/privacy_guarantees.rs`
7. Add governance education in `src/ui/education/governance_guide.rs`
8. Test onboarding flows with new users in `tests/onboarding/progressive_flows.rs`

### Implementing Formal Verification (Section 32)

1. Write TLA+ specification of consensus in `docs/verification/consensus.tla`
2. Create Coq proofs for crypto primitives in `docs/verification/crypto.v`
3. Set up Libfuzzer harnesses in `tests/fuzz/`
4. Add AFL++ corpus generation in `tests/fuzz/afl++/`
5. Implement differential fuzzing in `tests/fuzz/differential.rs`
6. Create runtime verification monitors in `src/verification/runtime_monitors.rs`
7. Add continuous fuzzing infrastructure in `src/verification/continuous/`
8. Verify invariants hold in `tests/verification/invariants.rs`

### Implementing Ethical Governance Constraints (Section 33)

1. Define voting power caps in `src/governance/constraints/voting_caps.rs`
2. Implement term limits in `src/governance/ethics/term_limits.rs`
3. Add diversity requirements in `src/governance/ethics/diversity.rs`
4. Create immutable governance log in `src/governance/transparency/action_log.rs`
5. Implement slashing transparency in `src/governance/transparency/slashing_log.rs`
6. Add appeal rights mechanism in `src/governance/ethics/appeals.rs`
7. Implement sortition for positions in `src/governance/ethics/sortition.rs`
8. Create citizens' assembly in `src/governance/ethics/citizens_assembly.rs`
9. Test governance constraints in `tests/governance/ethics.rs`

---

**Critical Reference**: See `ARCHITECTURE-2.0.md` for authoritative architecture specification and `plan.md` for implementation status, remaining work items, and phased delivery sequence.

---

# dchat Implementation Plan v3.0

> Comprehensive improvement plan covering missing implementations, code quality enhancements, handshake protocol improvements, and protocol stack recommendations.

**Generated**: December 10, 2025  
**Status**: Planning Document

---

## Table of Contents

1. [Missing Implementations](#1-missing-implementations)
2. [Code Quality Improvements](#2-code-quality-improvements)
3. [Server Handshake Protocol Improvements](#3-server-handshake-protocol-improvements)
4. [Protocol Stack Recommendations](#4-protocol-stack-recommendations)
5. [Implementation Priority](#5-implementation-priority)

---

## 1. Missing Implementations

### 1.1 Critical TODOs Found

| Location                         | Issue                               | Impact                            |
| -------------------------------- | ----------------------------------- | --------------------------------- |
| `src/main.rs:2831`               | Delta deduplication not implemented | Message sync inefficiency         |
| `dchat-bots/src/telegram.rs:97`  | Chat ID parsing incomplete          | Bot API failures                  |
| `dchat-network/src/nat.rs`       | UPnP external IP returns local IP   | NAT traversal broken              |
| `dchat-blockchain/src/client.rs` | Transaction logic placeholders      | Blockchain integration incomplete |

### 1.2 Placeholder/Mock Implementations

```
dchat-blockchain/src/client.rs     - Multiple "placeholder" comments
dchat-marketplace/src/lib.rs       - "stub" implementations
dchat-privacy/src/zk/mod.rs        - "mock" proof verification
```

### 1.3 Error Handling Issues

**121+ `unwrap()` calls in production code paths**, primarily in:

- `src/main.rs` - CLI handling
- Network event processing
- Configuration parsing

### 1.4 Recommended Fixes

#### Delta Deduplication (src/main.rs:2831)

```rust
// BEFORE: Not implemented
// TODO: Implement delta deduplication

// AFTER: Implement proper delta sync
pub struct DeltaSync {
    /// Last known sequence per peer
    peer_sequences: HashMap<PeerId, u64>,
    /// Bloom filter for recent message IDs
    recent_messages: BloomFilter,
    /// Merkle tree root for efficient diff
    merkle_root: [u8; 32],
}

impl DeltaSync {
    pub fn compute_delta(&self, peer_sequence: u64, peer_bloom: &BloomFilter) -> Delta {
        // Only send messages the peer hasn't seen
        let missing_sequences: Vec<u64> = self.messages
            .iter()
            .filter(|msg| msg.sequence > peer_sequence)
            .filter(|msg| !peer_bloom.might_contain(&msg.id))
            .map(|msg| msg.sequence)
            .collect();

        Delta {
            from_sequence: peer_sequence,
            message_ids: missing_sequences,
            merkle_proof: self.generate_merkle_proof(&missing_sequences),
        }
    }
}
```

#### NAT External IP Detection (dchat-network/src/nat.rs)

```rust
// BEFORE: Returns local IP
async fn get_upnp_external_ip() -> Result<IpAddr> {
    // ... SOAP request that returns local IP
}

// AFTER: Full SOAP implementation
async fn get_upnp_external_ip(gateway_url: &str) -> Result<IpAddr> {
    let soap_request = r#"<?xml version="1.0"?>
        <s:Envelope xmlns:s="http://schemas.xmlsoap.org/soap/envelope/"
                    s:encodingStyle="http://schemas.xmlsoap.org/soap/encoding/">
            <s:Body>
                <u:GetExternalIPAddress xmlns:u="urn:schemas-upnp-org:service:WANIPConnection:1"/>
            </s:Body>
        </s:Envelope>"#;

    let response = reqwest::Client::new()
        .post(gateway_url)
        .header("Content-Type", "text/xml; charset=utf-8")
        .header("SOAPAction", "\"urn:schemas-upnp-org:service:WANIPConnection:1#GetExternalIPAddress\"")
        .body(soap_request)
        .send()
        .await?;

    let body = response.text().await?;

    // Parse: <NewExternalIPAddress>1.2.3.4</NewExternalIPAddress>
    let re = regex::Regex::new(r"<NewExternalIPAddress>([^<]+)</NewExternalIPAddress>")?;
    let captures = re.captures(&body)
        .ok_or_else(|| Error::nat("No external IP in UPnP response"))?;

    captures[1].parse::<IpAddr>()
        .map_err(|e| Error::nat(format!("Invalid IP: {}", e)))
}
```

---

## 2. Code Quality Improvements

### 2.1 Error Handling: Replace unwrap() with Proper Propagation

```rust
// BEFORE: Panics on error
let config = Config::load("config.toml").unwrap();
let network = NetworkManager::new(config.network).await.unwrap();

// AFTER: Proper error propagation with context
let config = Config::load("config.toml")
    .map_err(|e| Error::config(format!("Failed to load config: {}", e)))?;

let network = NetworkManager::new(config.network)
    .await
    .map_err(|e| Error::network(format!("Network init failed: {}", e)))?;
```

### 2.2 Dependency Injection for Testability

```rust
// BEFORE: Hard-coded dependencies
pub struct MessageHandler {
    db: SqlitePool,
    network: NetworkManager,
}

impl MessageHandler {
    pub async fn new() -> Self {
        Self {
            db: SqlitePool::connect("sqlite:dchat.db").await.unwrap(),
            network: NetworkManager::new(Default::default()).await.unwrap(),
        }
    }
}

// AFTER: Trait-based injection
#[async_trait]
pub trait MessageStore: Send + Sync {
    async fn store(&self, msg: &Message) -> Result<()>;
    async fn get(&self, id: &MessageId) -> Result<Option<Message>>;
    async fn list_since(&self, since: DateTime<Utc>) -> Result<Vec<Message>>;
}

#[async_trait]
pub trait NetworkTransport: Send + Sync {
    async fn send(&self, peer: PeerId, data: Vec<u8>) -> Result<()>;
    async fn broadcast(&self, channel: &str, data: Vec<u8>) -> Result<()>;
}

pub struct MessageHandler<S: MessageStore, N: NetworkTransport> {
    store: S,
    network: N,
}

impl<S: MessageStore, N: NetworkTransport> MessageHandler<S, N> {
    pub fn new(store: S, network: N) -> Self {
        Self { store, network }
    }
}

// In tests:
struct MockStore { messages: Arc<Mutex<Vec<Message>>> }
struct MockNetwork { sent: Arc<Mutex<Vec<(PeerId, Vec<u8>)>>> }
```

### 2.3 Circuit Breaker for External Services

```rust
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

pub struct CircuitBreaker {
    failure_count: AtomicU32,
    last_failure: AtomicU64,
    state: AtomicU8,
    config: CircuitBreakerConfig,
}

#[derive(Clone)]
pub struct CircuitBreakerConfig {
    pub failure_threshold: u32,
    pub reset_timeout: Duration,
    pub half_open_max_calls: u32,
}

impl CircuitBreaker {
    pub async fn call<F, T, E>(&self, f: F) -> Result<T, CircuitBreakerError<E>>
    where
        F: Future<Output = Result<T, E>>,
    {
        match self.state() {
            State::Open => {
                if self.should_attempt_reset() {
                    self.transition_to(State::HalfOpen);
                } else {
                    return Err(CircuitBreakerError::CircuitOpen);
                }
            }
            State::HalfOpen | State::Closed => {}
        }

        match f.await {
            Ok(result) => {
                self.record_success();
                Ok(result)
            }
            Err(e) => {
                self.record_failure();
                Err(CircuitBreakerError::ServiceError(e))
            }
        }
    }
}

// Usage:
let breaker = CircuitBreaker::new(CircuitBreakerConfig {
    failure_threshold: 5,
    reset_timeout: Duration::from_secs(30),
    half_open_max_calls: 3,
});

match breaker.call(blockchain_client.submit_transaction(&tx)).await {
    Ok(hash) => tracing::info!("Transaction submitted: {}", hash),
    Err(CircuitBreakerError::CircuitOpen) => {
        tracing::warn!("Blockchain service unavailable, queuing transaction");
        pending_queue.push(tx);
    }
    Err(CircuitBreakerError::ServiceError(e)) => {
        tracing::error!("Transaction failed: {}", e);
    }
}
```

### 2.4 Validated Configuration

```rust
// BEFORE: Raw config with runtime validation scattered
#[derive(Deserialize)]
pub struct Config {
    pub network: NetworkConfig,
    pub storage: StorageConfig,
}

// AFTER: Validation at parse time
use garde::Validate;

#[derive(Deserialize, Validate)]
pub struct Config {
    #[garde(dive)]
    pub network: NetworkConfig,
    #[garde(dive)]
    pub storage: StorageConfig,
}

#[derive(Deserialize, Validate)]
pub struct NetworkConfig {
    #[garde(range(min = 1024, max = 65535))]
    pub port: u16,

    #[garde(length(min = 1))]
    pub bootstrap_nodes: Vec<String>,

    #[garde(range(min = 1, max = 1000))]
    pub max_connections: u32,

    #[garde(custom(validate_multiaddr))]
    pub listen_address: String,
}

fn validate_multiaddr(addr: &str, _ctx: &()) -> garde::Result {
    addr.parse::<Multiaddr>()
        .map(|_| ())
        .map_err(|e| garde::Error::new(format!("Invalid multiaddr: {}", e)))
}

impl Config {
    pub fn load(path: &str) -> Result<ValidatedConfig> {
        let raw: Config = toml::from_str(&std::fs::read_to_string(path)?)?;
        raw.validate(&())?;
        Ok(ValidatedConfig(raw))
    }
}

// Newtype ensures config is validated
pub struct ValidatedConfig(Config);
```

### 2.5 Property-Based Testing

```rust
use proptest::prelude::*;

proptest! {
    #[test]
    fn message_roundtrip(
        sender in "[a-f0-9]{64}",
        recipient in "[a-f0-9]{64}",
        content in ".*",
        timestamp in 0u64..u64::MAX,
    ) {
        let msg = Message {
            sender: UserId::from_hex(&sender).unwrap(),
            recipient: UserId::from_hex(&recipient).unwrap(),
            content: content.clone(),
            timestamp,
        };

        let encoded = msg.encode();
        let decoded = Message::decode(&encoded).unwrap();

        prop_assert_eq!(msg.sender, decoded.sender);
        prop_assert_eq!(msg.recipient, decoded.recipient);
        prop_assert_eq!(msg.content, decoded.content);
        prop_assert_eq!(msg.timestamp, decoded.timestamp);
    }

    #[test]
    fn encryption_roundtrip(
        plaintext in prop::collection::vec(any::<u8>(), 0..10000),
        key in prop::collection::vec(any::<u8>(), 32..=32),
    ) {
        let key: [u8; 32] = key.try_into().unwrap();
        let ciphertext = encrypt(&plaintext, &key).unwrap();
        let decrypted = decrypt(&ciphertext, &key).unwrap();
        prop_assert_eq!(plaintext, decrypted);
    }
}
```

### 2.6 Standardized Metrics

```rust
// BEFORE: Inconsistent metric naming
lazy_static! {
    static ref MSG_COUNTER: Counter = register_counter!("messages_sent", "...");
    static ref PEER_GAUGE: Gauge = register_gauge!("connected_peers", "...");
    static ref LATENCY: Histogram = register_histogram!("msg_latency", "...");
}

// AFTER: Standardized naming convention
pub struct DchatMetrics {
    // Format: dchat_<subsystem>_<metric>_<unit>
    pub messages_sent_total: CounterVec,
    pub messages_received_total: CounterVec,
    pub message_processing_duration_seconds: HistogramVec,
    pub peers_connected: Gauge,
    pub peers_discovered_total: Counter,
    pub handshakes_total: CounterVec,
    pub handshake_duration_seconds: Histogram,
    pub storage_operations_total: CounterVec,
    pub storage_operation_duration_seconds: HistogramVec,
}

impl DchatMetrics {
    pub fn new(registry: &Registry) -> Self {
        Self {
            messages_sent_total: register_counter_vec_with_registry!(
                "dchat_messages_sent_total",
                "Total messages sent",
                &["channel_type", "encryption"],
                registry
            ).unwrap(),

            message_processing_duration_seconds: register_histogram_vec_with_registry!(
                "dchat_message_processing_duration_seconds",
                "Message processing latency",
                &["operation"],
                vec![0.001, 0.005, 0.01, 0.025, 0.05, 0.1, 0.25, 0.5, 1.0],
                registry
            ).unwrap(),
            // ...
        }
    }
}
```

---

## 3. Server Handshake Protocol Improvements

### 3.1 Current Implementation Analysis

**Location**: `crates/dchat-crypto/src/noise.rs`, `crates/dchat-crypto/src/handshake.rs`

**Current Stack**:

- Noise Protocol XX pattern (mutual authentication)
- X25519 key exchange
- ChaChaPoly encryption
- BLAKE2s hashing

**Issues Identified**:

1. No protocol version negotiation in handshake messages
2. No cryptographic binding between Noise keys and peer identity
3. Opaque `Vec<u8>` message format (no structure)
4. No rate limiting on handshake attempts
5. Basic timeout cleanup (no per-phase timeouts)
6. No handshake-specific metrics

### 3.2 Typed Handshake State Machine

```rust
/// Protocol version for compatibility
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u16,
    pub minor: u16,
}

impl ProtocolVersion {
    pub const CURRENT: Self = Self { major: 2, minor: 0 };

    pub fn is_compatible(&self, other: &Self) -> bool {
        self.major == other.major
    }
}

/// Explicit state machine - compiler enforces valid transitions
#[derive(Debug)]
pub enum HandshakePhase {
    Initial,
    AwaitingMessage { expected_step: u8 },
    IdentityExchange { noise_session: NoiseSession },
    Completed {
        session: NoiseSession,
        verified_identity: VerifiedPeerIdentity,
    },
    Failed { reason: HandshakeFailure },
}

pub struct TypedHandshake {
    phase: HandshakePhase,
    noise_state: Option<snow::HandshakeState> ,
    role: HandshakeRole,
    local_identity: LocalIdentity,
    protocol_version: ProtocolVersion,
    started_at: Instant,
    peer_id: PeerId,
    message_log: Vec<HandshakeMessageLog>,
}

#[derive(Debug, Clone)]
pub struct HandshakeMessageLog {
    pub direction: MessageDirection,
    pub step: u8,
    pub timestamp: Instant,
    pub size_bytes: usize,
    pub hash: [u8; 32],
}

impl TypedHandshake {
    /// Only valid transition from Initial for initiator
    pub fn send_init(&mut self) -> Result<HandshakeMessage, HandshakeError> {
        match &self.phase {
            HandshakePhase::Initial => {
                let noise = self.noise_state.as_mut()
                    .ok_or(HandshakeError::InvalidState)?;

                let mut msg_buf = vec![0u8; 65535];
                let len = noise.write_message(&[], &mut msg_buf)?;
                msg_buf.truncate(len);

                self.log_message(MessageDirection::Outbound, 1, &msg_buf);
                self.phase = HandshakePhase::AwaitingMessage { expected_step: 2 };

                Ok(HandshakeMessage::Init {
                    version: self.protocol_version,
                    noise_payload: msg_buf,
                    supported_patterns: vec![NoisePattern::XX, NoisePattern::IK],
                })
            }
            _ => Err(HandshakeError::InvalidStateTransition {
                from: self.phase_name(),
                attempted: "send_init",
            }),
        }
    }
}
```

### 3.3 Cryptographic Identity Binding

```rust
/// Identity claim that binds Noise keys to peer identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IdentityClaim {
    pub peer_id: PeerId,
    pub noise_static_key: [u8; 32],
    pub timestamp: u64,
    pub capabilities: Option<SignedCapabilities>,
    pub signature: Signature,
}

impl IdentityClaim {
    pub fn create(
        identity_keypair: &IdentityKeyPair,
        noise_static_key: &[u8; 32],
        capabilities: Option<SignedCapabilities>,
    ) -> Self {
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let mut signing_data = Vec::new();
        signing_data.extend_from_slice(identity_keypair.public().as_bytes());
        signing_data.extend_from_slice(noise_static_key);
        signing_data.extend_from_slice(&timestamp.to_le_bytes());

        let signature = identity_keypair.sign(&signing_data);

        Self {
            peer_id: identity_keypair.peer_id(),
            noise_static_key: *noise_static_key,
            timestamp,
            capabilities,
            signature,
        }
    }

    /// Verify claim and check key binding
    pub fn verify(&self, received_noise_key: &[u8; 32]) -> Result<(), IdentityError> {
        // 1. Check timestamp freshness (prevent replay)
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)?
            .as_secs();

        const MAX_CLOCK_SKEW_SECONDS: u64 = 300; // 5 minutes
        if now.saturating_sub(self.timestamp) > MAX_CLOCK_SKEW_SECONDS {
            return Err(IdentityError::StaleTimestamp {
                claimed: self.timestamp,
                current: now,
            });
        }

        // 2. Verify the claimed Noise key matches what we received
        if self.noise_static_key != *received_noise_key {
            return Err(IdentityError::KeyMismatch {
                claimed: hex::encode(&self.noise_static_key),
                received: hex::encode(received_noise_key),
            });
        }

        // 3. Verify signature
        let mut signing_data = Vec::new();
        signing_data.extend_from_slice(self.peer_id.as_bytes());
        signing_data.extend_from_slice(&self.noise_static_key);
        signing_data.extend_from_slice(&self.timestamp.to_le_bytes());

        let public_key = PublicKey::from_peer_id(&self.peer_id)?;
        public_key.verify(&signing_data, &self.signature)?;

        Ok(())
    }
}
```

### 3.4 Structured Handshake Messages

```rust
/// Wire format for handshake messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeMessage {
    Init {
        version: ProtocolVersion,
        noise_payload: Vec<u8>,
        supported_patterns: Vec<NoisePattern>,
        psk_hint: Option<[u8; 16]>,
    },

    Response {
        version: ProtocolVersion,
        selected_pattern: NoisePattern,
        noise_payload: Vec<u8>,
    },

    Final {
        noise_payload: Vec<u8>,
        encrypted_identity: Vec<u8>,
    },

    Identity {
        encrypted_claim: Vec<u8>,
    },

    Ack {
        session_id: [u8; 32],
        confirmation: Vec<u8>,
    },

    Reject {
        reason: HandshakeRejectReason,
        message: String,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HandshakeRejectReason {
    VersionMismatch { supported: Vec<ProtocolVersion> },
    PatternNotSupported { requested: NoisePattern },
    IdentityVerificationFailed,
    RateLimited { retry_after_ms: u64 },
    ResourceExhausted,
    InternalError,
}

/// Framing wrapper for network transmission
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandshakeFrame {
    pub magic: [u8; 4], // b"DCHT"
    pub frame_type: u8,
    pub payload_len: u32,
    pub message: HandshakeMessage,
    pub frame_mac: Option<[u8; 16]>,
}

impl HandshakeFrame {
    pub const MAGIC: [u8; 4] = *b"DCHT";

    pub fn quick_validate(raw: &[u8]) -> Result<(), FrameError> {
        if raw.len() < 9 {
            return Err(FrameError::TooShort);
        }
        if &raw[0..4] != &Self::MAGIC {
            return Err(FrameError::InvalidMagic);
        }
        let payload_len = u32::from_le_bytes(raw[5..9].try_into().unwrap()) as usize;
        const MAX_HANDSHAKE_MESSAGE_SIZE: usize = 65536;
        if payload_len > MAX_HANDSHAKE_MESSAGE_SIZE {
            return Err(FrameError::PayloadTooLarge { size: payload_len });
        }
        Ok(())
    }
}
```

### 3.5 Rate Limiting

```rust
pub struct HandshakeRateLimiter {
    ip_limits: HashMap<IpAddr, RateLimitState>,
    peer_limits: HashMap<PeerId, RateLimitState>,
    global: RateLimitState,
    config: RateLimitConfig,
}

#[derive(Debug, Clone)]
pub struct RateLimitConfig {
    pub per_ip_limit: u32,
    pub per_peer_limit: u32,
    pub max_concurrent: u32,
    pub window: Duration,
    pub backoff_multiplier: f64,
}

impl HandshakeRateLimiter {
    pub fn check(&mut self, ip: IpAddr, peer_hint: Option<&PeerId>) -> RateLimitResult {
        let now = Instant::now();

        if self.global.concurrent >= self.config.max_concurrent {
            return RateLimitResult::Rejected {
                reason: RateLimitReason::GlobalLimit,
                retry_after: self.estimate_retry(None),
            };
        }

        let ip_state = self.ip_limits.entry(ip).or_default();
        ip_state.cleanup(now, self.config.window);

        if ip_state.count >= self.config.per_ip_limit {
            return RateLimitResult::Rejected {
                reason: RateLimitReason::IpLimit,
                retry_after: self.estimate_retry(Some(ip_state)),
            };
        }

        ip_state.count += 1;
        ip_state.last_attempt = now;
        self.global.concurrent += 1;

        RateLimitResult::Allowed {
            token: RateLimitToken::new(ip, now),
        }
    }

    pub fn release(&mut self, token: RateLimitToken, success: bool) {
        self.global.concurrent = self.global.concurrent.saturating_sub(1);

        if !success {
            if let Some(state) = self.ip_limits.get_mut(&token.ip) {
                state.failure_count += 1;
            }
        }
    }
}
```

### 3.6 Timeout Handling

```rust
pub struct TimeoutAwareHandshake {
    inner: TypedHandshake,
    deadline: Instant,
    phase_deadlines: HashMap<&'static str, Instant>,
    timeout_handle: Option<tokio::task::JoinHandle<()>>,
}

impl TimeoutAwareHandshake {
    pub fn new(inner: TypedHandshake, config: &TimeoutConfig) -> Self {
        let now = Instant::now();
        Self {
            inner,
            deadline: now + config.total_timeout,
            phase_deadlines: [
                ("noise", now + config.noise_timeout),
                ("identity", now + config.total_timeout),
            ].into_iter().collect(),
            timeout_handle: None,
        }
    }

    pub fn check_timeout(&self) -> Result<(), HandshakeError> {
        let now = Instant::now();

        if now > self.deadline {
            return Err(HandshakeError::Timeout {
                phase: "total",
                elapsed: now.duration_since(self.inner.started_at),
            });
        }

        let current_phase = self.inner.phase_name();
        if let Some(&phase_deadline) = self.phase_deadlines.get(current_phase) {
            if now > phase_deadline {
                return Err(HandshakeError::Timeout {
                    phase: current_phase,
                    elapsed: now.duration_since(self.inner.started_at),
                });
            }
        }

        Ok(())
    }

    pub async fn process_with_timeout(
        &mut self,
        message: &[u8],
    ) -> Result<Option<Vec<u8>>, HandshakeError> {
        self.check_timeout()?;

        let remaining = self.deadline.saturating_duration_since(Instant::now());

        tokio::select! {
            result = self.inner.process_message(message) => result,
            _ = tokio::time::sleep(remaining) => {
                Err(HandshakeError::Timeout {
                    phase: self.inner.phase_name(),
                    elapsed: Instant::now().duration_since(self.inner.started_at),
                })
            }
        }
    }
}
```

### 3.7 Handshake Metrics

```rust
pub struct HandshakeMetrics {
    pub initiated_total: Counter,
    pub received_total: Counter,
    pub completed: CounterVec,  // labels: [outcome, role]
    pub duration_seconds: Histogram,
    pub failures: CounterVec,   // labels: [reason]
    pub active: Gauge,
    pub rate_limited: Counter,
}

impl HandshakeMetrics {
    pub fn register(registry: &Registry) -> Self {
        Self {
            initiated_total: register_counter!(
                "dchat_handshake_initiated_total",
                "Total handshakes initiated",
            ).unwrap(),
            completed: register_counter_vec!(
                "dchat_handshake_completed_total",
                "Completed handshakes by outcome",
                &["outcome", "role"]
            ).unwrap(),
            duration_seconds: register_histogram!(
                "dchat_handshake_duration_seconds",
                "Handshake duration in seconds",
                vec![0.01, 0.05, 0.1, 0.25, 0.5, 1.0, 2.5, 5.0, 10.0]
            ).unwrap(),
            failures: register_counter_vec!(
                "dchat_handshake_failures_total",
                "Handshake failures by reason",
                &["reason"]
            ).unwrap(),
            active: register_gauge!(
                "dchat_handshake_active",
                "Currently active handshakes"
            ).unwrap(),
            rate_limited: register_counter!(
                "dchat_handshake_rate_limited_total",
                "Handshakes rejected due to rate limiting"
            ).unwrap(),
        }
    }
}
```

### 3.8 Handshake Improvement Summary

| Area                 | Current              | Improved                                 |
| -------------------- | -------------------- | ---------------------------------------- |
| **State Machine**    | Implicit enum states | Typed phases with compile-time checks    |
| **Identity Binding** | None                 | Cryptographic binding with signed claims |
| **Message Format**   | Opaque `Vec<u8>`     | Typed `HandshakeMessage` with versioning |
| **Rate Limiting**    | None                 | Per-IP, per-peer, and global limits      |
| **Timeouts**         | Periodic cleanup     | Active per-phase timeouts                |
| **Observability**    | Basic logging        | Prometheus metrics, audit logs           |

---

## 4. Protocol Stack Recommendations

### 4.1 Current Protocol Stack

| Layer             | Current                         | Status       |
| ----------------- | ------------------------------- | ------------ |
| **Transport**     | TCP                             | ✅ Reliable  |
| **Security**      | Noise XX (ChaChaPoly + BLAKE2s) | ✅ Excellent |
| **Multiplexing**  | Yamux                           | ✅ Good      |
| **Discovery**     | Kademlia DHT + mDNS             | ✅ Good      |
| **Messaging**     | Gossipsub                       | ✅ Good      |
| **Serialization** | Bincode/CBOR                    | ✅ Good      |

### 4.2 Add QUIC Transport (High Priority)

**Current**: TCP only in `transport.rs`

**Recommended**:

```rust
use libp2p::quic;

pub fn build_transport(keypair: &identity::Keypair) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    // QUIC transport (built-in encryption + multiplexing)
    let quic_transport = quic::tokio::Transport::new(quic::Config::new(keypair))
        .map(|(peer_id, muxer), _| (peer_id, StreamMuxerBox::new(muxer)));

    // TCP fallback for restrictive networks
    let tcp_transport = tcp::tokio::Transport::new(tcp::Config::default().nodelay(true));
    let tcp_with_noise = dns::tokio::Transport::system(tcp_transport)?
        .upgrade(upgrade::Version::V1)
        .authenticate(noise::Config::new(keypair)?)
        .multiplex(yamux::Config::default());

    // Prefer QUIC, fallback to TCP
    let transport = quic_transport
        .or_transport(tcp_with_noise)
        .map(|either, _| match either {
            Either::Left((peer_id, muxer)) => (peer_id, muxer),
            Either::Right((peer_id, muxer)) => (peer_id, StreamMuxerBox::new(muxer)),
        })
        .boxed();

    Ok(transport)
}
```

**Cargo.toml**:

```toml
libp2p = { version = "0.54", features = [
    "kad", "noise", "tcp", "dns", "websocket", "relay", "dcutr",
    "mdns", "identify", "ping", "gossipsub", "yamux", "tokio",
    "request-response", "macros",
    "quic",  # ADD THIS
] }
```

**Benefits**:

- 0-RTT connection establishment (vs 3-RTT for TCP+Noise)
- Built-in multiplexing (no Yamux overhead)
- Connection migration (survives IP changes on mobile)
- Better NAT traversal (UDP-based)
- No head-of-line blocking

### 4.3 Add WebRTC for Browser Clients (Medium Priority)

```rust
use libp2p::webrtc;

pub fn build_transport_with_webrtc(
    keypair: &identity::Keypair,
    webrtc_cert: webrtc::tokio::Certificate,
) -> Result<Boxed<(PeerId, StreamMuxerBox)>> {
    let webrtc_transport = webrtc::tokio::Transport::new(
        keypair.clone(),
        webrtc_cert,
    );

    // Combine: QUIC || WebRTC || TCP
    let transport = quic_transport
        .or_transport(webrtc_transport)
        .or_transport(tcp_noise_yamux)
        .boxed();

    Ok(transport)
}
```

**Benefits**:

- Enables web-based dchat clients
- P2P in browsers without server relay
- NAT traversal via ICE/TURN

### 4.4 Enhanced Discovery: Add Rendezvous Protocol (Low Priority)

```rust
use libp2p::rendezvous;

#[derive(NetworkBehaviour)]
pub struct DchatBehavior {
    // ... existing protocols ...
    pub rendezvous_client: rendezvous::client::Behaviour,
    pub rendezvous_server: Option<rendezvous::server::Behaviour>,
}

impl DchatBehavior {
    pub fn register_for_channel(&mut self, channel_id: &str, server: PeerId) {
        let namespace = rendezvous::Namespace::new(format!("dchat/channel/{}", channel_id))
            .expect("valid namespace");
        self.rendezvous_client.register(namespace, server, None);
    }

    pub fn discover_channel_peers(&mut self, channel_id: &str, server: PeerId) {
        let namespace = rendezvous::Namespace::new(format!("dchat/channel/{}", channel_id))
            .expect("valid namespace");
        self.rendezvous_client.discover(Some(namespace), None, None, server);
    }
}
```

**Benefits**:

- Faster channel discovery than DHT
- Privacy-preserving (only reveals namespace interest)
- Complements DHT

### 4.5 Hybrid Post-Quantum Handshake (Medium Priority)

```rust
/// Hybrid classical + post-quantum handshake
pub struct HybridHandshake {
    noise: NoiseHandshake,
    kyber_keypair: pqcrypto_kem::kyber768::Keypair,
    kyber_ciphertext: Option<Vec<u8>>,
}

impl HybridHandshake {
    pub fn derive_hybrid_key(&self, kyber_shared: &[u8]) -> [u8; 64] {
        let noise_key = self.noise.get_symmetric_key();

        let mut output = [0u8; 64];
        hkdf::Hkdf::<sha2::Sha256>::new(None, &[noise_key, kyber_shared].concat())
            .expand(b"dchat-hybrid-key", &mut output)
            .expect("valid length");
        output
    }
}
```

**Rationale**: Protect against "harvest now, decrypt later" attacks.

### 4.6 Schema-Based Wire Protocol (Medium Priority)

```protobuf
// dchat.proto
syntax = "proto3";
package dchat.v1;

message Envelope {
    uint32 version = 1;
    oneof payload {
        DirectMessage direct = 2;
        ChannelMessage channel = 3;
        HandshakeInit handshake_init = 4;
        HandshakeResponse handshake_response = 5;
    }
    bytes signature = 15;
}

message DirectMessage {
    bytes sender_id = 1;
    bytes recipient_id = 2;
    bytes ciphertext = 3;
    uint64 sequence = 4;
    uint64 timestamp = 5;
}

message HandshakeInit {
    uint32 protocol_version = 1;
    bytes noise_payload = 2;
    repeated string supported_patterns = 3;
}
```

**Benefits**:

- Backward compatibility
- Smaller wire size
- Code generation for SDKs
- Self-documenting protocol

### 4.7 Content-Addressed Messages with IPLD (Low Priority)

```rust
use libipld::{cbor::DagCborCodec, Cid, IpldCodec};
use libipld::multihash::{Code, MultihashDigest};

pub struct ContentAddressedMessage {
    pub cid: Cid,
    pub data: Vec<u8>,
}

impl ContentAddressedMessage {
    pub fn new(message: &DchatMessage) -> Result<Self> {
        let data = DagCborCodec.encode(message)?;
        let hash = Code::Blake3_256.digest(&data);
        let cid = Cid::new_v1(IpldCodec::DagCbor.into(), hash);

        Ok(Self { cid, data })
    }

    pub fn verify(&self) -> bool {
        let hash = Code::Blake3_256.digest(&self.data);
        let expected_cid = Cid::new_v1(IpldCodec::DagCbor.into(), hash);
        self.cid == expected_cid
    }
}
```

**Benefits**:

- Deduplication
- Integrity verification
- IPFS ecosystem compatibility

### 4.8 Protocol Stack Summary

| Layer             | Current         | Recommended                  | Priority  |
| ----------------- | --------------- | ---------------------------- | --------- |
| **Transport**     | TCP             | **QUIC + TCP fallback**      | 🔴 High   |
| **Browser**       | None            | **WebRTC**                   | 🟡 Medium |
| **Encryption**    | Noise XX        | **Hybrid: Noise + Kyber768** | 🟡 Medium |
| **Multiplexing**  | Yamux           | QUIC built-in                | —         |
| **Discovery**     | Kademlia + mDNS | **+ Rendezvous**             | 🟢 Low    |
| **Relay**         | relay + dcutr   | **Full Relay v2 config**     | 🔴 High   |
| **Serialization** | Bincode/CBOR    | **Protobuf**                 | 🟡 Medium |
| **Content**       | Custom          | **IPLD/DAG-CBOR**            | 🟢 Low    |

---

## 5. Implementation Priority

### Phase 1: Critical (Weeks 1-2)

1. **Fix NAT External IP Detection**
   - Location: `dchat-network/src/nat.rs`
   - Impact: NAT traversal currently broken

2. **Add QUIC Transport**
   - Location: `dchat-network/src/transport.rs`
   - Impact: 60% latency reduction, better mobile support

3. **Configure Relay v2 Properly**
   - Location: `dchat-network/src/behavior.rs`
   - Impact: Enable relay-assisted NAT traversal

4. **Replace Critical unwrap() Calls**
   - Location: `src/main.rs` (121+ instances)
   - Impact: Prevent runtime panics

### Phase 2: Important (Weeks 3-4)

5. **Implement Typed Handshake State Machine**
   - Location: `dchat-crypto/src/handshake.rs`
   - Impact: Compile-time state validation

6. **Add Identity Binding to Handshake**
   - Location: `dchat-crypto/src/handshake.rs`
   - Impact: Prevent MITM attacks

7. **Add Handshake Rate Limiting**
   - Location: `dchat-network/src/` (new module)
   - Impact: DoS protection

8. **Implement Delta Deduplication**
   - Location: `src/main.rs:2831`
   - Impact: Efficient message sync

### Phase 3: Enhancement (Weeks 5-6)

9. **Add Protobuf Wire Format**
   - Location: New `dchat-protocol` crate
   - Impact: Versioned, smaller messages

10. **Add WebRTC Transport**
    - Location: `dchat-network/src/transport.rs`
    - Impact: Browser client support

11. **Add Rendezvous Discovery**
    - Location: `dchat-network/src/behavior.rs`
    - Impact: Faster channel discovery

12. **Implement Handshake Metrics**
    - Location: `dchat-network/src/` (new module)
    - Impact: Production observability

### Phase 4: Future (Weeks 7+)

13. **Hybrid Post-Quantum Handshake**
    - Location: `dchat-crypto/src/handshake.rs`
    - Impact: Long-term quantum resistance

14. **IPLD Content Addressing**
    - Location: New `dchat-content` crate
    - Impact: Deduplication, IPFS compatibility

15. **Dependency Injection Refactoring**
    - Location: All crates
    - Impact: Better testability

16. **Property-Based Tests**
    - Location: All `tests/` directories
    - Impact: Higher confidence in correctness

---

## Appendix: Cargo.toml Changes

```toml
# Add to crates/dchat-network/Cargo.toml
libp2p = { version = "0.54", features = [
    "kad", "noise", "tcp", "dns", "websocket", "relay", "dcutr",
    "mdns", "identify", "ping", "gossipsub", "yamux", "tokio",
    "request-response", "macros",
    # NEW FEATURES:
    "quic",
    "webrtc",
    "rendezvous",
] }

# For property-based testing
[dev-dependencies]
proptest = "1.4"

# For validated config
garde = { version = "0.18", features = ["derive"] }

# For Protobuf (if adopted)
prost = "0.12"
prost-types = "0.12"

# For IPLD (if adopted)
libipld = "0.16"
```

---

## 6. Mainnet Launch Strategy

### 6.1 Current State Analysis

**Existing Implementation**:

- Genesis block creation for both chains ([genesis.rs](crates/dchat-chain/src/chain/genesis.rs))
- Tokenomics with 100B initial supply, 1T max cap ([tokenomics.rs](crates/dchat-blockchain/src/tokenomics.rs))
- Validator staking: 10,000 DCHAT minimum, 1M DCHAT maximum ([staking.rs](crates/dchat-blockchain/src/staking.rs))
- Relay staking: 10,000 DCHAT minimum ([relay_network.rs](crates/dchat-network/src/relay_network.rs))
- 7 Foundation validators planned across global regions
- 14 relays (2 per validator server)

**Current Genesis Configuration**:

```rust
initial_supply: 1_000_000_000_000_000_000 // 1 billion tokens (18 decimals)
```

---

### 6.2 Recommended Genesis Block Design

#### Token Allocation Model

```rust
/// Genesis token allocation - transparent and verifiable
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisAllocation {
    /// Total genesis supply (before any inflation)
    pub total_supply: u64,
    /// Individual allocations with vesting schedules
    pub allocations: Vec<AllocationEntry>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AllocationEntry {
    pub recipient: AllocationRecipient,
    pub amount: u64,
    pub percentage: f64,
    pub vesting: VestingSchedule,
    pub purpose: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AllocationRecipient {
    /// Foundation multi-sig treasury
    FoundationTreasury { multisig_threshold: u8, signers: Vec<String> },
    /// Foundation validator stake (auto-staked at genesis)
    FoundationValidator { validator_index: u8 },
    /// Foundation relay stake (auto-staked at genesis)
    FoundationRelay { relay_index: u8 },
    /// Community incentive pool
    CommunityPool,
    /// Ecosystem development grants
    EcosystemGrants,
    /// Team allocation (founders, developers)
    Team { member_id: String },
    /// Early contributors / advisors
    Advisors,
    /// Public distribution (faucet, airdrops)
    PublicDistribution,
    /// Liquidity bootstrapping pool
    LiquidityBootstrap,
    /// Insurance fund
    InsuranceFund,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum VestingSchedule {
    /// Immediately liquid at genesis
    Immediate,
    /// Linear vesting over period
    Linear {
        cliff_months: u32,
        vesting_months: u32,
        start_block: u64,
    },
    /// Milestone-based release
    Milestone {
        milestones: Vec<(String, u64)>, // (milestone_name, release_amount)
    },
    /// Locked until governance vote
    GovernanceLocked,
    /// Locked in staking (auto-staked at genesis)
    StakeLocked {
        min_stake_duration_days: u32,
    },
}
```

#### Suggested Allocation Breakdown

| Category                  | Percentage | Amount (1B total) | Vesting                         | Purpose                           |
| ------------------------- | ---------- | ----------------- | ------------------------------- | --------------------------------- |
| **Foundation Validators** | 7%         | 70M               | Stake-locked 1 year             | Initial 7 validators @ 10M each   |
| **Foundation Relays**     | 2.8%       | 28M               | Stake-locked 1 year             | Initial 14 relays @ 2M each       |
| **Foundation Treasury**   | 15%        | 150M              | 3/5 multisig, governance-locked | Operations, legal, infrastructure |
| **Community Incentives**  | 30%        | 300M              | Linear 4 years                  | Relay rewards, user incentives    |
| **Ecosystem Grants**      | 15%        | 150M              | Milestone-based                 | Developer grants, integrations    |
| **Team**                  | 15%        | 150M              | 1yr cliff + 3yr linear          | Founders, core developers         |
| **Advisors**              | 3%         | 30M               | 6mo cliff + 2yr linear          | Strategic advisors                |
| **Public Distribution**   | 5%         | 50M               | Immediate                       | Faucet, initial airdrops          |
| **Liquidity Bootstrap**   | 5%         | 50M               | Immediate                       | DEX liquidity, market making      |
| **Insurance Fund**        | 2.2%       | 22M               | Governance-locked               | User protection fund              |

**Total**: 100% = 1,000,000,000 DCHAT

---

### 6.3 Foundation Validator & Relay Staking Model

#### Who Pays for Stakes?

**My Recommendation: Pre-Funded Genesis Stakes (Option A)**

```rust
/// Genesis block includes pre-funded stakes for Foundation infrastructure
pub struct FoundationInfrastructure {
    /// Validators are auto-staked at genesis - no separate transaction needed
    pub validators: Vec<GenesisValidatorStake>,
    /// Relays are auto-staked at genesis
    pub relays: Vec<GenesisRelayStake>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidatorStake {
    pub validator_id: u8,
    pub public_key: String,
    pub stake_amount: u64,
    pub operator_address: String,
    /// Foundation-controlled key that can be transferred to community later
    pub governance_key: String,
    /// Geographic region for diversity
    pub region: String,
}

impl GenesisValidatorStake {
    pub fn foundation_set() -> Vec<Self> {
        vec![
            Self {
                validator_id: 0,
                public_key: "validator-0-pubkey".to_string(),
                stake_amount: 10_000_000_000_000, // 10M DCHAT
                operator_address: "ohio.validator.dchat.network".to_string(),
                governance_key: "foundation-governance-key-0".to_string(),
                region: "us-east".to_string(),
            },
            // ... 6 more validators across regions
        ]
    }
}
```

#### Comparison of Staking Payment Models

| Model                       | Pros                                                  | Cons                                       | Recommendation                |
| --------------------------- | ----------------------------------------------------- | ------------------------------------------ | ----------------------------- |
| **A: Pre-Funded Genesis**   | Simple, no bootstrap chicken-egg problem, transparent | Foundation controls initial stakes         | ✅ **Use for mainnet launch** |
| **B: Self-Funded Purchase** | Decentralized from day 1                              | No liquidity at genesis, complex bootstrap | ❌ Not practical              |
| **C: Governance Grant**     | Community approval                                    | Requires governance before chain exists    | ❌ Not practical              |
| **D: Loan from Treasury**   | Eventual repayment                                    | Complex accounting, gaming risk            | 🟡 Future option              |

**Rationale for Option A**:

1. **Bootstrap Problem**: Can't buy tokens before chain exists
2. **Transparency**: Genesis block is public, allocations are verifiable
3. **Security**: Foundation validators provide initial stability
4. **Decentralization Path**: Clear roadmap to transition control

---

### 6.4 Foundation → Community Transition Plan

#### Phase 1: Foundation Launch (Months 1-3)

```rust
/// Initial state at genesis
pub struct Phase1State {
    /// All 7 validators operated by Foundation
    pub foundation_validators: 7,
    pub community_validators: 0,
    /// All 14 relays operated by Foundation
    pub foundation_relays: 14,
    pub community_relays: 0,
    /// Governance: Foundation has veto power
    pub governance_mode: GovernanceMode::FoundationVeto,
}
```

**Actions**:

- Foundation operates all infrastructure
- Monitor for stability and bugs
- Faucet active for user onboarding
- Begin community validator application process

#### Phase 2: Community Onboarding (Months 4-6)

```rust
pub struct Phase2State {
    pub foundation_validators: 7,
    pub community_validators: 3, // First 3 community validators
    /// Target: 4/10 BFT threshold includes community
    pub bft_threshold: "4/10",
    pub foundation_relays: 14,
    pub community_relays: 10, // First community relays
    pub governance_mode: GovernanceMode::Hybrid,
}
```

**Actions**:

- Accept first 3 community validator applications
- Community validators must self-fund stake (from public distribution or purchase)
- Foundation provides staking documentation and support
- Governance proposals can pass with 2/3 community + Foundation approval

#### Phase 3: Majority Community (Months 7-12)

```rust
pub struct Phase3State {
    pub foundation_validators: 7,
    pub community_validators: 14, // 21 total validators
    /// Target: >50% community validators
    pub bft_threshold: "14/21",
    pub foundation_relays: 14,
    pub community_relays: 50,
    pub governance_mode: GovernanceMode::CommunityMajority,
}
```

**Actions**:

- Foundation validators begin unstaking 3 of 7
- Transfer unstaked tokens to community grants
- Governance fully controlled by token holders
- Foundation retains 4 validators as minority

#### Phase 4: Full Decentralization (Year 2+)

```rust
pub struct Phase4State {
    pub foundation_validators: 2, // Minimal Foundation presence
    pub community_validators: 98, // 100 max validators
    pub governance_mode: GovernanceMode::FullCommunity,
}
```

**Actions**:

- Foundation retains only 2 validators (for emergency recovery)
- All remaining Foundation stake delegated to community pools
- Foundation treasury managed by DAO

---

### 6.5 Staking Pool Architecture

#### Problem with Direct Staking

- Minimum 10,000 DCHAT for validators is high barrier
- Small holders can't participate in security
- Concentration risk

#### Recommended: Delegation Pools

```rust
/// Staking pool that aggregates small stakes for validator/relay operation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingPool {
    pub pool_id: Uuid,
    pub pool_type: PoolType,
    /// Validator or relay this pool backs
    pub operator_id: UserId,
    /// Operator's own stake (must be >= 10% of pool)
    pub operator_stake: u64,
    /// Total delegated from community
    pub delegated_stake: u64,
    /// Individual delegations
    pub delegators: HashMap<UserId, Delegation>,
    /// Commission rate (basis points, e.g., 1000 = 10%)
    pub commission_bps: u16,
    /// Pool performance metrics
    pub metrics: PoolMetrics,
    /// Auto-compound rewards or distribute
    pub auto_compound: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PoolType {
    Validator,
    Relay,
    /// Hybrid pool that backs both validators and relays
    Hybrid,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Delegation {
    pub delegator: UserId,
    pub amount: u64,
    pub delegated_at: DateTime<Utc>,
    /// Pending rewards (claimable)
    pub pending_rewards: u64,
    /// Lock period (optional)
    pub lock_until: Option<DateTime<Utc>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PoolMetrics {
    /// Historical APY (annualized)
    pub apy_30d: f64,
    /// Uptime percentage
    pub uptime_percentage: f64,
    /// Slashing events count
    pub slashing_count: u32,
    /// Total rewards distributed
    pub total_rewards_distributed: u64,
}

impl StakingPool {
    /// Calculate delegator's share of rewards
    pub fn calculate_delegator_rewards(
        &self,
        delegator: &UserId,
        pool_reward: u64,
    ) -> u64 {
        let delegation = match self.delegators.get(delegator) {
            Some(d) => d,
            None => return 0,
        };

        let total_stake = self.operator_stake + self.delegated_stake;
        let delegator_share = delegation.amount as f64 / total_stake as f64;

        // Deduct operator commission
        let after_commission = pool_reward * (10000 - self.commission_bps as u64) / 10000;

        (after_commission as f64 * delegator_share) as u64
    }

    /// Minimum operator stake (10% of total pool)
    pub fn min_operator_stake(&self) -> u64 {
        (self.delegated_stake + self.operator_stake) / 10
    }

    /// Check if pool meets requirements
    pub fn is_valid(&self) -> bool {
        // Operator must have at least 10% of total pool
        self.operator_stake >= self.min_operator_stake()
            // Total must meet minimum staking requirement
            && (self.operator_stake + self.delegated_stake) >= MIN_POOL_STAKE
    }
}

pub const MIN_POOL_STAKE: u64 = 10_000_000_000; // 10,000 DCHAT

/// Maximum commission rate
pub const MAX_COMMISSION_BPS: u16 = 2000; // 20%
```

#### Pool Benefits

1. **Lower Barrier**: Users can delegate any amount
2. **Operator Alignment**: Operator has skin-in-the-game (10% minimum)
3. **Slashing Protection**: Pool absorbs slashing proportionally
4. **Passive Income**: Delegators earn without running infrastructure

---

### 6.6 Genesis Block Creation Process

#### Step-by-Step Mainnet Genesis

```rust
/// Complete genesis creation workflow
pub struct GenesisCreationWorkflow {
    steps: Vec<GenesisStep>,
}

pub enum GenesisStep {
    /// 1. Generate validator keys offline (air-gapped)
    GenerateValidatorKeys {
        count: usize,
        key_ceremony_participants: Vec<String>,
    },

    /// 2. Verify keys via multi-party computation
    VerifyKeysCeremony {
        threshold: (u8, u8), // e.g., 4-of-7
        verification_hashes: Vec<String>,
    },

    /// 3. Create allocation CSV (publicly auditable)
    CreateAllocationManifest {
        allocations: Vec<AllocationEntry>,
        merkle_root: String,
    },

    /// 4. Multi-sig approval of allocations
    ApproveAllocations {
        required_signatures: u8,
        signers: Vec<String>,
        signatures: Vec<Signature>,
    },

    /// 5. Build genesis blocks for both chains
    BuildGenesisBlocks {
        chat_chain_config: ChatGenesisConfig,
        currency_chain_config: CurrencyGenesisConfig,
    },

    /// 6. Publish genesis hashes for verification
    PublishGenesisHashes {
        chat_genesis_hash: String,
        currency_genesis_hash: String,
        publication_channels: Vec<String>, // GitHub, Twitter, website
    },

    /// 7. Coordinate validator startup
    CoordinateValidatorStartup {
        target_timestamp: u64,
        validator_checklist: Vec<ValidatorReadiness>,
    },
}
```

#### Genesis Security Checklist

```markdown
## Genesis Block Security Checklist

### Key Generation (Week -2)

- [ ] Air-gapped machine for key generation
- [ ] Multiple witnesses for key ceremony
- [ ] Keys split via Shamir Secret Sharing (3-of-5)
- [ ] Encrypted backup to geographically distributed locations
- [ ] Hardware security module (HSM) integration for production keys

### Allocation Verification (Week -1)

- [ ] Allocation CSV published to GitHub
- [ ] Merkle tree of all allocations computed
- [ ] Independent audit of allocation math
- [ ] Community review period (7 days minimum)
- [ ] Multi-sig approval (4-of-7 Foundation signers)

### Genesis Creation (Day -1)

- [ ] Final allocation manifest locked
- [ ] Genesis blocks created on isolated machine
- [ ] Genesis hashes published to multiple channels
- [ ] Validator binary checksums verified
- [ ] All 7 validators confirm genesis hash match

### Launch (Day 0)

- [ ] Coordinated start time (e.g., 2025-01-15 00:00:00 UTC)
- [ ] Validators start in sequence with 30-second gaps
- [ ] First block produced within 5 minutes
- [ ] 4/7 consensus achieved
- [ ] Genesis allocations verified on-chain
- [ ] Block explorer shows correct balances
```

---

### 6.7 Economic Security Parameters

#### Recommended Constants

```rust
/// Economic security parameters for mainnet
pub mod mainnet_economics {
    /// Validator staking
    pub const MIN_VALIDATOR_STAKE: u64 = 10_000_000_000; // 10,000 DCHAT
    pub const MAX_VALIDATOR_STAKE: u64 = 100_000_000_000; // 100,000 DCHAT (prevent whale dominance)
    pub const VALIDATOR_UNSTAKE_COOLDOWN_DAYS: u32 = 14; // 2 weeks

    /// Relay staking
    pub const MIN_RELAY_STAKE: u64 = 1_000_000_000; // 1,000 DCHAT
    pub const MAX_RELAY_STAKE: u64 = 50_000_000_000; // 50,000 DCHAT
    pub const RELAY_UNSTAKE_COOLDOWN_DAYS: u32 = 7; // 1 week

    /// Slashing
    pub const DOUBLE_SIGN_SLASH_PERCENT: u8 = 5;
    pub const DOWNTIME_SLASH_PERCENT: u8 = 1; // Per 24h of downtime
    pub const MAX_SLASH_PERCENT: u8 = 100; // For malicious behavior

    /// Rewards
    pub const ANNUAL_INFLATION_PERCENT: u8 = 5;
    pub const VALIDATOR_REWARD_SHARE: u8 = 70; // 70% to validators
    pub const RELAY_REWARD_SHARE: u8 = 20; // 20% to relays
    pub const TREASURY_SHARE: u8 = 10; // 10% to treasury

    /// Governance
    pub const PROPOSAL_DEPOSIT: u64 = 100_000_000; // 100 DCHAT
    pub const VOTING_PERIOD_DAYS: u32 = 7;
    pub const QUORUM_PERCENT: u8 = 33; // 33% of staked tokens must vote
    pub const PASS_THRESHOLD_PERCENT: u8 = 50; // Simple majority

    /// Pools
    pub const MIN_POOL_OPERATOR_PERCENT: u8 = 10; // Operator must have 10% of pool
    pub const MAX_POOL_COMMISSION_PERCENT: u8 = 20;
    pub const MIN_DELEGATION: u64 = 100_000_000; // 100 DCHAT minimum delegation
}
```

---

### 6.8 Mainnet Launch Timeline

```
Week -4: Key Ceremony
├── Generate validator keys (air-gapped)
├── Multi-party verification
├── Distribute key shards
└── Store encrypted backups

Week -3: Allocation Finalization
├── Publish allocation CSV to GitHub
├── Community review period begins
├── Independent audit
└── Address any community concerns

Week -2: Genesis Preparation
├── Lock final allocations (multi-sig)
├── Create genesis blocks
├── Publish genesis hashes
├── Deploy validator binaries to servers
└── Testnet dress rehearsal

Week -1: Final Checks
├── All 7 validators confirm readiness
├── DNS records verified
├── Storage clusters operational
├── Monitoring dashboards active
└── Emergency contacts confirmed

Day 0: Launch
├── 00:00 UTC - Genesis timestamp
├── 00:00-00:05 - Validators 1-7 start (30s gaps)
├── 00:05 - First block produced
├── 00:10 - 4/7 consensus confirmed
├── 00:30 - Relays start
├── 01:00 - Full network operational
├── 06:00 - First stability checkpoint
└── 24:00 - Launch success declared

Week +1: Stabilization
├── Monitor block production
├── Verify reward distribution
├── Enable faucet
├── First community validator applications
└── Bug bounty program live

Month +1: First Expansion
├── First 3 community validators active
├── First community relays
├── Governance proposals enabled
└── Foundation provides staking support
```

---

### 6.9 Risk Mitigation

#### Pre-Launch Risks

| Risk                  | Mitigation                                            |
| --------------------- | ----------------------------------------------------- |
| **Key compromise**    | Shamir secret sharing, HSM, air-gapped generation     |
| **Allocation error**  | Multi-sig approval, public audit, merkle verification |
| **Consensus failure** | Testnet rehearsal, staged validator startup           |
| **Network partition** | Geographic distribution, multiple DNS providers       |

#### Post-Launch Risks

| Risk                   | Mitigation                                            |
| ---------------------- | ----------------------------------------------------- |
| **Foundation capture** | Transition plan with timeline, governance controls    |
| **Validator cartel**   | Max stake caps, geographic diversity requirements     |
| **Economic attack**    | Slashing, minimum stake requirements, cooldowns       |
| **Software bug**       | Circuit breaker, emergency governance, insurance fund |

---

### 6.10 Summary: My Recommendations

1. **Genesis Allocation**: Use pre-funded genesis stakes for Foundation infrastructure (7% validators, 2.8% relays)

2. **Foundation Pays Initially**: Foundation tokens are minted at genesis and auto-staked - no purchase needed

3. **Clear Transition Path**: 4-phase decentralization from Foundation-only to community-majority over 12 months

4. **Staking Pools**: Implement delegation pools so small holders can participate (minimum 100 DCHAT delegation)

5. **Economic Parameters**:
   - 10,000 DCHAT minimum validator stake
   - 1,000 DCHAT minimum relay stake
   - 100 DCHAT minimum delegation
   - 14-day validator unstake cooldown
   - 5% annual inflation with 70/20/10 split (validators/relays/treasury)

6. **Security**: Air-gapped key ceremony, multi-sig allocations, public audit period, testnet rehearsal

7. **Community Validators**: Self-funded from public distribution (5% = 50M tokens available via faucet/airdrops)

---

_End of Plan v3.0_
