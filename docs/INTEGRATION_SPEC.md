# DChat Integration Specification

> **Version**: 1.0.0  
> **Date**: 2026-01-06  
> **Status**: Authoritative Reference for Mainnet Wiring  
> **Source of Truth**: [src/future.md](../src/future.md)

---

## Table of Contents

1. [Overview](#overview)
2. [Canonical Message Lifecycle](#canonical-message-lifecycle)
3. [Node Roles & Manager Ownership](#node-roles--manager-ownership)
4. [Configuration vs Environment Variables](#configuration-vs-environment-variables)
5. [Feature Wiring Map](#feature-wiring-map)
6. [Implementation Phases](#implementation-phases)
7. [Verification Checkpoints](#verification-checkpoints)

---

## Overview

This document defines the end-to-end wiring requirements for bringing DChat from its current state (primitives implemented, not all wired) to production mainnet. It serves as the canonical reference for:

- Message flow from composition to delivery
- Which node roles own which managers
- What is configured via TOML vs environment variables
- The exact integration seams for each unwired feature

### Current State Summary

| Category | Wired | Partial | Unwired |
|----------|-------|---------|---------|
| Consensus | Fee System, PoRW | VRF Committees | PoT, TSC, Two-Stage Finality |
| Privacy | - | - | Onion Routing, E2E Encryption |
| Economics | Fee Distribution | Payment Channels | Message Credits |
| Platform | - | Bots | Mini-Apps, Marketplace |
| Infrastructure | Staking, Relay Eligibility | Oracle | Watchtower, Solana Bridge, Storage Providers |

---

## Canonical Message Lifecycle

```
┌──────────────────────────────────────────────────────────────────────────────┐
│                        CANONICAL MESSAGE LIFECYCLE                            │
└──────────────────────────────────────────────────────────────────────────────┘

  ┌─────────┐      ┌─────────────┐      ┌─────────────┐      ┌─────────────┐
  │ COMPOSE │ ──▶  │ FEE/CREDITS │ ──▶  │   ENCRYPT   │ ──▶  │    ROUTE    │
  └─────────┘      └─────────────┘      └─────────────┘      └─────────────┘
       │                  │                   │                    │
       ▼                  ▼                   ▼                    ▼
  UserManager      FeeOrchestrator     DoubleRatchet      OnionRoutingManager
  LightClient      PaymentChannel      QgeSession         SphinxPacket
                   MessageCredits      X3DH KeyExchange   CircuitBuilder

  ┌─────────────┐      ┌─────────────┐      ┌─────────────┐
  │   PERSIST   │ ──▶  │   DELIVER   │ ──▶  │   EVENTS    │
  └─────────────┘      └─────────────┘      └─────────────┘
       │                    │                    │
       ▼                    ▼                    ▼
  StorageFacade       NetworkManager       EventBus
  ProviderRegistry    RelayNetwork         Observability
  At-Rest Encrypt     Proof-of-Delivery    Metrics/Tracing
```

### Stage 1: Compose
- **Owner**: Client Node (`UserManager`, `LightClient`)
- **Input**: User plaintext, recipient ID, channel ID
- **Output**: `MessageEnvelope` with metadata

### Stage 2: Fee/Credits (Optional Off-Chain)
- **Owner**: Client Node + Relay Node
- **Components**:
  - `FeeOrchestrator` - idempotent fee charging
  - `MessageCreditsChannel` - off-chain micropayment (NOT WIRED)
  - `PaymentChannelManager` - channel lifecycle (NOT WIRED)
- **Flow**:
  1. Check if payment channel exists with relay
  2. If yes: deduct from channel balance (off-chain)
  3. If no: fall back to on-chain `FeeOrchestrator.charge()`
- **Config**: `[fees].enable_payment_channels = true`

### Stage 3: Encrypt
- **Owner**: Client Node
- **Components**:
  - `DoubleRatchetSession` - Signal protocol encryption (NOT WIRED)
  - `QgeSession` - Quorum-Gated Encryption envelope (NOT WIRED)
  - `X3DH` - Key agreement for new sessions (NOT WIRED)
- **Flow**:
  1. Retrieve or establish ratchet session with recipient
  2. Encrypt plaintext with message key
  3. Wrap in QGE envelope requiring epoch token for at-rest decrypt
- **Config**: `[crypto].enable_e2e = true` (default: true)

### Stage 4: Route
- **Owner**: Relay Node
- **Components**:
  - `OnionRoutingManager` - circuit management (NOT WIRED)
  - `SphinxPacket` - layered encryption (NOT WIRED)
  - `CircuitBuilder` - path selection (NOT WIRED)
- **Flow**:
  1. Client builds 3-5 hop circuit through relay quorum
  2. Each hop peels one encryption layer
  3. Final hop delivers to recipient or stores for offline
- **Runtime Switch**: `[network].routing_mode = "direct" | "onion"`
- **Default**: `"direct"` for rollout safety

### Stage 5: Persist
- **Owner**: Relay Node / Validator Node
- **Components**:
  - `StorageFacade` - unified storage API (NOT WIRED)
  - `ProviderRegistry` - decentralized storage providers (NOT WIRED)
  - `StorageRouter` - tiered storage selection (NOT WIRED)
- **Flow**:
  1. Messages < 64KB: inline in SQLite/Postgres
  2. Messages >= 64KB: blob storage (S3/IPFS)
  3. All storage encrypted at-rest with AES-256-GCM
- **Config**: `[storage].provider = "local" | "s3" | "ipfs"`

### Stage 6: Deliver
- **Owner**: Relay Node
- **Components**:
  - `NetworkManager` - libp2p delivery
  - `RelayNetworkManager` - relay selection
  - Proof-of-Delivery - incentive recording
- **Flow**:
  1. Route to recipient if online
  2. Store-and-forward if offline
  3. Record delivery proof for relay rewards

### Stage 7: Events
- **Owner**: All Nodes
- **Components**:
  - `EventBus` - internal pub/sub (TO BE DEFINED)
  - `Observability` - metrics/tracing
  - `WebhookDispatcher` - external notifications
- **Events**:
  - `MessageSent`, `MessageReceived`, `MessageFailed`
  - `ChannelJoined`, `ChannelLeft`
  - `PaymentReceived`, `PaymentFailed`

---

## Node Roles & Manager Ownership

### Node Types

| Node Type | Binary Entry | Primary Function |
|-----------|--------------|------------------|
| **Validator** | `dchat validator` | Consensus, block production, chain state |
| **Relay** | `dchat relay` | Message routing, privacy infrastructure, P2P relay |
| **Client** | `dchat user` | End-user messaging, light client operations |
| **Oracle** | (Future) | External data feeds, price aggregation |
| **Bot** | (Future) | Automated messaging, webhook handling |

### Manager Ownership Matrix

| Manager | Validator | Relay | Client | Notes |
|---------|-----------|-------|--------|-------|
| **HardenedPoRW** | ✅ Primary | ❌ | ❌ | Wired at main.rs:7869 |
| **HardenedPoT** | ✅ | ❌ | ❌ | NOT WIRED |
| **HardenedTSC** | ✅ | ❌ | ❌ | NOT WIRED |
| **VrfCommitteeSelector** | ✅ | ❌ | ❌ | NOT WIRED |
| **TwoStageFinalityManager** | ✅ | ❌ | ❌ | NOT WIRED |
| **AdmissionControl** | ✅ | ✅ | ❌ | NOT WIRED |
| **ChallengeResponseManager** | ✅ | ❌ | ❌ | NOT WIRED |
| **FeeOrchestrator** | ✅ | ✅ | ✅ | ✅ WIRED |
| **FeeDistributionManager** | ✅ | ❌ | ❌ | ✅ WIRED |
| **RelayEligibilityChecker** | ✅ | ❌ | ❌ | ✅ WIRED |
| **PaymentChannelManager** | ✅ | ✅ | ✅ | NOT WIRED |
| **MessageCreditsChannel** | ❌ | ✅ | ✅ | NOT WIRED |
| **OnionRoutingManager** | ❌ | ✅ Primary | ❌ | NOT WIRED |
| **DoubleRatchetSession** | ❌ | ❌ | ✅ Primary | NOT WIRED |
| **QgeSessionManager** | ❌ | ✅ | ✅ | NOT WIRED |
| **StorageFacade** | ✅ | ✅ | ✅ | NOT WIRED |
| **ProviderRegistry** | ✅ | ✅ | ❌ | NOT WIRED |
| **WatchtowerMonitor** | ✅ Primary | ❌ | ❌ | ✅ WIRED (Phase 3) |
| **OracleNetwork** | ✅ | ❌ | ❌ | ✅ WIRED (Phase 3) |
| **SolanaBridgeManager** | ✅ | ❌ | ❌ | Partial (needs RPC) |
| **BotFather** | ❌ | ✅ Primary | ❌ | ✅ Wired to CLI |
| **MiniAppRegistry** | ❌ | ✅ | ✅ | ✅ WIRED (Phase 4) |
| **MarketplaceManager** | ❌ | ✅ | ✅ | ✅ WIRED (Phase 4) |
| **TtsEngine** | ❌ | ❌ | ✅ | NOT WIRED |
| **AccessibilityManager** | ❌ | ❌ | ✅ | Partial |

---

## Configuration vs Environment Variables

### Hierarchy (Highest to Lowest Priority)

1. **Environment Variables** - Runtime overrides, secrets
2. **CLI Arguments** - Command-line flags
3. **Config File** - `config.toml` settings
4. **Defaults** - Hardcoded safe defaults

### Environment Variables (Secrets/Runtime)

| Variable | Purpose | Required For |
|----------|---------|--------------|
| `DCHAT_SENTRY_DSN` | Error monitoring | Production observability |
| `DCHAT_CURRENCY_CHAIN_RPC` | Currency chain endpoint | All nodes |
| `DCHAT_CHAT_CHAIN_RPC` | Chat chain endpoint | All nodes |
| `DCHAT_SOLANA_RPC` | Solana bridge endpoint | Bridge operations |
| `DCHAT_SLACK_WEBHOOK_URL` | Alert notifications | Production alerts |
| `DCHAT_PAGERDUTY_KEY` | PagerDuty integration | Critical alerts |
| `DCHAT_LIGHT_CLIENT_IDENTITY_PASSPHRASE` | Identity encryption | Light clients |
| `DCHAT_RELAY_KEYSTORE_PATH` | Relay identity path | Relay nodes |
| `AWS_REGION` | KMS region | HSM validators |
| `AWS_ACCESS_KEY_ID` | KMS authentication | HSM validators |
| `AWS_SECRET_ACCESS_KEY` | KMS authentication | HSM validators |
| `DCHAT_ALLOW_LOCALHOST_CHAIN_RPC_DEFAULTS` | Dev mode flag | **NEVER in production** |

### Config File Settings (Non-Secrets)

```toml
# Feature Flags (Phase 0)
[features]
enable_onion_routing = false      # Phase 2 - runtime switch
enable_payment_channels = false   # Phase 2 - economic rollout
enable_e2e_encryption = true      # Phase 1 - always on
enable_watchtower = false         # Phase 3 - validator only
enable_oracle = false             # Phase 3 - validator only
enable_bots = false               # Phase 4 - relay only
enable_miniapps = false           # Phase 4 - client only
enable_marketplace = false        # Phase 4 - all nodes
enable_vrf_committees = false     # Phase 5 - consensus hardening
enable_two_stage_finality = false # Phase 5 - consensus hardening

# Network Routing
[network]
routing_mode = "direct"           # "direct" | "onion"
min_circuit_hops = 3
max_circuit_hops = 5
circuit_rotation_secs = 600

# Storage
[storage]
provider = "local"                # "local" | "s3" | "ipfs"
inline_threshold_bytes = 65536    # 64KB
max_blob_size_bytes = 104857600   # 100MB
enable_at_rest_encryption = true

# Payment Channels
[payment_channels]
min_channel_capacity = 10000000000    # 10 DCHAT
max_channel_capacity = 100000000000000 # 100,000 DCHAT
dispute_window_secs = 172800          # 48 hours
auto_close_threshold = 0.1            # 10% remaining

# Oracle
[oracle]
min_stake = 1000000000000   # 1000 DCHAT
update_interval_secs = 60
max_outlier_deviation = 0.5
```

---

## Feature Wiring Map

### Legend

- **Module**: Rust module path
- **Entrypoint**: Where it should be instantiated
- **Integration Seam**: How to wire it
- **Config Knobs**: Feature flags and settings

---

### 1. Onion Routing

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_network::onion_routing::OnionRoutingManager` |
| **Symbol** | `dchat_network::network::onion::sphinx::SphinxPacket` |
| **Entrypoint** | `run_relay_node()` at main.rs:3913 |
| **Integration Seam** | After `NetworkManager::start()`, before event loop |
| **Owner** | Relay Node (primary), Validator Node (optional) |
| **Config Knobs** | `[network].routing_mode`, `min_circuit_hops`, `max_circuit_hops` |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In run_relay_node(), after network.start():
if config.features.enable_onion_routing {
    let onion_config = CircuitConfig {
        min_hops: config.network.min_circuit_hops,
        max_hops: config.network.max_circuit_hops,
        rotation_interval: Duration::from_secs(config.network.circuit_rotation_secs),
    };
    let onion_manager = OnionRoutingManager::new(onion_config);
    // Register Sphinx packet handler in network dispatch
    network.register_protocol_handler("/dchat/onion/1.0.0", onion_manager.handler());
}
```

---

### 2. E2E Encryption (Double Ratchet + QGE)

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_crypto::ratchet::DoubleRatchetSession` |
| **Module** | `dchat_crypto::qge::QgeSession` |
| **Module** | `dchat_crypto::x3dh::X3dhKeyAgreement` |
| **Entrypoint** | `run_user_node()`, `run_light_client()` |
| **Integration Seam** | Message send/receive path in `UserManager` |
| **Owner** | Client Node (primary) |
| **Config Knobs** | `[crypto].enable_e2e` (default: true) |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In UserManager::send_message():
let encrypted = if config.crypto.enable_e2e {
    let session = self.get_or_create_ratchet_session(recipient_id).await?;
    let ciphertext = session.encrypt(plaintext)?;
    QgeEnvelope::wrap(ciphertext, self.current_epoch_token()?)?
} else {
    plaintext.to_vec()
};
```

---

### 3. Payment Channels

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::payment_channels::PaymentChannelManager` |
| **Module** | `dchat_messaging::message_service::MessageCreditsChannel` |
| **Entrypoint** | All node types during init |
| **Integration Seam** | `FeeGateway.charge_for_message()` |
| **Owner** | All nodes |
| **Config Knobs** | `[payment_channels].*` |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In FeeGateway::charge_for_message():
if self.config.enable_payment_channels {
    if let Some(channel) = self.get_channel_with_relay(relay_id).await? {
        if channel.balance() >= fee_amount {
            channel.pay(fee_amount)?;
            return Ok(PaymentResult::OffChain);
        }
    }
}
// Fall back to on-chain
self.fee_orchestrator.charge(sender, fee_amount, operation_id).await
```

---

### 4. Storage Facade

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_storage::provider::facade::StorageFacade` |
| **Module** | `dchat_storage::provider::ProviderRegistry` |
| **Module** | `dchat_storage::provider::router::StorageRouter` |
| **Entrypoint** | All node types during init |
| **Integration Seam** | Replace direct DB calls for message attachments |
| **Owner** | All nodes |
| **Config Knobs** | `[storage].*` |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In node initialization:
let storage_config = StorageFacadeConfig {
    inline_threshold: config.storage.inline_threshold_bytes,
    max_blob_size: config.storage.max_blob_size_bytes,
    encryption_enabled: config.storage.enable_at_rest_encryption,
    provider: match config.storage.provider.as_str() {
        "s3" => StorageProvider::S3(s3_config),
        "ipfs" => StorageProvider::Ipfs(ipfs_config),
        _ => StorageProvider::Local(local_config),
    },
};
let storage_facade = StorageFacade::new(pg_pool, storage_config)?;
```

---

### 5. Watchtower

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::watchtower::WatchtowerMonitor` |
| **Entrypoint** | `run_validator_node()` |
| **Integration Seam** | Background task spawn after chain clients init |
| **Owner** | Validator Node only |
| **Config Knobs** | `[features].enable_watchtower` |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In run_validator_node(), after currency_chain init:
if config.features.enable_watchtower {
    let watchtower = WatchtowerMonitor::new(
        Arc::clone(&currency_chain),
        Duration::from_secs(30), // poll interval
    );
    let watchtower_handle = tokio::spawn(async move {
        watchtower.start().await;
    });
}
```

---

### 6. Oracle Network

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::oracle_network::OracleNetwork` |
| **Entrypoint** | `run_validator_node()` |
| **Integration Seam** | Init after staking verified, expose CLI for participation |
| **Owner** | Validator Node |
| **Config Knobs** | `[oracle].*` |
| **Status** | NOT WIRED |

**Wiring Task**:
```rust
// In run_validator_node():
if config.features.enable_oracle {
    let oracle_network = OracleNetwork::new(
        config.oracle.min_stake,
        config.oracle.update_interval_secs,
    );
    oracle_network.register_validator(validator_id, stake_amount).await?;
    // Wire to marketplace pricing
    marketplace_manager.set_oracle_source(oracle_network.clone());
}
```

---

### 7. Solana Bridge

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::solana::client::SolanaClient` |
| **Module** | `dchat_bridge::SolanaBridgeManager` |
| **Entrypoint** | `run_validator_node()` |
| **Integration Seam** | Init when `DCHAT_SOLANA_RPC` is set |
| **Owner** | Validator Node |
| **Config Knobs** | `DCHAT_SOLANA_RPC` env var |
| **Status** | Partial (RPC not resolved) |

**Wiring Task**:
```rust
// In run_validator_node(), after bridge init:
if let Ok(solana_rpc) = std::env::var("DCHAT_SOLANA_RPC") {
    let solana_client = SolanaClient::new(&solana_rpc)?;
    bridge.set_solana_client(solana_client);
    info!("✓ Solana bridge connected to {}", solana_rpc);
}
```

---

### 8. Bot Platform

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_bots::bot_father::BotFather` |
| **Module** | `dchat_bots::webhooks::WebhookDispatcher` |
| **Entrypoint** | `run_relay_node()` |
| **Integration Seam** | Init after network, expose CLI commands |
| **Owner** | Relay Node (primary) |
| **Config Knobs** | `[features].enable_bots` |
| **Status** | Test functions only |

**Wiring Task**:
```rust
// In run_relay_node():
if config.features.enable_bots {
    let bot_father = BotFather::new();
    // Register message handler for bot commands
    network.register_message_handler("/dchat/bot/1.0.0", bot_father.handler());
    // CLI commands: dchat bot create/list/delete/webhook
}
```

---

### 9. Mini-Apps

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_miniapps::registry::MiniAppRegistry` |
| **Module** | `dchat_miniapps::sandbox::SandboxedRuntime` |
| **Entrypoint** | `run_user_node()`, `run_relay_node()` |
| **Integration Seam** | Init registry, expose CLI/API |
| **Owner** | Client + Relay Nodes |
| **Config Knobs** | `[features].enable_miniapps` |
| **Status** | NOT WIRED |

---

### 10. Marketplace

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_marketplace::MarketplaceManager` |
| **Module** | `dchat_marketplace::escrow::EscrowContract` |
| **Entrypoint** | All node types |
| **Integration Seam** | Wire to payment channels, oracle, storage |
| **Owner** | All nodes |
| **Config Knobs** | `[features].enable_marketplace` |
| **Status** | Demo only |

---

### 11. VRF Committees (Hardened Consensus)

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::hardened_consensus::vrf_committees::CommitteeSelector` |
| **Entrypoint** | `run_validator_node()` |
| **Integration Seam** | Init in HardenedConsensusCoordinator |
| **Owner** | Validator Node |
| **Config Knobs** | `[features].enable_vrf_committees` |
| **Status** | NOT WIRED |

---

### 12. Two-Stage Finality

| Aspect | Detail |
|--------|--------|
| **Module** | `dchat_blockchain::hardened_consensus::two_stage_finality::TwoStageFinalityManager` |
| **Entrypoint** | `run_validator_node()` |
| **Integration Seam** | Init in HardenedConsensusCoordinator |
| **Owner** | Validator Node |
| **Config Knobs** | `[features].enable_two_stage_finality` |
| **Status** | NOT WIRED |

---

## Implementation Phases

### Phase 0: Contracts & Interfaces
- [ ] Add `[features]` section to config schema
- [ ] Define `EventBus` trait for internal pub/sub
- [ ] Define `MessageEnvelope` versioning (v1 format)
- [ ] Ensure safe defaults for all feature flags (disabled)
- [ ] Add feature flag plumbing to `Config` struct

### Phase 1: Storage & Encryption
- [ ] Wire `StorageFacade` into message persistence
- [ ] Integrate `DoubleRatchetSession` into send/receive
- [ ] Integrate `QgeSession` for at-rest encryption
- [ ] Add at-rest encryption to blob storage

### Phase 2: Onion Routing & Payment Channels
- [ ] Instantiate `OnionRoutingManager` in relay startup
- [ ] Register Sphinx packet handler in network dispatch
- [ ] Add `routing_mode` runtime switch
- [ ] Integrate `MessageCreditsChannel` into message send
- [ ] Implement channel open/fund/charge/dispute flow
- [ ] Add fallback to on-chain settlement

### Phase 3: Watchtower, Oracle, Solana Bridge
- [ ] Spawn `WatchtowerMonitor` in validator mode
- [ ] Wire `OracleNetwork` with CLI participation path
- [ ] Add `DCHAT_SOLANA_RPC` resolution at startup
- [ ] Connect Solana client to bridge

### Phase 4: Ecosystems (Bots, Mini-Apps, Marketplace)
- [x] Wire `BotFather` to relay network (already wired to CLI)
- [x] Add bot CLI commands (create/manage/rotate) (already implemented)
- [x] Wire `MiniAppRegistry` with lifecycle (wired to run_user_node with feature flag)
- [ ] Add sandbox enforcement and wallet intents
- [x] Wire `MarketplaceManager` with escrow/oracle/storage (wired to run_validator_node with feature flag)
- [ ] Add creator flow CLI commands

### Phase 5: Hardened Consensus
- [x] Verify current coordinator construction (HardenedPoRW initialized)
- [ ] Instantiate VRF committee selection (needs relay data conversion)
- [ ] Enable two-stage finality (built into HardenedPoRW)
- [ ] Wire admission control
- [ ] Wire challenge-response system

---

## Verification Checkpoints

### Per-Phase Criteria

Each phase must satisfy:

1. **Boot-time wiring works** - Node starts without errors with feature enabled
2. **Config defaults are safe** - Feature disabled by default, no breaking changes
3. **Observability exists** - Metrics/logging for new components
4. **Failure modes bounded** - Errors don't crash node, graceful degradation
5. **Rollout gated** - Feature flag controls activation
6. **Tests pass** - Unit tests near module, integration tests under `tests/`

### Test Locations

| Type | Location | Purpose |
|------|----------|---------|
| Unit | `src/*.rs`, `crates/*/src/*.rs` | Per-module logic |
| Integration | `tests/*.rs` | Cross-component flows |
| E2E | `examples/`, `scripts/` | Realistic scenarios |

### Done Criteria per Feature

- [ ] Feature can be enabled via config without code changes
- [ ] Startup logs show feature initialization
- [ ] Metrics exposed under `/metrics`
- [ ] Graceful shutdown handles feature components
- [ ] At least one integration test exercises the feature
- [ ] Documentation updated in this spec

---

## Appendix: Quick Reference

### Startup Entry Points

| Node Type | Function | Line |
|-----------|----------|------|
| Relay | `run_relay_node()` | main.rs:3913 |
| User | `run_user_node()` | main.rs:5349 |
| Validator | `run_validator_node()` | main.rs:6925 |
| Light Client | `run_light_client()` | main.rs:4943 |

### Key Imports Needed

```rust
// Onion Routing
use dchat_network::onion_routing::{OnionRoutingManager, CircuitConfig};
use dchat_network::network::onion::sphinx::SphinxPacket;

// E2E Encryption
use dchat_crypto::ratchet::DoubleRatchetSession;
use dchat_crypto::qge::QgeSession;
use dchat_crypto::x3dh::X3dhKeyAgreement;

// Payment Channels
use dchat_blockchain::payment_channels::PaymentChannelManager;
use dchat_messaging::message_service::MessageCreditsChannel;

// Storage
use dchat_storage::provider::facade::{StorageFacade, StorageFacadeConfig};
use dchat_storage::provider::ProviderRegistry;

// Watchtower
use dchat_blockchain::watchtower::WatchtowerMonitor;

// Oracle
use dchat_blockchain::oracle_network::OracleNetwork;

// Solana Bridge
use dchat_blockchain::solana::client::SolanaClient;

// Bots
use dchat_bots::bot_father::BotFather;

// Mini-Apps
use dchat_miniapps::registry::MiniAppRegistry;

// Marketplace
use dchat_marketplace::MarketplaceManager;

// Hardened Consensus
use dchat_blockchain::hardened_consensus::vrf_committees::CommitteeSelector;
use dchat_blockchain::hardened_consensus::two_stage_finality::TwoStageFinalityManager;
use dchat_blockchain::hardened_consensus::admission_control::AdmissionControl;
use dchat_blockchain::hardened_consensus::challenge_response::ChallengeResponseManager;
```

---

*This document is the authoritative reference for DChat mainnet wiring. Update it as features are integrated.*
