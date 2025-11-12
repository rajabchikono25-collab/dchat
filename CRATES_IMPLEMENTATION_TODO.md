# Mainnet Readiness Report — Key Crates (Nov 11, 2025)

This document summarizes mainnet-critical gaps, concrete action items, and linkage checks for:
`dchat-messaging`, `dchat-bridge`, `dchat-network` (note: requested as "dchat-networking"), `dchat-bots`, and `dchat-accessibility`.

Cargo status: `cargo check --workspace` completed successfully (no type/link errors). Inline unit tests exist; no dedicated integration test folders found.

## General
- Add dedicated `tests/` directories for cross-crate integration and property-based tests.
- Add soak/chaos tests for network subsystems (gossip, NAT, onion, relay).
- Harden config defaults for production; ensure secrets/keys never logged.
- Enable observability by default (metrics, tracing spans, sampling, log redaction).

---

## dchat-messaging
- Critical:
	- `delivery.rs::DeliveryProof::verify` returns true if signature present; implement real Ed25519 verification and on-chain TX confirmation.
	- Chain-backed token/NFT gating: `channel_access.rs` contains `TODO` to inject a `StakingVerifier` via DI; wire to currency/chat chain clients.
- High:
	- Backpressure and quota on `queue.rs` across users/tenants to prevent memory pressure.
	- Replay protection for `MessageId` collisions; document ordering guarantees around reorgs.
- Tests:
	- Add integration tests for ordering across partitions, expiration windows, and offline queue overflow/eviction.

---

## dchat-bridge
- Critical:
	- Multi-signature validation: complete production-grade signature verification (commented guidance exists in `multisig.rs`). Choose Ed25519 or BLS and standardize across the stack; add batch verification and slashing hooks.
	- Finality tracking and rollback: verify finality proofs are enforced before executing target-chain effects; add explicit rollback paths and tests.
- High:
	- Slashing evidence schema: verify evidence serialization is authenticated and auditable; connect to governance for appeals.
- Tests:
	- Integration tests for atomic swaps across chains and slashing flows (claim–challenge–respond).

---

## dchat-network
- Critical (must-fix before mainnet):
	- Gossip signatures: `gossip/protocol.rs` uses deterministic hash as a fake signature ("INSECURE FOR PRODUCTION"). Implement Ed25519 signing with the node’s identity key and strict verification on receipt.
	- Onion routing keys: `routing.rs` and `onion_routing.rs` use ephemeral relay keys with warnings. Generate persistent X25519 relay keypairs, store in encrypted keystore, publish public keys via discovery, and use for ECDH.
	- NAT traversal completeness:
		- UPnP external IP retrieval returns `External IP detection not fully implemented` — finish SOAP call handling and errors.
		- TURN messages have placeholder fields and partial attributes; complete RFC 5766 handling (auth, MESSAGE-INTEGRITY, XOR-PEER-ADDRESS, refresh/close).
		- Hole punching currently returns `Ok(false)` — implement real UDP hole punching and fallback logic.
- High:
	- Eclipse prevention & relay diversity: ensure path selection enforces ASN/geographic diversity consistently across `onion_routing` and `network/onion` modules.
	- Rate-limiting: wire per-peer/global rate limits from config to flood control (gossip and relay ingress).
	- Cover traffic & timing obfuscation: verify rates and disable in constrained environments; expose runtime toggles.
- Tests & Perf:
	- Add integration/fuzz tests for onion circuits, TURN/STUN state machines, and gossip propagation under churn.
	- Add latency/throughput benchmarks for NAT strategies and circuit build times.

---

## dchat-bots
- Critical:
	- Token handling: ensure bot tokens and webhook secrets are stored encrypted at rest; add rotation endpoints.
	- Rate limiting & abuse prevention for webhook and API endpoints.
- High:
	- Replace example `mock_*` assets/tokens with production-safe examples and disclaimers outside shipped binaries.
	- SQL migrations: ensure `sqlx` schema migrations run on startup with idempotency; add DB-level constraints and indexes for hot paths.
- Tests:
	- Integration tests for webhook HMAC verification, permission checks, and command routing.

---

## dchat-accessibility
- High:
	- TTS backends: validate platform engines (Windows SAPI, Linux speech-dispatcher, macOS AVSpeechSynthesizer) behind current API, with fallbacks.
	- Add automated WCAG checks in CI for UI surfaces using snapshots/linters.
- Tests:
	- Scenario tests for focus management, ARIA roles/labels, and TTS queue preemption.

---

## Linkage & Orphan Checks
- Verified module wiring:
	- `dchat-network`: `onion_routing.rs` exports `CircuitId`, `CircuitStatus`, `OnionRoutingManager` matching top-level re-exports. `network/onion/*` submodules are linked through `network/mod.rs`.
	- No orphan or misslinked modules detected within the reviewed crates.
- Legacy relay types (`RelayClient`, `RelayConfig`, `RelayNode`) are intentionally removed from `dchat-network` (Phase 3 migration) and now exist in `dchat-sdk-rust`. No dangling imports were found.
- Naming note: repository uses `crates/dchat-network` (not `dchat-networking`).

---

## Action Plan (Prioritized)
1) Network security hardening
	 - Implement real Ed25519 gossip signatures and strict verification.
	 - Switch onion routing to persistent X25519 relay keys with keystore + discovery publication.
2) NAT traversal completeness
	 - Finish UPnP external IP SOAP calls; finalize TURN allocate/channel/send/refresh/close; implement hole punching.
3) Bridge correctness
	 - Production multi-sig verification + batch verification; finalize finality checks and rollback tests.
4) Messaging correctness
	 - Real delivery proof verification with chain confirmation; wire DI for token/NFT gating to on-chain verifiers.
5) Observability & limits
	 - Enable metrics + tracing; enforce rate limits; add redaction; alerting for NAT/circuit failures.
6) Testing
	 - Add `tests/` integration suites per crate; fuzz NAT/onion; performance baselines and regressions.

Owners/next steps can be assigned per team; I can scaffold the missing test suites and key management modules on request.

---

## Additional Crates Findings (Scanned Post-Update)

### dchat-blockchain
- Critical:
	- `block_hierarchy.rs`: multiple placeholders for transaction/world state logic — replace with production implementations tied to consensus state.
	- `proof_of_transit.rs`: placeholder Dilithium public keys — implement real PQ key management or gate behind feature flags until ready.
	- `client.rs`: placeholder submission/confirmation flows — integrate with real RPC and finality checks.
- Tests:
	- `tests/consensus_gap_fixes.rs` uses placeholder signatures — convert to real signing with test keys.

### dchat-chain
- Critical:
	- `currency_chain/staking.rs` contains TODOs and mock values (chain client, tx id, block height, query responses). Implement real chain client integration and proper finality/receipt handling.
- Tests:
	- Add integration tests exercising staking operations against a local devnet/simulator.

### dchat-core
- Status:
	- No TODO/placeholder matches. Treat as stable foundation; ensure semver discipline and error taxonomy remain consistent.

### dchat-crypto
- High:
	- `crypto/handshake/noise.rs` uses placeholder first_message in tests; align tests with actual Noise handshake vectors.
	- Time-based tests reference real time; consider deterministic time mocking for stability.

### dchat-data
- Status:
	- No TODO/placeholder matches. Review schemas for forward-compat and migrations.

### dchat-deployment
- Status:
	- No code TODOs flagged; verify scripts handle idempotency and rollback for mainnet cutover.

### dchat-distribution
- Status:
	- No matches; confirm release signing and mirror verification paths.

### dchat-governance
- Status:
	- Minimal notes in `voting.rs`; proceed to finalize commit-reveal timings, slashing integration, and appeals flow.

### dchat-identity
- High:
	- Phase/Sprint markers for `biometric`, `enclave`, `mpc`, and `guardian_recovery` indicate staged features. For mainnet, ensure disabled behind feature flags unless production-ready.
	- `peer_registry.rs` notes replacing placeholder PeerIds — confirm identity binding is enforced before reputation scoring.

### dchat-marketplace
- High:
	- Tests contain `panic!` on mismatch assertions; fine for tests, but ensure escrow transitions and dispute flows have integration coverage.

### dchat-observability
- Status:
	- No matches; validate metrics/traces are on by default for mainnet nodes, with redaction policies.

### dchat-privacy
- Status:
	- No matches; confirm ZK/proof components are feature-gated and audited paths are enabled.

### dchat-sdk-rust
- Critical:
	- `client.rs`: placeholders for X25519 key derivation from ed25519 and libp2p integration; implement real identity/key derivation and network client wiring to match `dchat-network`.
	- TODOs for swarm cleanup and DHT routing; complete before SDK consumption by clients.
	- `relay.rs`: placeholder reputation/downtime defaults; pull from real relay telemetry.
- Tests:
	- Add end-to-end examples using the real network stack once wired.

### dchat-storage
- Status:
	- Docs reference Phase 3 completions; no code TODOs flagged. Validate dedup/compression with production workloads and enable metrics.

### dchat-testing
- Status:
	- No matches; expand to include chaos and fuzz harnesses referenced in network plan.

### dchat-validator
- High:
	- `health.rs` TODO indicates health checks should call actual HTTP/gRPC endpoints — implement real validator probes and expose metrics.

---

## Cross-Crate Linkage Notes
- No obvious mislinked or orphaned modules discovered in the additional crates via quick scan; deeper linkage verification recommended for `dchat-identity` (feature-gated modules) and `dchat-sdk-rust` (network wiring).
- Maintain feature flags for not-mainnet-ready components (biometric, enclave, MPC, PQ crypto) and ensure they are off by default in mainnet builds.
