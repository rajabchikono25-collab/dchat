use serde::{Deserialize, Serialize};
use std::fmt;
use thiserror::Error;

/// Current protocol version - updated with each protocol change
pub const CURRENT_PROTOCOL_VERSION: &str = "1.0.0";

/// Minimum acceptable protocol version - reject connections below this
pub const MIN_ACCEPTABLE_VERSION: &str = "1.0.0";

/// Maximum protocol version we support - reject future versions we don't understand
pub const MAX_ACCEPTABLE_VERSION: &str = "1.99.99";

/// Protocol version following semantic versioning (MAJOR.MINOR.PATCH)
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProtocolVersion {
    pub major: u32,
    pub minor: u32,
    pub patch: u32,
}

impl ProtocolVersion {
    /// Create a new protocol version
    pub fn new(major: u32, minor: u32, patch: u32) -> Self {
        Self {
            major,
            minor,
            patch,
        }
    }

    /// Parse a version string like "1.0.0"
    pub fn parse(s: &str) -> Result<Self, VersionError> {
        let parts: Vec<&str> = s.split('.').collect();
        if parts.len() != 3 {
            return Err(VersionError::InvalidFormat(s.to_string()));
        }

        let major = parts[0]
            .parse()
            .map_err(|_| VersionError::InvalidFormat(s.to_string()))?;
        let minor = parts[1]
            .parse()
            .map_err(|_| VersionError::InvalidFormat(s.to_string()))?;
        let patch = parts[2]
            .parse()
            .map_err(|_| VersionError::InvalidFormat(s.to_string()))?;

        Ok(Self::new(major, minor, patch))
    }

    /// Get the current protocol version
    pub fn current() -> Self {
        Self::parse(CURRENT_PROTOCOL_VERSION).expect("CURRENT_PROTOCOL_VERSION must be valid")
    }

    /// Get the minimum acceptable version
    pub fn min_acceptable() -> Self {
        Self::parse(MIN_ACCEPTABLE_VERSION).expect("MIN_ACCEPTABLE_VERSION must be valid")
    }

    /// Get the maximum acceptable version
    pub fn max_acceptable() -> Self {
        Self::parse(MAX_ACCEPTABLE_VERSION).expect("MAX_ACCEPTABLE_VERSION must be valid")
    }

    /// Check if this version is compatible with the current protocol
    pub fn is_compatible(&self) -> bool {
        let min = Self::min_acceptable();
        let max = Self::max_acceptable();
        self >= &min && self <= &max
    }

    /// Check if this version represents a protocol downgrade (security risk)
    pub fn is_downgrade(&self) -> bool {
        self < &Self::current()
    }

    /// Check if this version is a major version mismatch (breaking changes)
    pub fn is_major_mismatch(&self) -> bool {
        self.major != Self::current().major
    }

    /// Check if versions are compatible (same major version)
    pub fn is_compatible_with(&self, other: &ProtocolVersion) -> bool {
        // Major version must match for compatibility
        if self.major != other.major {
            return false;
        }

        // Both must be within acceptable range
        self.is_compatible() && other.is_compatible()
    }

    /// Convert to string representation
    pub fn to_string(&self) -> String {
        format!("{}.{}.{}", self.major, self.minor, self.patch)
    }
}

impl fmt::Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

/// Errors related to protocol versioning
#[derive(Debug, Error)]
pub enum VersionError {
    #[error("Invalid version format: {0}")]
    InvalidFormat(String),

    #[error("Version {0} is below minimum acceptable version {1}")]
    BelowMinimum(String, String),

    #[error("Version {0} is above maximum acceptable version {1}")]
    AboveMaximum(String, String),

    #[error("Version {0} represents a protocol downgrade from {1}")]
    Downgrade(String, String),

    #[error("Incompatible major version: {0} vs {1}")]
    MajorMismatch(String, String),

    #[error("Version negotiation failed: local={0}, remote={1}")]
    NegotiationFailed(String, String),
}

/// Version negotiation result
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiationResult {
    /// Both versions are compatible, connection accepted
    Compatible {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },

    /// Remote version is too old
    RemoteTooOld {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },

    /// Remote version is too new (we're outdated)
    RemoteTooNew {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },

    /// Major version mismatch (breaking changes)
    MajorMismatch {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },

    /// Potential downgrade attack detected
    DowngradeAttack {
        local: ProtocolVersion,
        remote: ProtocolVersion,
    },
}

impl NegotiationResult {
    /// Check if negotiation was successful
    pub fn is_success(&self) -> bool {
        matches!(self, NegotiationResult::Compatible { .. })
    }

    /// Get a human-readable description
    pub fn description(&self) -> String {
        match self {
            NegotiationResult::Compatible { local, remote } => {
                format!("Compatible: local={}, remote={}", local, remote)
            }
            NegotiationResult::RemoteTooOld { local, remote } => {
                format!(
                    "Remote version {} is too old (minimum: {})",
                    remote,
                    ProtocolVersion::min_acceptable()
                )
            }
            NegotiationResult::RemoteTooNew { local, remote } => {
                format!(
                    "Remote version {} is too new (maximum: {})",
                    remote,
                    ProtocolVersion::max_acceptable()
                )
            }
            NegotiationResult::MajorMismatch { local, remote } => {
                format!(
                    "Major version mismatch: local={}, remote={}",
                    local.major, remote.major
                )
            }
            NegotiationResult::DowngradeAttack { local, remote } => {
                format!(
                    "Potential downgrade attack: remote {} < local {}",
                    remote, local
                )
            }
        }
    }
}

/// Negotiate protocol version with a remote peer
pub fn negotiate_version(remote_version: &ProtocolVersion) -> NegotiationResult {
    let local = ProtocolVersion::current();

    // Check for downgrade attack (remote claims older version)
    if remote_version.is_downgrade() {
        return NegotiationResult::DowngradeAttack {
            local: local.clone(),
            remote: remote_version.clone(),
        };
    }

    // Check for major version mismatch
    if remote_version.is_major_mismatch() {
        return NegotiationResult::MajorMismatch {
            local: local.clone(),
            remote: remote_version.clone(),
        };
    }

    // Check if remote version is too old
    if remote_version < &ProtocolVersion::min_acceptable() {
        return NegotiationResult::RemoteTooOld {
            local: local.clone(),
            remote: remote_version.clone(),
        };
    }

    // Check if remote version is too new
    if remote_version > &ProtocolVersion::max_acceptable() {
        return NegotiationResult::RemoteTooNew {
            local: local.clone(),
            remote: remote_version.clone(),
        };
    }

    // Versions are compatible
    NegotiationResult::Compatible {
        local,
        remote: remote_version.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version_parsing() {
        let v = ProtocolVersion::parse("1.2.3").unwrap();
        assert_eq!(v.major, 1);
        assert_eq!(v.minor, 2);
        assert_eq!(v.patch, 3);
        assert_eq!(v.to_string(), "1.2.3");
    }

    #[test]
    fn test_invalid_version_format() {
        assert!(ProtocolVersion::parse("1.2").is_err());
        assert!(ProtocolVersion::parse("1.2.3.4").is_err());
        assert!(ProtocolVersion::parse("abc").is_err());
    }

    #[test]
    fn test_version_comparison() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(1, 0, 1);
        let v3 = ProtocolVersion::new(1, 1, 0);
        let v4 = ProtocolVersion::new(2, 0, 0);

        assert!(v1 < v2);
        assert!(v2 < v3);
        assert!(v3 < v4);
        assert!(v1 < v4);
    }

    #[test]
    fn test_current_version() {
        let current = ProtocolVersion::current();
        assert_eq!(current.to_string(), CURRENT_PROTOCOL_VERSION);
        assert!(current.is_compatible());
    }

    #[test]
    fn test_compatibility() {
        let current = ProtocolVersion::current();
        assert!(current.is_compatible());

        let old = ProtocolVersion::new(0, 9, 0);
        assert!(!old.is_compatible());

        let future = ProtocolVersion::new(99, 0, 0);
        assert!(!future.is_compatible());
    }

    #[test]
    fn test_downgrade_detection() {
        let current = ProtocolVersion::current();
        assert!(!current.is_downgrade());

        let older = ProtocolVersion::new(
            current.major.saturating_sub(1).max(0),
            current.minor,
            current.patch,
        );
        assert!(older.is_downgrade());
    }

    #[test]
    fn test_major_mismatch() {
        let v1 = ProtocolVersion::new(1, 0, 0);
        let v2 = ProtocolVersion::new(2, 0, 0);

        assert!(v2.is_major_mismatch());
        assert!(!v1.is_compatible_with(&v2));
    }

    #[test]
    fn test_version_negotiation_success() {
        let current = ProtocolVersion::current();
        let result = negotiate_version(&current);
        assert!(result.is_success());
        assert!(matches!(result, NegotiationResult::Compatible { .. }));
    }

    #[test]
    fn test_version_negotiation_downgrade_attack() {
        // Downgrade check happens FIRST in negotiate_version()
        // So version 0.9.0 will trigger DowngradeAttack before RemoteTooOld
        let old = ProtocolVersion::new(0, 9, 0);
        let result = negotiate_version(&old);
        assert!(!result.is_success());
        // Should detect as DowngradeAttack since is_downgrade() checks first
        assert!(matches!(result, NegotiationResult::DowngradeAttack { .. }));
    }

    #[test]
    fn test_version_negotiation_major_mismatch() {
        let future = ProtocolVersion::new(2, 0, 0);
        let result = negotiate_version(&future);
        assert!(!result.is_success());
        assert!(matches!(result, NegotiationResult::MajorMismatch { .. }));
    }

    #[test]
    fn test_version_negotiation_too_new() {
        let far_future = ProtocolVersion::new(99, 0, 0);
        let result = negotiate_version(&far_future);
        assert!(!result.is_success());
        // Will be caught by MajorMismatch first
        assert!(matches!(result, NegotiationResult::MajorMismatch { .. }));
    }

    #[test]
    fn test_negotiation_result_description() {
        let current = ProtocolVersion::current();
        let result = negotiate_version(&current);
        let desc = result.description();
        assert!(desc.contains("Compatible"));
    }
}
