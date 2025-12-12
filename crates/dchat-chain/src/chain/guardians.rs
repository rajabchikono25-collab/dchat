//! On-chain guardian registration and recovery verification
//!
//! Integrates with dchat-identity::guardian_recovery to provide blockchain-based
//! guardian management with timelock enforcement and ZK proof verification.
//!
//! Features:
//! - On-chain guardian registration with stake requirements
//! - Timelock verification using block timestamps
//! - Recovery request tracking and signature validation
//! - Integration with dchat-privacy for ZK guardian anonymity proofs
//! - Nullifier storage to prevent ZK proof reuse

use blake3::Hasher;
use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};

/// On-chain guardian registration transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegisterGuardianTx {
    /// Identity being protected
    pub identity_id: String,
    /// Guardian's public key (for signature verification)
    pub guardian_public_key: Vec<u8>,
    /// Guardian identifier (anonymous)
    pub guardian_id: String,
    /// Stake amount locked by guardian (prevents Sybil attacks)
    pub stake_amount: u64,
    /// Transaction timestamp
    pub timestamp: DateTime<Utc>,
    /// Optional ZK proof of guardian relationship (prevents correlation)
    pub zk_proof: Option<Vec<u8>>,
}

impl RegisterGuardianTx {
    /// Create a new guardian registration transaction
    pub fn new(
        identity_id: String,
        guardian_public_key: Vec<u8>,
        guardian_id: String,
        stake_amount: u64,
    ) -> Self {
        Self {
            identity_id,
            guardian_public_key,
            guardian_id,
            stake_amount,
            timestamp: Utc::now(),
            zk_proof: None,
        }
    }

    /// Add ZK proof for anonymous guardian registration
    pub fn with_zk_proof(mut self, proof: Vec<u8>) -> Self {
        self.zk_proof = Some(proof);
        self
    }

    /// Serialize for blockchain submission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::chain(format!("Serialization failed: {}", e)))
    }

    /// Deserialize from blockchain
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| Error::chain(format!("Deserialization failed: {}", e)))
    }
}

/// On-chain recovery request transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InitiateRecoveryTx {
    /// Request identifier
    pub request_id: String,
    /// Identity being recovered
    pub identity_id: String,
    /// New device public key to be authorized
    pub new_device_public_key: Vec<u8>,
    /// Block height when timelock expires
    pub timelock_expires_at_block: u64,
    /// Block timestamp when timelock expires
    pub timelock_expires_at_timestamp: i64,
    /// Required guardian signatures (M-of-N)
    pub required_signatures: usize,
    /// Transaction initiator signature
    pub initiator_signature: Vec<u8>,
}

impl InitiateRecoveryTx {
    /// Create a new recovery initiation transaction
    pub fn new(
        request_id: String,
        identity_id: String,
        new_device_public_key: Vec<u8>,
        timelock_blocks: u64,
        current_block: u64,
        required_signatures: usize,
    ) -> Self {
        Self {
            request_id,
            identity_id,
            new_device_public_key,
            timelock_expires_at_block: current_block + timelock_blocks,
            timelock_expires_at_timestamp: Utc::now().timestamp() + (timelock_blocks as i64 * 12), // Assume 12s block time
            required_signatures,
            initiator_signature: Vec::new(),
        }
    }

    /// Add initiator signature (proves request legitimacy)
    pub fn with_signature(mut self, signature: Vec<u8>) -> Self {
        self.initiator_signature = signature;
        self
    }

    /// Serialize for blockchain submission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::chain(format!("Serialization failed: {}", e)))
    }

    /// Deserialize from blockchain
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| Error::chain(format!("Deserialization failed: {}", e)))
    }
}

/// Guardian signature submission transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubmitGuardianSignatureTx {
    /// Recovery request identifier
    pub request_id: String,
    /// Guardian identifier
    pub guardian_id: String,
    /// Guardian's signature on recovery message
    pub signature: Vec<u8>,
    /// Block height when signature was submitted
    pub submitted_at_block: u64,
}

impl SubmitGuardianSignatureTx {
    /// Create a new guardian signature transaction
    pub fn new(
        request_id: String,
        guardian_id: String,
        signature: Vec<u8>,
        current_block: u64,
    ) -> Self {
        Self {
            request_id,
            guardian_id,
            signature,
            submitted_at_block: current_block,
        }
    }

    /// Serialize for blockchain submission
    pub fn to_bytes(&self) -> Result<Vec<u8>> {
        bincode::serialize(self).map_err(|e| Error::chain(format!("Serialization failed: {}", e)))
    }

    /// Deserialize from blockchain
    pub fn from_bytes(bytes: &[u8]) -> Result<Self> {
        bincode::deserialize(bytes)
            .map_err(|e| Error::chain(format!("Deserialization failed: {}", e)))
    }
}

/// On-chain guardian state tracker
pub struct GuardianChainState {
    /// Guardians registered for each identity
    guardians: HashMap<String, Vec<GuardianRegistration>>,
    /// Active recovery requests
    recovery_requests: HashMap<String, RecoveryRequestState>,
    /// Minimum stake required to become a guardian
    min_guardian_stake: u64,
    /// Current block height
    current_block_height: u64,
    /// Nullifier storage - prevents ZK proof reuse
    /// Key: nullifier hash, Value: (identity_id, block_height, timestamp)
    nullifiers: HashMap<[u8; 32], NullifierRecord>,
    /// Recovery nullifiers - prevents recovery request replay
    recovery_nullifiers: HashSet<[u8; 32]>,
}

/// Record of a used nullifier stored on-chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NullifierRecord {
    /// Identity this nullifier is associated with
    pub identity_id: String,
    /// Block height when nullifier was recorded
    pub block_height: u64,
    /// Timestamp when nullifier was recorded
    pub timestamp: i64,
    /// Type of nullifier
    pub nullifier_type: NullifierType,
}

/// Types of nullifiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum NullifierType {
    /// Guardian registration proof nullifier
    GuardianRegistration,
    /// Recovery initiation nullifier
    RecoveryInitiation,
    /// Guardian signature nullifier
    GuardianSignature,
}

/// Guardian registration stored on-chain
#[derive(Debug, Clone)]
pub struct GuardianRegistration {
    /// Guardian's unique identifier
    pub guardian_id: String,
    /// Guardian's public key for signature verification
    pub public_key: VerifyingKey,
    /// Amount staked by this guardian
    pub stake_amount: u64,
    /// Block height when guardian was registered
    pub registered_at_block: u64,
    /// Whether guardian is currently active
    pub active: bool,
}

/// Recovery request state on-chain
#[derive(Debug, Clone)]
pub struct RecoveryRequestState {
    /// Identity being recovered
    pub identity_id: String,
    /// New device's public key
    pub new_device_public_key: Vec<u8>,
    /// Block height when timelock expires
    pub timelock_expires_at_block: u64,
    /// Number of signatures required
    pub required_signatures: usize,
    /// Collected guardian signatures
    pub signatures: HashMap<String, Vec<u8>>,
    /// Current status of recovery
    pub status: OnChainRecoveryStatus,
    /// Nullifier to prevent duplicate recovery requests
    pub nullifier: [u8; 32],
}

/// On-chain recovery status
#[derive(Debug, Clone, PartialEq)]
pub enum OnChainRecoveryStatus {
    /// Waiting for timelock
    Pending,
    /// Collecting signatures
    Active,
    /// Successfully completed
    Completed,
    /// Cancelled by user or guardians
    Cancelled,
}

impl GuardianChainState {
    /// Create a new guardian chain state tracker
    pub fn new(min_guardian_stake: u64) -> Self {
        Self {
            guardians: HashMap::new(),
            recovery_requests: HashMap::new(),
            min_guardian_stake,
            current_block_height: 0,
            nullifiers: HashMap::new(),
            recovery_nullifiers: HashSet::new(),
        }
    }

    /// Update current block height
    pub fn set_block_height(&mut self, height: u64) {
        self.current_block_height = height;
    }

    /// Compute nullifier hash from proof data
    ///
    /// The nullifier is derived from:
    /// - The ZK proof's unique identifier (commitment or serial number)
    /// - The identity being protected
    /// - Additional context to prevent cross-purpose reuse
    fn compute_nullifier(&self, proof: &[u8], identity_id: &str, context: &str) -> [u8; 32] {
        let mut hasher = Hasher::new();
        hasher.update(b"dchat-guardian-nullifier-v1");
        hasher.update(context.as_bytes());
        hasher.update(identity_id.as_bytes());
        hasher.update(proof);
        *hasher.finalize().as_bytes()
    }

    /// Check if a nullifier has been used
    pub fn is_nullifier_used(&self, nullifier: &[u8; 32]) -> bool {
        self.nullifiers.contains_key(nullifier)
    }

    /// Record a nullifier as used
    ///
    /// Returns error if nullifier was already used (replay attack prevention)
    pub fn record_nullifier(
        &mut self,
        nullifier: [u8; 32],
        identity_id: String,
        nullifier_type: NullifierType,
    ) -> Result<()> {
        if self.nullifiers.contains_key(&nullifier) {
            let existing = &self.nullifiers[&nullifier];
            return Err(Error::validation(format!(
                "Nullifier already used at block {} for identity {} (type: {:?})",
                existing.block_height, existing.identity_id, existing.nullifier_type
            )));
        }

        let record = NullifierRecord {
            identity_id,
            block_height: self.current_block_height,
            timestamp: Utc::now().timestamp(),
            nullifier_type,
        };

        self.nullifiers.insert(nullifier, record);

        tracing::info!(
            "Recorded nullifier {} at block {} (type: {:?})",
            hex::encode(&nullifier[..8]),
            self.current_block_height,
            nullifier_type
        );

        Ok(())
    }

    /// Get nullifier record if it exists
    pub fn get_nullifier_record(&self, nullifier: &[u8; 32]) -> Option<&NullifierRecord> {
        self.nullifiers.get(nullifier)
    }

    /// Get all nullifiers for an identity
    pub fn get_nullifiers_for_identity(
        &self,
        identity_id: &str,
    ) -> Vec<([u8; 32], &NullifierRecord)> {
        self.nullifiers
            .iter()
            .filter(|(_, record)| record.identity_id == identity_id)
            .map(|(hash, record)| (*hash, record))
            .collect()
    }

    /// Prune old nullifiers (optional cleanup for very old entries)
    ///
    /// In production, nullifiers should be kept indefinitely to prevent replay.
    /// This method is for testing or if a time-limited nullifier policy is desired.
    pub fn prune_nullifiers_before_block(&mut self, block_height: u64) -> usize {
        let before_count = self.nullifiers.len();
        self.nullifiers
            .retain(|_, record| record.block_height >= block_height);
        let removed = before_count - self.nullifiers.len();

        if removed > 0 {
            tracing::warn!(
                "Pruned {} nullifiers before block {} (remaining: {})",
                removed,
                block_height,
                self.nullifiers.len()
            );
        }

        removed
    }

    /// Export nullifiers for persistence/sync
    pub fn export_nullifiers(&self) -> Vec<([u8; 32], NullifierRecord)> {
        self.nullifiers
            .iter()
            .map(|(k, v)| (*k, v.clone()))
            .collect()
    }

    /// Import nullifiers from persistence/sync
    pub fn import_nullifiers(&mut self, nullifiers: Vec<([u8; 32], NullifierRecord)>) {
        for (hash, record) in nullifiers {
            self.nullifiers.entry(hash).or_insert(record);
        }
    }

    /// Process guardian registration transaction
    pub fn register_guardian(&mut self, tx: RegisterGuardianTx, block_height: u64) -> Result<()> {
        // Validate stake amount
        if tx.stake_amount < self.min_guardian_stake {
            return Err(Error::validation(format!(
                "Insufficient guardian stake: {} < {}",
                tx.stake_amount, self.min_guardian_stake
            )));
        }

        // Parse public key
        let public_key_bytes: [u8; 32] = tx
            .guardian_public_key
            .clone()
            .try_into()
            .map_err(|_| Error::crypto("Invalid public key length"))?;

        let public_key = VerifyingKey::from_bytes(&public_key_bytes)
            .map_err(|e| Error::crypto(format!("Invalid public key: {}", e)))?;

        // Verify ZK proof if provided and record nullifier
        if let Some(zk_proof) = &tx.zk_proof {
            // Compute nullifier for this proof
            let nullifier =
                self.compute_nullifier(zk_proof, &tx.identity_id, "guardian-registration");

            // Check if nullifier was already used (prevents proof reuse)
            if self.is_nullifier_used(&nullifier) {
                let record = self.get_nullifier_record(&nullifier).unwrap();
                return Err(Error::validation(format!(
                    "ZK proof already used at block {} - potential replay attack",
                    record.block_height
                )));
            }

            // Verify the ZK proof cryptographically
            self.verify_guardian_zk_proof(zk_proof, &tx.identity_id, &tx.guardian_id)?;

            // Record nullifier on-chain to prevent future reuse
            self.record_nullifier(
                nullifier,
                tx.identity_id.clone(),
                NullifierType::GuardianRegistration,
            )?;
        }

        // Create registration
        let registration = GuardianRegistration {
            guardian_id: tx.guardian_id.clone(),
            public_key,
            stake_amount: tx.stake_amount,
            registered_at_block: block_height,
            active: true,
        };

        // Add to guardians map
        self.guardians
            .entry(tx.identity_id)
            .or_insert_with(Vec::new)
            .push(registration);

        Ok(())
    }

    /// Process recovery initiation transaction
    pub fn initiate_recovery(&mut self, tx: InitiateRecoveryTx, _block_height: u64) -> Result<()> {
        // Compute recovery nullifier to prevent duplicate requests
        let recovery_nullifier = {
            let mut hasher = Hasher::new();
            hasher.update(b"dchat-recovery-nullifier-v1");
            hasher.update(tx.identity_id.as_bytes());
            hasher.update(&tx.new_device_public_key);
            hasher.update(&tx.timelock_expires_at_block.to_le_bytes());
            *hasher.finalize().as_bytes()
        };

        // Check if this exact recovery was already initiated
        if self.recovery_nullifiers.contains(&recovery_nullifier) {
            return Err(Error::validation(
                "Duplicate recovery request - this exact recovery was already initiated",
            ));
        }

        // Verify identity has enough guardians
        let guardians = self
            .guardians
            .get(&tx.identity_id)
            .ok_or_else(|| Error::validation("No guardians registered for identity"))?;

        let active_guardians = guardians.iter().filter(|g| g.active).count();

        if active_guardians < tx.required_signatures {
            return Err(Error::validation(format!(
                "Not enough active guardians: {} < {}",
                active_guardians, tx.required_signatures
            )));
        }

        // Record recovery nullifier
        self.recovery_nullifiers.insert(recovery_nullifier);

        tracing::info!(
            "Recovery initiated for identity {} with nullifier {}",
            tx.identity_id,
            hex::encode(&recovery_nullifier[..8])
        );

        // Create recovery request
        let request_state = RecoveryRequestState {
            identity_id: tx.identity_id,
            new_device_public_key: tx.new_device_public_key,
            timelock_expires_at_block: tx.timelock_expires_at_block,
            required_signatures: tx.required_signatures,
            signatures: HashMap::new(),
            status: OnChainRecoveryStatus::Pending,
            nullifier: recovery_nullifier,
        };

        self.recovery_requests.insert(tx.request_id, request_state);

        Ok(())
    }

    /// Process guardian signature submission
    pub fn submit_guardian_signature(&mut self, tx: SubmitGuardianSignatureTx) -> Result<()> {
        // Compute signature nullifier to prevent double-signing
        let sig_nullifier = {
            let mut hasher = Hasher::new();
            hasher.update(b"dchat-guardian-sig-nullifier-v1");
            hasher.update(tx.request_id.as_bytes());
            hasher.update(tx.guardian_id.as_bytes());
            *hasher.finalize().as_bytes()
        };

        // Check if this guardian already signed this request
        if self.is_nullifier_used(&sig_nullifier) {
            return Err(Error::validation(format!(
                "Guardian {} already submitted signature for request {}",
                tx.guardian_id, tx.request_id
            )));
        }

        // Get recovery request data for validation first (immutable borrow)
        let (timelock_block, current_status, identity_id, new_device_key, required_sigs) = {
            let request = self
                .recovery_requests
                .get(&tx.request_id)
                .ok_or_else(|| Error::validation("Recovery request not found"))?;

            (
                request.timelock_expires_at_block,
                request.status.clone(),
                request.identity_id.clone(),
                request.new_device_public_key.clone(),
                request.required_signatures,
            )
        };

        // Check timelock
        if self.current_block_height < timelock_block {
            return Err(Error::validation(format!(
                "Timelock not expired: current block {} < expires at {}",
                self.current_block_height, timelock_block
            )));
        }

        // Get guardian to verify signature
        let guardians = self
            .guardians
            .get(&identity_id)
            .ok_or_else(|| Error::validation("No guardians found"))?;

        let guardian = guardians
            .iter()
            .find(|g| g.guardian_id == tx.guardian_id && g.active)
            .ok_or_else(|| Error::validation("Guardian not found or inactive"))?;

        // Create recovery message for signature verification
        let message = {
            let msg = format!(
                "RECOVERY:{}:{}:{}",
                identity_id,
                hex::encode(&new_device_key),
                timelock_block
            );
            msg.into_bytes()
        };

        let signature_bytes: [u8; 64] = tx
            .signature
            .clone()
            .try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;

        let signature = Signature::from_bytes(&signature_bytes);

        guardian
            .public_key
            .verify(&message, &signature)
            .map_err(|_| Error::crypto("Invalid guardian signature"))?;

        // Now we can mutably borrow and update
        let request = self.recovery_requests.get_mut(&tx.request_id).unwrap(); // Safe - we validated existence above

        // Update status if just became active
        if current_status == OnChainRecoveryStatus::Pending {
            request.status = OnChainRecoveryStatus::Active;
        }

        // Add signature
        request
            .signatures
            .insert(tx.guardian_id.clone(), tx.signature);

        // Check if complete
        if request.signatures.len() >= required_sigs {
            request.status = OnChainRecoveryStatus::Completed;
            tracing::info!(
                "Recovery {} completed with {}/{} signatures",
                tx.request_id,
                request.signatures.len(),
                request.required_signatures
            );
        }

        // Now record the nullifier (self is no longer borrowing request)
        // We need to record this to the nullifiers map
        let record = NullifierRecord {
            identity_id,
            block_height: self.current_block_height,
            timestamp: Utc::now().timestamp(),
            nullifier_type: NullifierType::GuardianSignature,
        };
        self.nullifiers.insert(sig_nullifier, record);

        Ok(())
    }

    /// Check if recovery is complete
    pub fn is_recovery_complete(&self, request_id: &str) -> Result<bool> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        Ok(request.status == OnChainRecoveryStatus::Completed)
    }

    /// Get new device key from completed recovery
    pub fn get_recovered_device_key(&self, request_id: &str) -> Result<Vec<u8>> {
        let request = self
            .recovery_requests
            .get(request_id)
            .ok_or_else(|| Error::validation("Recovery request not found"))?;

        if request.status != OnChainRecoveryStatus::Completed {
            return Err(Error::validation("Recovery not complete"));
        }

        Ok(request.new_device_public_key.clone())
    }

    /// Verify ZK proof for guardian anonymity
    ///
    /// This method verifies the cryptographic validity of the ZK proof.
    /// Nullifier checking is done separately in register_guardian().
    fn verify_guardian_zk_proof(
        &self,
        proof: &[u8],
        identity_id: &str,
        guardian_id: &str,
    ) -> Result<()> {
        use dchat_privacy::zk_proofs::{ContactProof, Groth16Keys, ZkVerifier};
        use once_cell::sync::Lazy;

        // Static Groth16 keys (initialized once)
        static ZK_KEYS: Lazy<Groth16Keys> = Lazy::new(|| {
            let mut rng = rand::thread_rng();
            Groth16Keys::setup(&mut rng).expect("Failed to setup ZK keys")
        });

        // Deserialize the ZK proof
        let contact_proof: ContactProof = bincode::deserialize(proof)
            .map_err(|e| Error::validation(format!("Invalid ZK proof format: {}", e)))?;

        // Convert identity_id to UserId for verification
        let identity_uuid = uuid::Uuid::parse_str(identity_id)
            .map_err(|e| Error::validation(format!("Invalid identity ID format: {}", e)))?;
        let identity_user_id = dchat_core::UserId(identity_uuid);

        // Create verifier and verify the ZK proof cryptographically
        let verifier = ZkVerifier::new(&ZK_KEYS);
        let proof_valid = verifier
            .verify_contact(&contact_proof, &identity_user_id)
            .map_err(|e| Error::validation(format!("ZK proof verification failed: {}", e)))?;

        if !proof_valid {
            return Err(Error::validation(
                "ZK proof verification failed: invalid proof structure",
            ));
        }

        // Verify this guardian_id isn't already registered (duplicate check)
        let guardians_for_identity = self.guardians.get(identity_id);
        if let Some(existing_guardians) = guardians_for_identity {
            for guardian in existing_guardians {
                if guardian.guardian_id == guardian_id && guardian.active {
                    return Err(Error::validation(
                        "Guardian already registered for this identity",
                    ));
                }
            }
        }

        // Extract and verify proof timestamp/freshness
        // The proof should contain a timestamp or block height to ensure freshness
        // This prevents using old proofs that might have been leaked
        let proof_age_blocks = self.estimate_proof_age(&contact_proof);
        const MAX_PROOF_AGE_BLOCKS: u64 = 100; // ~20 minutes at 12s blocks

        if proof_age_blocks > MAX_PROOF_AGE_BLOCKS {
            return Err(Error::validation(format!(
                "ZK proof too old: {} blocks (max: {})",
                proof_age_blocks, MAX_PROOF_AGE_BLOCKS
            )));
        }

        tracing::info!(
            "✓ ZK proof verified for guardian {} protecting identity {} (age: {} blocks)",
            guardian_id,
            identity_id,
            proof_age_blocks
        );

        Ok(())
    }

    /// Estimate proof age in blocks based on nullifier entropy
    ///
    /// The nullifier embeds block height at proof creation time in its lower 8 bytes.
    /// Format: nullifier[0..8] = current_block_height (big-endian)
    fn estimate_proof_age(&self, proof: &dchat_privacy::zk_proofs::ContactProof) -> u64 {
        // Extract block height from nullifier lower 8 bytes
        // The proof creator embeds current_block_height when generating the proof
        let proof_block_bytes: [u8; 8] = proof.nullifier[0..8].try_into().unwrap_or([0u8; 8]);
        let proof_block_height = u64::from_be_bytes(proof_block_bytes);

        // Calculate age as difference from current block
        self.current_block_height.saturating_sub(proof_block_height)
    }

    /// Create recovery message for guardian signing
    pub fn create_recovery_message(&self, request: &RecoveryRequestState) -> Result<Vec<u8>> {
        let message = format!(
            "RECOVERY:{}:{}:{}",
            request.identity_id,
            hex::encode(&request.new_device_public_key),
            request.timelock_expires_at_block
        );

        Ok(message.into_bytes())
    }

    /// Get all guardians for an identity
    pub fn get_guardians(&self, identity_id: &str) -> Vec<(String, u64, bool)> {
        self.guardians
            .get(identity_id)
            .map(|guardians| {
                guardians
                    .iter()
                    .map(|g| (g.guardian_id.clone(), g.stake_amount, g.active))
                    .collect()
            })
            .unwrap_or_default()
    }

    /// Deactivate a guardian (slashing or voluntary removal)
    pub fn deactivate_guardian(&mut self, identity_id: &str, guardian_id: &str) -> Result<()> {
        let guardians = self
            .guardians
            .get_mut(identity_id)
            .ok_or_else(|| Error::validation("Identity not found"))?;

        let guardian = guardians
            .iter_mut()
            .find(|g| g.guardian_id == guardian_id)
            .ok_or_else(|| Error::validation("Guardian not found"))?;

        guardian.active = false;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;

    #[test]
    fn test_guardian_registration_on_chain() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        let signing_key = SigningKey::from_bytes(&[1; 32]);
        let tx = RegisterGuardianTx::new(
            "user123".to_string(),
            signing_key.verifying_key().to_bytes().to_vec(),
            "guardian-1".to_string(),
            15000,
        );

        let result = chain_state.register_guardian(tx, 100);
        assert!(result.is_ok());

        let guardians = chain_state.get_guardians("user123");
        assert_eq!(guardians.len(), 1);
        assert_eq!(guardians[0].0, "guardian-1");
        assert_eq!(guardians[0].1, 15000);
        assert_eq!(guardians[0].2, true); // active
    }

    #[test]
    fn test_nullifier_storage_and_retrieval() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        // Create a test nullifier
        let nullifier = [42u8; 32];

        // Initially not used
        assert!(!chain_state.is_nullifier_used(&nullifier));

        // Record nullifier
        chain_state
            .record_nullifier(
                nullifier,
                "user123".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        // Now should be used
        assert!(chain_state.is_nullifier_used(&nullifier));

        // Verify record
        let record = chain_state.get_nullifier_record(&nullifier).unwrap();
        assert_eq!(record.identity_id, "user123");
        assert_eq!(record.block_height, 100);
        assert_eq!(record.nullifier_type, NullifierType::GuardianRegistration);
    }

    #[test]
    fn test_nullifier_prevents_reuse() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        let nullifier = [42u8; 32];

        // First recording should succeed
        chain_state
            .record_nullifier(
                nullifier,
                "user123".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        // Second recording should fail
        let result = chain_state.record_nullifier(
            nullifier,
            "user456".to_string(),
            NullifierType::GuardianRegistration,
        );

        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already used"));
    }

    #[test]
    fn test_get_nullifiers_for_identity() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        // Record multiple nullifiers for same identity
        chain_state
            .record_nullifier(
                [1u8; 32],
                "user123".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();
        chain_state.set_block_height(101);
        chain_state
            .record_nullifier(
                [2u8; 32],
                "user123".to_string(),
                NullifierType::GuardianSignature,
            )
            .unwrap();
        chain_state.set_block_height(102);
        chain_state
            .record_nullifier(
                [3u8; 32],
                "other_user".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        let nullifiers = chain_state.get_nullifiers_for_identity("user123");
        assert_eq!(nullifiers.len(), 2);
    }

    #[test]
    fn test_nullifier_export_import() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        chain_state
            .record_nullifier(
                [1u8; 32],
                "user1".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();
        chain_state
            .record_nullifier(
                [2u8; 32],
                "user2".to_string(),
                NullifierType::RecoveryInitiation,
            )
            .unwrap();

        // Export
        let exported = chain_state.export_nullifiers();
        assert_eq!(exported.len(), 2);

        // Import into new state
        let mut new_state = GuardianChainState::new(10000);
        new_state.import_nullifiers(exported);

        assert!(new_state.is_nullifier_used(&[1u8; 32]));
        assert!(new_state.is_nullifier_used(&[2u8; 32]));
        assert!(!new_state.is_nullifier_used(&[3u8; 32]));
    }

    #[test]
    fn test_recovery_prevents_duplicate_requests() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        // Register guardians
        for i in 0..3 {
            let signing_key = SigningKey::from_bytes(&[i; 32]);
            let tx = RegisterGuardianTx::new(
                "user123".to_string(),
                signing_key.verifying_key().to_bytes().to_vec(),
                format!("guardian-{}", i),
                15000,
            );
            chain_state.register_guardian(tx, 100).unwrap();
        }

        // First recovery should succeed
        let recovery_tx = InitiateRecoveryTx::new(
            "recovery-1".to_string(),
            "user123".to_string(),
            vec![1, 2, 3, 4],
            400,
            100,
            2,
        );
        chain_state
            .initiate_recovery(recovery_tx.clone(), 100)
            .unwrap();

        // Exact same recovery should fail (duplicate nullifier)
        let recovery_tx2 = InitiateRecoveryTx::new(
            "recovery-2".to_string(), // Different ID
            "user123".to_string(),
            vec![1, 2, 3, 4], // Same key
            400,              // Same timelock
            100,
            2,
        );
        let result = chain_state.initiate_recovery(recovery_tx2, 100);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Duplicate"));
    }

    #[test]
    fn test_recovery_timelock_validation() {
        let mut chain_state = GuardianChainState::new(10000);
        chain_state.set_block_height(100);

        // Register guardians
        for i in 0..3 {
            let signing_key = SigningKey::from_bytes(&[i; 32]);
            let tx = RegisterGuardianTx::new(
                "user123".to_string(),
                signing_key.verifying_key().to_bytes().to_vec(),
                format!("guardian-{}", i),
                15000,
            );
            chain_state.register_guardian(tx, 100).unwrap();
        }

        // Initiate recovery with timelock at block 500
        let recovery_tx = InitiateRecoveryTx::new(
            "recovery-1".to_string(),
            "user123".to_string(),
            vec![1, 2, 3, 4],
            400, // Timelock 400 blocks
            100, // Current block
            2,   // Require 2-of-3 signatures
        );

        chain_state.initiate_recovery(recovery_tx, 100).unwrap();

        // Try to submit signature before timelock expires
        let signature_tx = SubmitGuardianSignatureTx::new(
            "recovery-1".to_string(),
            "guardian-0".to_string(),
            vec![0; 64],
            100,
        );

        let result = chain_state.submit_guardian_signature(signature_tx);
        assert!(result.is_err()); // Should fail - timelock not expired

        // Advance chain to block 500 (timelock expired)
        chain_state.set_block_height(500);

        // Now signature submission should work (though will fail verification)
        let signature_tx = SubmitGuardianSignatureTx::new(
            "recovery-1".to_string(),
            "guardian-0".to_string(),
            vec![0; 64],
            500,
        );

        // This will still fail due to invalid signature, but timelock check passes
        let result = chain_state.submit_guardian_signature(signature_tx);
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("signature")); // Fails on signature validation, not timelock
    }

    #[test]
    fn test_insufficient_stake_rejection() {
        let mut chain_state = GuardianChainState::new(10000);

        let signing_key = SigningKey::from_bytes(&[1; 32]);
        let tx = RegisterGuardianTx::new(
            "user123".to_string(),
            signing_key.verifying_key().to_bytes().to_vec(),
            "guardian-1".to_string(),
            5000, // Below minimum 10000
        );

        let result = chain_state.register_guardian(tx, 100);
        assert!(result.is_err());
    }

    #[test]
    fn test_nullifier_pruning() {
        let mut chain_state = GuardianChainState::new(10000);

        // Add nullifiers at different block heights
        chain_state.set_block_height(100);
        chain_state
            .record_nullifier(
                [1u8; 32],
                "user1".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        chain_state.set_block_height(200);
        chain_state
            .record_nullifier(
                [2u8; 32],
                "user2".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        chain_state.set_block_height(300);
        chain_state
            .record_nullifier(
                [3u8; 32],
                "user3".to_string(),
                NullifierType::GuardianRegistration,
            )
            .unwrap();

        assert_eq!(chain_state.nullifiers.len(), 3);

        // Prune nullifiers before block 200
        let removed = chain_state.prune_nullifiers_before_block(200);
        assert_eq!(removed, 1);
        assert_eq!(chain_state.nullifiers.len(), 2);

        // Old nullifier should be gone
        assert!(!chain_state.is_nullifier_used(&[1u8; 32]));
        assert!(chain_state.is_nullifier_used(&[2u8; 32]));
        assert!(chain_state.is_nullifier_used(&[3u8; 32]));
    }
}
