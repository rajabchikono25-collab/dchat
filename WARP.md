# WARP.md

This file provides guidance to WARP (warp.dev) when working with code in this repository.

## Core Commands

### Workspace build & basic workflow
- Build all crates (debug):
  - `cargo build`
- Build all crates (release, used in production docs):
  - `cargo build --release`
- Run all tests for the workspace:
  - `cargo test`
  - `cargo test --all`
- Run tests with full logging:
  - `RUST_LOG=debug cargo test -- --nocapture`

### Running nodes (demo / local dev)
Commands are from `README.md`; the main binary is the `dchat` CLI in `src/main.rs`.

- Quick demo (multiple roles):
  - Start a relay node:
    - `cargo run --release -- --role relay --port 9090`
  - Start a validator node:
    - `cargo run --release -- --role validator --port 9091`
  - Start user nodes (interactive chat):
    - `cargo run -- --role user --name "Alice" --port 9092`
    - `cargo run -- --role user --name "Bob" --port 9093`

- Minimal examples (from Copilot rules):
  - `cargo run --release -- --role relay`
  - `cargo run -- --role user`

### Tests & benchmarks

#### Workspace / integration tests
- All tests (debug):
  - `cargo test`
- Integration tests that expect a local chain/testnet:
  - `cargo test --test integration_tests`
- Chaos tests (ensure network/consensus chaos harness stays green, referenced in docs):
  - `cargo test --test chaos_tests`

#### Storage crate (database-backed storage)
From `crates/dchat-storage/README.md` and `DOCUMENTATION_INDEX.md`:
- Unit tests (in‑memory + core logic):
  - `cargo test --package dchat-storage`
  - `cargo test --package dchat-storage --lib deduplication`
- Integration tests with a Postgres-compatible DB:
  - Set `TEST_DATABASE_URL` then run:
    - `export TEST_DATABASE_URL="postgresql://localhost/dchat_test"`
    - `cargo test --package dchat-storage --features test-db db_tests`
- Run storage load test example (Phase 4, if needed):
  - `cargo run --release --example load_test -- --messages 1000000`
- Run storage migrations helper scripts (from `crates/dchat-storage/DOCUMENTATION_INDEX.md`):
  - `./crates/dchat-storage/scripts/run-migrations.sh "postgresql://localhost/dchat"`
  - `./crates/dchat-storage/scripts/run-migrations.ps1 "postgresql://localhost/dchat"`

#### Benchmarks (criterion-based)
From `benchmarks/README.md` and `Cargo.toml`:
- Run individual benchmark suites:
  - `cargo bench --bench message_throughput`
  - `cargo bench --bench crypto_performance`
  - `cargo bench --bench database_queries`
  - `cargo bench --bench network_latency`
- Run all benchmarks:
  - `cargo bench`
- Run benchmarks while managing baselines:
  - `cargo bench -- --save-baseline main`
  - `cargo bench -- --baseline main`

### Deployment & automation tooling

Automation lives under `AUTOMATION_README.md` and the `terraform/` and `ansible/` subdirectories.

#### Terraform (multi-region validator/infra provisioning)
From `AUTOMATION_README.md`:
- Initialize in `terraform/`:
  - `cd automation/terraform` (or the directory referenced in that file)
  - `terraform init`
- Create and edit variables file:
  - `cp terraform.tfvars.example terraform.tfvars`
  - Edit with your values (SSH key, MinIO/Redis secrets, etc.).
- Plan and apply:
  - `terraform plan`
  - `terraform apply`
- Useful Terraform utilities:
  - `terraform show`
  - `terraform state list`
  - `terraform destroy`
  - `terraform apply -target=aws_instance.validator_ohio`

#### Ansible (post‑provision configuration)
From `AUTOMATION_README.md` (in `automation/ansible`):
- Test connectivity:
  - `ansible validators -i inventory.ini -m ping`
- Dry‑run the main playbook:
  - `ansible-playbook -i inventory.ini playbook.yml --check`
- Full deploy:
  - `ansible-playbook -i inventory.ini playbook.yml`
- Region‑ or host‑scoped deploys:
  - `ansible-playbook -i inventory.ini playbook.yml --limit aws_validators`
  - `ansible-playbook -i inventory.ini playbook.yml --limit validator1-ohio.schikuno.top`

#### Post‑deployment validation
Also from `AUTOMATION_README.md`:
- Verify DNS configuration from repo root (PowerShell):
  - `./verify-dns.ps1`
- On a validator host (after SSH):
  - `./status.sh`
- Local service health checks on a validator:
  - `docker exec redis redis-cli ping`
  - `curl http://localhost:9000/minio/health/live`

## High‑Level Architecture & Structure

This project is a Rust workspace organized as a set of composable crates plus a top‑level `dchat` binary. It implements a dual‑chain decentralized chat protocol (chat chain + currency chain) with strong privacy, identity, and governance features.

### Workspace layout & key crates

The workspace members (from `Cargo.toml`) define the main subsystems:
- **Core & shared types**:
  - `crates/dchat-core`: configuration, shared error types, common domain primitives, and the internal event bus.
  - `crates/dchat-data`: shared data models used across crates.
- **Cryptography & identity**:
  - `crates/dchat-crypto`: Noise Protocol sessions, Ed25519 signing, key rotation, hashing/KDFs, and hooks for post‑quantum schemes.
  - `crates/dchat-identity`: hierarchical key derivation, device/identity models, guardian recovery, multi‑device sync, and the scaffolding for secure enclave + MPC signing.
- **Networking & messaging**:
  - `crates/dchat-network`: libp2p transport/swarm, Kademlia DHT discovery, gossip, relay network, NAT traversal scaffolding, onion‑routing primitives, eclipse‑attack prevention, and rate‑limiting.
  - `crates/dchat-messaging`: message types, ordering, queues, delivery proofs, expiration, channel access control and staking‑aware gating, plus rich media support.
- **Blockchain & consensus integration**:
  - `crates/dchat-chain`: on‑chain primitives (transactions, sharding, slashing, pruning, insurance fund, dispute resolution).
  - `crates/dchat-blockchain`: clients for the chat and currency chains, economic primitives, PoRW/PoT logic, oracle network, and cross‑chain helpers.
  - `crates/dchat-bridge`: cross‑chain bridge primitives (BLS finality, multisig, bridge‑level slashing) used to keep chat and currency chains in sync.
- **Storage & data plane**:
  - `crates/dchat-storage`: compression, deduplication, SQLite/SQLx backing stores, storage bonds & micropayments, IPFS integration, and tiered storage architecture (hot/warm/cold/archive) for messages and media.
- **Governance, marketplace, bots, and ecosystem**:
  - `crates/dchat-governance`: DAO voting, protocol upgrades, moderation and abuse reporting primitives.
  - `crates/dchat-marketplace`: NFTs/digital goods, creator economy logic, and escrow.
  - `crates/dchat-bots`: bot framework (commands, webhooks, permissions, token security) plus a Rust SDK integration surface.
- **Observability, validators, deployment & distribution**:
  - `crates/dchat-observability`: Prometheus metrics, tracing, and alerting primitives.
  - `crates/dchat-validator`: validator health and threshold logic for multi‑region deployments.
  - `crates/dchat-deployment`: multi‑region deployment orchestration, backup systems, and health monitoring.
  - `crates/dchat-distribution`: version/package distribution and upgrade plumbing.
- **UX‑adjacent crates**:
  - `crates/dchat-accessibility`: cross‑platform accessibility primitives (WCAG‑aligned abstractions, integration hooks for UI layers).
  - `crates/dchat-vr`: VR/AR session primitives (avatars, spatial audio, gestures) used by advanced clients.
- **SDKs, testing and benchmarks**:
  - `crates/dchat-sdk-rust`: Rust client SDK for external apps and bots.
  - `crates/dchat-testing`: shared testing/chaos utilities used by integration tests (e.g., `tests/chaos/`).
  - `benchmarks/`: Criterion benchmarks for throughput, networking, DB, crypto, and scaling.

### Runtime composition & main entrypoint

The main binary (`src/main.rs`, exposed via the `[[bin]]` named `dchat`) wires these crates together into concrete node roles:
- The typical execution pipeline is:
  - `dchat-core` (config & types)
  - → `dchat-crypto` (keys/sessions)
  - → `dchat-network` (transport, discovery, relay mesh)
  - → `dchat-messaging` (message construction, ordering, queues)
  - → `dchat-chain` / `dchat-blockchain` (on‑chain ordering, rewards, slashing)
  - → `dchat-storage` (durable history and media storage).
- Node behavior is selected by CLI flags such as `--role relay`, `--role validator`, or `--role user`; the `main.rs` CLI also contains production checks (minimum peers, staking preconditions, health endpoints, graceful shutdown, etc.).

Agents modifying orchestration, node behavior, or CLI flags should search in `src/main.rs` first, then follow calls into the corresponding crates.

### Dual‑chain model & bridge

- **Chat chain**: owns identity, channels, message ordering, governance, and reputation. Most “social” and messaging semantics live here.
- **Currency chain**: owns token supply, staking/bonding, relay and validator rewards, marketplace settlement, and treasury/insurance.
- **Bridge (`dchat-bridge` + `dchat-blockchain::cross_chain`)**: manages cross‑chain consistency (e.g., using BLS‑aggregated finality proofs) so that chat‑chain actions which depend on economic state remain atomic with their currency‑chain counterparts.

When adding or changing any feature that spans identity, messaging, and economics (e.g., token‑gated channels, reward logic), expect to touch:
- Messaging/identity crates for local behavior and types.
- Chain/blockchain crates for transactions and economic rules.
- Bridge and possibly governance crates for cross‑chain guarantees and policy.

### Documentation map (for deeper context)

- `README.md`: high‑level explanation of dchat, current readiness, key innovations, and links to core docs.
- `ARCHITECTURE.md`: conceptual architecture of the 34 subsystems (design‑level view).
- `ARCHITECTURE-2.0.md`: forensic analysis of the actual implementation status across crates (what is complete vs. stubbed and what still needs production hardening).
- `DOCUMENTATION_INDEX.md`: global index into production‑readiness documents (hardening guides, deployment plans, status dashboards, and roadmaps).
- `crates/dchat-storage/DOCUMENTATION_INDEX.md`: focused index for the storage subsystem (deduplication/compression, DB schema, and tiering architecture).
- `.github/copilot-instructions.md`: additional, highly detailed architectural and file‑layout guidance; useful when you need to understand where a concern “should” live even if some paths are aspirational.

Future Warp agents should consult these docs before making large architectural changes or introducing new cross‑cutting features (governance, bridge logic, cryptography, or storage economics).