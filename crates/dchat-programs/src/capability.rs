//! Capability tokens for fine-grained program permissions

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};

/// Capability token ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct CapabilityId(pub [u8; 32]);

impl CapabilityId {
    /// Create new capability ID from bytes
    pub fn new(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Generate unique ID from components
    pub fn generate(
        issuer: &Pubkey,
        grantee: &Pubkey,
        scope: &CapabilityScope,
        nonce: u64,
    ) -> Self {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&issuer.0);
        hasher.update(&grantee.0);
        hasher.update(&bincode::serialize(scope).unwrap_or_default());
        hasher.update(&nonce.to_le_bytes());
        Self(hasher.finalize().into())
    }

    /// Zero capability ID
    pub const fn zero() -> Self {
        Self([0u8; 32])
    }
}

/// What the capability allows
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CapabilityScope {
    /// Program this capability applies to
    pub program_id: Pubkey,
    /// Specific accounts (empty = all owned by grantee)
    pub accounts: Vec<Pubkey>,
    /// Allowed methods (empty = all methods)
    pub methods: Vec<String>,
    /// Maximum lamports that can be transferred per invocation
    pub max_transfer_per_call: Option<u64>,
    /// Maximum total lamports that can be transferred
    pub max_transfer_total: Option<u64>,
    /// Maximum compute units per call
    pub max_compute_per_call: Option<u64>,
}

impl CapabilityScope {
    /// Create scope for a specific program
    pub fn for_program(program_id: Pubkey) -> Self {
        Self {
            program_id,
            accounts: Vec::new(),
            methods: Vec::new(),
            max_transfer_per_call: None,
            max_transfer_total: None,
            max_compute_per_call: None,
        }
    }

    /// Limit to specific accounts
    pub fn with_accounts(mut self, accounts: Vec<Pubkey>) -> Self {
        self.accounts = accounts;
        self
    }

    /// Limit to specific methods
    pub fn with_methods(mut self, methods: Vec<String>) -> Self {
        self.methods = methods;
        self
    }

    /// Set maximum transfer per call
    pub fn with_max_transfer_per_call(mut self, amount: u64) -> Self {
        self.max_transfer_per_call = Some(amount);
        self
    }

    /// Set maximum total transfer
    pub fn with_max_transfer_total(mut self, amount: u64) -> Self {
        self.max_transfer_total = Some(amount);
        self
    }

    /// Set maximum compute per call
    pub fn with_max_compute_per_call(mut self, units: u64) -> Self {
        self.max_compute_per_call = Some(units);
        self
    }

    /// Check if an action is within scope
    pub fn allows_action(&self, action: &CapabilityAction) -> bool {
        // Check program matches
        if self.program_id != action.program_id {
            return false;
        }

        // Check method if restricted
        if !self.methods.is_empty() && !self.methods.contains(&action.method) {
            return false;
        }

        // Check account if restricted
        if !self.accounts.is_empty() {
            for acc in &action.accounts {
                if !self.accounts.contains(acc) {
                    return false;
                }
            }
        }

        // Check transfer limits
        if let Some(max) = self.max_transfer_per_call {
            if action.transfer_amount > max {
                return false;
            }
        }

        true
    }
}

/// An action that requires capability check
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityAction {
    /// Target program
    pub program_id: Pubkey,
    /// Method being called
    pub method: String,
    /// Accounts being accessed
    pub accounts: Vec<Pubkey>,
    /// Amount being transferred
    pub transfer_amount: u64,
}

/// Capability token with limits and expiry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CapabilityToken {
    /// Unique identifier
    pub id: CapabilityId,
    /// Who issued this capability
    pub issuer: Pubkey,
    /// Who can use this capability
    pub grantee: Pubkey,
    /// What actions are allowed
    pub scope: CapabilityScope,
    /// When this capability expires (unix timestamp)
    pub expires_at: u64,
    /// Maximum number of uses (None = unlimited)
    pub max_uses: Option<u32>,
    /// Current use count
    pub use_count: u32,
    /// Total amount transferred using this capability
    pub total_transferred: u64,
    /// Whether this capability is revoked
    pub revoked: bool,
    /// Creation timestamp
    pub created_at: u64,
    /// Signature from issuer
    #[serde(with = "BigArray")]
    pub signature: [u8; 64],
}

impl CapabilityToken {
    /// Create a new capability token
    pub fn new(
        issuer: Pubkey,
        grantee: Pubkey,
        scope: CapabilityScope,
        expires_at: u64,
        max_uses: Option<u32>,
        nonce: u64,
    ) -> Self {
        let id = CapabilityId::generate(&issuer, &grantee, &scope, nonce);
        let created_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        Self {
            id,
            issuer,
            grantee,
            scope,
            expires_at,
            max_uses,
            use_count: 0,
            total_transferred: 0,
            revoked: false,
            created_at,
            signature: [0u8; 64], // Set after signing
        }
    }

    /// Check if the capability is valid for use
    pub fn is_valid(&self) -> ProgramResult<()> {
        // Check revocation
        if self.revoked {
            return Err(ProgramError::CapabilityRevoked);
        }

        // Check expiry
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        if now > self.expires_at {
            return Err(ProgramError::CapabilityExpired);
        }

        // Check use count
        if let Some(max) = self.max_uses {
            if self.use_count >= max {
                return Err(ProgramError::CapabilityExhausted);
            }
        }

        Ok(())
    }

    /// Check if action is allowed by this capability
    pub fn allows(&self, action: &CapabilityAction) -> ProgramResult<()> {
        // First check if capability is valid
        self.is_valid()?;

        // Check if action is within scope
        if !self.scope.allows_action(action) {
            return Err(ProgramError::CapabilityOutOfScope);
        }

        // Check total transfer limit
        if let Some(max) = self.scope.max_transfer_total {
            if self.total_transferred + action.transfer_amount > max {
                return Err(ProgramError::CapabilityTransferLimitExceeded);
            }
        }

        Ok(())
    }

    /// Record usage of this capability
    pub fn record_use(&mut self, transfer_amount: u64) -> ProgramResult<()> {
        self.is_valid()?;

        self.use_count += 1;
        self.total_transferred += transfer_amount;

        Ok(())
    }

    /// Revoke this capability
    pub fn revoke(&mut self) {
        self.revoked = true;
    }

    /// Get bytes to sign for this capability
    pub fn signing_message(&self) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(b"dchat-capability-v1:");
        msg.extend_from_slice(&self.id.0);
        msg.extend_from_slice(&self.issuer.0);
        msg.extend_from_slice(&self.grantee.0);
        msg.extend_from_slice(&self.expires_at.to_le_bytes());
        if let Some(max) = self.max_uses {
            msg.extend_from_slice(&max.to_le_bytes());
        }
        msg.extend_from_slice(&bincode::serialize(&self.scope).unwrap_or_default());
        msg
    }

    /// Set signature
    pub fn set_signature(&mut self, sig: [u8; 64]) {
        self.signature = sig;
    }
}

/// On-chain capability registry
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CapabilityRegistry {
    /// All issued capabilities by ID
    capabilities: HashMap<CapabilityId, CapabilityToken>,
    /// Capabilities by grantee
    by_grantee: HashMap<Pubkey, Vec<CapabilityId>>,
    /// Capabilities by issuer
    by_issuer: HashMap<Pubkey, Vec<CapabilityId>>,
}

impl CapabilityRegistry {
    /// Create new registry
    pub fn new() -> Self {
        Self::default()
    }

    /// Issue a new capability
    pub fn issue(&mut self, token: CapabilityToken) -> ProgramResult<CapabilityId> {
        let id = token.id;

        // Check for duplicate
        if self.capabilities.contains_key(&id) {
            return Err(ProgramError::DuplicateCapability);
        }

        // Index by grantee
        self.by_grantee.entry(token.grantee).or_default().push(id);

        // Index by issuer
        self.by_issuer.entry(token.issuer).or_default().push(id);

        self.capabilities.insert(id, token);
        Ok(id)
    }

    /// Get a capability by ID
    pub fn get(&self, id: &CapabilityId) -> Option<&CapabilityToken> {
        self.capabilities.get(id)
    }

    /// Get mutable capability by ID
    pub fn get_mut(&mut self, id: &CapabilityId) -> Option<&mut CapabilityToken> {
        self.capabilities.get_mut(id)
    }

    /// Revoke a capability
    pub fn revoke(&mut self, id: &CapabilityId, revoker: &Pubkey) -> ProgramResult<()> {
        let cap = self
            .capabilities
            .get_mut(id)
            .ok_or(ProgramError::CapabilityNotFound)?;

        // Only issuer can revoke
        if cap.issuer != *revoker {
            return Err(ProgramError::CapabilityUnauthorized);
        }

        cap.revoke();
        Ok(())
    }

    /// Get capabilities for a grantee
    pub fn for_grantee(&self, grantee: &Pubkey) -> Vec<&CapabilityToken> {
        self.by_grantee
            .get(grantee)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.capabilities.get(id))
                    .filter(|cap| cap.is_valid().is_ok())
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Get capabilities issued by an issuer
    pub fn by_issuer(&self, issuer: &Pubkey) -> Vec<&CapabilityToken> {
        self.by_issuer
            .get(issuer)
            .map(|ids| {
                ids.iter()
                    .filter_map(|id| self.capabilities.get(id))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Check if grantee has valid capability for action
    pub fn check_capability(
        &self,
        grantee: &Pubkey,
        action: &CapabilityAction,
    ) -> ProgramResult<CapabilityId> {
        for cap in self.for_grantee(grantee) {
            if cap.allows(action).is_ok() {
                return Ok(cap.id);
            }
        }
        Err(ProgramError::CapabilityNotFound)
    }

    /// Use a capability for an action
    pub fn use_capability(
        &mut self,
        id: &CapabilityId,
        grantee: &Pubkey,
        action: &CapabilityAction,
    ) -> ProgramResult<()> {
        let cap = self
            .capabilities
            .get_mut(id)
            .ok_or(ProgramError::CapabilityNotFound)?;

        // Verify grantee
        if cap.grantee != *grantee {
            return Err(ProgramError::CapabilityUnauthorized);
        }

        // Verify action is allowed
        cap.allows(action)?;

        // Record usage
        cap.record_use(action.transfer_amount)
    }

    /// Cleanup expired capabilities
    pub fn cleanup_expired(&mut self) {
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs();

        self.capabilities
            .retain(|_, cap| cap.expires_at > now || !cap.revoked);

        // Rebuild indexes
        self.by_grantee.clear();
        self.by_issuer.clear();

        for (id, cap) in &self.capabilities {
            self.by_grantee.entry(cap.grantee).or_default().push(*id);
            self.by_issuer.entry(cap.issuer).or_default().push(*id);
        }
    }
}

/// Capability program for managing capabilities on-chain
pub struct CapabilityProgram;

impl CapabilityProgram {
    /// Program ID for capability management
    pub const PROGRAM_ID: Pubkey = crate::native_programs::CAPABILITY_PROGRAM_ID;

    /// Issue instruction discriminator
    pub const ISSUE: u8 = 0;
    /// Revoke instruction discriminator
    pub const REVOKE: u8 = 1;
    /// Use instruction discriminator
    pub const USE: u8 = 2;
    /// Extend instruction discriminator
    pub const EXTEND: u8 = 3;

    /// Create issue instruction
    pub fn issue_instruction(
        issuer: Pubkey,
        grantee: Pubkey,
        scope: CapabilityScope,
        duration: Duration,
        max_uses: Option<u32>,
    ) -> crate::instruction::Instruction {
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            + duration.as_secs();

        let data = CapabilityInstruction::Issue {
            grantee,
            scope,
            expires_at,
            max_uses,
        };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![crate::account::AccountMeta::new(issuer, true)],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }

    /// Create revoke instruction
    pub fn revoke_instruction(
        issuer: Pubkey,
        capability_id: CapabilityId,
    ) -> crate::instruction::Instruction {
        let data = CapabilityInstruction::Revoke { capability_id };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![crate::account::AccountMeta::new(issuer, true)],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }
}

/// Capability instruction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CapabilityInstruction {
    /// Issue a new capability
    Issue {
        grantee: Pubkey,
        scope: CapabilityScope,
        expires_at: u64,
        max_uses: Option<u32>,
    },
    /// Revoke an existing capability
    Revoke { capability_id: CapabilityId },
    /// Record capability usage
    Use {
        capability_id: CapabilityId,
        action: CapabilityAction,
    },
    /// Extend capability expiry
    Extend {
        capability_id: CapabilityId,
        new_expires_at: u64,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_capability_creation() {
        let issuer = Pubkey::new([1u8; 32]);
        let grantee = Pubkey::new([2u8; 32]);
        let program = Pubkey::new([3u8; 32]);

        let scope = CapabilityScope::for_program(program).with_max_transfer_per_call(1000);

        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600; // 1 hour

        let token = CapabilityToken::new(issuer, grantee, scope, expires_at, Some(10), 0);

        assert_eq!(token.issuer, issuer);
        assert_eq!(token.grantee, grantee);
        assert_eq!(token.max_uses, Some(10));
        assert_eq!(token.use_count, 0);
        assert!(!token.revoked);
    }

    #[test]
    fn test_capability_scope_check() {
        let program = Pubkey::new([3u8; 32]);
        let account = Pubkey::new([4u8; 32]);

        let scope = CapabilityScope::for_program(program)
            .with_methods(vec!["transfer".to_string()])
            .with_accounts(vec![account])
            .with_max_transfer_per_call(1000);

        // Allowed action
        let action = CapabilityAction {
            program_id: program,
            method: "transfer".to_string(),
            accounts: vec![account],
            transfer_amount: 500,
        };
        assert!(scope.allows_action(&action));

        // Wrong method
        let action = CapabilityAction {
            program_id: program,
            method: "mint".to_string(),
            accounts: vec![account],
            transfer_amount: 500,
        };
        assert!(!scope.allows_action(&action));

        // Wrong account
        let action = CapabilityAction {
            program_id: program,
            method: "transfer".to_string(),
            accounts: vec![Pubkey::new([5u8; 32])],
            transfer_amount: 500,
        };
        assert!(!scope.allows_action(&action));

        // Over limit
        let action = CapabilityAction {
            program_id: program,
            method: "transfer".to_string(),
            accounts: vec![account],
            transfer_amount: 2000,
        };
        assert!(!scope.allows_action(&action));
    }

    #[test]
    fn test_capability_use_count() {
        let issuer = Pubkey::new([1u8; 32]);
        let grantee = Pubkey::new([2u8; 32]);
        let program = Pubkey::new([3u8; 32]);

        let scope = CapabilityScope::for_program(program);
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let mut token = CapabilityToken::new(issuer, grantee, scope, expires_at, Some(2), 0);

        // First use
        assert!(token.record_use(100).is_ok());
        assert_eq!(token.use_count, 1);

        // Second use
        assert!(token.record_use(100).is_ok());
        assert_eq!(token.use_count, 2);

        // Third use should fail
        assert!(matches!(
            token.record_use(100),
            Err(ProgramError::CapabilityExhausted)
        ));
    }

    #[test]
    fn test_capability_registry() {
        let mut registry = CapabilityRegistry::new();

        let issuer = Pubkey::new([1u8; 32]);
        let grantee = Pubkey::new([2u8; 32]);
        let program = Pubkey::new([3u8; 32]);

        let scope = CapabilityScope::for_program(program);
        let expires_at = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
            + 3600;

        let token = CapabilityToken::new(issuer, grantee, scope, expires_at, None, 0);

        let id = registry.issue(token).unwrap();

        // Should be findable
        assert!(registry.get(&id).is_some());
        assert_eq!(registry.for_grantee(&grantee).len(), 1);
        assert_eq!(registry.by_issuer(&issuer).len(), 1);

        // Revoke
        registry.revoke(&id, &issuer).unwrap();
        assert!(registry.get(&id).unwrap().revoked);

        // Grantee should have no valid capabilities
        assert_eq!(registry.for_grantee(&grantee).len(), 0);
    }
}
