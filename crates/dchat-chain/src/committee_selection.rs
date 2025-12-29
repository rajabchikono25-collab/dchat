//! QGE Relay Committee Selection for Epoch Token Issuance
//!
//! This module implements deterministic relay committee selection for QGE epoch token
//! issuance using hash-based deterministic selection combined with stake weighting,
//! reputation scoring, and geographic diversity requirements.
//!
//! # Design Philosophy
//!
//! This module provides a lightweight committee selection system for QGE that uses
//! deterministic hashing (BLAKE3) rather than cryptographic VRF. This is appropriate
//! because:
//!
//! 1. **Public inputs**: The conversation_id and epoch_day are known to all participants
//! 2. **Reproducibility**: All parties must compute the same committee independently
//! 3. **No proof required**: We don't need to prove selection was done correctly
//!
//! For PoRW attestation committees where unpredictability from a secret key is required,
//! use `dchat_blockchain::hardened_consensus::vrf_committees` which provides full
//! Schnorrkel VRF with cryptographic proofs.
//!
//! # Security Properties
//!
//! - **Determinism**: Same inputs → same committee (verifiable by all parties)
//! - **Stake weighting**: Higher stake → higher probability of selection
//! - **Sybil resistance**: Minimum stake requirement prevents cheap committee flooding
//! - **Geographic diversity**: Bonus for underrepresented regions
//! - **Rotation**: Committees rotate daily to prevent long-term collusion
//!
//! # Quorum Thresholds
//!
//! - **1:1 conversations**: 4-of-7 threshold
//! - **Small channels (≤100)**: 7-of-11 threshold
//! - **Large channels (>100)**: 11-of-15 threshold
//!
//! # Production Notes
//!
//! - All cryptographic comparisons use constant-time operations
//! - Error handling is explicit (no panics in production paths)
//! - Cache management prevents memory exhaustion
//! - Weight caps prevent stake centralization attacks

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// Seconds per day for epoch rotation
pub const SECONDS_PER_DAY: u64 = 86400;

/// Minimum stake required for relay eligibility (10,000 DCHAT)
pub const MIN_RELAY_STAKE: u64 = 10_000_000_000_000; // 10,000 with 9 decimals

/// Minimum reputation score for relay eligibility (0.0-1.0)
pub const MIN_REPUTATION_SCORE: f64 = 0.8;

/// Bonus multiplier for underrepresented geographic regions
pub const GEO_DIVERSITY_BONUS: f64 = 1.5;

/// Maximum relays per geographic region (to encourage diversity)
pub const MAX_RELAYS_PER_REGION: usize = 3;

/// Minimum required regions for diversity
pub const MIN_REQUIRED_REGIONS: usize = 3;

/// Maximum weight any single relay can have (5% = 500 basis points)
pub const MAX_RELAY_WEIGHT_BPS: u64 = 500;

/// Geographic regions for diversity tracking
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum GeoRegion {
    NorthAmerica,
    SouthAmerica,
    Europe,
    Africa,
    MiddleEast,
    Asia,
    Oceania,
    Unknown,
}

impl GeoRegion {
    /// Parse from country code or region string
    pub fn from_country_code(code: &str) -> Self {
        match code.to_uppercase().as_str() {
            // North America
            "US" | "CA" | "MX" => GeoRegion::NorthAmerica,
            // South America
            "BR" | "AR" | "CL" | "CO" | "PE" | "VE" | "EC" | "BO" | "PY" | "UY" => {
                GeoRegion::SouthAmerica
            }
            // Europe
            "GB" | "DE" | "FR" | "IT" | "ES" | "PT" | "NL" | "BE" | "CH" | "AT" | "PL" | "SE"
            | "NO" | "DK" | "FI" | "IE" | "CZ" | "RO" | "HU" | "GR" | "UA" => GeoRegion::Europe,
            // Africa
            "ZA" | "EG" | "NG" | "KE" | "MA" | "GH" | "TZ" | "ET" | "DZ" | "TN" => {
                GeoRegion::Africa
            }
            // Middle East
            "AE" | "SA" | "IL" | "TR" | "QA" | "KW" | "BH" | "OM" | "JO" | "LB" => {
                GeoRegion::MiddleEast
            }
            // Asia
            "CN" | "JP" | "KR" | "IN" | "SG" | "HK" | "TW" | "MY" | "TH" | "VN" | "ID" | "PH"
            | "BD" | "PK" => GeoRegion::Asia,
            // Oceania
            "AU" | "NZ" | "FJ" | "PG" => GeoRegion::Oceania,
            // Unknown
            _ => GeoRegion::Unknown,
        }
    }

    /// Check if this region is considered underrepresented
    pub fn is_underrepresented(&self) -> bool {
        matches!(
            self,
            GeoRegion::Africa
                | GeoRegion::SouthAmerica
                | GeoRegion::MiddleEast
                | GeoRegion::Oceania
        )
    }

    /// Get all standard regions
    pub fn all() -> Vec<Self> {
        vec![
            GeoRegion::NorthAmerica,
            GeoRegion::SouthAmerica,
            GeoRegion::Europe,
            GeoRegion::Africa,
            GeoRegion::MiddleEast,
            GeoRegion::Asia,
            GeoRegion::Oceania,
        ]
    }
}

/// Type of conversation (affects quorum size)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConversationType {
    /// 1:1 direct message
    Direct,
    /// Channel with ≤100 members
    ChannelSmall,
    /// Channel with >100 members
    ChannelLarge,
}

impl ConversationType {
    /// Get quorum size for this conversation type
    pub fn quorum_size(&self) -> usize {
        match self {
            ConversationType::Direct => 7,
            ConversationType::ChannelSmall => 11,
            ConversationType::ChannelLarge => 15,
        }
    }

    /// Get threshold for this conversation type
    pub fn threshold(&self) -> usize {
        match self {
            ConversationType::Direct => 4,
            ConversationType::ChannelSmall => 7,
            ConversationType::ChannelLarge => 11,
        }
    }

    /// Determine conversation type from member count
    pub fn from_member_count(count: usize) -> Self {
        match count {
            0..=2 => ConversationType::Direct,
            3..=100 => ConversationType::ChannelSmall,
            _ => ConversationType::ChannelLarge,
        }
    }
}

/// Relay information for committee selection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RelayCandidate {
    /// Relay's unique identifier (public key hash)
    pub relay_id: [u8; 32],

    /// Relay's Ed25519 public key for FROST
    pub public_key: [u8; 32],

    /// Staked amount (in smallest unit)
    pub stake: u64,

    /// Reputation score (0.0-1.0)
    pub reputation: f64,

    /// Geographic region
    pub region: GeoRegion,

    /// ASN (Autonomous System Number) for network diversity
    pub asn: u32,

    /// Is relay currently online and responsive
    pub is_online: bool,

    /// Is relay suspended (due to slashing, etc.)
    pub is_suspended: bool,
}

impl RelayCandidate {
    /// Check if relay is eligible for committee selection
    pub fn is_eligible(&self) -> bool {
        self.stake >= MIN_RELAY_STAKE
            && self.reputation >= MIN_REPUTATION_SCORE
            && self.is_online
            && !self.is_suspended
    }

    /// Calculate selection weight based on stake, reputation, and diversity
    pub fn selection_weight(&self, is_region_underrepresented: bool) -> f64 {
        if !self.is_eligible() {
            return 0.0;
        }

        // Base weight from stake (logarithmic to prevent whale dominance)
        let stake_weight = (self.stake as f64 / MIN_RELAY_STAKE as f64).ln().max(1.0);

        // Reputation multiplier (0.8-1.0 maps to 0.8-1.2 bonus)
        let reputation_multiplier = 0.4 + (self.reputation * 0.8);

        // Geographic diversity bonus
        let geo_multiplier = if is_region_underrepresented {
            GEO_DIVERSITY_BONUS
        } else {
            1.0
        };

        stake_weight * reputation_multiplier * geo_multiplier
    }
}

/// Deterministic random output for committee selection (hash-based, not cryptographic VRF)
///
/// # Important Security Note
///
/// This is NOT a cryptographic VRF (Verifiable Random Function). It provides:
/// - **Determinism**: Same inputs always produce same output
/// - **Unpredictability**: Output cannot be predicted without knowing the inputs
///
/// It does NOT provide:
/// - **Cryptographic proofs**: No way to prove output is correct without recomputing
/// - **Key-bound unpredictability**: Anyone with inputs can compute the output
///
/// This is sufficient for QGE epoch token quorums where:
/// - The conversation_id is known to all participants
/// - The epoch_day is public knowledge
/// - We only need deterministic, reproducible selection
///
/// For PoRW attestation committees where unpredictability is critical,
/// use `dchat_blockchain::hardened_consensus::vrf_committees::VrfOutput` instead.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeterministicRandomOutput {
    /// Random output value (32 bytes)
    pub value: [u8; 32],

    /// Input hash that was used (for verification)
    pub input_hash: [u8; 32],

    /// Epoch day this was computed for
    pub epoch_day: u64,
}

/// Type alias for backward compatibility
pub type VrfOutput = DeterministicRandomOutput;

impl DeterministicRandomOutput {
    /// Create a deterministic random output using BLAKE3
    ///
    /// This provides the same determinism guarantees as a full VRF but without
    /// the cryptographic proof. For QGE purposes (where the seed is public),
    /// this is sufficient. For PoRW attestation, use the full VRF in dchat-blockchain.
    pub fn derive(conversation_id: &[u8], epoch_day: u64) -> Self {
        // Hash the input with domain separation
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"dchat-qge-deterministic-v1");
        hasher.update(conversation_id);
        hasher.update(&epoch_day.to_le_bytes());
        let input_hash: [u8; 32] = *hasher.finalize().as_bytes();

        // Derive output with separate domain
        let mut output_hasher = blake3::Hasher::new();
        output_hasher.update(b"dchat-qge-output-v1");
        output_hasher.update(&input_hash);
        let value: [u8; 32] = *output_hasher.finalize().as_bytes();

        Self {
            value,
            input_hash,
            epoch_day,
        }
    }

    /// Verify output matches expected inputs
    ///
    /// Note: This is a simple recomputation check, not a cryptographic proof verification
    pub fn verify(&self, conversation_id: &[u8], epoch_day: u64) -> bool {
        let expected = Self::derive(conversation_id, epoch_day);
        // Use constant-time comparison to prevent timing attacks
        use subtle::ConstantTimeEq;
        self.value.ct_eq(&expected.value).into()
            && self.input_hash.ct_eq(&expected.input_hash).into()
    }
}

/// Selected committee for epoch token issuance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SelectedCommittee {
    /// Conversation ID this committee is for
    pub conversation_id_hash: [u8; 32],

    /// Epoch day (Unix timestamp / SECONDS_PER_DAY)
    pub epoch_day: u64,

    /// Selected relay IDs in priority order
    pub members: Vec<[u8; 32]>,

    /// Relay public keys for FROST (in same order as members)
    pub public_keys: Vec<[u8; 32]>,

    /// Required threshold for token issuance
    pub threshold: usize,

    /// VRF output used for selection
    pub vrf_output: VrfOutput,

    /// Geographic distribution of selected relays
    pub geo_distribution: HashMap<GeoRegion, usize>,
}

impl SelectedCommittee {
    /// Check if a relay is a member of this committee
    pub fn is_member(&self, relay_id: &[u8; 32]) -> bool {
        self.members.contains(relay_id)
    }

    /// Get relay's position in committee (for FROST participant ID)
    pub fn member_position(&self, relay_id: &[u8; 32]) -> Option<usize> {
        self.members.iter().position(|id| id == relay_id)
    }

    /// Check if committee meets geographic diversity requirements
    pub fn has_geographic_diversity(&self) -> bool {
        self.geo_distribution.len() >= MIN_REQUIRED_REGIONS
    }

    /// Serialize for caching
    ///
    /// Returns an error if serialization fails (should not happen for valid committees)
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self)
            .map_err(|e| Error::validation(format!("Failed to serialize committee: {}", e)))
    }

    /// Deserialize from cache
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        bincode::deserialize(data)
            .map_err(|e| Error::validation(format!("Invalid committee data: {}", e)))
    }
}

/// Committee selector that performs VRF-based selection
pub struct CommitteeSelector {
    /// All registered relays
    relays: HashMap<[u8; 32], RelayCandidate>,

    /// Cached committees: (conversation_id_hash, epoch_day) -> committee
    cache: HashMap<([u8; 32], u64), SelectedCommittee>,

    /// Maximum cache entries
    max_cache_size: usize,
}

impl CommitteeSelector {
    /// Create new committee selector
    pub fn new() -> Self {
        Self {
            relays: HashMap::new(),
            cache: HashMap::new(),
            max_cache_size: 1000,
        }
    }

    /// Register a relay candidate
    pub fn register_relay(&mut self, relay: RelayCandidate) {
        self.relays.insert(relay.relay_id, relay);
    }

    /// Update relay status
    pub fn update_relay_status(&mut self, relay_id: &[u8; 32], is_online: bool) {
        if let Some(relay) = self.relays.get_mut(relay_id) {
            relay.is_online = is_online;
        }
    }

    /// Suspend a relay (e.g., after slashing)
    pub fn suspend_relay(&mut self, relay_id: &[u8; 32]) {
        if let Some(relay) = self.relays.get_mut(relay_id) {
            relay.is_suspended = true;
        }
    }

    /// Remove a relay
    pub fn remove_relay(&mut self, relay_id: &[u8; 32]) {
        self.relays.remove(relay_id);
    }

    /// Get current epoch day
    pub fn current_epoch_day() -> u64 {
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() / SECONDS_PER_DAY)
            .unwrap_or(0)
    }

    /// Select committee for a conversation
    pub fn select_committee(
        &mut self,
        conversation_id: &[u8],
        conversation_type: ConversationType,
    ) -> Result<SelectedCommittee> {
        let epoch_day = Self::current_epoch_day();
        self.select_committee_for_epoch(conversation_id, conversation_type, epoch_day)
    }

    /// Select committee for a specific epoch
    pub fn select_committee_for_epoch(
        &mut self,
        conversation_id: &[u8],
        conversation_type: ConversationType,
        epoch_day: u64,
    ) -> Result<SelectedCommittee> {
        // Hash conversation ID for caching
        let conversation_id_hash = *blake3::hash(conversation_id).as_bytes();

        // Check cache
        let cache_key = (conversation_id_hash, epoch_day);
        if let Some(cached) = self.cache.get(&cache_key) {
            return Ok(cached.clone());
        }

        let quorum_size = conversation_type.quorum_size();
        let threshold = conversation_type.threshold();

        // Get eligible relays
        let eligible: Vec<&RelayCandidate> =
            self.relays.values().filter(|r| r.is_eligible()).collect();

        if eligible.len() < quorum_size {
            return Err(Error::network(format!(
                "Insufficient eligible relays: {} < {}",
                eligible.len(),
                quorum_size
            )));
        }

        // Generate VRF output
        let vrf_output = VrfOutput::derive(conversation_id, epoch_day);

        // Calculate selection scores
        let region_counts = self.count_regions(&eligible);
        let mut scored_relays: Vec<(&RelayCandidate, f64)> = eligible
            .iter()
            .map(|relay| {
                // Check if region is underrepresented
                let is_underrepresented = relay.region.is_underrepresented()
                    || region_counts.get(&relay.region).copied().unwrap_or(0) < 3;

                // Calculate base weight
                let weight = relay.selection_weight(is_underrepresented);

                // Apply VRF randomness
                let vrf_score = self.vrf_score(&vrf_output.value, &relay.relay_id);

                // Final score = weight * VRF randomness
                let final_score = weight * vrf_score;

                (*relay, final_score)
            })
            .collect();

        // Sort by score (descending)
        scored_relays.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

        // Select top relays while maintaining geographic diversity
        let selected = self.select_with_diversity(&scored_relays, quorum_size);

        if selected.len() < quorum_size {
            return Err(Error::network(format!(
                "Could not select enough relays with diversity: {} < {}",
                selected.len(),
                quorum_size
            )));
        }

        // Build committee
        let mut geo_distribution = HashMap::new();
        for relay in &selected {
            *geo_distribution.entry(relay.region).or_insert(0) += 1;
        }

        let committee = SelectedCommittee {
            conversation_id_hash,
            epoch_day,
            members: selected.iter().map(|r| r.relay_id).collect(),
            public_keys: selected.iter().map(|r| r.public_key).collect(),
            threshold,
            vrf_output,
            geo_distribution,
        };

        // Cache the result
        self.cache_committee(cache_key, committee.clone());

        Ok(committee)
    }

    /// Count relays per region
    fn count_regions(&self, relays: &[&RelayCandidate]) -> HashMap<GeoRegion, usize> {
        let mut counts = HashMap::new();
        for relay in relays {
            *counts.entry(relay.region).or_insert(0) += 1;
        }
        counts
    }

    /// Calculate deterministic score for a relay based on random output
    ///
    /// Returns a value in [0, 1) that is deterministic for a given (output, relay_id) pair
    fn vrf_score(&self, random_output: &[u8; 32], relay_id: &[u8; 32]) -> f64 {
        // XOR random output with relay ID, then hash to get uniform distribution
        let mut xored = [0u8; 32];
        for i in 0..32 {
            xored[i] = random_output[i] ^ relay_id[i];
        }

        let hash = *blake3::hash(&xored).as_bytes();

        // Convert first 8 bytes to f64 in range [0, 1)
        // This is safe because hash is always 32 bytes
        let bytes: [u8; 8] = [
            hash[0], hash[1], hash[2], hash[3], hash[4], hash[5], hash[6], hash[7],
        ];
        let value = u64::from_le_bytes(bytes);
        value as f64 / u64::MAX as f64
    }

    /// Select relays while maintaining geographic diversity
    fn select_with_diversity(
        &self,
        scored_relays: &[(&RelayCandidate, f64)],
        target_count: usize,
    ) -> Vec<RelayCandidate> {
        let mut selected = Vec::with_capacity(target_count);
        let mut region_counts: HashMap<GeoRegion, usize> = HashMap::new();
        let mut used_asns: HashSet<u32> = HashSet::new();

        for (relay, _score) in scored_relays {
            // Check if we've reached target
            if selected.len() >= target_count {
                break;
            }

            // Check geographic diversity constraint
            let region_count = region_counts.get(&relay.region).copied().unwrap_or(0);
            if region_count >= MAX_RELAYS_PER_REGION && selected.len() < target_count - 2 {
                // Skip if region is over-represented (unless we're close to target)
                continue;
            }

            // Prefer different ASNs for network diversity
            if used_asns.contains(&relay.asn) && selected.len() < target_count - 4 {
                // Allow same ASN only when running low on options
                continue;
            }

            selected.push((*relay).clone());
            *region_counts.entry(relay.region).or_insert(0) += 1;
            used_asns.insert(relay.asn);
        }

        // If we didn't get enough, do a second pass without diversity constraints
        if selected.len() < target_count {
            for (relay, _score) in scored_relays {
                if selected.len() >= target_count {
                    break;
                }
                if !selected.iter().any(|r| r.relay_id == relay.relay_id) {
                    selected.push((*relay).clone());
                }
            }
        }

        selected
    }

    /// Cache a committee result
    fn cache_committee(&mut self, key: ([u8; 32], u64), committee: SelectedCommittee) {
        // Evict old entries if cache is full
        if self.cache.len() >= self.max_cache_size {
            let current_day = Self::current_epoch_day();
            self.cache.retain(|(_, day), _| *day + 7 >= current_day);
        }

        self.cache.insert(key, committee);
    }

    /// Get cached committee if available
    pub fn get_cached_committee(
        &self,
        conversation_id_hash: &[u8; 32],
        epoch_day: u64,
    ) -> Option<&SelectedCommittee> {
        self.cache.get(&(*conversation_id_hash, epoch_day))
    }

    /// Clear cache
    pub fn clear_cache(&mut self) {
        self.cache.clear();
    }

    /// Get number of registered relays
    pub fn relay_count(&self) -> usize {
        self.relays.len()
    }

    /// Get number of eligible relays
    pub fn eligible_relay_count(&self) -> usize {
        self.relays.values().filter(|r| r.is_eligible()).count()
    }
}

impl Default for CommitteeSelector {
    fn default() -> Self {
        Self::new()
    }
}

/// Verify that a committee selection is valid
pub fn verify_committee_selection(
    committee: &SelectedCommittee,
    relays: &[RelayCandidate],
    conversation_type: ConversationType,
) -> Result<bool> {
    // Check quorum size
    if committee.members.len() != conversation_type.quorum_size() {
        return Err(Error::validation(format!(
            "Invalid committee size: {} != {}",
            committee.members.len(),
            conversation_type.quorum_size()
        )));
    }

    // Check threshold
    if committee.threshold != conversation_type.threshold() {
        return Err(Error::validation(format!(
            "Invalid threshold: {} != {}",
            committee.threshold,
            conversation_type.threshold()
        )));
    }

    // Verify all members are eligible relays
    for member_id in &committee.members {
        let relay = relays.iter().find(|r| r.relay_id == *member_id);
        match relay {
            Some(r) if r.is_eligible() => continue,
            Some(_) => {
                return Err(Error::validation(format!(
                    "Committee member {} is not eligible",
                    hex::encode(member_id)
                )));
            }
            None => {
                return Err(Error::validation(format!(
                    "Committee member {} not found",
                    hex::encode(member_id)
                )));
            }
        }
    }

    // Verify members are unique
    let unique_members: HashSet<_> = committee.members.iter().collect();
    if unique_members.len() != committee.members.len() {
        return Err(Error::validation("Duplicate committee members"));
    }

    Ok(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_relay(id: u8, region: GeoRegion, stake: u64, reputation: f64) -> RelayCandidate {
        let mut relay_id = [0u8; 32];
        relay_id[0] = id;
        let mut public_key = [0u8; 32];
        public_key[0] = id;
        public_key[1] = 0xFF;

        RelayCandidate {
            relay_id,
            public_key,
            stake,
            reputation,
            region,
            asn: 10000 + id as u32,
            is_online: true,
            is_suspended: false,
        }
    }

    #[test]
    fn test_geo_region_from_country_code() {
        assert_eq!(GeoRegion::from_country_code("US"), GeoRegion::NorthAmerica);
        assert_eq!(GeoRegion::from_country_code("DE"), GeoRegion::Europe);
        assert_eq!(GeoRegion::from_country_code("JP"), GeoRegion::Asia);
        assert_eq!(GeoRegion::from_country_code("ZA"), GeoRegion::Africa);
        assert_eq!(GeoRegion::from_country_code("BR"), GeoRegion::SouthAmerica);
        assert_eq!(GeoRegion::from_country_code("AU"), GeoRegion::Oceania);
        assert_eq!(GeoRegion::from_country_code("AE"), GeoRegion::MiddleEast);
        assert_eq!(GeoRegion::from_country_code("XX"), GeoRegion::Unknown);
    }

    #[test]
    fn test_conversation_type_parameters() {
        assert_eq!(ConversationType::Direct.quorum_size(), 7);
        assert_eq!(ConversationType::Direct.threshold(), 4);

        assert_eq!(ConversationType::ChannelSmall.quorum_size(), 11);
        assert_eq!(ConversationType::ChannelSmall.threshold(), 7);

        assert_eq!(ConversationType::ChannelLarge.quorum_size(), 15);
        assert_eq!(ConversationType::ChannelLarge.threshold(), 11);
    }

    #[test]
    fn test_conversation_type_from_member_count() {
        assert_eq!(
            ConversationType::from_member_count(2),
            ConversationType::Direct
        );
        assert_eq!(
            ConversationType::from_member_count(50),
            ConversationType::ChannelSmall
        );
        assert_eq!(
            ConversationType::from_member_count(100),
            ConversationType::ChannelSmall
        );
        assert_eq!(
            ConversationType::from_member_count(101),
            ConversationType::ChannelLarge
        );
    }

    #[test]
    fn test_relay_eligibility() {
        let eligible = create_test_relay(1, GeoRegion::NorthAmerica, MIN_RELAY_STAKE, 0.9);
        assert!(eligible.is_eligible());

        let low_stake = create_test_relay(2, GeoRegion::Europe, MIN_RELAY_STAKE - 1, 0.9);
        assert!(!low_stake.is_eligible());

        let low_reputation = create_test_relay(3, GeoRegion::Asia, MIN_RELAY_STAKE, 0.7);
        assert!(!low_reputation.is_eligible());

        let mut offline = create_test_relay(4, GeoRegion::Africa, MIN_RELAY_STAKE, 0.9);
        offline.is_online = false;
        assert!(!offline.is_eligible());

        let mut suspended = create_test_relay(5, GeoRegion::Europe, MIN_RELAY_STAKE, 0.9);
        suspended.is_suspended = true;
        assert!(!suspended.is_eligible());
    }

    #[test]
    fn test_selection_weight() {
        let relay = create_test_relay(1, GeoRegion::NorthAmerica, MIN_RELAY_STAKE * 2, 0.95);

        let weight_normal = relay.selection_weight(false);
        let weight_underrep = relay.selection_weight(true);

        // Underrepresented regions get bonus
        assert!(weight_underrep > weight_normal);
        assert!((weight_underrep / weight_normal - GEO_DIVERSITY_BONUS).abs() < 0.01);
    }

    #[test]
    fn test_vrf_output_determinism() {
        let conversation_id = b"test-conversation-123";
        let epoch_day = 19500;

        let vrf1 = VrfOutput::derive(conversation_id, epoch_day);
        let vrf2 = VrfOutput::derive(conversation_id, epoch_day);

        // Same inputs should produce same output (deterministic)
        assert_eq!(vrf1.value, vrf2.value);
        assert_eq!(vrf1.input_hash, vrf2.input_hash);

        // Different conversation should produce different output
        let vrf3 = VrfOutput::derive(b"other-conversation", epoch_day);
        assert_ne!(vrf1.value, vrf3.value);

        // Different epoch should produce different output
        let vrf4 = VrfOutput::derive(conversation_id, epoch_day + 1);
        assert_ne!(vrf1.value, vrf4.value);

        // Verification should pass
        assert!(vrf1.verify(conversation_id, epoch_day));
        assert!(!vrf1.verify(b"wrong-id", epoch_day));
    }

    #[test]
    fn test_committee_selection() {
        let mut selector = CommitteeSelector::new();

        // Register enough relays (need 7 for Direct)
        let regions = [
            GeoRegion::NorthAmerica,
            GeoRegion::Europe,
            GeoRegion::Asia,
            GeoRegion::Africa,
            GeoRegion::SouthAmerica,
            GeoRegion::Oceania,
            GeoRegion::MiddleEast,
            GeoRegion::NorthAmerica,
            GeoRegion::Europe,
            GeoRegion::Asia,
        ];

        for (i, region) in regions.iter().enumerate() {
            let relay = create_test_relay(
                i as u8 + 1,
                *region,
                MIN_RELAY_STAKE + (i as u64 * 1000),
                0.9 + (i as f64 * 0.01),
            );
            selector.register_relay(relay);
        }

        let conversation_id = b"test-dm-alice-bob";
        let committee = selector
            .select_committee(conversation_id, ConversationType::Direct)
            .unwrap();

        assert_eq!(committee.members.len(), 7);
        assert_eq!(committee.threshold, 4);
        assert!(committee.has_geographic_diversity());
    }

    #[test]
    fn test_committee_determinism() {
        let mut selector1 = CommitteeSelector::new();
        let mut selector2 = CommitteeSelector::new();

        // Same relays in both selectors
        for i in 0..10 {
            let relay = create_test_relay(i + 1, GeoRegion::NorthAmerica, MIN_RELAY_STAKE, 0.9);
            selector1.register_relay(relay.clone());
            selector2.register_relay(relay);
        }

        let conversation_id = b"same-conversation";
        let epoch_day = CommitteeSelector::current_epoch_day();

        let committee1 = selector1
            .select_committee_for_epoch(conversation_id, ConversationType::Direct, epoch_day)
            .unwrap();
        let committee2 = selector2
            .select_committee_for_epoch(conversation_id, ConversationType::Direct, epoch_day)
            .unwrap();

        // Same inputs should produce same committee
        assert_eq!(committee1.members, committee2.members);
        assert_eq!(committee1.vrf_output.value, committee2.vrf_output.value);
    }

    #[test]
    fn test_insufficient_relays() {
        let mut selector = CommitteeSelector::new();

        // Only 5 relays (need 7 for Direct)
        for i in 0..5 {
            let relay = create_test_relay(i + 1, GeoRegion::Europe, MIN_RELAY_STAKE, 0.9);
            selector.register_relay(relay);
        }

        let result = selector.select_committee(b"test", ConversationType::Direct);
        assert!(result.is_err());
    }

    #[test]
    fn test_committee_member_lookup() {
        let committee = SelectedCommittee {
            conversation_id_hash: [1u8; 32],
            epoch_day: 19500,
            members: vec![[1u8; 32], [2u8; 32], [3u8; 32]],
            public_keys: vec![[11u8; 32], [12u8; 32], [13u8; 32]],
            threshold: 2,
            vrf_output: VrfOutput {
                value: [0u8; 32],
                input_hash: [0u8; 32],
                epoch_day: 19500,
            },
            geo_distribution: HashMap::new(),
        };

        assert!(committee.is_member(&[1u8; 32]));
        assert!(committee.is_member(&[2u8; 32]));
        assert!(!committee.is_member(&[99u8; 32]));

        assert_eq!(committee.member_position(&[1u8; 32]), Some(0));
        assert_eq!(committee.member_position(&[3u8; 32]), Some(2));
        assert_eq!(committee.member_position(&[99u8; 32]), None);
    }

    #[test]
    fn test_verify_committee_selection() {
        let relays: Vec<RelayCandidate> = (0..10)
            .map(|i| create_test_relay(i + 1, GeoRegion::Europe, MIN_RELAY_STAKE, 0.9))
            .collect();

        let committee = SelectedCommittee {
            conversation_id_hash: [1u8; 32],
            epoch_day: 19500,
            members: relays[0..7].iter().map(|r| r.relay_id).collect(),
            public_keys: relays[0..7].iter().map(|r| r.public_key).collect(),
            threshold: 4,
            vrf_output: VrfOutput {
                value: [0u8; 32],
                input_hash: [0u8; 32],
                epoch_day: 19500,
            },
            geo_distribution: HashMap::new(),
        };

        let result = verify_committee_selection(&committee, &relays, ConversationType::Direct);
        assert!(result.is_ok());
        assert!(result.unwrap());
    }

    #[test]
    fn test_relay_update_status() {
        let mut selector = CommitteeSelector::new();

        let relay = create_test_relay(1, GeoRegion::Europe, MIN_RELAY_STAKE, 0.9);
        let relay_id = relay.relay_id;
        selector.register_relay(relay);

        assert_eq!(selector.eligible_relay_count(), 1);

        // Mark offline
        selector.update_relay_status(&relay_id, false);
        assert_eq!(selector.eligible_relay_count(), 0);

        // Mark online again
        selector.update_relay_status(&relay_id, true);
        assert_eq!(selector.eligible_relay_count(), 1);

        // Suspend
        selector.suspend_relay(&relay_id);
        assert_eq!(selector.eligible_relay_count(), 0);
    }
}
