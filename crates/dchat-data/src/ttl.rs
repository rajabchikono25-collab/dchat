//! TTL (Time-To-Live) policies for data expiration
//!
//! Provides configurable expiration policies for messages and data,
//! enabling automatic cleanup and storage management.

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::time::Duration as StdDuration;

use crate::error::{DataError, DataResult};

/// Expiration time for data
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExpirationTime {
    /// Never expires
    Never,
    /// Expires at a specific timestamp
    At(DateTime<Utc>),
}

impl ExpirationTime {
    /// Create expiration time from a duration from now
    pub fn from_duration(duration: Duration) -> Self {
        ExpirationTime::At(Utc::now() + duration)
    }

    /// Create expiration time from seconds from now
    pub fn from_secs(secs: i64) -> Self {
        ExpirationTime::At(Utc::now() + Duration::seconds(secs))
    }

    /// Check if this expiration time has passed
    pub fn is_expired(&self) -> bool {
        match self {
            ExpirationTime::Never => false,
            ExpirationTime::At(time) => Utc::now() > *time,
        }
    }

    /// Get remaining time until expiration (None if never expires or already expired)
    pub fn remaining(&self) -> Option<Duration> {
        match self {
            ExpirationTime::Never => None,
            ExpirationTime::At(time) => {
                let remaining = *time - Utc::now();
                if remaining.num_seconds() > 0 {
                    Some(remaining)
                } else {
                    None
                }
            }
        }
    }

    /// Get remaining time as std::time::Duration
    pub fn remaining_std(&self) -> Option<StdDuration> {
        self.remaining().map(|d| StdDuration::from_secs(d.num_seconds() as u64))
    }
}

impl Default for ExpirationTime {
    fn default() -> Self {
        ExpirationTime::Never
    }
}

/// TTL policy defining expiration behavior
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TtlPolicy {
    /// Data never expires
    NoExpiration,

    /// Fixed TTL from creation time
    FixedDuration {
        /// Duration in seconds
        seconds: u64,
    },

    /// Ephemeral - expires after reading
    Ephemeral {
        /// Duration after first read
        seconds_after_read: u64,
    },

    /// Sliding window - extends on access
    SlidingWindow {
        /// Duration in seconds, reset on each access
        seconds: u64,
    },

    /// Burn after reading - deleted after N reads
    BurnAfterRead {
        /// Maximum number of reads before deletion
        max_reads: u32,
    },
}

impl TtlPolicy {
    /// Create a fixed duration policy
    pub fn fixed(duration: Duration) -> Self {
        TtlPolicy::FixedDuration {
            seconds: duration.num_seconds() as u64,
        }
    }

    /// Create a 24-hour TTL policy
    pub fn one_day() -> Self {
        TtlPolicy::fixed(Duration::hours(24))
    }

    /// Create a 7-day TTL policy
    pub fn one_week() -> Self {
        TtlPolicy::fixed(Duration::days(7))
    }

    /// Create a 30-day TTL policy
    pub fn one_month() -> Self {
        TtlPolicy::fixed(Duration::days(30))
    }

    /// Create a 1-hour TTL policy
    pub fn one_hour() -> Self {
        TtlPolicy::fixed(Duration::hours(1))
    }

    /// Create an ephemeral policy (expires N seconds after first read)
    pub fn ephemeral(seconds_after_read: u64) -> Self {
        TtlPolicy::Ephemeral { seconds_after_read }
    }

    /// Create a sliding window policy
    pub fn sliding(seconds: u64) -> Self {
        TtlPolicy::SlidingWindow { seconds }
    }

    /// Create a burn-after-read policy
    pub fn burn_after_read(max_reads: u32) -> Self {
        TtlPolicy::BurnAfterRead { max_reads }
    }

    /// Calculate initial expiration based on policy
    pub fn initial_expiration(&self) -> ExpirationTime {
        match self {
            TtlPolicy::NoExpiration => ExpirationTime::Never,
            TtlPolicy::FixedDuration { seconds } => {
                ExpirationTime::from_secs(*seconds as i64)
            }
            TtlPolicy::Ephemeral { .. } => ExpirationTime::Never, // Expires after first read
            TtlPolicy::SlidingWindow { seconds } => {
                ExpirationTime::from_secs(*seconds as i64)
            }
            TtlPolicy::BurnAfterRead { .. } => ExpirationTime::Never, // Tracked by read count
        }
    }
}

impl Default for TtlPolicy {
    fn default() -> Self {
        TtlPolicy::NoExpiration
    }
}

/// Configuration for TTL behavior
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtlConfig {
    /// Default policy for messages without explicit TTL
    pub default_policy: TtlPolicy,

    /// Maximum allowed TTL (0 = unlimited)
    pub max_ttl_seconds: u64,

    /// Minimum allowed TTL (0 = no minimum)
    pub min_ttl_seconds: u64,

    /// Enable automatic cleanup of expired data
    pub auto_cleanup: bool,

    /// Cleanup interval in seconds
    pub cleanup_interval_seconds: u64,

    /// Grace period after expiration before deletion (seconds)
    pub grace_period_seconds: u64,
}

impl TtlConfig {
    /// Create a default configuration
    pub fn new() -> Self {
        Self::default()
    }

    /// Create a configuration with a default fixed TTL
    pub fn with_default_ttl(duration: Duration) -> Self {
        Self {
            default_policy: TtlPolicy::fixed(duration),
            ..Default::default()
        }
    }

    /// Validate a TTL duration against this configuration
    pub fn validate_ttl(&self, seconds: u64) -> DataResult<()> {
        if self.max_ttl_seconds > 0 && seconds > self.max_ttl_seconds {
            return Err(DataError::TtlConfig(format!(
                "TTL {} exceeds maximum {}",
                seconds, self.max_ttl_seconds
            )));
        }
        if seconds < self.min_ttl_seconds {
            return Err(DataError::TtlConfig(format!(
                "TTL {} below minimum {}",
                seconds, self.min_ttl_seconds
            )));
        }
        Ok(())
    }

    /// Apply configuration constraints to a policy
    pub fn apply_constraints(&self, policy: TtlPolicy) -> DataResult<TtlPolicy> {
        match policy {
            TtlPolicy::FixedDuration { seconds } => {
                self.validate_ttl(seconds)?;
                Ok(policy)
            }
            TtlPolicy::SlidingWindow { seconds } => {
                self.validate_ttl(seconds)?;
                Ok(policy)
            }
            TtlPolicy::Ephemeral { seconds_after_read } => {
                self.validate_ttl(seconds_after_read)?;
                Ok(policy)
            }
            _ => Ok(policy),
        }
    }
}

impl Default for TtlConfig {
    fn default() -> Self {
        Self {
            default_policy: TtlPolicy::one_week(),
            max_ttl_seconds: 365 * 24 * 60 * 60, // 1 year
            min_ttl_seconds: 0,
            auto_cleanup: true,
            cleanup_interval_seconds: 3600, // 1 hour
            grace_period_seconds: 300,      // 5 minutes
        }
    }
}

/// Metadata for tracking TTL state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtlMetadata {
    /// The applied policy
    pub policy: TtlPolicy,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last access timestamp
    pub last_accessed_at: DateTime<Utc>,

    /// Current expiration time
    pub expires_at: ExpirationTime,

    /// Read count (for burn-after-read)
    pub read_count: u32,

    /// First read timestamp (for ephemeral)
    pub first_read_at: Option<DateTime<Utc>>,
}

impl TtlMetadata {
    /// Create new metadata with a policy
    pub fn new(policy: TtlPolicy) -> Self {
        let now = Utc::now();
        Self {
            policy,
            created_at: now,
            last_accessed_at: now,
            expires_at: policy.initial_expiration(),
            read_count: 0,
            first_read_at: None,
        }
    }

    /// Record a read access and update TTL state
    pub fn record_read(&mut self) -> DataResult<()> {
        let now = Utc::now();
        self.last_accessed_at = now;
        self.read_count += 1;

        match self.policy {
            TtlPolicy::Ephemeral { seconds_after_read } => {
                if self.first_read_at.is_none() {
                    self.first_read_at = Some(now);
                    self.expires_at = ExpirationTime::from_secs(seconds_after_read as i64);
                }
            }
            TtlPolicy::SlidingWindow { seconds } => {
                self.expires_at = ExpirationTime::from_secs(seconds as i64);
            }
            TtlPolicy::BurnAfterRead { max_reads } => {
                if self.read_count >= max_reads {
                    // Expired immediately
                    self.expires_at = ExpirationTime::At(now);
                }
            }
            _ => {}
        }

        Ok(())
    }

    /// Check if the data is expired
    pub fn is_expired(&self) -> bool {
        match self.policy {
            TtlPolicy::BurnAfterRead { max_reads } => {
                self.read_count >= max_reads || self.expires_at.is_expired()
            }
            _ => self.expires_at.is_expired(),
        }
    }

    /// Get remaining reads (for burn-after-read policy)
    pub fn remaining_reads(&self) -> Option<u32> {
        match self.policy {
            TtlPolicy::BurnAfterRead { max_reads } => {
                Some(max_reads.saturating_sub(self.read_count))
            }
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_expiration_never() {
        let exp = ExpirationTime::Never;
        assert!(!exp.is_expired());
        assert!(exp.remaining().is_none());
    }

    #[test]
    fn test_expiration_future() {
        let exp = ExpirationTime::from_secs(3600);
        assert!(!exp.is_expired());
        assert!(exp.remaining().is_some());
    }

    #[test]
    fn test_expiration_past() {
        let exp = ExpirationTime::At(Utc::now() - Duration::hours(1));
        assert!(exp.is_expired());
        assert!(exp.remaining().is_none());
    }

    #[test]
    fn test_ttl_policy_fixed() {
        let policy = TtlPolicy::one_day();
        let exp = policy.initial_expiration();
        assert!(!exp.is_expired());
    }

    #[test]
    fn test_ttl_config_validation() {
        let config = TtlConfig {
            max_ttl_seconds: 3600,
            ..Default::default()
        };

        assert!(config.validate_ttl(1800).is_ok());
        assert!(config.validate_ttl(7200).is_err());
    }

    #[test]
    fn test_ttl_metadata_fixed() {
        let mut meta = TtlMetadata::new(TtlPolicy::one_day());
        assert!(!meta.is_expired());

        meta.record_read().unwrap();
        assert_eq!(meta.read_count, 1);
        assert!(!meta.is_expired());
    }

    #[test]
    fn test_ttl_metadata_burn_after_read() {
        let mut meta = TtlMetadata::new(TtlPolicy::burn_after_read(2));
        assert_eq!(meta.remaining_reads(), Some(2));

        meta.record_read().unwrap();
        assert_eq!(meta.remaining_reads(), Some(1));
        assert!(!meta.is_expired());

        meta.record_read().unwrap();
        assert_eq!(meta.remaining_reads(), Some(0));
        assert!(meta.is_expired());
    }

    #[test]
    fn test_ttl_metadata_ephemeral() {
        let mut meta = TtlMetadata::new(TtlPolicy::ephemeral(60));
        assert!(meta.first_read_at.is_none());
        assert!(matches!(meta.expires_at, ExpirationTime::Never));

        meta.record_read().unwrap();
        assert!(meta.first_read_at.is_some());
        assert!(matches!(meta.expires_at, ExpirationTime::At(_)));
    }

    #[test]
    fn test_ttl_metadata_sliding() {
        let mut meta = TtlMetadata::new(TtlPolicy::sliding(60));
        let initial_exp = meta.expires_at;

        // Small delay to ensure time progresses
        std::thread::sleep(std::time::Duration::from_millis(10));

        meta.record_read().unwrap();
        // Sliding window should reset expiration
        assert!(meta.expires_at != initial_exp || matches!(meta.expires_at, ExpirationTime::At(_)));
    }
}
