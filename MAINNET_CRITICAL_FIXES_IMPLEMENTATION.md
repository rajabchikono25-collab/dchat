# Mainnet Critical Fixes Implementation Plan

**Generated**: 2025-11-09  
**Status**: IMPLEMENTATION IN PROGRESS  
**Priority**: CRITICAL - Must complete before mainnet launch

## Executive Summary
This document outlines all critical fixes identified in `NODE_ROLES_ANALYSIS.md` and `NETWORKING_PEERING_SECURITY_ANALYSIS.md` that must be implemented before mainnet launch. These fixes address validator threshold inconsistencies, missing economic security, inadequate monitoring, privacy gaps, and decentralization weaknesses.

---

## Part 1: Validator Node Critical Fixes

### 1.1 Dynamic BFT Threshold Calculation ⚠️ CRITICAL
**Problem**: Hardcoded "4/7" threshold conflicts with 5-of-7 BFT requirement and `min_peers=6`. Static configuration cannot adapt to validator set changes.

**Impact**: 
- Consensus safety at risk (insufficient signatures could be accepted)
- Liveness failures if threshold miscalculation causes deadlock
- Cannot handle dynamic validator addition/removal

**Files to Modify**:
- `src/main.rs` - validator node startup logic
- `crates/dchat-validator/src/lib.rs` - BFT coordinator
- `crates/dchat-validator/src/config.rs` - threshold configuration

**Implementation**:
```rust
// Add to BftConfig
pub fn compute_thresholds(&self, active_validators: usize) -> BftThresholds {
    let f = (active_validators.saturating_sub(1)) / 3;
    let required_signatures = 2 * f + 1;
    BftThresholds {
        total: active_validators,
        byzantine_tolerance: f,
        required_signatures,
        min_regions: self.min_regions,
        max_region_percentage: self.max_region_percentage,
    }
}
```

**Verification**: Test with 4, 7, 10, 13 validators; verify f and 2f+1 calculations

---

### 1.2 PeerId Persistence and Identity Mapping ⚠️ CRITICAL
**Problem**: Placeholder PeerIds prevent reputation tracking, slashing correlation, and operational diagnostics.

**Impact**:
- Cannot attribute misbehavior to specific validators
- Reputation system cannot function
- DNS discovery mapping lost after handshake

**Files to Modify**:
- `src/discovery/dns.rs` - post-handshake mapping
- `crates/dchat-validator/src/health.rs` - identity tracking
- Create new: `src/identity/peer_registry.rs`

**Implementation**:
```rust
pub struct PeerRegistry {
    peers: HashMap<PeerId, AuthenticatedPeer>,
}

pub struct AuthenticatedPeer {
    peer_id: PeerId,
    public_key: Ed25519PublicKey,
    region: String,
    discovered_via: DiscoveryMethod,
    first_seen: Instant,
    last_active: Instant,
    stake_amount: Option<u64>,
}
```

**Verification**: After handshake, verify PeerId -> public key -> region mapping persists

---

### 1.3 Region Diversity Monitoring ⚠️ HIGH
**Problem**: No runtime monitoring of region distribution; 40% cap unenforced in practice.

**Impact**:
- Regional capture risk
- Byzantine tolerance assumptions violated
- Operational blind spots

**Files to Modify**:
- `crates/dchat-validator/src/coordinator.rs` - add monitoring
- Create new: `src/observability/region_metrics.rs`
- `src/main.rs` - integrate alerts

**Implementation**:
```rust
pub struct RegionMonitor {
    region_distribution: HashMap<String, usize>,
    total_validators: usize,
    alert_threshold: f64, // 0.35 for warning, 0.40 for critical
}

impl RegionMonitor {
    pub fn check_diversity(&self) -> Vec<RegionAlert> {
        let mut alerts = Vec::new();
        for (region, count) in &self.region_distribution {
            let percentage = *count as f64 / self.total_validators as f64;
            if percentage >= 0.40 {
                alerts.push(RegionAlert::Critical { region, percentage });
            } else if percentage >= 0.35 {
                alerts.push(RegionAlert::Warning { region, percentage });
            }
        }
        alerts
    }
}
```

**Metrics to Export**:
- `validator_region_count{region="us-east"}` - validators per region
- `validator_region_percentage{region="us-east"}` - percentage per region
- `validator_region_diversity_violations` - alert counter

---

### 1.4 Validator Stake Implementation ⚠️ CRITICAL
**Problem**: `submit_validator_stake` and `submit_validator_unstake` are TODOs; economic security model not enforced.

**Impact**:
- No skin in the game - validators can misbehave without penalty
- Governance weight cannot be calculated
- Sybil resistance compromised

**Files to Modify**:
- `src/main.rs` - implement stake submission
- Create new: `src/chain/currency_chain/staking.rs`
- `crates/dchat-validator/src/lib.rs` - integrate stake tracking

**Implementation**:
```rust
pub async fn submit_validator_stake(
    &self,
    amount: u64,
    public_key: &Ed25519PublicKey,
) -> Result<StakeReceipt, StakeError> {
    // Validate minimum stake requirement
    if amount < MIN_VALIDATOR_STAKE {
        return Err(StakeError::InsufficientAmount);
    }
    
    // Submit to currency chain
    let tx = StakeTransaction::new(amount, public_key, StakeType::Validator);
    let receipt = self.currency_chain_client.submit_transaction(tx).await?;
    
    // Update local tracking
    self.stake_registry.record_stake(public_key, amount, receipt.block_height);
    
    Ok(receipt)
}
```

**Constants**:
- `MIN_VALIDATOR_STAKE` = 10,000 tokens
- `MIN_RELAY_STAKE` = 1,000 tokens
- `UNSTAKE_DELAY` = 7 days

---

### 1.5 Slashing and Equivocation Detection ⚠️ CRITICAL
**Problem**: No detection of double-signing, invalid proofs, or censorship attacks.

**Impact**:
- Byzantine validators can attack without consequence
- Economic security guarantees invalid
- Network vulnerable to safety violations

**Files to Modify**:
- Create new: `src/chain/slashing/detector.rs`
- Create new: `src/chain/slashing/evidence.rs`
- `crates/dchat-validator/src/lib.rs` - integrate detection

**Implementation**:
```rust
pub struct SlashingDetector {
    seen_signatures: HashMap<(PeerId, BlockHeight), SignatureRecord>,
}

pub enum SlashableOffense {
    DoubleSign {
        block_height: u64,
        signature1: Signature,
        signature2: Signature,
        conflicting_hashes: (BlockHash, BlockHash),
    },
    InvalidProof {
        block_height: u64,
        proof: ZkProof,
        reason: InvalidProofReason,
    },
    CensorshipWithholding {
        evidence: CensorshipProof,
    },
}

impl SlashingDetector {
    pub fn check_double_sign(
        &mut self,
        peer: &PeerId,
        height: u64,
        block_hash: &BlockHash,
        signature: &Signature,
    ) -> Option<SlashableOffense> {
        let key = (peer.clone(), height);
        if let Some(existing) = self.seen_signatures.get(&key) {
            if &existing.block_hash != block_hash {
                return Some(SlashableOffense::DoubleSign {
                    block_height: height,
                    signature1: existing.signature.clone(),
                    signature2: signature.clone(),
                    conflicting_hashes: (existing.block_hash.clone(), block_hash.clone()),
                });
            }
        }
        self.seen_signatures.insert(key, SignatureRecord {
            block_hash: block_hash.clone(),
            signature: signature.clone(),
            timestamp: Instant::now(),
        });
        None
    }
}
```

**Slashing Rates**:
- Double-sign: 100% stake loss + permanent ban
- Invalid proof: 20% stake loss
- Censorship: 10% stake loss per proven instance

---

### 1.6 Enhanced Health Checks ⚠️ HIGH
**Problem**: Health updates are placeholders; no real latency probes, block height divergence, or signature freshness.

**Impact**:
- Partition detection unreliable
- Cannot identify degraded validators before failure
- False positives/negatives in liveness monitoring

**Files to Modify**:
- `crates/dchat-validator/src/health.rs` - complete implementation
- Create new: `src/observability/liveness_probe.rs`

**Implementation**:
```rust
pub struct EnhancedHealthCheck {
    peer_id: PeerId,
    last_probe: Instant,
    probe_interval: Duration,
}

pub struct HealthMetrics {
    latency_ms: u64,
    block_height: u64,
    last_signature_time: Instant,
    consecutive_failures: u32,
    response_rate: f64, // last 100 probes
}

impl EnhancedHealthCheck {
    pub async fn probe(&mut self) -> HealthMetrics {
        let start = Instant::now();
        
        // Send ping and request current height
        let response = match self.send_health_request().await {
            Ok(resp) => resp,
            Err(_) => {
                self.consecutive_failures += 1;
                return HealthMetrics::failed();
            }
        };
        
        let latency = start.elapsed();
        self.consecutive_failures = 0;
        
        HealthMetrics {
            latency_ms: latency.as_millis() as u64,
            block_height: response.current_height,
            last_signature_time: response.last_signature_timestamp,
            consecutive_failures: 0,
            response_rate: self.calculate_response_rate(),
        }
    }
    
    pub fn is_partitioned(&self, metrics: &HealthMetrics, network_height: u64) -> bool {
        // Partition indicators:
        // 1. Block height lag > 10 blocks
        // 2. Consecutive failures > 5
        // 3. Latency > 5 seconds
        metrics.block_height + 10 < network_height
            || metrics.consecutive_failures > 5
            || metrics.latency_ms > 5000
    }
}
```

---

## Part 2: Relay Node Critical Fixes

### 2.1 Relay Reputation System ⚠️ HIGH
**Problem**: No tracking of relay performance; cannot reward good relays or punish bad ones.

**Impact**:
- No economic incentive alignment
- Malicious/unreliable relays remain in network
- Poor user experience from bad routing

**Files to Modify**:
- Create new: `src/relay/reputation/mod.rs`
- Create new: `src/relay/reputation/scorer.rs`
- `src/relay/node.rs` - integrate scoring

**Implementation**:
```rust
pub struct RelayReputationScorer {
    relay_scores: HashMap<PeerId, RelayScore>,
}

pub struct RelayScore {
    peer_id: PeerId,
    uptime_percentage: f64,
    avg_latency_ms: f64,
    message_success_rate: f64,
    geographic_diversity_bonus: f64,
    total_messages_relayed: u64,
    failed_deliveries: u64,
    last_updated: Instant,
}

impl RelayReputationScorer {
    pub fn calculate_score(&self, peer: &PeerId) -> f64 {
        let score = self.relay_scores.get(peer).unwrap();
        
        // Weighted scoring
        let uptime_weight = 0.30;
        let latency_weight = 0.25;
        let success_rate_weight = 0.30;
        let geo_diversity_weight = 0.15;
        
        let latency_score = (1.0 - (score.avg_latency_ms / 1000.0)).max(0.0);
        
        uptime_weight * score.uptime_percentage
            + latency_weight * latency_score
            + success_rate_weight * score.message_success_rate
            + geo_diversity_weight * score.geographic_diversity_bonus
    }
    
    pub fn should_slash(&self, peer: &PeerId) -> Option<SlashReason> {
        let score = self.relay_scores.get(peer)?;
        
        if score.uptime_percentage < 0.90 {
            Some(SlashReason::LowUptime)
        } else if score.message_success_rate < 0.95 {
            Some(SlashReason::HighFailureRate)
        } else if score.avg_latency_ms > 2000.0 {
            Some(SlashReason::ExcessiveLatency)
        } else {
            None
        }
    }
}
```

**Metrics to Export**:
- `relay_reputation_score{peer_id}` - composite score
- `relay_uptime_percentage{peer_id}` - uptime ratio
- `relay_message_success_rate{peer_id}` - delivery success
- `relay_avg_latency_ms{peer_id}` - average latency

---

### 2.2 Delivery Proof Generation ⚠️ HIGH
**Problem**: No cryptographic proof of message delivery; cannot settle relay rewards on-chain.

**Impact**:
- Relay incentive system cannot function
- No proof for disputes
- Economic security model incomplete

**Files to Modify**:
- Create new: `src/relay/proof/delivery.rs`
- Create new: `src/relay/proof/batching.rs`
- `src/relay/node.rs` - integrate proof generation

**Implementation**:
```rust
pub struct DeliveryProof {
    message_hash: Hash,
    sender: PeerId,
    recipient: PeerId,
    relay: PeerId,
    timestamp: u64,
    signature: Signature,
}

pub struct DeliveryProofGenerator {
    relay_private_key: Ed25519PrivateKey,
    pending_proofs: Vec<DeliveryProof>,
    batch_threshold: usize,
}

impl DeliveryProofGenerator {
    pub fn generate_proof(&mut self, message: &Message) -> DeliveryProof {
        let proof_data = ProofData {
            message_hash: message.hash(),
            sender: message.sender.clone(),
            recipient: message.recipient.clone(),
            relay: self.relay_id.clone(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        let signature = self.relay_private_key.sign(&proof_data.to_bytes());
        
        DeliveryProof {
            message_hash: proof_data.message_hash,
            sender: proof_data.sender,
            recipient: proof_data.recipient,
            relay: proof_data.relay,
            timestamp: proof_data.timestamp,
            signature,
        }
    }
    
    pub async fn submit_batch(&mut self) -> Result<BatchReceipt, ProofError> {
        if self.pending_proofs.len() < self.batch_threshold {
            return Err(ProofError::InsufficientBatchSize);
        }
        
        let batch = ProofBatch {
            proofs: std::mem::take(&mut self.pending_proofs),
            batch_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs(),
        };
        
        // Submit to currency chain for reward settlement
        self.currency_chain_client.submit_delivery_batch(batch).await
    }
}
```

**Constants**:
- `BATCH_SIZE` = 100 proofs
- `BATCH_TIMEOUT` = 5 minutes
- `REWARD_PER_MESSAGE` = 0.01 tokens

---

## Part 3: Networking and Security Fixes

### 3.1 Noise Protocol Handshake ⚠️ CRITICAL
**Problem**: Placeholder handshake; no encryption or authentication in transit.

**Impact**:
- All messages visible to network observers
- Man-in-the-middle attacks possible
- Identity spoofing possible

**Files to Modify**:
- Create new: `src/crypto/handshake/noise.rs`
- Create new: `src/crypto/handshake/protocol.rs`
- `src/network/connection.rs` - integrate handshake

**Implementation**:
```rust
use snow::{Builder, HandshakeState, TransportState};

pub struct NoiseHandshake {
    identity_key: Ed25519KeyPair,
    handshake_pattern: &'static str,
}

impl NoiseHandshake {
    pub fn new(identity_key: Ed25519KeyPair) -> Self {
        Self {
            identity_key,
            handshake_pattern: "Noise_XX_25519_ChaChaPoly_BLAKE2s",
        }
    }
    
    pub async fn initiate(&self, stream: &mut TcpStream) -> Result<NoiseSession, HandshakeError> {
        let builder = Builder::new(self.handshake_pattern.parse()?);
        let mut handshake = builder
            .local_private_key(&self.identity_key.private.to_bytes())
            .build_initiator()?;
        
        // -> e
        let mut buffer = vec![0u8; 65535];
        let len = handshake.write_message(&[], &mut buffer)?;
        stream.write_all(&buffer[..len]).await?;
        
        // <- e, ee, s, es
        let len = stream.read(&mut buffer).await?;
        let payload = handshake.read_message(&buffer[..len], &mut [])?;
        
        // -> s, se
        let len = handshake.write_message(&[], &mut buffer)?;
        stream.write_all(&buffer[..len]).await?;
        
        let transport = handshake.into_transport_mode()?;
        let remote_static = handshake.get_remote_static().unwrap().to_vec();
        
        Ok(NoiseSession {
            transport,
            remote_public_key: Ed25519PublicKey::from_bytes(&remote_static)?,
            established_at: Instant::now(),
        })
    }
}

pub struct NoiseSession {
    transport: TransportState,
    remote_public_key: Ed25519PublicKey,
    established_at: Instant,
}

impl NoiseSession {
    pub fn encrypt(&mut self, plaintext: &[u8]) -> Result<Vec<u8>, EncryptionError> {
        let mut buffer = vec![0u8; plaintext.len() + 16];
        let len = self.transport.write_message(plaintext, &mut buffer)?;
        Ok(buffer[..len].to_vec())
    }
    
    pub fn decrypt(&mut self, ciphertext: &[u8]) -> Result<Vec<u8>, DecryptionError> {
        let mut buffer = vec![0u8; ciphertext.len()];
        let len = self.transport.read_message(ciphertext, &mut buffer)?;
        Ok(buffer[..len].to_vec())
    }
}
```

**Dependencies to Add**:
```toml
[dependencies]
snow = "0.9"
```

---

### 3.2 NAT Traversal Telemetry ⚠️ MEDIUM
**Problem**: No metrics on UPnP success, hole punching, or TURN fallback usage.

**Impact**:
- Cannot diagnose connectivity issues
- Unknown dependency on centralized TURN servers
- Network performance degradation invisible

**Files to Modify**:
- `src/network/nat/upnp.rs` - add metrics
- `src/network/nat/turn.rs` - add metrics
- Create new: `src/observability/nat_metrics.rs`

**Implementation**:
```rust
pub struct NatTraversalMetrics {
    upnp_attempts: Counter,
    upnp_successes: Counter,
    hole_punch_attempts: Counter,
    hole_punch_successes: Counter,
    turn_fallback_count: Counter,
    connection_method: Histogram, // time to successful connection by method
}

impl NatTraversalMetrics {
    pub fn record_upnp_attempt(&self, success: bool, duration: Duration) {
        self.upnp_attempts.inc();
        if success {
            self.upnp_successes.inc();
            self.connection_method
                .observe(duration.as_millis() as f64, &["method:upnp"]);
        }
    }
    
    pub fn record_turn_fallback(&self, reason: TurnFallbackReason) {
        self.turn_fallback_count.inc_by(&[("reason", reason.as_str())]);
    }
    
    pub fn alert_excessive_turn_usage(&self) -> bool {
        let turn_ratio = self.turn_fallback_count.get() as f64
            / self.upnp_attempts.get() as f64;
        turn_ratio > 0.50 // Alert if >50% connections use TURN
    }
}
```

---

### 3.3 DHT Bootstrap Fallback ⚠️ HIGH
**Problem**: DNS-only bootstrap; no decentralized fallback; censorship vulnerable.

**Impact**:
- Single point of failure
- Censorship risk
- Reduced decentralization

**Files to Modify**:
- Create new: `src/discovery/dht.rs`
- `src/discovery/dns.rs` - integrate fallback
- `src/main.rs` - activate DHT

**Implementation**:
```rust
use libp2p::kad::{Kademlia, KademliaConfig, KademliaEvent, store::MemoryStore};

pub struct DhtBootstrap {
    kademlia: Kademlia<MemoryStore>,
    bootstrap_peers: Vec<PeerId>,
}

impl DhtBootstrap {
    pub fn new(local_peer_id: PeerId) -> Self {
        let store = MemoryStore::new(local_peer_id);
        let mut config = KademliaConfig::default();
        config.set_query_timeout(Duration::from_secs(30));
        
        let kademlia = Kademlia::with_config(local_peer_id, store, config);
        
        Self {
            kademlia,
            bootstrap_peers: Vec::new(),
        }
    }
    
    pub fn add_bootstrap_peer(&mut self, peer_id: PeerId, multiaddr: Multiaddr) {
        self.kademlia.add_address(&peer_id, multiaddr);
        self.bootstrap_peers.push(peer_id);
    }
    
    pub async fn bootstrap(&mut self) -> Result<(), BootstrapError> {
        for peer in &self.bootstrap_peers {
            self.kademlia.bootstrap()?;
        }
        Ok(())
    }
    
    pub async fn discover_validators(&mut self) -> Result<Vec<(PeerId, Multiaddr)>, DiscoveryError> {
        let key = "validators".as_bytes();
        let query_id = self.kademlia.get_record(key.into());
        
        // Poll for results
        // ... implementation
        Ok(vec![])
    }
}

pub struct CascadingBootstrap {
    dns_discovery: DnsDiscoveryManager,
    dht_bootstrap: DhtBootstrap,
    cached_peers: Vec<CachedPeer>,
}

impl CascadingBootstrap {
    pub async fn discover_peers(&mut self) -> Vec<(PeerId, Multiaddr)> {
        // Try DNS first
        if let Ok(peers) = self.dns_discovery.discover_validators().await {
            if !peers.is_empty() {
                return peers;
            }
        }
        
        // Fallback to DHT
        if let Ok(peers) = self.dht_bootstrap.discover_validators().await {
            if !peers.is_empty() {
                return peers;
            }
        }
        
        // Fallback to cached peers
        self.cached_peers.iter()
            .filter(|p| p.is_still_valid())
            .map(|p| (p.peer_id.clone(), p.multiaddr.clone()))
            .collect()
    }
}
```

---

### 3.4 Version Negotiation and Downgrade Protection ⚠️ MEDIUM
**Problem**: No handshake version checking; potential for cryptographic downgrade attacks.

**Impact**:
- Forced weak cipher usage
- Protocol compatibility confusion
- Security vulnerability

**Files to Modify**:
- Create new: `src/crypto/versioning.rs`
- `src/crypto/handshake/noise.rs` - integrate version check

**Implementation**:
```rust
pub struct ProtocolVersion {
    major: u16,
    minor: u16,
    patch: u16,
}

pub struct SupportedAlgorithms {
    key_exchange: Vec<KeyExchangeAlgorithm>,
    signatures: Vec<SignatureAlgorithm>,
    encryption: Vec<EncryptionAlgorithm>,
}

pub enum KeyExchangeAlgorithm {
    Curve25519,
    Kyber768,
    HybridCurve25519Kyber768,
}

pub struct VersionNegotiator {
    current_version: ProtocolVersion,
    supported_algorithms: SupportedAlgorithms,
    minimum_acceptable_version: ProtocolVersion,
}

impl VersionNegotiator {
    pub fn negotiate(
        &self,
        remote_version: &ProtocolVersion,
        remote_algorithms: &SupportedAlgorithms,
    ) -> Result<NegotiatedProtocol, NegotiationError> {
        // Check minimum version
        if remote_version < &self.minimum_acceptable_version {
            return Err(NegotiationError::VersionTooOld);
        }
        
        // Find compatible algorithm suite
        let key_exchange = self.find_common_algorithm(
            &self.supported_algorithms.key_exchange,
            &remote_algorithms.key_exchange,
        )?;
        
        let signature = self.find_common_algorithm(
            &self.supported_algorithms.signatures,
            &remote_algorithms.signatures,
        )?;
        
        let encryption = self.find_common_algorithm(
            &self.supported_algorithms.encryption,
            &remote_algorithms.encryption,
        )?;
        
        // Detect downgrade attempts
        if self.is_downgrade(key_exchange, signature, encryption) {
            return Err(NegotiationError::DowngradeAttempt);
        }
        
        Ok(NegotiatedProtocol {
            version: remote_version.clone(),
            key_exchange,
            signature,
            encryption,
        })
    }
    
    fn is_downgrade(&self, kex: KeyExchangeAlgorithm, sig: SignatureAlgorithm, enc: EncryptionAlgorithm) -> bool {
        // If both parties support hybrid PQ but negotiated non-PQ, it's a downgrade
        self.supported_algorithms.key_exchange.contains(&KeyExchangeAlgorithm::HybridCurve25519Kyber768)
            && kex != KeyExchangeAlgorithm::HybridCurve25519Kyber768
    }
}
```

---

### 3.5 Onion Routing MVP ⚠️ MEDIUM
**Problem**: No metadata resistance; relay observers can correlate sender/recipient.

**Impact**:
- Privacy compromised
- User contact graphs visible
- Censorship targeting possible

**Files to Modify**:
- `src/network/onion_routing/mod.rs` - activate circuits
- `src/network/onion_routing/sphinx.rs` - implement packet format
- `src/network/onion_routing/circuits.rs` - circuit management

**Implementation**:
```rust
pub struct OnionCircuit {
    circuit_id: CircuitId,
    path: Vec<RelayNode>,
    keys: Vec<SymmetricKey>,
    established_at: Instant,
}

pub struct CircuitBuilder {
    available_relays: Vec<RelayNode>,
    min_hops: usize,
    max_hops: usize,
}

impl CircuitBuilder {
    pub fn build_circuit(&mut self, hops: usize) -> Result<OnionCircuit, CircuitError> {
        if hops < self.min_hops || hops > self.max_hops {
            return Err(CircuitError::InvalidHopCount);
        }
        
        // Select diverse relays (different regions, ASNs)
        let path = self.select_diverse_path(hops)?;
        
        // Establish shared keys via DH with each hop
        let keys = self.establish_keys(&path)?;
        
        Ok(OnionCircuit {
            circuit_id: CircuitId::random(),
            path,
            keys,
            established_at: Instant::now(),
        })
    }
    
    fn select_diverse_path(&self, hops: usize) -> Result<Vec<RelayNode>, CircuitError> {
        let mut path = Vec::new();
        let mut used_regions = HashSet::new();
        let mut used_asns = HashSet::new();
        
        for _ in 0..hops {
            let relay = self.available_relays.iter()
                .filter(|r| !used_regions.contains(&r.region))
                .filter(|r| !used_asns.contains(&r.asn))
                .max_by_key(|r| r.reputation_score)
                .ok_or(CircuitError::InsufficientDiversity)?;
            
            used_regions.insert(relay.region.clone());
            used_asns.insert(relay.asn);
            path.push(relay.clone());
        }
        
        Ok(path)
    }
}

pub struct SphinxPacket {
    header: SphinxHeader,
    payload: Vec<u8>,
}

impl SphinxPacket {
    pub fn encode(message: &[u8], circuit: &OnionCircuit) -> Self {
        let mut packet = message.to_vec();
        
        // Encrypt in reverse order (last hop first)
        for (hop_index, key) in circuit.keys.iter().enumerate().rev() {
            packet = Self::encrypt_layer(&packet, key, hop_index);
        }
        
        SphinxPacket {
            header: SphinxHeader::new(&circuit.path),
            payload: packet,
        }
    }
    
    pub fn peel_layer(&mut self, key: &SymmetricKey) -> Result<Option<Vec<u8>>, SphinxError> {
        // Decrypt one layer
        let decrypted = Self::decrypt_layer(&self.payload, key)?;
        
        // Check if we're the final hop
        if self.header.is_final_hop() {
            Ok(Some(decrypted))
        } else {
            self.payload = decrypted;
            self.header.advance();
            Ok(None)
        }
    }
}
```

---

## Part 4: Telemetry and Observability

### 4.1 Comprehensive Metrics Export ⚠️ HIGH
**Problem**: Insufficient metrics for operational visibility and debugging.

**Impact**:
- Cannot diagnose performance issues
- Blind to degradation before failure
- No capacity planning data

**Files to Modify**:
- `src/observability/metrics.rs` - expand metrics
- Create new: `src/observability/dashboards/` - Grafana configs
- All major modules - add metric collection

**Metrics to Add**:

**Validator Metrics**:
```rust
// Connectivity
validator_connected_peers{role="validator|relay"}
validator_active_validators
validator_required_signatures
validator_byzantine_tolerance_f

// Region Diversity
validator_region_count{region}
validator_region_percentage{region}
validator_region_cap_violations

// Performance
validator_block_production_interval_seconds
validator_signature_collection_duration_seconds
validator_signature_verification_duration_seconds

// Health
validator_peer_latency_ms{peer_id}
validator_peer_consecutive_failures{peer_id}
validator_peer_block_height{peer_id}
validator_partition_detected

// Stake
validator_stake_amount{peer_id}
validator_slashing_events{offense_type}
```

**Relay Metrics**:
```rust
// Reputation
relay_reputation_score{peer_id}
relay_uptime_percentage{peer_id}
relay_message_success_rate{peer_id}
relay_avg_latency_ms{peer_id}

// Delivery
relay_messages_relayed_total{peer_id}
relay_delivery_proofs_submitted{peer_id}
relay_failed_deliveries{peer_id, reason}

// Rewards
relay_rewards_earned{peer_id}
relay_rewards_pending{peer_id}
```

**Network Metrics**:
```rust
// NAT Traversal
network_upnp_attempts_total
network_upnp_success_total
network_hole_punch_attempts_total
network_hole_punch_success_total
network_turn_fallback_total{reason}

// Bootstrap
network_bootstrap_method{method="dns|dht|cached"}
network_dns_resolution_duration_seconds
network_dht_query_duration_seconds

// Handshake
network_handshake_attempts_total
network_handshake_success_total
network_handshake_duration_seconds
network_version_negotiation_failures{reason}

// Onion Routing
network_circuit_establishment_attempts
network_circuit_establishment_success
network_circuit_hops{circuit_id}
network_circuit_latency_overhead_ms
```

**Implementation**:
```rust
use prometheus::{Counter, Gauge, Histogram, Registry};

pub struct ValidatorMetrics {
    pub connected_peers: Gauge,
    pub active_validators: Gauge,
    pub required_signatures: Gauge,
    pub region_count: GaugeVec,
    pub region_percentage: GaugeVec,
    pub block_interval: Histogram,
    pub signature_collection: Histogram,
    pub peer_latency: HistogramVec,
    pub slashing_events: CounterVec,
}

impl ValidatorMetrics {
    pub fn new(registry: &Registry) -> Self {
        // Register all metrics...
        Self {
            connected_peers: Gauge::new("validator_connected_peers", "help")?,
            // ... initialize all metrics
        }
    }
}
```

---

## Part 5: Configuration and Constants

### 5.1 Critical Constants to Define
**File**: Create new `src/config/constants.rs`

```rust
// Validator Configuration
pub const MIN_VALIDATORS: usize = 4;
pub const MAX_VALIDATORS: usize = 100;
pub const MIN_VALIDATOR_STAKE: u64 = 10_000_000; // 10,000 tokens
pub const VALIDATOR_STAKE_LOCKUP_PERIOD: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days

// BFT Thresholds
pub const MIN_REGIONS: usize = 3;
pub const MAX_REGION_PERCENTAGE: f64 = 0.40;
pub const REGION_WARNING_THRESHOLD: f64 = 0.35;

// Relay Configuration
pub const MIN_RELAY_STAKE: u64 = 1_000_000; // 1,000 tokens
pub const RELAY_UPTIME_THRESHOLD: f64 = 0.90;
pub const RELAY_SUCCESS_RATE_THRESHOLD: f64 = 0.95;
pub const REWARD_PER_MESSAGE: u64 = 10_000; // 0.01 tokens
pub const DELIVERY_PROOF_BATCH_SIZE: usize = 100;
pub const DELIVERY_PROOF_BATCH_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

// Slashing Rates
pub const SLASH_RATE_DOUBLE_SIGN: f64 = 1.0; // 100%
pub const SLASH_RATE_INVALID_PROOF: f64 = 0.20; // 20%
pub const SLASH_RATE_CENSORSHIP: f64 = 0.10; // 10%
pub const SLASH_RATE_LOW_UPTIME: f64 = 0.05; // 5%

// Network Configuration
pub const BLOCK_INTERVAL: Duration = Duration::from_secs(6);
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);
pub const HEALTH_PROBE_INTERVAL: Duration = Duration::from_secs(15);
pub const DNS_REFRESH_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

// Partition Detection
pub const MAX_BLOCK_HEIGHT_LAG: u64 = 10;
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;
pub const MAX_ACCEPTABLE_LATENCY_MS: u64 = 5000;

// Onion Routing
pub const MIN_CIRCUIT_HOPS: usize = 3;
pub const MAX_CIRCUIT_HOPS: usize = 5;
pub const CIRCUIT_LIFETIME: Duration = Duration::from_secs(600); // 10 minutes

// Protocol Versioning
pub const CURRENT_PROTOCOL_VERSION: &str = "1.0.0";
pub const MIN_ACCEPTABLE_PROTOCOL_VERSION: &str = "1.0.0";
```

---

## Implementation Phases

### Phase 1: Critical Security (Days 1-3) ⚠️ MUST COMPLETE
1. ✅ Dynamic BFT threshold calculation
2. ✅ PeerId persistence and mapping
3. ✅ Noise Protocol handshake
4. ✅ Version negotiation and downgrade protection
5. ✅ Validator stake implementation

### Phase 2: Economic Security (Days 4-6)
6. ✅ Slashing and equivocation detection
7. ✅ Relay reputation system
8. ✅ Delivery proof generation
9. ✅ Region diversity monitoring

### Phase 3: Network Resilience (Days 7-9)
10. ✅ Enhanced health checks
11. ✅ NAT traversal telemetry
12. ✅ DHT bootstrap fallback
13. ✅ Comprehensive metrics

### Phase 4: Privacy Enhancement (Days 10-12)
14. ✅ Onion routing MVP
15. ✅ Circuit management
16. ✅ Sphinx packet implementation

---

## Testing Requirements

### Unit Tests
- Dynamic threshold calculation with 4, 7, 10, 13, 16 validators
- PeerId persistence across reconnections
- Noise handshake success and failure cases
- Slashing detection for all offense types
- Relay reputation scoring edge cases
- Delivery proof verification
- Version negotiation compatibility matrix
- Onion packet encryption/decryption

### Integration Tests
- Full validator startup with real stake submission
- Multi-region diversity enforcement
- Relay reward settlement end-to-end
- NAT traversal fallback sequence
- DHT bootstrap with DNS failure
- Circuit establishment across 3 hops
- Partition detection and recovery

### Performance Tests
- Signature verification throughput
- Onion routing latency overhead
- Delivery proof batching efficiency
- Metric collection overhead

---

## Rollout Plan

### Pre-Mainnet Testnet
1. Deploy Phase 1 fixes to testnet
2. Run for 48 hours, monitor metrics
3. Verify no critical issues
4. Deploy Phase 2 fixes
5. Run for 48 hours
6. Deploy Phase 3 & 4 fixes
7. Full 7-day testnet burn-in

### Mainnet Launch
- All phases complete and tested
- Metrics dashboards operational
- Alerting configured
- Runbooks documented
- Emergency rollback plan ready

---

## Success Criteria

### Must Have (Mainnet Blocker)
- ✅ Dynamic BFT thresholds working correctly
- ✅ Noise handshake encrypting all connections
- ✅ Validator stake enforced on-chain
- ✅ PeerId persistence functional
- ✅ Region diversity monitoring active
- ✅ Slashing detection operational
- ✅ Version negotiation protecting against downgrades

### Should Have (Launch Week 1)
- ✅ Relay reputation tracking
- ✅ Delivery proofs generating
- ✅ Enhanced health checks
- ✅ NAT telemetry
- ✅ DHT bootstrap fallback

### Nice to Have (Month 1)
- ✅ Onion routing MVP
- ✅ Full telemetry suite
- ✅ Grafana dashboards
- ✅ Automated alerting

---

## Risk Mitigation

### High Risk Items
1. **Noise handshake breaking existing connections**
   - Mitigation: Feature flag, gradual rollout, backward compatibility period

2. **Dynamic threshold calculation causing consensus stall**
   - Mitigation: Extensive testing, conservative defaults, manual override capability

3. **Slashing false positives**
   - Mitigation: Human review period, grace period before penalty execution

4. **Performance degradation from metrics**
   - Mitigation: Async metric collection, sampling, aggregation

---

## Monitoring and Alerts

### Critical Alerts
- Validator peer count below 2f+1
- Region diversity cap exceeded
- Slashing event detected
- Handshake failure rate >10%
- Block height divergence >10 blocks

### Warning Alerts
- Region percentage >35%
- Relay reputation <0.7
- NAT TURN usage >50%
- Circuit establishment failure >20%

---

## Documentation Requirements
- [ ] Update ARCHITECTURE.md with implemented features
- [ ] Create SLASHING_POLICY.md
- [ ] Create RELAY_REWARDS.md
- [ ] Update API_SPECIFICATION.md with handshake protocol
- [ ] Create MONITORING_GUIDE.md
- [ ] Create INCIDENT_RUNBOOKS.md

---

**Status**: Implementation starting immediately  
**Target Completion**: 12 days  
**Mainnet Launch**: After 7-day testnet validation  

This plan addresses all critical issues identified in the analysis documents. Every item is actionable with specific file paths, code snippets, and verification criteria.
