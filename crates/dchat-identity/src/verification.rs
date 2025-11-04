//! Identity verification and badges

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{Signature, UserId};
use serde::{Deserialize, Serialize};

/// A verified badge for an identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerifiedBadge {
    pub badge_type: BadgeType,
    pub issued_at: DateTime<Utc>,
    pub expires_at: Option<DateTime<Utc>>,
    pub issuer: String,
    pub proof: VerificationProof,
}

/// Types of verification badges
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum BadgeType {
    /// Early adopter badge
    EarlyAdopter,
    /// Channel creator
    ChannelCreator,
    /// Verified identity (linked to external source)
    Verified,
    /// Governance participant
    GovernanceParticipant,
    /// Relay operator
    RelayOperator,
    /// Developer
    Developer,
    /// Custom badge
    Custom(String),
}

/// Proof of verification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VerificationProof {
    pub proof_type: ProofType,
    pub signature: Signature,
    pub metadata: std::collections::HashMap<String, String>,
}

/// Types of verification proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProofType {
    /// Self-signed proof
    SelfSigned,
    /// Signed by a trusted authority
    AuthoritySigned { authority: String },
    /// Linked to external identity (Twitter, GitHub, etc.)
    ExternalLink { platform: String, username: String },
    /// Proof of on-chain action
    OnChainProof { chain: String, tx_hash: String },
    /// Custom proof type
    Custom(String),
}

impl VerifiedBadge {
    /// Check if badge has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            Utc::now() > expires_at
        } else {
            false
        }
    }

    /// Check if badge is valid
    pub fn is_valid(&self) -> bool {
        !self.is_expired()
    }
}

/// Manages verified badges
pub struct BadgeManager {
    user_badges: std::collections::HashMap<UserId, Vec<VerifiedBadge>>,
}

impl BadgeManager {
    /// Create a new badge manager
    pub fn new() -> Self {
        Self {
            user_badges: std::collections::HashMap::new(),
        }
    }

    /// Award a badge to a user
    pub fn award_badge(&mut self, user_id: UserId, badge: VerifiedBadge) -> Result<()> {
        let badges = self.user_badges.entry(user_id).or_default();

        // Check if user already has this type of badge
        if badges
            .iter()
            .any(|b| b.badge_type == badge.badge_type && b.is_valid())
        {
            return Err(Error::identity("User already has this badge"));
        }

        badges.push(badge);
        Ok(())
    }

    /// Get badges for a user
    pub fn get_badges(&self, user_id: &UserId) -> Vec<&VerifiedBadge> {
        self.user_badges
            .get(user_id)
            .map(|badges| badges.iter().filter(|b| b.is_valid()).collect())
            .unwrap_or_default()
    }

    /// Check if user has a specific badge
    pub fn has_badge(&self, user_id: &UserId, badge_type: &BadgeType) -> bool {
        self.user_badges
            .get(user_id)
            .map(|badges| {
                badges
                    .iter()
                    .any(|b| b.badge_type == *badge_type && b.is_valid())
            })
            .unwrap_or(false)
    }

    /// Remove expired badges
    pub fn cleanup_expired_badges(&mut self) {
        for badges in self.user_badges.values_mut() {
            badges.retain(|b| b.is_valid());
        }
    }
}

impl Default for BadgeManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_badge_expiry() {
        let now = Utc::now();
        let past = now - chrono::Duration::days(1);
        let future = now + chrono::Duration::days(1);

        let expired_badge = VerifiedBadge {
            badge_type: BadgeType::EarlyAdopter,
            issued_at: past,
            expires_at: Some(past),
            issuer: "system".to_string(),
            proof: VerificationProof {
                proof_type: ProofType::SelfSigned,
                signature: Signature::new(vec![0u8; 64]),
                metadata: Default::default(),
            },
        };

        assert!(expired_badge.is_expired());
        assert!(!expired_badge.is_valid());

        let valid_badge = VerifiedBadge {
            badge_type: BadgeType::EarlyAdopter,
            issued_at: now,
            expires_at: Some(future),
            issuer: "system".to_string(),
            proof: VerificationProof {
                proof_type: ProofType::SelfSigned,
                signature: Signature::new(vec![0u8; 64]),
                metadata: Default::default(),
            },
        };

        assert!(!valid_badge.is_expired());
        assert!(valid_badge.is_valid());
    }
}
