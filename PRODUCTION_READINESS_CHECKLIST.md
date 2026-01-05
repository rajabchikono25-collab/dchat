# DChat Production Readiness Checklist

Generated: 2026-01-05

This checklist maps the top 20 production gaps identified in `src/` to concrete wiring tasks, required configurations, and affected CLI/node modes.

---

## Legend

| Priority | Meaning |
|----------|---------|
| 🔴 **P0** | Hard blocker — must fix before mainnet launch |
| 🟠 **P1** | High risk — can cause prod failures or security issues |
| 🟡 **P2** | Medium — feature incomplete or ops footgun |
| 🟢 **P3** | Low — polish, edge cases, or optional features |

| Mode | Description |
|------|-------------|
| `validator` | Full validator node |
| `light-client` | Light client mode |
| `cli` | Command-line operations (rewards, miniapps, VR, etc.) |
| `all` | Affects all modes |

---

## 1. Keyless Hardware Keys

| Attribute | Value |
|-----------|-------|
| **Priority** | 🔴 P0 |
| **Files** | `src/onboarding/keyless/enclave.rs` |
| **Modes Affected** | `light-client`, `cli` (onboarding flows) |

### Current State
- Release builds `panic!` if no TPM/Secure Enclave/StrongBox is detected
- Hardware key generation is `unimplemented!()` — requires native platform SDK integration
- Software fallback only allowed when `DCHAT_ALLOW_SOFTWARE_KEYS=true`

### Required Wiring

| Task | Details |
|------|---------|
| **Native SDK integration** | Implement `HardwareKeyStore` trait for each platform (Windows TPM, macOS Secure Enclave, Android StrongBox, iOS Secure Enclave) |
| **Fallback policy config** | Add `[keyless]` section to config: `allow_software_keys = false` (default) |
| **Env var (dev/test)** | `DCHAT_ALLOW_SOFTWARE_KEYS=true` — only for dev/test builds |
| **Feature flag** | Consider `#[cfg(feature = "hardware-keys")]` to gate native deps |

### Acceptance Criteria
- [ ] Hardware key generation works on at least one target platform
- [ ] Release builds without `DCHAT_ALLOW_SOFTWARE_KEYS` gracefully error (not panic) when no hardware available
- [ ] Software fallback is disabled by default in production configs

---

## 2. Biometric Authentication Stubbed

| Attribute | Value |
|-----------|-------|
| **Priority** | 🔴 P0 (if biometrics required for keyless) |
| **Files** | `src/onboarding/keyless/biometric.rs` |
| **Modes Affected** | `light-client`, `cli` (onboarding) |

### Current State
- Biometric auth is env-var simulation only
- `DCHAT_BIOMETRIC_AVAILABLE=true` / `DCHAT_BIOMETRIC_OK=true` control behavior
- No actual platform SDK hooks (Face ID, Touch ID, Android BiometricPrompt, Windows Hello)

### Required Wiring

| Task | Details |
|------|---------|
| **Platform SDK integration** | Implement `BiometricAuthenticator` trait per platform |
| **Graceful degradation** | If biometrics unavailable, fall back to PIN/password with clear UX |
| **Config option** | `[keyless] require_biometric = true` (can be false for accessibility) |
| **Remove env-var simulation** | Gate behind `#[cfg(test)]` or `#[cfg(feature = "mock-biometric")]` |

### Acceptance Criteria
- [ ] Biometric prompt appears on supported devices
- [ ] Fallback auth works when biometrics unavailable/disabled
- [ ] Env-var simulation removed from release builds

---

## 3. Foundation Validators Use Placeholder Pubkeys

| Attribute | Value |
|-----------|-------|
| **Priority** | 🔴 P0 |
| **Files** | `src/main.rs` (foundation validator initialization) |
| **Modes Affected** | `validator` (genesis/bootstrap) |

### Current State
- Public keys derived deterministically from DNS hashes
- Comment explicitly states: "production must read real key files"
- Affects genesis validator set and initial stake distribution

### Required Wiring

| Task | Details |
|------|---------|
| **Key file loading** | Read validator pubkeys from `genesis/*.json` or config `[foundation_validators]` section |
| **Key format** | Define canonical format (hex, base64, or PEM) |
| **Validation** | Verify key format and uniqueness at startup |
| **Genesis script** | Update `ansible/generate-validator-keys.sh` to produce production keys |

### Config Example
```toml
[foundation_validators]
keys_dir = "/etc/dchat/genesis/validators/"
# OR inline:
# [[foundation_validators.keys]]
# name = "validator-india"
# pubkey = "0x..."
```

### Acceptance Criteria
- [ ] Validator nodes load real pubkeys from config/files
- [ ] Genesis block contains correct foundation validator set
- [ ] DNS-hash derivation removed or gated behind `#[cfg(test)]`

---

## 4. Miniapp Registry Lookup Not Wired

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 |
| **Files** | `src/cli_handlers/miniapp.rs`, `src/main.rs` |
| **Modes Affected** | `cli` (miniapp commands) |

### Current State
- `--app-id` path prints "not yet implemented"
- Only local WASM path works for miniapp launch

### Required Wiring

| Task | Details |
|------|---------|
| **Registry contract** | Deploy/define on-chain miniapp registry |
| **RPC query** | Implement `MiniappRegistry::lookup(app_id) -> MiniappManifest` |
| **Config** | `[miniapps] registry_contract = "0x..."` |
| **Caching** | Local cache with TTL for registry lookups |

### Acceptance Criteria
- [ ] `dchat miniapp launch --app-id <id>` fetches manifest from chain
- [ ] Manifest includes WASM hash, developer identity, permissions
- [ ] Graceful error if app not found in registry

---

## 5. Miniapp Developer Identity is Placeholder

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/main.rs` (miniapp handling) |
| **Modes Affected** | `cli`, `light-client` (miniapp sandbox) |

### Current State
- `DeveloperId::from_bytes([0u8; 32])` hardcoded
- `AppId` derivation not bound to real developer identity
- Security/integrity gap for miniapp provenance

### Required Wiring

| Task | Details |
|------|---------|
| **Developer registration** | On-chain developer identity linked to signing key |
| **Manifest signing** | Miniapp manifest signed by developer key |
| **AppId derivation** | `AppId = hash(developer_pubkey, app_name, version)` |
| **Verification** | Verify developer signature before sandbox launch |

### Acceptance Criteria
- [ ] AppId correctly derived from developer identity
- [ ] Unsigned/mismatched manifests rejected
- [ ] Placeholder `[0u8; 32]` removed from production path

---

## 6. On-Chain Program Manifest Query Not Wired

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 |
| **Files** | `src/main.rs` (program inspection) |
| **Modes Affected** | `cli` |

### Current State
- Program manifest inspection only works for local WASM files
- Chain query path prints "not yet implemented"

### Required Wiring

| Task | Details |
|------|---------|
| **Program registry RPC** | `get_program_manifest(program_id) -> ProgramManifest` |
| **WASM hash verification** | Fetch on-chain hash, compare to local if both provided |
| **Config** | Reuse `[miniapps] registry_contract` or separate `[programs]` section |

### Acceptance Criteria
- [ ] `dchat program inspect --program-id <id>` fetches from chain
- [ ] Manifest includes permissions, entry points, version

---

## 7. ServiceContext Accepts Empty RPC URLs

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/service_context.rs` |
| **Modes Affected** | `all` |

### Current State
- `require_currency_chain(false)` / `require_chat_chain(false)` silently sets RPC URL to `""`
- Comment: "will fail on actual use"
- Latent failure if caller forgets `require=true`

### Required Wiring

| Task | Details |
|------|---------|
| **Strict mode default** | Change default to `require=true` for production configs |
| **Startup validation** | Log warning or fail fast if RPC URL is empty and chain features used |
| **Config validation** | Add `validate_config()` step at startup |
| **Feature gating** | If chain not needed (e.g., offline mode), use explicit `--offline` flag |

### Config Example
```toml
[chains]
currency_rpc_url = "https://currency.dchat.network/rpc"  # Required
chat_rpc_url = "https://chat.dchat.network/rpc"          # Required
# offline_mode = false  # Explicit opt-out
```

### Acceptance Criteria
- [ ] Empty RPC URLs cause startup error (not silent)
- [ ] Offline mode is explicit opt-in
- [ ] CI tests validate production config completeness

---

## 8. Relay-Work Block Height Uses Placeholder Genesis

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/main.rs`, `src/relay_work_store.rs` |
| **Modes Affected** | `validator` |

### Current State
- `DEFAULT_GENESIS_TIMESTAMP` constant used for time-based block height estimation
- Other paths use config-driven genesis
- Skew can affect reward attribution

### Required Wiring

| Task | Details |
|------|---------|
| **Unified genesis source** | All code paths read genesis timestamp from config |
| **Config entry** | `[chain] genesis_timestamp = 1735689600` (Unix epoch) |
| **Remove constant** | Replace `DEFAULT_GENESIS_TIMESTAMP` with config read |
| **Fallback** | If truly needed, log warning when using fallback |

### Acceptance Criteria
- [ ] Single source of truth for genesis timestamp
- [ ] No hardcoded timestamp constants in production paths
- [ ] Reward calculations consistent across restarts

---

## 9. Relay-Work Store Constant is Placeholder

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/relay_work_store.rs` |
| **Modes Affected** | `validator` |

### Current State
- `DEFAULT_GENESIS_TIMESTAMP` documented as placeholder
- Same issue as #8

### Required Wiring
See item #8 — same fix applies.

---

## 10. Light Client Offline Queue Returns Placeholder MessageId

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 |
| **Files** | `src/light_client.rs` |
| **Modes Affected** | `light-client` |

### Current State
- `queue_operation()` returns `MessageId(op_uuid)` before send/finality
- If UI treats as final, it won't match chain-derived IDs later

### Required Wiring

| Task | Details |
|------|---------|
| **Pending ID type** | Return `PendingMessageId` wrapper (not `MessageId`) |
| **Status tracking** | UI shows "pending" state until finality confirmed |
| **ID reconciliation** | On finality, map `PendingMessageId` → `MessageId` |
| **Offline queue events** | Emit `MessageQueued`, `MessageSent`, `MessageFinalized` events |

### Acceptance Criteria
- [ ] UI distinguishes pending vs finalized messages
- [ ] No ID mismatch after finality
- [ ] Offline queue operations tracked correctly

---

## 11. Queued DMs Fail Without FeeGateway

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/light_client.rs` |
| **Modes Affected** | `light-client` |

### Current State
- If FeeGateway not attached, DM ops fail with "Call set_fee_gateway() first"
- Users see hard failures and stuck queues

### Required Wiring

| Task | Details |
|------|---------|
| **Mandatory FeeGateway** | Require FeeGateway at `LightClient::new()` construction |
| **Startup validation** | Fail fast if FeeGateway missing in production mode |
| **Test mode** | Allow `#[cfg(test)]` to skip FeeGateway requirement |
| **Error recovery** | If FeeGateway attached later, retry stuck queue |

### Acceptance Criteria
- [ ] Production light clients always have FeeGateway
- [ ] Clear error at startup, not on first DM attempt
- [ ] Queue retry works after FeeGateway attached

---

## 12. Consensus Task Can Crash Process

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/main.rs` (consensus task) |
| **Modes Affected** | `validator` |

### Current State
- Currency client creation uses `panic!` on error
- Transient RPC/misconfig crashes the process

### Required Wiring

| Task | Details |
|------|---------|
| **Graceful error handling** | Replace `panic!` with `Result` return |
| **Retry with backoff** | Exponential backoff for transient RPC errors |
| **Circuit breaker** | After N failures, controlled shutdown with clear logs |
| **Health endpoint** | Expose `/health` that reflects consensus task status |

### Acceptance Criteria
- [ ] Transient RPC errors don't crash validator
- [ ] Persistent failures trigger graceful shutdown
- [ ] Logs clearly indicate error cause

---

## 13. Network Path Has `expect()` on Address Parse

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/main.rs` (network initialization) |
| **Modes Affected** | `validator`, `light-client` |

### Current State
- `FALLBACK_LISTEN_ADDR` parse uses `expect()`
- Bad config or unexpected state crashes process

### Required Wiring

| Task | Details |
|------|---------|
| **Config validation** | Validate listen address at config load time |
| **Graceful fallback** | If parse fails, use safe default or error cleanly |
| **Startup checks** | Add `validate_network_config()` step |

### Acceptance Criteria
- [ ] Invalid listen address produces clear error, not panic
- [ ] Config validation catches issues before network init

---

## 14. Fee Bypass Exists Behind `test-bypass` Feature

| Attribute | Value |
|-----------|-------|
| **Priority** | 🔴 P0 (security) |
| **Files** | `src/user_management.rs` |
| **Modes Affected** | `all` |

### Current State
- Deprecated methods bypass FeeGateway when `test-bypass` feature enabled
- Allows unpaid storage

### Required Wiring

| Task | Details |
|------|---------|
| **CI enforcement** | Ensure `test-bypass` never in release feature set |
| **Cargo.toml audit** | Remove `test-bypass` from default features |
| **Build script** | Add check: `#[cfg(all(feature = "test-bypass", not(debug_assertions)))] compile_error!()` |
| **Deprecation** | Mark bypass methods with `#[deprecated]` and plan removal |

### Acceptance Criteria
- [ ] Release builds fail if `test-bypass` enabled
- [ ] CI explicitly tests that release profile excludes bypass
- [ ] Bypass methods removed in future version

---

## 15. User Key Export Uses `unwrap()` for JSON

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟢 P3 |
| **Files** | `src/user_management.rs` |
| **Modes Affected** | `cli` |

### Current State
- `serde_json::to_string_pretty(...).unwrap()` can panic on serialization failure

### Required Wiring

| Task | Details |
|------|---------|
| **Error propagation** | Replace `.unwrap()` with `?` or `.map_err()` |
| **User-facing error** | "Failed to export key: {reason}" |

### Acceptance Criteria
- [ ] No `unwrap()` on JSON serialization in production paths
- [ ] Graceful error message on failure

---

## 16. Rewards CLI Uses Hardcoded Default RPC URL

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 |
| **Files** | `src/main.rs` (rewards CLI) |
| **Modes Affected** | `cli` |

### Current State
- Defaults to `https://currency.dchat.network/rpc` if `DCHAT_CURRENCY_RPC_URL` not set
- May be intentional, but risky for region-pinned or isolated deployments

### Required Wiring

| Task | Details |
|------|---------|
| **Explicit config requirement** | In production, require explicit RPC URL (no default) |
| **Warning log** | If using default, log warning: "Using default RPC URL" |
| **Config file support** | Read from `config.toml` as well as env var |
| **Region pinning** | Document recommended URLs per region |

### Acceptance Criteria
- [ ] Production deployments require explicit RPC URL
- [ ] Default URL only used in dev/quickstart scenarios
- [ ] Clear docs on RPC URL configuration

---

## 17. FeeGateway State Persistence Boundary

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 |
| **Files** | `src/fee_gateway.rs` |
| **Modes Affected** | `light-client`, `validator` |

### Current State
- `load_persisted_state()` restores operation mappings only
- Escrow records managed by FeeOrchestrator
- Restart/idempotency depends on FeeOrchestrator persistence

### Required Wiring

| Task | Details |
|------|---------|
| **FeeOrchestrator persistence** | Ensure FeeOrchestrator saves escrow state to same or coordinated store |
| **Atomic state** | Consider unified state file for FeeGateway + FeeOrchestrator |
| **Recovery protocol** | Document what happens if state files are inconsistent |
| **Startup reconciliation** | On restart, reconcile pending escrows with chain state |

### Acceptance Criteria
- [ ] Restart preserves both operation mappings and escrow state
- [ ] Inconsistent state detected and logged
- [ ] Recovery procedure documented

---

## 18. FeeGateway Lock `.unwrap()` Pervasively

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟠 P1 |
| **Files** | `src/fee_gateway.rs` |
| **Modes Affected** | `all` |

### Current State
- Many `.write().unwrap()` / `.read().unwrap()` calls
- Single panic in lock holder can poison and take down fee processing

### Required Wiring

| Task | Details |
|------|---------|
| **Poison recovery** | Use `lock.write().unwrap_or_else(\|e\| e.into_inner())` pattern |
| **Or switch lock type** | Consider `parking_lot::RwLock` (no poisoning) |
| **Audit all locks** | Search for `.unwrap()` after lock acquisition |
| **Panic isolation** | Consider `catch_unwind` for fee processing tasks |

### Acceptance Criteria
- [ ] Lock poisoning doesn't cascade to other operations
- [ ] Alternative lock crate evaluated and documented
- [ ] No `.unwrap()` directly after lock acquisition

---

## 19. VR CLI is Placeholder Only

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟢 P3 (if VR not in scope) |
| **Files** | `src/main.rs` (VR commands) |
| **Modes Affected** | `cli` |

### Current State
- Commands print "in production, this would initialize the VR runtime…"
- No actual VR SDK integration

### Required Wiring

| Task | Details |
|------|---------|
| **Scope decision** | Is VR in scope for initial launch? |
| **If yes** | Integrate VR SDK (OpenXR, WebXR, etc.) |
| **If no** | Hide commands behind `#[cfg(feature = "vr")]` or remove |
| **Docs** | Document VR roadmap |

### Acceptance Criteria
- [ ] VR commands either work or are hidden from production CLI
- [ ] No "placeholder" messages in production output

---

## 20. Miniapp Sandbox Launch is CLI Mock Only

| Attribute | Value |
|-----------|-------|
| **Priority** | 🟡 P2 (if miniapps in scope) |
| **Files** | `src/main.rs` (miniapp sandbox) |
| **Modes Affected** | `cli`, `light-client` |

### Current State
- Prints init message and "in a full client, this would open a WebView/iframe sandbox"
- No actual sandbox implementation

### Required Wiring

| Task | Details |
|------|---------|
| **Sandbox runtime** | Implement WebView/iframe host for desktop/mobile clients |
| **WASM execution** | Integrate wasmtime/wasmer for sandboxed execution |
| **Permission model** | Enforce declared permissions from manifest |
| **IPC protocol** | Define message passing between sandbox and host |
| **CLI mode** | For CLI, consider headless WASM execution with stdio IPC |

### Acceptance Criteria
- [ ] Miniapps run in isolated sandbox
- [ ] Permissions enforced per manifest
- [ ] CLI can run miniapps headlessly (if applicable)

---

## Summary by Priority

| Priority | Count | Items |
|----------|-------|-------|
| 🔴 P0 | 4 | #1, #2, #3, #14 |
| 🟠 P1 | 8 | #5, #7, #8, #11, #12, #13, #18, #9 |
| 🟡 P2 | 6 | #4, #6, #10, #16, #17, #20 |
| 🟢 P3 | 2 | #15, #19 |

---

## Pre-Launch Checklist

### Environment Variables (Production Required)

| Variable | Purpose | Default |
|----------|---------|---------|
| `DCHAT_CURRENCY_RPC_URL` | Currency chain RPC endpoint | ❌ None (must set) |
| `DCHAT_CHAT_RPC_URL` | Chat chain RPC endpoint | ❌ None (must set) |
| `DCHAT_GENESIS_TIMESTAMP` | Genesis block timestamp | From config file |
| `DCHAT_ALLOW_SOFTWARE_KEYS` | Allow software keystore (dev only) | `false` |

### Config File Required Sections

```toml
[chains]
currency_rpc_url = "https://..."
chat_rpc_url = "https://..."
genesis_timestamp = 1735689600

[foundation_validators]
keys_dir = "/etc/dchat/genesis/validators/"

[keyless]
allow_software_keys = false
require_biometric = true

[fees]
gateway_persistence_path = "/var/lib/dchat/fee_gateway.state"
orchestrator_persistence_path = "/var/lib/dchat/fee_orchestrator.state"
```

### Build Flags

| Flag | Production Value | Purpose |
|------|------------------|---------|
| `--release` | ✅ Required | Optimized build |
| `--features test-bypass` | ❌ Forbidden | Fee bypass |
| `--features mock-biometric` | ❌ Forbidden | Biometric simulation |
| `--features hardware-keys` | ✅ Required (when available) | Real hardware keystore |

### Services Required

| Service | Purpose | Health Check |
|---------|---------|--------------|
| Currency Chain RPC | Token operations, fees | `GET /health` |
| Chat Chain RPC | Message operations | `GET /health` |
| FeeOrchestrator | Escrow management | Internal |
| Bootstrap Peers | P2P network | Peer count > 0 |

---

## Next Steps

1. **P0 items first**: Hardware keys, biometrics, foundation validator keys, fee bypass removal
2. **P1 stability**: Lock handling, panic removal, config validation
3. **P2 features**: Miniapp registry, program inspection, offline queue semantics
4. **P3 polish**: JSON unwrap, VR placeholder removal

---

*This checklist should be reviewed with the team and updated as items are resolved.*
