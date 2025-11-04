//! Guardian-based account recovery system

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{PublicKey, UserId};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// A guardian who can help recover an account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Guardian {
    pub guardian_id: UserId,
    pub public_key: PublicKey,
    pub added_at: DateTime<Utc>,
    pub trusted: bool,
}

/// A recovery request requiring guardian approval
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecoveryRequest {
    pub request_id: String,
    pub user_id: UserId,
    pub new_public_key: PublicKey,
    pub created_at: DateTime<Utc>,
    pub timelock_until: DateTime<Utc>,
    pub approvals: Vec<GuardianApproval>,
    pub required_approvals: usize,
    pub status: RecoveryStatus,
}

/// Guardian approval for a recovery request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuardianApproval {
    pub guardian_id: UserId,
    pub approved: bool,
    pub signed_at: DateTime<Utc>,
    pub signature: Vec<u8>,
}

/// Status of a recovery request
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum RecoveryStatus {
    Pending,
    TimelockActive,
    Approved,
    Rejected,
    Cancelled,
    Executed,
}

/// Manages guardians and account recovery
pub struct GuardianManager {
    user_guardians: HashMap<UserId, Vec<Guardian>>,
    recovery_requests: HashMap<String, RecoveryRequest>,
    timelock_hours: i64,
}

impl GuardianManager {
    /// Create a new guardian manager
    pub fn new(timelock_hours: i64) -> Self {
        Self {
            user_guardians: HashMap::new(),
            recovery_requests: HashMap::new(),
            timelock_hours,
        }
    }

    /// Add a guardian for a user
    pub fn add_guardian(&mut self, user_id: UserId, guardian: Guardian) -> Result<()> {
        let guardians = self.user_guardians.entry(user_id).or_default();

        // Check if guardian already exists
        if guardians
            .iter()
            .any(|g| g.guardian_id == guardian.guardian_id)
        {
            return Err(Error::identity("Guardian already added"));
        }

        guardians.push(guardian);
        Ok(())
    }

    /// Get guardians for a user
    pub fn get_guardians(&self, user_id: &UserId) -> Vec<&Guardian> {
        self.user_guardians
            .get(user_id)
            .map(|guardians| guardians.iter().collect())
            .unwrap_or_default()
    }

    /// Remove a guardian
    pub fn remove_guardian(&mut self, user_id: &UserId, guardian_id: &UserId) -> Result<()> {
        let guardians = self
            .user_guardians
            .get_mut(user_id)
            .ok_or_else(|| Error::identity("User has no guardians"))?;

        let initial_len = guardians.len();
        guardians.retain(|g| g.guardian_id != *guardian_id);

        if guardians.len() == initial_len {
            return Err(Error::identity("Guardian not found"));
        }

        Ok(())
    }

    /// Initiate a recovery request
    pub fn initiate_recovery(
        &mut self,
        request_id: String,
        user_id: UserId,
        new_public_key: PublicKey,
    ) -> Result<()> {
        // Check if user has guardians
        let guardians = self
            .user_guardians
            .get(&user_id)
            .ok_or_else(|| Error::identity("User has no guardians"))?;

        if guardians.is_empty() {
            return Err(Error::identity("User has no guardians"));
        }

        // Calculate required approvals (e.g., 2/3 majority)
        let required_approvals = (guardians.len() * 2).div_ceil(3); // Ceiling division

        let now = Utc::now();
        let timelock_until = now + chrono::Duration::hours(self.timelock_hours);

        let request = RecoveryRequest {
            request_id: request_id.clone(),
            user_id,
            new_public_key,
            created_at: now,
            timelock_until,
            approvals: Vec::new(),
            required_approvals,
            status: RecoveryStatus::Pending,
        };

        self.recovery_requests.insert(request_id, request);
        Ok(())
    }

    /// Guardian approves a recovery request
    pub fn approve_recovery(
        &mut self,
        request_id: &str,
        guardian_id: UserId,
        signature: Vec<u8>,
    ) -> Result<()> {
        let request = self
            .recovery_requests
            .get_mut(request_id)
            .ok_or_else(|| Error::identity("Recovery request not found"))?;

        if request.status != RecoveryStatus::Pending
            && request.status != RecoveryStatus::TimelockActive
        {
            return Err(Error::identity("Recovery request is not pending"));
        }

        // Check if guardian is valid for this user
        let guardians = self
            .user_guardians
            .get(&request.user_id)
            .ok_or_else(|| Error::identity("User has no guardians"))?;

        if !guardians.iter().any(|g| g.guardian_id == guardian_id) {
            return Err(Error::identity("Not a guardian for this user"));
        }

        // Check if guardian already approved
        if request
            .approvals
            .iter()
            .any(|a| a.guardian_id == guardian_id)
        {
            return Err(Error::identity("Guardian already approved"));
        }

        // Add approval
        let approval = GuardianApproval {
            guardian_id,
            approved: true,
            signed_at: Utc::now(),
            signature,
        };

        request.approvals.push(approval);

        // Check if enough approvals
        let approval_count = request.approvals.iter().filter(|a| a.approved).count();
        if approval_count >= request.required_approvals {
            request.status = RecoveryStatus::TimelockActive;
        }

        Ok(())
    }

    /// Check if recovery request can be executed
    pub fn can_execute_recovery(&self, request_id: &str) -> Result<bool> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::identity("Recovery request not found"))?;

        if request.status != RecoveryStatus::TimelockActive {
            return Ok(false);
        }

        // Check if timelock has expired
        Ok(Utc::now() >= request.timelock_until)
    }

    /// Execute a recovery request
    pub fn execute_recovery(&mut self, request_id: &str) -> Result<RecoveryRequest> {
        if !self.can_execute_recovery(request_id)? {
            return Err(Error::identity("Recovery request cannot be executed yet"));
        }

        let mut request = self
            .recovery_requests
            .remove(request_id)
            .ok_or_else(|| Error::identity("Recovery request not found"))?;

        request.status = RecoveryStatus::Executed;
        Ok(request)
    }

    /// Cancel a recovery request
    pub fn cancel_recovery(&mut self, request_id: &str) -> Result<()> {
        let request = self
            .recovery_requests
            .get_mut(request_id)
            .ok_or_else(|| Error::identity("Recovery request not found"))?;

        request.status = RecoveryStatus::Cancelled;
        Ok(())
    }

    /// Get recovery request
    pub fn get_recovery_request(&self, request_id: &str) -> Option<&RecoveryRequest> {
        self.recovery_requests.get(request_id)
    }

    /// Get pending recovery requests for a user
    pub fn get_pending_recoveries(&self, user_id: &UserId) -> Vec<&RecoveryRequest> {
        self.recovery_requests
            .values()
            .filter(|r| r.user_id == *user_id && r.status == RecoveryStatus::Pending)
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    #[test]
    fn test_guardian_management() {
        let mut manager = GuardianManager::new(24);
        let user_id = UserId::new();
        let guardian_id = UserId::new();

        let keypair = KeyPair::generate();
        let guardian = Guardian {
            guardian_id: guardian_id.clone(),
            public_key: keypair.public_key().to_core_public_key(),
            added_at: Utc::now(),
            trusted: true,
        };

        // Add guardian
        assert!(manager.add_guardian(user_id.clone(), guardian).is_ok());

        // Get guardians
        let guardians = manager.get_guardians(&user_id);
        assert_eq!(guardians.len(), 1);

        // Remove guardian
        assert!(manager.remove_guardian(&user_id, &guardian_id).is_ok());
        assert_eq!(manager.get_guardians(&user_id).len(), 0);
    }

    #[test]
    fn test_recovery_process() {
        let mut manager = GuardianManager::new(24);
        let user_id = UserId::new();

        // Add 3 guardians
        for _i in 0..3 {
            let keypair = KeyPair::generate();
            let guardian = Guardian {
                guardian_id: UserId::new(),
                public_key: keypair.public_key().to_core_public_key(),
                added_at: Utc::now(),
                trusted: true,
            };
            manager.add_guardian(user_id.clone(), guardian).unwrap();
        }

        // Initiate recovery
        let new_keypair = KeyPair::generate();
        let new_pubkey = new_keypair.public_key().to_core_public_key();

        assert!(manager
            .initiate_recovery("recovery1".to_string(), user_id.clone(), new_pubkey,)
            .is_ok());

        // Get recovery request
        let request = manager.get_recovery_request("recovery1");
        assert!(request.is_some());
        assert_eq!(request.unwrap().required_approvals, 2); // 2/3 of 3
    }
}
