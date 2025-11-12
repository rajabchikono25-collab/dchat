# dchat Mainnet Readiness: CLI Coverage, Hard-Coded Values, and Rebuild Plan

Date: 2025-11-12

This report inventories the current crates, fuzz targets, SDKs, and critical entry points, highlights mainnet risks (hard-coded values, disabled or placeholder logic), and proposes a concrete, low-risk rebuild plan to wire the CLI and `main.rs` to all functionality using configuration only (no hard-coded mainnet values).

## Scope Reviewed
- `crates/**` (workspace members from root `Cargo.toml`)
- `fuzz/fuzz_targets/*`
- `src/main.rs` (5103 LOC, many subcommand handlers inline)
- `src/user_management.rs`
- `sdk/{rust,typescript,python,dart}`

---

## Inventory Summary

### Workspace Crates (from `Cargo.toml`)
- Core: `dchat-core` (config, types, errors), `dchat-crypto`, `dchat-identity`, `dchat-network`, `dchat-messaging`, `dchat-storage`, `dchat-blockchain`, `dchat-chain`, `dchat-validator`
- Features: `dchat-privacy`, `dchat-governance`, `dchat-distribution`, `dchat-marketplace`, `dchat-observability`, `dchat-bridge`, `dchat-accessibility`, `dchat-bots`, `dchat-testing`, `dchat-data`, `dchat-deployment`, `dchat-sdk-rust`

These appear well-factored by concern; `src/lib.rs` re-exports them under `dchat::prelude::*` used by `main.rs`.

### SDKs
- Rust: `sdk/rust` with `src/{blockchain,crypto,user}`
- TypeScript: `sdk/typescript` with `src/{blockchain,client,config,relay,user}`
- Python: `sdk/python/dchat`
- Dart: `sdk/dart/lib`

Recommendation: keep SDKs versioned with the network protocol version; add a single canonical config schema shared with the node.

### Fuzz Targets
- `fuzz/fuzz_targets/*`: `identity_derivation.rs`, `keypair_generation.rs`, `message_parsing.rs`, `network_packet.rs`, `noise_handshake.rs`

Recommendation: run nightly in CI and before protocol bumps; capture seeds for regressions.

---

## Findings: `main.rs` Entry and CLI Coverage

`main.rs` already exposes a broad CLI surface via `clap`, with subcommands:
- `Relay`, `User`, `Validator`, `Testnet`, `Keygen`, `Account`, `Database`, `Health`, `Bot`, `Marketplace`, `Accessibility`, `Chaos`, `Governance`, `Token`, `Update`, `Deploy`.

Handlers exist inline for most commands (e.g., `run_account_command`, `run_bot_command`, etc.). Notable gaps and risks:

- Relay runtime is marked as temporarily disabled (crate migration comment). Network is brought up, but relay business logic (proof-of-delivery, reputation, forwarding loops) is not started.
- DNS discovery uses `DnsDiscoveryConfig::default()` with no domain provided by config; peers are assigned `PeerId::random()` placeholders when building bootstrap list (incorrect for production).
- STUN servers and public ports are hard-coded. Defaults are acceptable, but mainnet must source from config or env only.
- Health and metrics defaults differ from `config-production.toml` (defaults point to `:80` and `:9090` in code vs `:8080`/`:9090` in config). Enforce config-first.
- Testnet generation hard-codes base port `7070` series and local addresses; must be parameterized.
- Database backup/restore code paths contain TODOs (no WAL-consistent snapshot, no `backup_to_file` implementation).
- Marketplace list flow prints “needs to be implemented”; core listing APIs are missing.
- Several CLI flows construct chain clients with `*_Config::default()`; mainnet endpoints must be provided from config, not defaults.

---

## Findings: Hard-Coded or Implicit Mainnet Values

Observed in `main.rs` and configs:
- `const PUBLIC_TLS_PORT: u16 = 443;`
- `const PUBLIC_HTTP_PORT: u16 = 80;`
- STUN servers: `stun.l.google.com:19302`, `stun1.l.google.com:19302`
- Docker demo peer hostnames: `dchat-relay1`, `dchat-relay2`, `dchat-relay3`
- Listen addresses/defaults with `0.0.0.0` and local `127.0.0.1`

Risk: Any implicit default leaking into production violates “no hard-coded mainnet values”.

Required action: All network addresses, ports, discovery domains, bootstrap peers, and STUN/TURN lists must come from config (`config.toml` or env). Defaults may exist, but mainnet builds must require explicit values.

---

## Findings: `src/user_management.rs`

Strengths:
- Clean user lifecycle: keygen → on-chain registration → DB persistence.
- DM and channel post flows record ordering on chat chain and persist messages locally.

Risks / Gaps:
- Uses `ChatChainClient::new(ChatChainConfig::default())` in CLI handler (see `run_account_command`): must be injected from loaded `Config` to pick correct RPCs/chain IDs.
- Stores private key hex in the create response printed to console/file; mainnet UX should avoid printing raw private keys unless explicitly requested and gated.
- Database path for accounts is hard-coded to `./dchat_accounts.db`; use `config.storage.data_dir` consistently.

---

## Configuration: What Must Be Configurable (No Hard-Coded Mainnet)

Augment `dchat-core` config schema to ensure these fields exist and are required for mainnet mode:

- `network.listen_addresses: Vec<String>` (multiaddr preferred)
- `network.bootstrap_peers: Vec<String>` (multiaddr with `/p2p/<PeerId>`)
- `network.stun_servers: Vec<String>` and `network.turn_servers: Vec<String>`
- `network.enable_upnp: bool`, `network.enable_hole_punching: bool`
- `network.discovery.domain: String` (DNS discovery base), `network.discovery.min_peers`, `max_peers`, `k_bucket_size`, `alpha`, `query_timeout_ms`
- `network.default_channel: Option<String>` (avoid hard-coded "global")
- `observability.metrics_addr: String`, `observability.health_addr: String`
- `chains.chat.rpc_url: String`, `chains.chat.chain_id: String`
- `chains.currency.rpc_url: String`, `chains.currency.chain_id: String`
- `bridge.finality_threshold: u64`
- `storage.data_dir: PathBuf`, DB pool settings (already present)

Enforce: in "mainnet" mode (e.g., `--config` marked as mainnet), error if any of the above are missing.

---

## CLI-to-Crate Wiring Plan (Touch Everything)

Introduce a lightweight Service facade per concern in its crate, called by `main.rs`. Each facade should take the unified `Config` and avoid defaults.

- Relay: `dchat-network::relay::RelayService::start(config)`
  - Inputs: listen_addrs, bootstrap peers, DNS discovery domain, STUN/TURN, NAT.
  - Replace `PeerId::random()` with verified peer IDs from DNS records or explicit multiaddrs.
  - Start proof-of-delivery and reputation loops (`relay::proof`, `relay::reputation`).

- User: `dchat::client::DchatClientBuilder` already exists; ensure it accepts `Config` and bootstrap peers; remove hard-coded `#global` channel fallback.

- Validator: `dchat-validator::ValidatorService::start(config, key_source, rpc)`
  - Pull key from HSM/KMS or file based on config; do not accept unconfigured defaults.

- Testnet: Move compose/genesis generation to `dchat-deployment` and parameterize ports/domains entirely from flags/config (no 7070 assumptions).

- Keygen: `dchat-crypto` for key ops; ensure printing private keys is opt-in with `--show-private` or output to encrypted keystore by default.

- Account: Use `UserManager` but inject `Database` and chain clients from `Config`; database path from `config.storage.data_dir`.

- Database: Implement `Database::backup_to_file` and `restore_from_file` with WAL-consistent snapshots.

- Bot, Marketplace, Accessibility, Chaos, Governance, Token, Update, Deploy:
  - Each crate exposes `*Service` with explicit `start()/run()` or `execute(action, config)`.
  - Replace inline logic in `main.rs` with thin dispatch to these services.

Refactor pattern:
```rust
// in main.rs
match cli.command {
  Commands::Relay { .. } => dchat_network::relay::RelayService::from_config(&config).run().await,
  Commands::Account { action } => dchat::account::AccountService::new(&config).execute(action).await,
  // ... and so on for each domain
}
```

Benefits: Containment of domain logic in crates, consistent config injection, simpler `main.rs`, and easier testing.

---

## Immediate Remediations Before Launch (Low-Risk, Fast)

1) Config-only Networking
- Remove use of `PUBLIC_TLS_PORT`/`PUBLIC_HTTP_PORT` for relay/user defaults when `--config` is supplied.
- Read `stun_servers`, `turn_servers`, and discovery domain from config; if absent in mainnet mode, fail fast.

2) DNS Discovery Correctness
- Require DNS records to include `/p2p/<PeerId>`; reject entries without a PeerId (no `PeerId::random()` placeholders).

3) Default Channel Name
- Read from `config.network.default_channel` or require explicit `--channel` for `User` command; avoid the hard-coded "global".

4) Chain Client Construction
- Replace all `*_Config::default()` with config-derived values. Validator must use provided `chain_rpc`; user/account flows must use `config.chains.chat` and `config.chains.currency`.

5) Database Paths
- Unify to `config.storage.data_dir` for all flows (accounts, messages, backups). Remove ad-hoc `./dchat_accounts.db`.

6) Health/Metrics
- Bind addresses from `config` exclusively; do not open on `0.0.0.0` unless explicitly configured.

7) Marketplace List API
- Implement listing enumeration in `dchat-marketplace` (in-memory or DB-backed) so CLI `List` is functional.

---

## Cutover Plan (Phased, Minimal Risk)

Phase 0 (≤30 min)
- Enforce config-only paths for network, discovery, chains in `main.rs` (no defaults in mainnet mode).
- Wire `Account` and `Validator` flows to use `Config` objects for chain RPCs.
- Disable relay forwarding if not fully migrated, but keep network boot-up for peer/listen and health.

Phase 1 (Post-launch hotfix window)
- Land RelayService in `dchat-network` and switch `main.rs` to use it. Remove temporary log-only relay path.
- Implement `Database::backup_to_file` and marketplace list.

Phase 2
- Extract all inline `run_*` handlers to crate services and add integration tests per command.
- Add "mainnet profile" validator in startup that asserts required config keys are set.

---

## “No Hard-Coded Mainnet” Checklist

- [ ] No default ports/hosts used when `--config` provided.
- [ ] All peer addresses from config or DNS SRV/TXT with `/p2p/<PeerId>`.
- [ ] STUN/TURN/DNS discovery values from config only.
- [ ] Chain RPC URLs and IDs from config; no `_Config::default()` in production code paths.
- [ ] Health/metrics bind from config; `0.0.0.0` only by explicit choice.
- [ ] Default channel name not hard-coded.
- [ ] Database paths only from `config.storage`.
- [ ] Secrets never printed by default; gated with explicit flags.

---

## Suggested Config Snippet (TOML)

```toml
[network]
listen_addresses = ["/ip4/0.0.0.0/tcp/7070"]
bootstrap_peers = ["/dns4/relay1.example.org/tcp/7070/p2p/12D3KooW..."]
stun_servers = ["stun:stun.example.org:3478"]
turn_servers = []
enable_upnp = false
enable_hole_punching = true
default_channel = "public"

[network.discovery]
domain = "_p2p._tcp.relays.example.org"
min_peers = 8
max_peers = 256
k_bucket_size = 20
alpha = 3
query_timeout_ms = 5000

[observability]
metrics_addr = "127.0.0.1:9090"
health_addr = "127.0.0.1:8080"

[chains.chat]
rpc_url = "https://chat-chain.example.org"
chain_id = "dchat-chat-mainnet"

[chains.currency]
rpc_url = "https://currency-chain.example.org"
chain_id = "dchat-currency-mainnet"

[bridge]
finality_threshold = 3

[storage]
data_dir = "/var/lib/dchat"
```

---

## Final Notes

The quickest path to safe mainnet in the next 30 minutes is to enforce config-only parameters and disable any code paths that still rely on placeholders. The CLI surface is rich; converting each handler to call a crate-owned service with injected `Config` will “touch everything” while keeping `main.rs` stable and testable.
