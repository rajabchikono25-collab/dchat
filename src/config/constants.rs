// Configuration Constants for dchat Mainnet
//
// This module defines all critical constants used throughout the dchat system.
// These values control consensus, economic security, networking, and privacy features.

use std::time::Duration;

// ============================================================================
// Validator Configuration
// ============================================================================

/// Minimum number of validators required for network operation
pub const MIN_VALIDATORS: usize = 4;

/// Maximum supported validators in the network
pub const MAX_VALIDATORS: usize = 100;

/// Minimum stake required to become a validator (10,000 tokens)
pub const MIN_VALIDATOR_STAKE: u64 = 10_000_000; // Scaled by 1000 for precision

/// Validator stake lockup period after unstaking
pub const VALIDATOR_STAKE_LOCKUP_PERIOD: Duration = Duration::from_secs(7 * 24 * 3600); // 7 days

// ============================================================================
// BFT Consensus Thresholds
// ============================================================================

/// Minimum number of distinct geographic regions required
pub const MIN_REGIONS: usize = 3;

/// Maximum percentage of validators allowed from any single region
pub const MAX_REGION_PERCENTAGE: f64 = 0.40;

/// Warning threshold for region concentration (triggers alert)
pub const REGION_WARNING_THRESHOLD: f64 = 0.35;

// ============================================================================
// Relay Node Configuration
// ============================================================================

/// Minimum stake required to operate a relay node (1,000 tokens)
pub const MIN_RELAY_STAKE: u64 = 1_000_000; // Scaled by 1000 for precision

/// Minimum uptime percentage for relay reputation (90%)
pub const RELAY_UPTIME_THRESHOLD: f64 = 0.90;

/// Minimum message success rate for relay reputation (95%)
pub const RELAY_SUCCESS_RATE_THRESHOLD: f64 = 0.95;

/// Reward per successfully relayed message (0.01 tokens)
pub const REWARD_PER_MESSAGE: u64 = 10_000; // Scaled by 1000 for precision

/// Number of delivery proofs to batch before submitting to chain
pub const DELIVERY_PROOF_BATCH_SIZE: usize = 100;

/// Maximum time to wait before submitting incomplete batch
pub const DELIVERY_PROOF_BATCH_TIMEOUT: Duration = Duration::from_secs(300); // 5 minutes

// ============================================================================
// Slashing Rates
// ============================================================================

/// Slashing rate for double-signing (100% - permanent ban)
pub const SLASH_RATE_DOUBLE_SIGN: f64 = 1.0;

/// Slashing rate for submitting invalid proof (20%)
pub const SLASH_RATE_INVALID_PROOF: f64 = 0.20;

/// Slashing rate for censorship attack (10%)
pub const SLASH_RATE_CENSORSHIP: f64 = 0.10;

/// Slashing rate for low uptime (5%)
pub const SLASH_RATE_LOW_UPTIME: f64 = 0.05;

/// Slashing rate for high failure rate (5%)
pub const SLASH_RATE_HIGH_FAILURE_RATE: f64 = 0.05;

// ============================================================================
// Network Configuration
// ============================================================================

/// Target block production interval
pub const BLOCK_INTERVAL: Duration = Duration::from_secs(6);

/// Handshake timeout for establishing encrypted connection
pub const HANDSHAKE_TIMEOUT: Duration = Duration::from_secs(30);

/// Health probe interval for checking validator liveness
pub const HEALTH_PROBE_INTERVAL: Duration = Duration::from_secs(15);

/// DNS record refresh interval
pub const DNS_REFRESH_INTERVAL: Duration = Duration::from_secs(300); // 5 minutes

// ============================================================================
// Partition Detection
// ============================================================================

/// Maximum allowed block height lag before partition is suspected
pub const MAX_BLOCK_HEIGHT_LAG: u64 = 10;

/// Maximum consecutive health check failures before marking unhealthy
pub const MAX_CONSECUTIVE_FAILURES: u32 = 5;

/// Maximum acceptable latency in milliseconds
pub const MAX_ACCEPTABLE_LATENCY_MS: u64 = 5000;

// ============================================================================
// Onion Routing (Metadata Resistance)
// ============================================================================

/// Minimum number of hops in an onion circuit
pub const MIN_CIRCUIT_HOPS: usize = 3;

/// Maximum number of hops in an onion circuit
pub const MAX_CIRCUIT_HOPS: usize = 5;

/// Circuit lifetime before rotation (10 minutes)
pub const CIRCUIT_LIFETIME: Duration = Duration::from_secs(600);

// ============================================================================
// Protocol Versioning
// ============================================================================

/// Current protocol version
pub const CURRENT_PROTOCOL_VERSION: &str = "1.0.0";

/// Minimum acceptable protocol version for backward compatibility
pub const MIN_ACCEPTABLE_PROTOCOL_VERSION: &str = "1.0.0";

// ============================================================================
// Reputation and Scoring
// ============================================================================

/// Weight for uptime in relay reputation scoring
pub const REPUTATION_UPTIME_WEIGHT: f64 = 0.30;

/// Weight for latency in relay reputation scoring
pub const REPUTATION_LATENCY_WEIGHT: f64 = 0.25;

/// Weight for success rate in relay reputation scoring
pub const REPUTATION_SUCCESS_RATE_WEIGHT: f64 = 0.30;

/// Weight for geographic diversity in relay reputation scoring
pub const REPUTATION_GEO_DIVERSITY_WEIGHT: f64 = 0.15;
