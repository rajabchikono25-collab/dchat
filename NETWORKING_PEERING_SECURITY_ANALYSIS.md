# Networking, Peering, Distribution, Handshaking, Decentralization, and Security Analysis

This document consolidates current behavior and gaps across key runtime concerns: networking, server peering, distribution, handshaking, decentralization, and security. It reflects repository context (src/, crates/) and prior architectural guidance.

---

## 1. Networking

### Current Design
- Protocol stack: libp2p (gossipsub, Kademlia DHT planned per `ARCHITECTURE.md`), TCP multiaddrs, DNS-based discovery for initial bootstrap; `API_SPECIFICATION.md` declares P2P interface stability and `noise_xx` transport intent.
- Discovery: `DnsDiscoveryManager` periodically resolves validator/relay subdomains; planned decentralized bootstrap fallback path: DNS → DHT → IPFS (see decentralized bootstrap section of `ARCHITECTURE.md`).
- Connectivity thresholds: relay runtime targets ≥5 peers; validator runtime waits for peers (logs reference “4/7” while `min_peers`=6) — mismatch with BFT requirement (5-of-7 signatures) and region diversity caps.
- NAT traversal: UPnP and hole punching flags present; TURN/STUN fallback and automatic pathfinding sequence (bootstrap → hole punch → TURN relay) described in `ARCHITECTURE.md` but not fully instrumented.
- Docker/local modes: local auto-discovery tries intra-container addresses for test/dev; future anycast and multi-path routing for eclipse attack prevention (`src/network/relay_selection/`).
- Planned onion routing stack (`src/network/onion_routing/`): circuit path selection, layered encryption, Sphinx packet handling; currently placeholder.
- Version negotiation & algorithm suites (Noise/Curve25519 → hybrid PQ) planned — handshake must exchange supported suites (upgrade path in `ARCHITECTURE.md`).

### Strengths
- DNS-driven bootstrap simplifies operations under elastic infrastructure; operational seeds tracked (`ansible/bootstrap-peers.txt`).
- libp2p primitives (gossipsub, planned Kademlia, hole punching) enable transport flexibility and future decentralized peer location.
- Background DNS refresh reduces manual maintenance for IP drift; negative caching/backoff planned can further stabilize.
- Architectural layering anticipates onion routing and multi-hop circuits for metadata resistance without redesign of core message flow.
- Clear handshake cryptographic roadmap (Noise → hybrid PQ) supports long-term cryptographic agility.

### Gaps / Risks
- Fixed peer thresholds (5 for relays; 6 for validators) do not adapt to network size/topology; ignoring dynamic f and 2f+1 can reduce safety or liveness clarity.
- Limited telemetry on per-peer latency, failure rates, churn, NAT success/fallback path choice — observability insufficient for routing optimization.
- NAT traversal contingencies are unmeasured; no success counters for UPnP mappings, hole punching attempts, or TURN fallback utilization.
- Absence of DHT bootstrap fallback (Kademlia not yet active) creates reliance on centralized DNS — censorship risk and single control plane.
- Onion routing modules present but inactive — metadata leakage persists at relays; path selection and guard pinning logic unused.
- Version negotiation not enforced; potential for downgrade or mismatch if algorithm suite changes.

### Suggested KPIs
- Dynamic quorum satisfaction: connected_validators / (2f+1) (with f recalculated from active N).
- Peer connectivity vs target (active peers / adaptive target per role & region diversity).
- Dial success rate & TTFC (time to first successful authenticated connection).
- Latency distribution per peer (P50/P95/P99), propagation end-to-end (publish → all validators signature receipt).
- NAT traversal success/fallback matrix: UPnP success %, hole punch success %, TURN usage %, average additional latency per fallback.
- Bootstrap path usage: DNS-only %, DNS→DHT %, DHT→IPFS fallback occurrences.
- Onion circuit establishment success rate (once active) and average hops latency overhead.
- Version negotiation success %, suite mismatch/downgrade attempts detected.

### Immediate Improvements
- Implement dynamic peer target & quorum computation (derive f; enforce 2f+1 and region caps; expose metrics).
- Activate DHT bootstrap path; maintain seed list & rotate; measure fallback usage; integrate IPFS bootstrap backup.
- Emit per-peer telemetry (latency, failures, backoff state, NAT method) via Prometheus; add churn gauges.
- NAT diagnostics counters (UPnP mapping success, hole punching attempts/success, TURN fallback count) + alert on excessive TURN reliance.
- Integrate onion routing MVP (3-hop circuit, Sphinx packet wrapper) for experimental metadata resistance; track circuit success & latency delta.
- Enforce handshake version & algorithm negotiation; reject unsupported/downgraded suites and log incidents.
- Negative DNS caching & exponential refresh backoff to mitigate flapping records; TTL adherence metrics.

---

## 2. Server Peering

### Current Design
- Validators: DNS discovery + threshold wait before starting block loop (placeholder consensus). `MultiRegionCoordinator` defines BFT constraints (7 total, 5 required signatures, ≥3 regions, ≤40% per region). Future Kademlia DHT advertisement planned.
- Relays: DNS/manual bootstrap merged; min_peers=5; tracks basic state and uptime. Bootstrap peers enumerated in `ansible/bootstrap-peers.txt`; connection verification script `ansible/check-connections.sh` greps journal logs for bootstrap events.

### Strengths
- Enforced multi-region diversity assumptions provide anti-correlation and capture resistance.
- Simple peering rules reduce initial complexity for deployment.

### Gaps / Risks
- Runtime mismatch: log “4/7” vs policy 5-of-7 signatures; static `min_peers`=6 denies dynamic changes (e.g., validator removal/addition).
- Placeholder PeerIds impede mapping of DNS → authenticated identity; reputation and slashing hooks cannot correlate misbehavior.
- No partial-quorum startup: validators in minority partition cannot sync or assist recovery; absence of degraded-mode state broadcast.
- Missing region drift detection: if a region surpasses 40% cap runtime does not proactively warn or adjust peer selection.
- Bootstrap reliance on DNS only; absence of DHT advertisement increases risk of targeted DNS disruption.

### Suggested KPIs
- Connected validators vs required 2f+1 (dynamic) and vs total N.
- Region diversity: regions represented, max_region_percentage vs cap, drift time to remediation.
- Peer stability: median session duration, reconnection frequency per peer, churn rate.
- Bootstrap diversity: number of distinct bootstrap sources used (DNS, DHT, cached peers).
- Identity resolution latency: time from discovery to authenticated PeerId persistence.

### Immediate Improvements
- Dynamic threshold & region enforcement (compute f, 2f+1, validate region caps continuously; log deviations).
- Persist authenticated PeerIds and attach region metadata; expose to monitoring for reputation/slashing prerequisites.
- Degraded-mode operation: allow block relay/sync functions with partial quorum while flagging liveness risk.
- Region drift alerts: threshold crossing events (e.g., region >35% warn, >40% critical).
- DHT advertisement: validators publish multiaddrs, enabling DNS-independent peer discovery.

---

## 3. Distribution

### Current Design
- Operational distribution: multi-region deployment, DNS indirection, horizontal relay scale-out.
- Censorship-resistant distribution (planned): F-Droid/IPFS/Bittorrent application distribution and mirror networks.

### Strengths
- Multi-region placement and DNS allow fast failover and rolling changes.
- Planned decentralized app distribution reduces centralized dependency.

### Gaps / Risks
- Lack of automated region balancing and over-concentration detection in live telemetry.
- App distribution pathways not yet integrated into CI/CD; signature verification and mirror sync are roadmap items.

### Suggested KPIs
- Regional node count and max region share over time.
- DNS record freshness (age, mismatch incidents).
- Release propagation time across mirrors and client update success rate (once implemented).

### Immediate Improvements
- Add regional diversity monitors and alerting; guide operator rebalancing.
- Implement signed artifact verification and mirror sync health checks in CI/CD.

---

## 4. Handshaking

### Current Design
- Identity handshake: placeholder PeerIds until handshake completes; intended Noise-based authenticated handshake with Ed25519 identities (future hybrid PQ upgrade).
- Mapping of DNS entries to authenticated PeerIds not persisted; operational scripts rely on log greps rather than authoritative identity table.
- Version negotiation & algorithm suites (Noise/Curve25519 → Noise/Hybrid25519-Kyber768) specified; downgrade protection not enforced yet.

### Strengths
- Clear cryptographic direction (Noise + Ed25519) ensures confidentiality and authentication once integrated.

### Gaps / Risks
- Pre-handshake ambiguity prevents early trust scoring, relay reputation weighting, and rapid misbehavior attribution.
- No attestation/device-proof binding (TEE/MPC) for validator/relay nodes; weaker operator provenance & potential for key exfiltration.
- Lack of handshake metrics (time, failures, retry paths) impedes debugging of connection stalls.
- Missing downgrade detection for algorithm suites could allow weakened cipher negotiation.

### Suggested KPIs
- Handshake success rate; median/95th latency to authenticated channel.
- Identity persistence coverage (% peers with stored mapping within T seconds).
- Downgrade attempt count / rejected unsupported suite count.
- Attestation verification success rate (post integration).

### Immediate Improvements
- Deploy Noise handshake & capture metrics; persist mapping (peer_id → public_key → region → stake).
- Add downgrade protection & suite compatibility check; alert on mismatches.
- Integrate optional device attestation (enclave report / MPC proof) for validator/relay identity strengthening.
- Provide identity resolution cache with expiration & refresh strategy; expose to monitoring dashboard.

---

## 5. Decentralization

### Current Design
- Validator BFT policy: 5-of-7 signatures with regional diversity caps (`MultiRegionCoordinator`).
- Relay incentives (planned): uptime scoring, delivery proofs, fairness model (`src/economics/relay/fairness.rs` planned) and geographic bonuses.
- Governance (planned): DAO voting, voting power caps, diversity & term limits; relay slashing & insurance fund for misbehavior.
- Decentralized bootstrap roadmap: eventual DHT + IPFS fallback reduces central DNS dependency.

### Strengths
- Region caps and minimum region count materially improve decentralization quality, not just quantity.
- Incentivized relay network design supports wide ingress without central choke points.

### Gaps / Risks
- Region constraints unenforced at runtime (no alerts/auto rebalance guidance).
- Incentives & slashing not on-chain — centralization risk as only altruistic or large operators persist.
- Missing relay fairness counters (message relayed vs rewarded) invites potential token-draining or reward gaming once enabled.
- Lack of decentralized bootstrap fallback increases dependence on DNS under censorship scenarios.

### Suggested KPIs
- Nakamoto-style index (validators needed for >X% signatures).
- Region diversity: entropy score, cap violation frequency, time-to-remediation.
- Relay operator diversity (ASN/Org, stake dispersion, uptime percentile distribution).
- Incentive alignment metrics: delivery proofs submitted vs rewarded, fairness deviation.
- Bootstrap decentralization: % peers discovered via non-DNS paths.

### Immediate Improvements
- Runtime region cap monitor + operator guidance & auto-suggested relocation.
- Delivery proof pipeline & fairness calculator; integrate staking & slashing triggers.
- Decentralized bootstrap activation (DHT/IPFS) & measurement framework.
- Relay operator diversity incentives (geographic bonus weighting) & circuit breaker for anomalous reward patterns.

---

## 6. Security

### Current Design
- Cryptography (architected): Noise Protocol transport, Ed25519 identity, hybrid PQ (Curve25519+Kyber) planned.
- Validator safety: ed25519 signature verification on blocks; partition detection scaffolding (`detect_partition`), no equivocation detection yet.
- Privacy (planned): onion routing, Sphinx packets, cover traffic, mix network option; guard relay pinning and bridge relays for censorship bypass.
- Economic security: slashing & collateral requirements planned for relay misbehavior; insurance fund concept.

### Strengths
- Strong cryptographic posture with a roadmap for PQ migration.
- Multi-region BFT assumptions reduce risk of regional capture.

### Gaps / Risks
- No slashing/equivocation detection (double-sign, censorship withholding) — undermines deterrence.
- DNS poisoning/record staleness risk; absence of DNSSEC validation or pinned keys.
- Metadata leakage: no onion circuits, cover traffic, timing obfuscation active yet.
- Perf risk: independent ed25519 verification, no batch or BLS threshold signatures.
- Downgrade & algorithm suite negotiation unprotected; potential cryptographic agility weakness.
- NAT fallback usage unmonitored; possible silent reliance on TURN increasing latency & centralization.

### Suggested KPIs
- Slashing events: double-sign detections, censorship proofs, withheld message challenge outcomes.
- Encryption coverage: % connections with Noise + % upgraded to hybrid PQ.
- DNS trust score: DNSSEC validation %, stale record incidents, negative cache hit ratio.
- Privacy metrics: onion circuit success %, average hop count, cover traffic ratio, timing variance score.
- Signature performance: batch verify throughput, average block verification time.
- NAT fallback metrics: TURN reliance %, hole punch success %, UPnP mapping lifetime.
- Algorithm negotiation: successful suite negotiation %, downgrade attempt count.

### Immediate Improvements
- Implement slashing (double-sign, censorship withholding, fraudulent delivery proof) & penalty accounting.
- DNSSEC validation + pinned peer public keys; TTL monitoring & stale record alerting.
- Onion circuits + cover traffic scheduler (baseline rate adaptation); timing & padding obfuscation.
- Batch/aggregate signature verification or adopt threshold/BLS for block signatures.
- Suite negotiation enforcement + downgrade detection logging & rejection.
- NAT fallback instrumentation & alert on excessive TURN usage; path optimization suggestions.

---

## Prioritized Actions (30–60 Days)
1) Unify dynamic quorum/thresholds in validator runtime (derive from active set); export region diversity metrics.
2) Integrate Noise handshake and persist identity mapping; enrich telemetry.
3) Add relay reputation + delivery proof pipeline (proto settlement path); telemetry for latency/churn.
4) DNS hardening (negative caching, DNSSEC/pinning), and NAT diagnostics.
5) Slashing hooks and incident metrics; begin aggregation/batching of signature verification.

---

## Quick Reference: Key Modules/Symbols
- `DnsDiscoveryManager` – DNS-based discovery with periodic refresh.
- `MultiRegionCoordinator` / `BftConfig` – Validator regional diversity + 5-of-7 requirement.
- `RelayConfig` / `RelayNode` – Relay lifecycle, min_peers=5, uptime scaffolding.
- `run_validator_node` – Validator startup thresholds, placeholder consensus loop, TODO staking.
- `src/network/nat/` – UPnP, TURN, hole punching logic (planned instrumentation).
- `src/network/onion_routing/` – Onion circuit, Sphinx packet, path selection stubs.
- `src/network/relay_selection/` – Eclipse attack prevention, multi-path routing.
- `src/economics/relay/` – Fairness, uptime rewards, geographic bonuses (planned).
- `ansible/bootstrap-peers.txt` / `ansible/check-connections.sh` – Operational bootstrap seeds & connectivity verification.
- `API_SPECIFICATION.md` – P2P/libp2p API, handshake scheme (`noise_xx`), bootstrap peer examples.

---

Generated by repository inspection and prior architectural context.
