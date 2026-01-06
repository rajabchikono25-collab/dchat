# DChat Wiring Map

> **Last Updated**: 2026-01-06  
> **Purpose**: Track integration status of each unwired feature  
> **Authoritative Source**: [src/future.md](../src/future.md)

---

## Quick Status Dashboard

| Phase | Feature | Status | Priority |
|-------|---------|--------|----------|
| 0 | Feature Flags Config | ✅ DONE | Blocker |
| 0 | EventBus Trait | 🔴 TODO | High |
| 0 | MessageEnvelope Versioning | 🟡 Exists | Low |
| 1 | StorageFacade Wiring | 🔴 TODO | High |
| 1 | E2E Encryption (Ratchet) | 🔴 TODO | Critical |
| 2 | Onion Routing | 🔴 TODO | High |
| 2 | Payment Channels | 🔴 TODO | High |
| 3 | Watchtower | 🔴 TODO | Medium |
| 3 | Oracle Network | 🔴 TODO | Medium |
| 3 | Solana Bridge RPC | 🟡 Partial | Medium |
| 4 | Bot Platform | 🟡 Test Only | Medium |
| 4 | Mini-Apps | 🔴 TODO | Medium |
| 4 | Marketplace | 🔴 TODO | Medium |
| 5 | VRF Committees | 🔴 TODO | Medium |
| 5 | Two-Stage Finality | 🔴 TODO | Medium |
| 5 | Admission Control | 🔴 TODO | Low |
| 5 | Challenge-Response | 🔴 TODO | Low |

---

## Detailed Feature Map

### Phase 0: Contracts & Interfaces

#### 0.1 Feature Flags Config

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT IMPLEMENTED |
| **Location** | `crates/dchat-core/src/config/mod.rs` |
| **Task** | Add `FeaturesConfig` struct with all feature toggles |
| **Owner Module** | dchat-core |
| **Integration Seam** | Add to `Config` struct, wire to all node startup paths |
| **Config Knobs** | All features disabled by default |

**Implementation**:
```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct FeaturesConfig {
    pub enable_onion_routing: bool,
    pub enable_payment_channels: bool,
    pub enable_e2e_encryption: bool,
    pub enable_watchtower: bool,
    pub enable_oracle: bool,
    pub enable_bots: bool,
    pub enable_miniapps: bool,
    pub enable_marketplace: bool,
    pub enable_vrf_committees: bool,
    pub enable_two_stage_finality: bool,
    pub enable_storage_providers: bool,
}
```

---

### Phase 1: Storage & Encryption

#### 1.1 StorageFacade Wiring

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_storage::provider::facade::StorageFacade` |
| **File** | `crates/dchat-storage/src/provider/facade.rs:249` |
| **Constructor** | `StorageFacade::new(config)` |
| **Entrypoints** | All node types during init |
| **Integration Seam** | Replace direct DB calls for message attachments |
| **Dependencies** | `PgPool`, `StorageFacadeConfig` |
| **Config Knobs** | `[storage].provider`, `inline_threshold_bytes`, `enable_at_rest_encryption` |

**Wiring Location in main.rs**:
- `run_relay_node()` at line ~4300 (after database init)
- `run_validator_node()` at line ~7500 (after database init)
- `run_light_client()` at line ~5100 (after database init)

---

#### 1.2 E2E Encryption (Double Ratchet)

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_crypto::ratchet` (module exists) |
| **Key Struct** | Needs `DoubleRatchetSession` or similar |
| **Entrypoints** | Client nodes (UserManager, LightClient) |
| **Integration Seam** | Message send/receive path |
| **Dependencies** | X3DH key agreement, QGE session manager |
| **Config Knobs** | `[features].enable_e2e_encryption` (default: true) |

**Note**: The `dchat_crypto::ratchet` module structure exists but needs verification of the exact API.

---

### Phase 2: Onion Routing & Payment Channels

#### 2.1 Onion Routing

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_network::onion_routing::OnionRoutingManager` |
| **File** | `crates/dchat-network/src/onion_routing.rs:376` |
| **Constructor** | `OnionRoutingManager::new(CircuitConfig)` at line 389 |
| **Entrypoints** | `run_relay_node()` after NetworkManager::start() |
| **Integration Seam** | Network dispatch layer, Sphinx packet handling |
| **Dependencies** | `CircuitConfig`, relay peer list |
| **Config Knobs** | `[network].routing_mode`, `min_circuit_hops`, `max_circuit_hops` |

**Wiring Location**: main.rs ~4450 (after network.start() in relay node)

---

#### 2.2 Payment Channels

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module (Manager)** | `dchat_blockchain::payment_channels::PaymentChannelManager` |
| **File** | `crates/dchat-blockchain/src/payment_channels.rs:438` |
| **Module (Credits)** | `dchat_messaging::message_service::MessageCreditsChannel` |
| **File** | `crates/dchat-messaging/src/message_service.rs:478` |
| **Entrypoints** | All nodes (FeeGateway integration) |
| **Integration Seam** | `FeeGateway.charge_for_message()` |
| **Dependencies** | CurrencyChainClient, relay network |
| **Config Knobs** | `[payment_channels].*` |

**Wiring Location**: FeeGateway initialization in all node types

---

### Phase 3: Watchtower, Oracle, Solana

#### 3.1 Watchtower

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_blockchain::watchtower::WatchtowerMonitor` |
| **File** | `crates/dchat-blockchain/src/watchtower.rs:636` |
| **Constructor** | `WatchtowerMonitor::new(currency_client, poll_interval)` |
| **Entrypoints** | `run_validator_node()` only |
| **Integration Seam** | Background task spawn after chain clients init |
| **Dependencies** | `CurrencyChainClient` |
| **Config Knobs** | `[features].enable_watchtower` |

**Wiring Location**: main.rs ~7800 (validator event loop setup)

---

#### 3.2 Oracle Network

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_blockchain::oracle_network::OracleNetwork` |
| **File** | `crates/dchat-blockchain/src/oracle_network.rs:168` |
| **Constructor** | `OracleNetwork::new(min_stake, update_interval)` |
| **Entrypoints** | `run_validator_node()` |
| **Integration Seam** | Init after staking verified, connect to marketplace |
| **Dependencies** | Staking verification |
| **Config Knobs** | `[oracle].min_stake`, `update_interval_secs` |

**Wiring Location**: main.rs ~7850 (after staking manager init)

---

#### 3.3 Solana Bridge

| Aspect | Value |
|--------|-------|
| **Status** | 🟡 PARTIAL (RPC not resolved) |
| **Module** | `dchat_blockchain::solana::client::SolanaClient` |
| **File** | `crates/dchat-blockchain/src/solana/client.rs:20` |
| **Module** | `dchat_bridge::solana_bridge::SolanaBridgeManager` |
| **File** | `crates/dchat-bridge/src/solana_bridge.rs:159` |
| **Entrypoints** | `run_validator_node()` |
| **Integration Seam** | Init when `DCHAT_SOLANA_RPC` env var is set |
| **Dependencies** | Solana RPC URL, validator keys |
| **Config Knobs** | `DCHAT_SOLANA_RPC` env var |

**Wiring Location**: main.rs ~7600 (bridge initialization section)

---

### Phase 4: Ecosystems

#### 4.1 Bot Platform

| Aspect | Value |
|--------|-------|
| **Status** | 🟡 TEST ONLY |
| **Module** | `dchat_bots::bot_manager::BotFather` |
| **File** | `crates/dchat-bots/src/bot_manager.rs:98` |
| **Constructor** | `BotFather::new()` |
| **Entrypoints** | `run_relay_node()` |
| **Integration Seam** | Network message handler registration |
| **Dependencies** | NetworkManager, webhook system |
| **Config Knobs** | `[features].enable_bots` |

**Wiring Location**: main.rs ~4500 (relay network setup)

---

#### 4.2 Mini-Apps

| Aspect | Value |
|--------|-------|
| **Status** | � WIRED (Phase 4) |
| **Module** | `dchat_miniapps::registry::MiniAppRegistry` |
| **File** | `crates/dchat-miniapps/src/registry.rs:378` |
| **Constructor** | `MiniAppRegistry::new(...)` |
| **Entrypoints** | Client nodes, relay nodes |
| **Integration Seam** | CLI commands, lifecycle management |
| **Dependencies** | Storage, wallet integration |
| **Config Knobs** | `[features].enable_miniapps` |

---

#### 4.3 Marketplace

| Aspect | Value |
|--------|-------|
| **Status** | � WIRED (Phase 4) |
| **Module** | `dchat_marketplace::MarketplaceManager` |
| **File** | `crates/dchat-marketplace/src/lib.rs:280` |
| **Constructor** | `MarketplaceManager::new(...)` |
| **Entrypoints** | All nodes |
| **Integration Seam** | Payment channels, oracle, storage |
| **Dependencies** | Oracle for pricing, escrow system |
| **Config Knobs** | `[features].enable_marketplace` |

---

### Phase 5: Hardened Consensus

#### 5.1 VRF Committees

| Aspect | Value |
|--------|-------|
| **Status** | 🟢 FULLY WIRED (epoch transition triggers initialization with relay data) |
| **Module** | `dchat_blockchain::hardened_consensus::vrf_committees::CommitteeSelector` |
| **File** | `crates/dchat-blockchain/src/hardened_consensus/vrf_committees.rs:491` |
| **Also** | `dchat_chain::committee_selection::CommitteeSelector` at line 366 |
| **Entrypoints** | `run_validator_node()` (HardenedConsensusCoordinator) |
| **Integration Seam** | Consensus coordinator initialization |
| **Dependencies** | VRF keys, relay registry |
| **Config Knobs** | `[features].enable_vrf_committees` |

---

#### 5.2 Two-Stage Finality

| Aspect | Value |
|--------|-------|
| **Status** | � WIRED (built into HardenedPoRW consensus engine) |
| **Module** | `dchat_blockchain::hardened_consensus::two_stage_finality::TwoStageFinality` |
| **File** | `crates/dchat-blockchain/src/hardened_consensus/two_stage_finality.rs:785` |
| **Entrypoints** | `run_validator_node()` |
| **Integration Seam** | Block finalization pipeline |
| **Dependencies** | Checkpoint manager |
| **Config Knobs** | `[features].enable_two_stage_finality` |

---

#### 5.3 Admission Control

| Aspect | Value |
|--------|-------|
| **Status** | 🔴 NOT WIRED |
| **Module** | `dchat_blockchain::hardened_consensus::admission_control::AdmissionController` |
| **File** | `crates/dchat-blockchain/src/hardened_consensus/admission_control.rs:640` |
| **Entrypoints** | Validator and relay nodes |
| **Integration Seam** | Network layer, message processing |
| **Dependencies** | Rate limiting, diversity tracking |
| **Config Knobs** | Admission control config |

---

## Test Verification Checklist

For each feature, verify:

- [ ] Unit tests exist in module file
- [ ] Integration test in `tests/` exercises feature
- [ ] Feature can be toggled via config
- [ ] Startup logs show feature status
- [ ] Metrics exposed for feature
- [ ] Graceful degradation on failure
- [ ] Documentation updated

---

## Implementation Order

1. **Phase 0** - Add feature flags (unblocks all other phases)
2. **Phase 1** - Storage + E2E (security foundation)
3. **Phase 2** - Onion + Payment (privacy + economics)
4. **Phase 3** - Watchtower + Oracle + Solana (infrastructure)
5. **Phase 4** - Bots + Mini-Apps + Marketplace (ecosystem)
6. **Phase 5** - Hardened Consensus (advanced security)

---

*Update this document as features are implemented.*
