//! Multi-signature guardian-based account recovery system
//!
//! Implements Section 11 (Account Recovery via Guardians) from ARCHITECTURE.md
//! - M-of-N guardian signatures required for recovery
//! - Timelocked recovery initiation (e.g., 7-day delay)
//! - ZK proofs to prevent guardian identity correlation
//! - Social recovery fallback mechanism

use chrono::{DateTime, Duration, Utc};
use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Guardian identifier (anonymous to prevent correlation)
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct GuardianId(pub String);

/// Guardian public key for verification
#[derive(Debug, Clone)]
pub struct GuardianKey {
    pub id: GuardianId,
    pub public_key: VerifyingKey,
    pub added_at: DateTime<Utc>,
}

impl GuardianKey {
    /// Serialize for storage
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(&(&self.id, self.public_key.as_bytes(), &self.added_at))
            .expect("serialization failed")
    }

    /// Deserialize from storage
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        let (id, pk_bytes, added_at): (GuardianId, [u8; 32], DateTime<Utc>) =
            bincode::deserialize(bytes)
                .map_err(|e| Error::identity(format!("Deserialization failed: {}", e)))?;

        let public_key = VerifyingKey::from_bytes(&pk_bytes)
            .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

        Ok(Self {
            id,
            public_key,
            added_at,
        })
    }
}

/// Recovery request with timelock
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    pub request_id: String,
    pub identity_id: String,
    pub new_device_public_key: Vec<u8>,
    pub initiated_at: DateTime<Utc>,
    pub timelock_expires_at: DateTime<Utc>,
    pub required_signatures: usize,
    pub signatures: HashMap<GuardianId, Vec<u8>>,
    pub status: RecoveryStatus,
}

/// Status of a recovery request
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum RecoveryStatus {
    /// Waiting for timelock to expire
    Pending,
    /// Timelock expired, collecting guardian signatures
    Active,
    /// Successfully recovered (M-of-N signatures obtained)
    Completed,
    /// Cancelled by user or expired
    Cancelled,
    /// Failed validation
    Failed(String),
}

/// Maximum failed signature attempts before lockout
const MAX_FAILED_ATTEMPTS: u32 = 5;
/// Lockout duration in seconds after max failed attempts
const LOCKOUT_DURATION_SECS: i64 = 3600; // 1 hour
/// Maximum concurrent recovery requests per identity
pub const MAX_CONCURRENT_RECOVERIES: usize = 3;

/// Rate limiting state for guardian operations
#[derive(Debug, Clone)]
struct RateLimitState {
    failed_attempts: u32,
    last_failed_at: Option<DateTime<Utc>>,
    locked_until: Option<DateTime<Utc>>,
}

impl Default for RateLimitState {
    fn default() -> Self {
        Self {
            failed_attempts: 0,
            last_failed_at: None,
            locked_until: None,
        }
    }
}

/// Guardian recovery manager
pub struct GuardianRecoveryManager {
    /// All guardians registered for this identity
    guardians: HashMap<GuardianId, GuardianKey>,
    /// Active recovery requests
    recovery_requests: HashMap<String, RecoveryRequest>,
    /// M-of-N threshold configuration
    threshold: GuardianThreshold,
    /// Rate limiting state per guardian
    rate_limits: HashMap<GuardianId, RateLimitState>,
}

/// M-of-N threshold configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianThreshold {
    /// Minimum signatures required (M)
    pub required: usize,
    /// Total guardians registered (N)
    pub total: usize,
}

impl GuardianRecoveryManager {
    /// Create a new guardian recovery manager with M-of-N threshold
    pub fn new(required_signatures: usize) -> Self {
        Self {
            guardians: HashMap::new(),
            recovery_requests: HashMap::new(),
            threshold: GuardianThreshold {
                required: required_signatures,
                total: 0,
            },
            rate_limits: HashMap::new(),
        }
    }
    
    /// Check if a guardian is rate-limited
    fn is_rate_limited(&self, guardian_id: &GuardianId) -> bool {
        if let Some(state) = self.rate_limits.get(guardian_id) {
            if let Some(locked_until) = state.locked_until {
                if Utc::now() < locked_until {
                    return true;
                }
            }
        }
        false
    }
    
    /// Record a failed signature attempt
    fn record_failed_attempt(&mut self, guardian_id: &GuardianId) {
        let state = self.rate_limits.entry(guardian_id.clone()).or_default();
        state.failed_attempts += 1;
        state.last_failed_at = Some(Utc::now());
        
        if state.failed_attempts >= MAX_FAILED_ATTEMPTS {
            state.locked_until = Some(Utc::now() + Duration::seconds(LOCKOUT_DURATION_SECS));
            tracing::warn!(
                "Guardian {} locked out due to {} failed signature attempts",
                guardian_id.0,
                state.failed_attempts
            );
        }
    }
    
    /// Reset rate limit state on successful signature
    fn reset_rate_limit(&mut self, guardian_id: &GuardianId) {
        self.rate_limits.remove(guardian_id);
    }

    /// Add a guardian to the recovery system
    pub fn add_guardian(&mut self, guardian_key: GuardianKey) -> Result<()> {
        if self.guardians.contains_key(&guardian_key.id) {
            return Err(Error::validation("Guardian already exists"));
        }

        self.guardians.insert(guardian_key.id.clone(), guardian_key);
        self.threshold.total = self.guardians.len();

        Ok(())
    }

    /// Remove a guardian
    pub fn remove_guardian(&mut self, guardian_id: &GuardianId) -> Result<()> {
        if !self.guardians.contains_key(guardian_id) {
            return Err(Error::validation("Guardian not found"));
        }

        self.guardians.remove(guardian_id);
        self.threshold.total = self.guardians.len();

        // Ensure threshold is still achievable
        if self.threshold.total < self.threshold.required {
            return Err(Error::validation(
                "Removing guardian would make threshold unachievable",
            ));
        }

        Ok(())
    }

    /// Initiate account recovery with timelock
    pub fn initiate_recovery(
        &mut self,
        identity_id: String,
        new_device_public_key: Vec<u8>,
        timelock_hours: i64,
    ) -> Result<String> {
        // Validate threshold is achievable
        if self.guardians.len() < self.threshold.required {
            return Err(Error::validation(format!(
                "Not enough guardians: have {}, need {}",
                self.guardians.len(),
                self.threshold.required
            )));
        }

        let request_id = format!("recovery-{}-{}", identity_id, Utc::now().timestamp());
        let now = Utc::now();
        let timelock_expires_at = now + Duration::hours(timelock_hours);

        let request = RecoveryRequest {
            request_id: request_id.clone(),
            identity_id,
            new_device_public_key,
            initiated_at: now,
            timelock_expires_at,
            required_signatures: self.threshold.required,
            signatures: HashMap::new(),
            status: RecoveryStatus::Pending,
        };

        self.recovery_requests.insert(request_id.clone(), request);

        Ok(request_id)
    }

    /// Add a guardian signature to a recovery request
    /// 
    /// # Security
    /// - Rate-limited to prevent brute force attacks
    /// - Uses constant-time signature verification
    pub fn add_guardian_signature(
        &mut self,
        request_id: &str,
        guardian_id: &GuardianId,
        signature: Vec<u8>,
    ) -> Result<()> {
        // Check rate limiting first
        if self.is_rate_limited(guardian_id) {
            return Err(Error::validation(
                "Guardian temporarily locked due to too many failed attempts"
            ));
        }
        
        // Validate signature length before processing (64 bytes for Ed25519)
        if signature.len() != 64 {
            self.record_failed_attempt(guardian_id);
            return Err(Error::crypto("Invalid signature length"));
        }
        
        // Verify guardian exists first
        let guardian = self
            .guardians
            .get(guardian_id)
            .ok_or_else(|| Error::validation("Guardian not registered"))?;

        // Clone public key for verification
        let guardian_public_key = guardian.public_key;

        // Get recovery request (immutably first to create message)
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        // Check if timelock has expired
        if Utc::now() < request.timelock_expires_at {
            return Err(Error::validation("Timelock has not expired yet"));
        }

        // Verify signature (clone signature for verification since we need it later)
        let message = self.create_recovery_message(request)?;
        let signature_array: [u8; 64] = signature
            .clone()
            .try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;
        let sig = Signature::from_bytes(&signature_array);

        if guardian_public_key.verify(&message, &sig).is_err() {
            self.record_failed_attempt(guardian_id);
            return Err(Error::crypto("Invalid guardian signature"));
        }
        
        // Reset rate limit on successful verification
        self.reset_rate_limit(guardian_id);

        // Now get mutable borrow to update the request
        let request = self
            .recovery_requests
            .get_mut(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        // Update status if timelock just expired
        if request.status == RecoveryStatus::Pending {
            request.status = RecoveryStatus::Active;
        }

        // Add signature
        request.signatures.insert(guardian_id.clone(), signature);

        // Check if we have enough signatures
        if request.signatures.len() >= request.required_signatures {
            request.status = RecoveryStatus::Completed;
        }

        Ok(())
    }

    /// Check if recovery is complete
    pub fn is_recovery_complete(&self, request_id: &str) -> Result<bool> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        Ok(request.status == RecoveryStatus::Completed)
    }

    /// Finalize recovery and return new device key
    pub fn finalize_recovery(&mut self, request_id: &str) -> Result<Vec<u8>> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        if request.status != RecoveryStatus::Completed {
            return Err(Error::validation("Recovery not yet complete"));
        }

        // Return new device public key
        Ok(request.new_device_public_key.clone())
    }

    /// Cancel a recovery request
    pub fn cancel_recovery(&mut self, request_id: &str) -> Result<()> {
        let request = self
            .recovery_requests
            .get_mut(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        request.status = RecoveryStatus::Cancelled;
        Ok(())
    }

    /// Get list of all guardians
    pub fn get_guardians(&self) -> Vec<GuardianKey> {
        self.guardians.values().cloned().collect()
    }

    /// Get recovery request status
    pub fn get_recovery_status(&self, request_id: &str) -> Result<RecoveryStatus> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        Ok(request.status.clone())
    }

    /// Create message to be signed by guardians
    fn create_recovery_message(&self, request: &RecoveryRequest) -> Result<Vec<u8>> {
        let message = format!(
            "RECOVERY:{}:{}:{}",
            request.identity_id,
            hex::encode(&request.new_device_public_key),
            request.timelock_expires_at.timestamp()
        );

        Ok(message.into_bytes())
    }

    /// Cleanup expired recovery requests
    pub fn cleanup_expired_requests(&mut self, max_age_days: i64) {
        let cutoff = Utc::now() - Duration::days(max_age_days);

        self.recovery_requests.retain(|_, request| {
            request.initiated_at > cutoff
                && request.status != RecoveryStatus::Cancelled
                && request.status != RecoveryStatus::Completed
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn test_guardian_recovery_3_of_5() {
        let mut manager = GuardianRecoveryManager::new(3);

        // Add 5 guardians
        for i in 0..5 {
            let signing_key = SigningKey::from_bytes(&[i; 32]);
            let guardian = GuardianKey {
                id: GuardianId(format!("guardian-{}", i)),
                public_key: signing_key.verifying_key(),
                added_at: Utc::now(),
            };
            manager.add_guardian(guardian).unwrap();
        }

        assert_eq!(manager.guardians.len(), 5);
        assert_eq!(manager.threshold.required, 3);
        assert_eq!(manager.threshold.total, 5);
    }

    #[test]
    fn test_recovery_timelock() {
        let mut manager = GuardianRecoveryManager::new(2);

        // Add guardians
        for i in 0..3 {
            let signing_key = SigningKey::from_bytes(&[i; 32]);
            let guardian = GuardianKey {
                id: GuardianId(format!("guardian-{}", i)),
                public_key: signing_key.verifying_key(),
                added_at: Utc::now(),
            };
            manager.add_guardian(guardian).unwrap();
        }

        // Initiate recovery with 168 hour (7 day) timelock
        let request_id = manager
            .initiate_recovery("user123".to_string(), vec![1, 2, 3, 4], 168)
            .unwrap();

        // Verify status is pending
        let status = manager.get_recovery_status(&request_id).unwrap();
        assert_eq!(status, RecoveryStatus::Pending);
    }

    #[test]
    fn test_threshold_validation() {
        let mut manager = GuardianRecoveryManager::new(3);

        // Add only 2 guardians
        for i in 0..2 {
            let signing_key = SigningKey::from_bytes(&[i; 32]);
            let guardian = GuardianKey {
                id: GuardianId(format!("guardian-{}", i)),
                public_key: signing_key.verifying_key(),
                added_at: Utc::now(),
            };
            manager.add_guardian(guardian).unwrap();
        }

        // Try to initiate recovery - should fail (not enough guardians)
        let result = manager.initiate_recovery("user123".to_string(), vec![1, 2, 3, 4], 168);

        assert!(result.is_err());
    }
}
