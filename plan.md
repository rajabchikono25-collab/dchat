# dchat implementation plan

## 1. Scope and objectives
This plan captures remaining implementation gaps and hardening work for dchat. It focuses on areas where the architecture and documentation describe a production design but parts of the code are still stubbed, partially integrated, or explicitly marked as "in production" placeholders. It does not execute any changes; it only defines concrete work items.

Primary goals:
- Finish or wire up partially implemented subsystems (network, SDK, identity/attestation, governance/guardians, bots, VR integration, dispute resolution).
- Close the loop between on-chain components and in-memory placeholders (validator registry lookups, treasury operations, routing, distribution).
- Add hardening: tests, metrics, observability, backup validation, and security posture.

## 2. Current state overview (concise)

### 2.1 Sharding and rebalancing
- `crates/dchat-chain/src/sharding/rebalancing.rs` implements a consistent hash ring, shard load scoring, and a `RebalancingScheduler` with tests.
- `crates/dchat-chain/src/sharding/state_migration.rs` implements a two-phase/streaming migration flow (`MigrationCoordinator`, `TwoPhaseCommit`, rollback, snapshots).
- These are largely algorithm-complete but not fully integrated into the live runtime scheduler and observability.

### 2.2 Network, routing, and discovery
- NAT traversal (STUN/UPnP/hole punching/TURN) is implemented with tests (`nat_traversal.rs`, `nat/turn.rs`).
- Gossip signing and verification are implemented with Ed25519 in `dchat-network::gossip::protocol`.
- Onion routing crypto and path building are implemented, but some relay handlers still return decrypted payloads rather than forwarding through a network channel.
- Legacy DHT discovery (`crates/dchat-network/src/discovery/dht_legacy.rs`) is stubbed: `try_dht_discovery` returns an empty list with TODO comments about real Kademlia integration.

### 2.3 Rust SDK
- `crates/dchat-sdk-rust/src/client.rs` derives Noise keys and sketches a libp2p swarm, but `connect`, `disconnect`, and `send_message` are only partially wired and do not drive a live swarm or submit to the chain.

### 2.4 Identity, attestation, guardians, and dispute resolution
- Identity MPC and FROST-based threshold signing are implemented, but production-grade attestation and keyless onboarding are not.
- `crates/dchat-identity/src/attestation.rs` verifies only a simulated attestation string and issues a `VerifiedBadge` with a dummy signature; comments mark this as a development helper.
- `src/onboarding/keyless/enclave.rs` simulates enclave behavior and device attestation rather than integrating with platform TPm/SE/OS APIs.
- Guardian-based recovery in `crates/dchat-chain/src/chain/guardians.rs` is implemented, including ZK proof verification for guardian anonymity and on-chain state, but nullifier storage and freshness checks are marked as "in production" concerns.
- Dispute resolution in `crates/dchat-chain/src/dispute_resolution.rs` validates fork evidence with Ed25519 and sequence numbers, but `get_validator_pubkey` currently derives a key via BLAKE3 as a temporary stand-in for the on-chain validator registry.
- Multi-region validator coordination in `crates/dchat-validator/src/multi_region.rs` implements BFT signature checks and geographic diversity, but assumes an in-memory validator set and local signing key rather than a fully integrated on-chain registry and key management.

### 2.5 Governance and DAO
- `crates/dchat-governance/src/protocol_dao.rs` implements proposals, voting, treasury accounting, and execution hooks.
- Execution functions (`execute_parameter_change`, `execute_protocol_upgrade`, `execute_emergency_action`, grant programs, etc.) are currently stubs that update in-memory state or return `ExecutionResult::Success` with comments indicating that real chain state transitions are missing.

### 2.6 Bots and bot messaging
- `crates/dchat-bots/src/messaging_integration.rs`:
  - `BotMessagingClient` builds messages and queues them but uses a placeholder Noise session; `encrypt_message_content` currently returns plaintext bytes.
  - `MessageRouter::route_message` only logs intent and does not integrate with a DHT/libp2p routing layer.
  - `MessageRouter::submit_to_blockchain` returns a dummy sequence number without creating a real transaction.
- `crates/dchat-bots/src/bot_api.rs`:
  - `BotApi` methods log and compute hashes but treat on-chain queries and permission checks as TODOs.
  - `BotClient` talks to an HTTP API, which is fine, but on-chain semantics (ordering, ownership, permissions) are not wired through.

### 2.7 VR platforms and haptics
- `crates/dchat-vr/src/platforms/openxr.rs` and `visionos.rs` implement `VrPlatform` for OpenXR and visionOS, but many methods are simulation shims with comments describing real OpenXR/ARKit/RealityKit wiring.
- `crates/dchat-vr/src/haptics.rs` provides a `HapticManager` and maps gestures to high-level haptic events; handling of full custom waveforms is intentionally simplified with a note that production should process the full sequence.

### 2.8 ZK proofs and privacy
- `crates/dchat-privacy/src/zk_proofs.rs` implements Groth16 circuits for contact and reputation proofs, setup, serialization, `ZkProver`, and `ZkVerifier`.
- Guardians use `ZkVerifier::verify_contact` for anonymity proofs; other components can leverage the same keys.
- MPC-backed trusted setup integration and operational flows (ceremony outputs, key rotation, storage) are not yet wired end-to-end.

### 2.9 Deployment, backups, and observability
- Deployment scripts in `crates/dchat-deployment` describe Terraform/Ansible and cluster bootstrapping with a mix of automation and manual steps, particularly for storage and monitoring.
- `backup_system.rs` defines helpers and validation, but end-to-end backup/restore pipelines (plus alerts and periodic validation) are not fully exercised.

## 3. Implementation tasks by subsystem

### 3.1 Sharding and rebalancing
Goals: turn the existing sharding/rebalancing logic into a production scheduler with safe state migration, metrics, and operational controls.

Planned work:
- **Runtime integration**
  - Wire `RebalancingScheduler` into the main chain node loop (`src/main.rs`) so that it:
    - Periodically evaluates shard load based on real metrics (block production rate, queue depth, disk usage) rather than only synthetic tests.
    - Emits rebalancing plans that invoke `MigrationCoordinator`.
  - Define configuration knobs for rebalancing frequency, safety thresholds (max shard movement per window), and maintenance windows.
- **State migration robustness**
  - Validate that `state_migration.rs` handles partial failures: simulate node restarts, network partitions, and rollbacks in integration tests.
  - Add idempotency guarantees and explicit invariants (e.g., no double-commit, consistent shard ownership mapping before/after migration).
- **Metrics and observability**
  - Expose metrics for shard load, pending migrations, duration and outcome of each migration, and rollback counts.
  - Add structured logs and tracing spans around rebalancing decisions and migrations.
- **Testing**
  - Add property tests or integration tests that:
    - Rebalance under skewed load and verify no shard is overloaded beyond configured thresholds.
    - Perform rolling upgrades with concurrent rebalancing and ensure no data loss or double inclusion.

### 3.2 Network, routing, and discovery
Goals: harden onion routing and gossip, replace legacy placeholders with real DHT usage, and improve NAT/relay observability.

Planned work:
- **Onion routing integration**
  - Replace relay handlers that return decrypted payloads with actual forwarding via the network channel used by libp2p.
  - Ensure relay nodes maintain stable X25519 keys (via `dchat-network` keystore) and persist path state as needed.
  - Add tests for multi-hop paths (3+ hops), replay protection, and failure handling when a middle relay drops.
- **Legacy DHT bridging**
  - Implement real Kademlia-based discovery in `dht_legacy.rs::try_dht_discovery` by integrating with the existing libp2p DHT.
  - Define a migration path for any legacy discovery mechanisms so that callers transparently get modern DHT behavior.
- **NAT traversal and TURN hardening**
  - Add telemetry around NAT strategy selection (direct, UPnP, hole punching, TURN) and success/failure rates.
  - Ensure TURN credentials and allocations are rotated and cleaned up correctly; add tests for long-running sessions.
- **Gossip and validator identity**
  - Confirm that gossip signature verification uses the same identity/peer registry data structures as consensus (no split brain between `PeerId` and on-chain validator IDs).
  - Add metrics for invalid signatures, misbehaving peers, and gossip storms.

### 3.3 Rust SDK
Goals: expose a production-ready Rust SDK that can connect to the network, send/receive messages, and interact with the chain.

Planned work:
- **Connect/disconnect**
  - Implement full libp2p `Swarm` creation in `dchat-sdk-rust::client` using the same transport, multiplexing, and behaviours as the main node, but scoped to client needs.
  - Implement `connect` to:
    - Build and start the swarm.
    - Join the appropriate discovery and messaging topics.
    - Expose a connection handle that can be awaited or polled for events.
  - Implement `disconnect` with graceful shutdown: stop swarm tasks, close streams, and flush pending messages.
- **Message send/receive APIs**
  - Implement `send_message` to:
    - Construct `dchat-messaging::Message` with correct sender identity and encryption.
    - Route via the libp2p layer and submit a hash to the chain where applicable.
  - Add a `subscribe`/`receive` API exposing an async stream of decrypted messages for a user.
- **Error handling, retries, and backpressure**
  - Define clear error types for network vs. chain failures.
  - Add basic retry policies and backpressure when downstream queues are full.
- **Documentation and examples**
  - Provide sample client code demonstrating one-to-one, group, and channel messaging using the SDK.

### 3.4 Identity, attestation, guardians, dispute resolution, and validators
Goals: move from simulated attestation and placeholder keys to real secure identity bindings, and tighten guardian and dispute flows.

Planned work:
- **Device attestation and keyless onboarding**
  - Replace the simulated `"dchat-enclave-attestation-v1"` check in `attestation.rs` with real platform-specific attestation verification (Android/Play Integrity, iOS/DeviceCheck, WebAuthn, etc.).
  - Extend `src/onboarding/keyless/enclave.rs` to:
    - Request and package platform attestation tokens.
    - Bind device keys to attestation results and user identities.
  - Define storage for attestation metadata, expiry, and revocation.
- **Guardian ZK and nullifier handling**
  - Persist ZK nullifiers on-chain to prevent proof reuse in guardian registration and recovery flows.
  - Extend `verify_guardian_zk_proof` to check on-chain nullifier sets and proof freshness (timestamp or block height inside proofs).
  - Add tests for replay attempts and duplicate guardian registrations.
- **Validator registry integration**
  - Replace `dispute_resolution::get_validator_pubkey`'s deterministic BLAKE3-derived key with real lookups from the blockchain validator registry, including caching and error handling.
  - Ensure `multi_region.rs` uses the same registry (and avoids unchecked `unwrap()` when converting keys and signatures).
  - Add cross-module tests that submit fork evidence and verify it triggers slashing across regions using genuine keys.

### 3.5 Governance and DAO
Goals: move DAO actions from in-memory stubs to real protocol effects and ensure governance-driven changes are auditable and safe.

Planned work:
- Wire `ProtocolDaoManager` execution paths (`execute_parameter_change`, `execute_protocol_upgrade`, `execute_emergency_action`, grant programs) to:
  - Emit transactions that update protocol parameters in the chain state.
  - Coordinate with staking/treasury modules for fund movements and reserved balances.
  - Log and store execution metadata (block height, tx hash) in a canonical on-chain location.
- Implement guards for emergency actions and upgrades:
  - Require stronger quorum thresholds and multi-stage confirmations for destructive actions.
  - Add tests for misconfigured or malicious proposals (e.g., draining treasury, disabling safety invariants).

### 3.6 Bots and bot messaging
Goals: give bots first-class, secure access to messaging with proper routing, encryption, and on-chain accountability.

Planned work:
- **BotMessagingClient encryption and routing**
  - Replace the placeholder `NoiseSession` with a real Noise/Libp2p-based encryption session.
  - Require an initialized encryption session in `encrypt_message_content`, returning an error if not configured.
  - Implement `MessageRouter::route_message` to:
    - Use the libp2p DHT or centralized registry to locate recipients or channel relays.
    - Forward messages via relay nodes and handle delivery acknowledgements.
  - Implement `submit_to_blockchain` to create and submit real transactions carrying message hashes and metadata.
- **BotApi / BotClient semantics**
  - Wire `BotApi` methods to:
    - Check ownership and permissions via chain queries for edits and deletions.
    - Record message IDs and hashes for audit trails.
  - Ensure `BotClient` HTTP endpoints map cleanly onto the messaging and chain operations, including error propagation.
- **Rate limiting and abuse prevention**
  - Add per-bot rate limiting and quotas (API-level and chain-level), with tests for abuse scenarios.

### 3.7 VR platforms and haptics
Goals: evolve the VR layer from simulation into usable integrations for supported devices, while keeping it optional for core network operations.

Planned work:
- **OpenXR integration**
  - Replace println-based shims with real OpenXR initialization and session control.
  - Implement hand tracking and transforms using OpenXR joint APIs; wire `convert_hand_joints` to real data.
- **visionOS integration**
  - Integrate with ARKit/RealityKit and spatial audio for head and hand tracking.
  - Implement real haptic feedback via platform APIs (CHHapticEngine/TapticEngine).
- **HapticManager improvements**
  - Extend custom waveform handling to process and schedule full sequences instead of only the first segment.
  - Add tests for long waveforms and overlapping events.

### 3.8 ZK proofs and privacy
Goals: ensure ZK components are production-ready, auditable, and integrated with MPC-based trusted setup.

Planned work:
- Integrate `Groth16Keys::setup` with an MPC ceremony pipeline, documenting how proving and verifying keys are generated, verified, and distributed.
- Define formats and storage for proving/verifying keys (on-chain pointer vs. off-chain storage with hashes on-chain).
- Add rotation and revocation mechanisms for ZK keys, including migration strategies for existing proofs.
- Expand ZK usage beyond guardians/contacts where appropriate (e.g., reputation proofs in moderation or access control), reusing the existing circuits.

### 3.9 Deployment, backups, and observability
Goals: reduce manual steps, make deployments reproducible, and ensure data durability.

Planned work:
- **Deployment automation**
  - Convert "Manual step" comments in deployment binaries into idempotent automated tasks where feasible (cluster creation, storage bootstrapping, monitoring stack setup).
  - Provide clearly documented flags for environments (dev, staging, mainnet) with safe defaults.
- **Backup and restore**
  - Implement scheduled backup jobs using `backup_system.rs` with automated verification (e.g., periodic restore tests in staging).
  - Add alerts for missed or failed backups and for restore validation failures.
- **Observability**
  - Standardize metrics and logs across validators, relays, storage, bots, and SDK clients.
  - Define dashboards for shard health, consensus health, messaging latency, and error rates.

## 4. Cross-cutting hardening tasks

- **Documentation alignment**
  - Update `ARCHITECTURE-2.0.md` and related status docs to:
    - Reflect implemented features (KMS, NAT traversal, onion routing, MPC/FROST, state validation, BFT broadcast).
    - Correct outdated notes (e.g., XOR-based MPC and onion routing replaced with FROST and AEAD-based designs).
  - Document any remaining intentional deviations from the original architecture.
- **Security review**
  - Perform a focused review on:
    - Use of deterministic keys (e.g., temporary validator pubkey derivation) and remove them from production paths.
    - All TODO/"In production" markers in crypto, networking, and identity code.
  - Add fuzz tests where appropriate (parsers, gossip handling, TURN/STUN, ZK proof verification inputs).
- **Testing and CI**
  - Ensure new tests are integrated into existing test/chaos frameworks and CI pipelines.
  - Add targeted chaos scenarios: shard migrations during network partitions, validator region outages, and misconfigured bots.

## 5. Suggested sequencing (high level)

1. **Phase 0 – Documentation and safety**
   - Update architecture/status docs; remove deterministic validator keys from any production configuration; add basic metrics hooks where trivial.
2. **Phase 1 – Core network and SDK**
   - Finish onion routing and DHT integration; wire sharding/rebalancing into runtime; deliver a usable Rust SDK.
3. **Phase 2 – Identity, governance, and dispute resolution**
   - Implement real device attestation, guardian nullifier storage, validator registry integration, and DAO execution effects.
4. **Phase 3 – Bots, VR, and ecosystem**
   - Complete bot messaging pipeline and HTTP API semantics; progressively harden VR integrations and haptics for supported devices.
5. **Phase 4 – ZK, deployment, and backup maturity**
   - Tie Groth16 setup to MPC, finalize ZK key lifecycle, and fully automate backups, restore validation, and observability.
