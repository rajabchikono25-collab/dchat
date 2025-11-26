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
- **`ARCHITECTURE.md`**: Legacy architecture document with component breakdown and threat model.

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
