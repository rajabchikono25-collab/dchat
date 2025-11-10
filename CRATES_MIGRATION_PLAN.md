# dchat Crate Migration Plan

## 1. Objectives
- Modularize monolithic `src/` implementation into existing workspace crates under `crates/`.
- Preserve 100% functionality (152 passing tests) throughout phased migration.
- Maintain git history via `git mv` operations.
- Establish clean dependency boundaries and crate ownership of code & dependencies.

## 2. Scope
In-scope directories to migrate from `src/`:
- `config/` (shared constants/types) → `dchat-core`
- `crypto/` → `dchat-crypto`
- `identity/` → `dchat-identity`
- `chain/` (currency_chain + slashing) → `dchat-chain`
- `validator/` → `dchat-validator`
- `network/` (including `onion/`) → `dchat-network`
- `discovery/` → `dchat-network/src/discovery/`
- `relay/` (proof + reputation) → `dchat-network/src/relay/`
- `observability/` → `dchat-observability`
- `deployment.rs` → `dchat-deployment`

Out-of-scope (already modular or not required now):
- Existing fully implemented crates (messaging, storage, governance, bridge, marketplace, bots, privacy, accessibility, sdk, testing, distribution).
- `main.rs` (CLI entrypoint) will be adjusted **after** migration.
- `lib.rs` will become a facade re-export layer last.

## 3. Source → Target Mapping (Summary)
| Source Path | Target Crate Path |
|-------------|-------------------|
| `src/config/**` | `crates/dchat-core/src/config/**` |
| `src/crypto/mod.rs` | `crates/dchat-crypto/src/lib.rs` (or keep `mod.rs` rename) |
| `src/crypto/handshake/*` | `crates/dchat-crypto/src/handshake/*` |
| `src/crypto/versioning.rs` | `crates/dchat-crypto/src/versioning.rs` |
| `src/identity/**` | `crates/dchat-identity/src/**` |
| `src/chain/currency_chain/*` | `crates/dchat-chain/src/currency_chain/*` |
| `src/chain/slashing/*` | `crates/dchat-chain/src/slashing/*` |
| `src/chain/mod.rs` | `crates/dchat-chain/src/lib.rs` (merge exports) |
| `src/validator/*` | `crates/dchat-validator/src/*` |
| `src/network/nat_telemetry.rs` | `crates/dchat-network/src/nat_telemetry.rs` |
| `src/network/onion/*` | `crates/dchat-network/src/onion/*` |
| `src/network/mod.rs` | `crates/dchat-network/src/lib.rs` |
| `src/discovery/dht.rs` | `crates/dchat-network/src/discovery/dht.rs` |
| `src/discovery/mod.rs` | `crates/dchat-network/src/discovery/mod.rs` |
| `src/relay/proof/*` | `crates/dchat-network/src/relay/proof/*` |
| `src/relay/reputation/*` | `crates/dchat-network/src/relay/reputation/*` |
| `src/relay/mod.rs` | `crates/dchat-network/src/relay/mod.rs` |
| `src/observability/*` | `crates/dchat-observability/src/*` |
| `src/deployment.rs` | `crates/dchat-deployment/src/lib.rs` |

## 4. Dependency Order (Critical Path)
1. Core Foundations: `dchat-core` → `dchat-crypto` → `dchat-identity`
2. Consensus & Validation: `dchat-chain` → `dchat-validator`
3. Networking & Relay: `dchat-network` (includes discovery + relay)
4. Ops & Monitoring: `dchat-observability`
5. Deployment Tooling: `dchat-deployment`
6. Facade & Final Integration: root `lib.rs` + CLI adjustments

## 5. Phase Breakdown
### Phase 1: Foundation
Move config, crypto, identity. Update imports & crate `Cargo.toml` files. Verify tests.
### Phase 2: Chain & Validator
Migrate chain & validator logic; adjust any cross-crate references.
### Phase 3: Networking & Relay
Relocate network, discovery, relay, onion routing files.
### Phase 4: Observability
Move metrics + region monitoring.
### Phase 5: Deployment
Move `deployment.rs` into its crate.
### Phase 6: Facade & Cleanup
Refactor root `lib.rs` to re-export public APIs from crates for backward compatibility; adjust root integration tests & CLI `main.rs` imports.

## 6. Detailed Steps Per Phase
Each phase uses: branch creation, file moves, dependency updates, test verification.

#### Common Pre-Step
```pwsh
# Create migration branch
git checkout -b refactor/crates-migration
```

#### Phase 1
```pwsh
# Move config
git mv src/config crates/dchat-core/src/
# Move crypto
git mv src/crypto crates/dchat-crypto/src/
# Move identity
git mv src/identity crates/dchat-identity/src/
# Update imports (example): replace `crate::crypto::` with `dchat_crypto::`
# Run targeted tests
cargo test -p dchat-core -p dchat-crypto -p dchat-identity
```
Commit when green:
```pwsh
git add .
git commit -m "Phase 1: move config, crypto, identity into crates"
```

#### Phase 2
```pwsh
git mv src/chain crates/dchat-chain/src/
git mv src/validator crates/dchat-validator/src/
# Fix imports: `crate::chain::` → `dchat_chain::`, etc.
cargo test -p dchat-chain -p dchat-validator
```
Commit.

#### Phase 3
```pwsh
# Create subdirs if missing
git mv src/network crates/dchat-network/src/
mkdir crates/dchat-network/src/discovery
mkdir crates/dchat-network/src/relay
# Move discovery
git mv src/discovery/dht.rs crates/dchat-network/src/discovery/dht.rs
# Move relay
git mv src/relay/proof crates/dchat-network/src/relay/proof
git mv src/relay/reputation crates/dchat-network/src/relay/reputation
# Adjust onion paths if needed
cargo test -p dchat-network
```
Commit.

#### Phase 4
```pwsh
git mv src/observability crates/dchat-observability/src/
cargo test -p dchat-observability
```
Commit.

#### Phase 5
```pwsh
git mv src/deployment.rs crates/dchat-deployment/src/lib.rs
cargo test -p dchat-deployment
```
Commit.

#### Phase 6
- Update root `lib.rs` to re-export selected crate APIs for external consumers.
- Clean obsolete internal modules from root.
- Adjust `main.rs` imports from `crate::` to crate names.
- Final full test run:
```pwsh
cargo test --workspace
cargo clippy --workspace -D warnings
cargo fmt --all
```
Commit final phase.

## 7. Cargo.toml Ownership Adjustments
- Remove crate-specific deps from root and add them to owning crates:
  - `dchat-crypto`: `snow`, `ed25519-dalek`, `x25519-dalek`, `aes-gcm`, `chacha20poly1305`, `blake3`, `sha2`, `zeroize`
  - `dchat-network`: `libp2p`, `rand`, `aes-gcm`, `chacha20poly1305`, `blake3`
  - `dchat-observability`: `prometheus`, `once_cell`
  - `dchat-validator`: depends on `dchat-chain`, `dchat-core`, plus crypto libs if used directly
  - `dchat-core`: minimal shared utilities (serde, thiserror, tracing, chrono, uuid)
- Ensure each crate has `[dev-dependencies]` for any test-only libs.

## 8. Import Path Transformation Rules
| Old | New |
|-----|-----|
| `crate::crypto::handshake::` | `dchat_crypto::handshake::` |
| `crate::identity::peer_registry::` | `dchat_identity::peer_registry::` |
| `crate::network::onion::sphinx::` | `dchat_network::onion::sphinx::` |
| `crate::relay::reputation::` | `dchat_network::relay::reputation::` |
| `crate::observability::metrics::` | `dchat_observability::metrics::` |
| `crate::validator::thresholds::` | `dchat_validator::thresholds::` |
| `crate::chain::slashing::` | `dchat_chain::slashing::` |
| `crate::config::constants::` | `dchat_core::config::constants::` |

Tip: Use search & replace per phase; verify with `cargo check`.

## 9. Testing Strategy
- After each phase: run crate-specific tests.
- After large import rewrites: `cargo check` then `cargo test -q`.
- Final acceptance: `cargo test --workspace` must show 152 passing or updated count if tests relocated.
- Optional: enable incremental verification with CI (if available).

## 10. Risk & Mitigation
| Risk | Mitigation |
|------|------------|
| Broken imports | Phase-by-phase limited moves + immediate `cargo check` |
| Hidden cyclic deps | Early migration of foundational crates (core/crypto/identity) |
| Lost history | Use `git mv` only, avoid delete + re-add |
| Test regressions | Run tests per crate every phase |
| Overlapping module names | Confirm unique crate prefixes (`dchat_*`) |
| Dependency leakage | Move deps to owning crate; review root Cargo.toml diff |

## 11. Rollback Plan
- If a phase fails: `git reset --hard HEAD~1` to revert last commit.
- If deep conflict arises: stash current changes, reapply moves in smaller batches.
- Maintain branch isolation until full migration validated.

## 12. Validation Criteria
- All tests pass after each phase.
- No unused dependency warnings in crates.
- Root `lib.rs` compiles and provides stable public API (or documented breaking changes).
- `cargo clippy --workspace -D warnings` passes.
- `cargo fmt --all` yields clean formatting.

## 13. Post-Migration Actions
- Update `ARCHITECTURE.md` to reflect crate ownership.
- Add crate-level README.md (auto-gen skeleton if missing).
- Enable per-crate semantic versioning strategy.
- Configure CI to run per-crate jobs (parallel test builds).
- Document external API surface in root README.

## 14. Suggested Commit Messages
- Phase 1: "migrate: config, crypto, identity to crates"
- Phase 2: "migrate: chain and validator modules"
- Phase 3: "migrate: network, discovery, relay, onion routing"
- Phase 4: "migrate: observability metrics & region monitoring"
- Phase 5: "migrate: deployment orchestration"
- Phase 6: "refactor: root facade exports & final integration"

## 15. Glossary
- **Facade**: Root `lib.rs` re-export layer maintaining compatibility.
- **Workspace Member**: A crate listed in root `[workspace]`.
- **Phased Migration**: Incremental movement reducing blast radius.
- **Relay**: Incentivized node delivering messages.
- **Sphinx Packet**: Onion routing packet with layered encryption.
- **NAT Telemetry**: Metrics determining best traversal method.

## 16. Quick Command Reference
```pwsh
# Phase example sequence
git checkout -b refactor/crates-migration
# Phase 1 moves
git mv src/config crates/dchat-core/src/
git mv src/crypto crates/dchat-crypto/src/
git mv src/identity crates/dchat-identity/src/
# Test
cargo test -p dchat-core -p dchat-crypto -p dchat-identity
# Commit
git add .
git commit -m "migrate: phase 1 foundations"
```

## 17. Acceptance Checklist
- [ ] Phase 1 tests green
- [ ] Phase 2 tests green
- [ ] Phase 3 tests green
- [ ] Phase 4 tests green
- [ ] Phase 5 tests green
- [ ] Facade exports validated
- [ ] Workspace test pass (all)
- [ ] Clippy and fmt clean
- [ ] Docs updated

---
**Ready to execute Phase 1 when approved.**
