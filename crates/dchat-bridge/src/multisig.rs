use crate::types::{BridgeError, TransactionId};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, PoisonError, RwLock, RwLockReadGuard, RwLockWriteGuard};

/// Validator identity with public key
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ValidatorId {
    pub id: UserId,
    pub public_key: Vec<u8>, // Ed25519 public key
}

impl ValidatorId {
    /// Create a new validator ID
    pub fn new(id: UserId, public_key: Vec<u8>) -> Self {
        Self { id, public_key }
    }
}

/// Validator signature on a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: ValidatorId,
    pub signature: Vec<u8>, // Ed25519 signature
    pub signed_at: chrono::DateTime<chrono::Utc>,
}

/// Multi-signature configuration (M-of-N)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSigConfig {
    /// Minimum number of signatures required (M)
    pub threshold: usize,
    /// Total number of validators (N)
    pub total_validators: usize,
    /// Active validator set
    pub validators: Vec<ValidatorId>,
}

impl MultiSigConfig {
    /// Create a new multi-sig configuration
    pub fn new(threshold: usize, validators: Vec<ValidatorId>) -> Result<Self, BridgeError> {
        let total = validators.len();

        if threshold == 0 {
            return Err(BridgeError::InvalidThreshold);
        }

        if threshold > total {
            return Err(BridgeError::InvalidThreshold);
        }

        // Check for duplicate validators
        let unique_ids: HashSet<_> = validators.iter().map(|v| &v.id).collect();
        if unique_ids.len() != validators.len() {
            return Err(BridgeError::DuplicateValidator);
        }

        Ok(Self {
            threshold,
            total_validators: total,
            validators,
        })
    }

    /// Check if threshold is reached
    pub fn has_quorum(&self, signature_count: usize) -> bool {
        signature_count >= self.threshold
    }

    /// Get validator by ID
    pub fn get_validator(&self, validator_id: &UserId) -> Option<&ValidatorId> {
        self.validators.iter().find(|v| &v.id == validator_id)
    }
}

/// Multi-signature state for a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MultiSigState {
    pub transaction_id: TransactionId,
    pub config: MultiSigConfig,
    pub signatures: Vec<ValidatorSignature>,
    pub quorum_reached: bool,
}

impl MultiSigState {
    /// Create a new multi-sig state
    pub fn new(transaction_id: TransactionId, config: MultiSigConfig) -> Self {
        Self {
            transaction_id,
            config,
            signatures: Vec::new(),
            quorum_reached: false,
        }
    }

    /// Add a validator signature
    pub fn add_signature(&mut self, signature: ValidatorSignature) -> Result<bool, BridgeError> {
        // Verify validator is in the active set
        if !self
            .config
            .validators
            .iter()
            .any(|v| v.id == signature.validator_id.id)
        {
            return Err(BridgeError::UnknownValidator);
        }

        // Check for duplicate signature
        if self
            .signatures
            .iter()
            .any(|s| s.validator_id.id == signature.validator_id.id)
        {
            return Err(BridgeError::DuplicateSignature);
        }

        self.signatures.push(signature);

        // Check if quorum reached
        if !self.quorum_reached && self.config.has_quorum(self.signatures.len()) {
            self.quorum_reached = true;
            return Ok(true);
        }

        Ok(false)
    }

    /// Get signature count
    pub fn signature_count(&self) -> usize {
        self.signatures.len()
    }

    /// Verify signature with Ed25519 cryptography
    pub fn verify_signature(
        &self,
        signature: &ValidatorSignature,
        message: &[u8],
    ) -> Result<(), BridgeError> {
        use ed25519_dalek::{Signature, VerifyingKey};

        // 1. Check signature length (Ed25519 signatures are exactly 64 bytes)
        if signature.signature.len() != 64 {
            return Err(BridgeError::InvalidSignature);
        }

        // 2. Extract validator's public key (32 bytes)
        if signature.validator_id.public_key.len() != 32 {
            return Err(BridgeError::InvalidSignature);
        }

        // 3. Perform Ed25519 verification
        let verifying_key = VerifyingKey::from_bytes(
            signature
                .validator_id
                .public_key
                .as_slice()
                .try_into()
                .map_err(|_| BridgeError::InvalidSignature)?,
        )
        .map_err(|_| BridgeError::InvalidSignature)?;

        let sig = Signature::from_bytes(
            signature
                .signature
                .as_slice()
                .try_into()
                .map_err(|_| BridgeError::InvalidSignature)?,
        );

        verifying_key
            .verify_strict(message, &sig)
            .map_err(|_| BridgeError::InvalidSignature)?;

        // 4. Check timestamp freshness (prevent replay attacks)
        let now = chrono::Utc::now();
        let age = now.signed_duration_since(signature.signed_at);

        if age.num_seconds().abs() > 300 {
            // More than 5 minutes old/future
            return Err(BridgeError::InvalidSignature);
        }

        tracing::debug!(
            "Verified signature from validator {:?}",
            signature.validator_id.id
        );
        Ok(())
    }
}

/// Multi-signature manager
pub struct MultiSigManager {
    /// Active multi-sig configurations per transaction
    states: Arc<RwLock<HashMap<TransactionId, MultiSigState>>>,
    /// Global validator set (can be rotated)
    global_config: Arc<RwLock<MultiSigConfig>>,
}

/// Multi-sig manager error types
#[derive(Debug, Clone)]
pub enum MultiSigError {
    /// Lock was poisoned by a panicking thread
    LockPoisoned,
    /// Bridge error wrapper
    Bridge(BridgeError),
}

impl std::fmt::Display for MultiSigError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MultiSigError::LockPoisoned => write!(f, "Internal lock was poisoned"),
            MultiSigError::Bridge(e) => write!(f, "Bridge error: {}", e),
        }
    }
}

impl std::error::Error for MultiSigError {}

impl<T> From<PoisonError<T>> for MultiSigError {
    fn from(_: PoisonError<T>) -> Self {
        MultiSigError::LockPoisoned
    }
}

impl From<BridgeError> for MultiSigError {
    fn from(e: BridgeError) -> Self {
        MultiSigError::Bridge(e)
    }
}

impl MultiSigManager {
    /// Create a new multi-sig manager
    pub fn new(config: MultiSigConfig) -> Self {
        Self {
            states: Arc::new(RwLock::new(HashMap::new())),
            global_config: Arc::new(RwLock::new(config)),
        }
    }

    /// Helper to acquire read lock on states with proper error handling
    fn states_read(
        &self,
    ) -> Result<RwLockReadGuard<'_, HashMap<TransactionId, MultiSigState>>, MultiSigError> {
        self.states.read().map_err(|_| MultiSigError::LockPoisoned)
    }

    /// Helper to acquire write lock on states with proper error handling
    fn states_write(
        &self,
    ) -> Result<RwLockWriteGuard<'_, HashMap<TransactionId, MultiSigState>>, MultiSigError> {
        self.states.write().map_err(|_| MultiSigError::LockPoisoned)
    }

    /// Helper to acquire read lock on config with proper error handling
    fn config_read(&self) -> Result<RwLockReadGuard<'_, MultiSigConfig>, MultiSigError> {
        self.global_config.read().map_err(|_| MultiSigError::LockPoisoned)
    }

    /// Helper to acquire write lock on config with proper error handling
    fn config_write(&self) -> Result<RwLockWriteGuard<'_, MultiSigConfig>, MultiSigError> {
        self.global_config.write().map_err(|_| MultiSigError::LockPoisoned)
    }

    /// Initialize multi-sig for a transaction
    pub fn init_transaction(&self, transaction_id: TransactionId) -> Result<(), MultiSigError> {
        let config = self.config_read()?.clone();
        let state = MultiSigState::new(transaction_id, config);

        let mut states = self.states_write()?;
        if states.contains_key(&transaction_id) {
            return Err(MultiSigError::Bridge(BridgeError::TransactionAlreadyExists));
        }

        states.insert(transaction_id, state);
        Ok(())
    }

    /// Submit a validator signature
    pub fn submit_signature(
        &self,
        transaction_id: TransactionId,
        signature: ValidatorSignature,
        message: &[u8],
    ) -> Result<bool, MultiSigError> {
        let mut states = self.states_write()?;
        let state = states
            .get_mut(&transaction_id)
            .ok_or(MultiSigError::Bridge(BridgeError::TransactionNotFound))?;

        // Verify signature cryptographically
        state.verify_signature(&signature, message)?;

        // Add signature and check if quorum reached
        Ok(state.add_signature(signature)?)
    }

    /// Check if transaction has quorum
    pub fn has_quorum(&self, transaction_id: TransactionId) -> Result<bool, MultiSigError> {
        let states = self.states_read()?;
        Ok(states
            .get(&transaction_id)
            .map(|s| s.quorum_reached)
            .unwrap_or(false))
    }

    /// Get signature count for transaction
    pub fn get_signature_count(&self, transaction_id: TransactionId) -> Result<usize, MultiSigError> {
        let states = self.states_read()?;
        Ok(states
            .get(&transaction_id)
            .map(|s| s.signature_count())
            .unwrap_or(0))
    }

    /// Rotate validator set (for dynamic validator management)
    pub fn rotate_validators(&self, new_config: MultiSigConfig) -> Result<(), MultiSigError> {
        let mut config = self.config_write()?;
        *config = new_config;
        Ok(())
    }

    /// Get current validator set
    pub fn get_validators(&self) -> Result<Vec<ValidatorId>, MultiSigError> {
        Ok(self.config_read()?.validators.clone())
    }

    /// Get multi-sig state for transaction
    pub fn get_state(&self, transaction_id: TransactionId) -> Result<Option<MultiSigState>, MultiSigError> {
        let states = self.states_read()?;
        Ok(states.get(&transaction_id).cloned())
    }

    /// Clean up completed transactions
    pub fn cleanup_transaction(&self, transaction_id: TransactionId) -> Result<(), MultiSigError> {
        let mut states = self.states_write()?;
        states.remove(&transaction_id);
        Ok(())
    }
}

/// BLS signature aggregation for efficient multi-signature verification
/// Reduces on-chain verification cost from O(n) to O(1)
pub struct SignatureAggregator;

impl SignatureAggregator {
    /// Aggregate multiple BLS signatures into a single signature.
    ///
    /// Uses BLS12-381 curve (min_pk variant) for efficient multi-signature aggregation.
    /// Benefits:
    /// - Single 96-byte signature replaces N signatures
    /// - On-chain verification cost is O(1) instead of O(N)
    /// - Reduces cross-chain transaction size significantly
    pub fn aggregate(signatures: &[ValidatorSignature]) -> Vec<u8> {
        use blst::min_pk::{AggregateSignature, Signature};

        if signatures.is_empty() {
            return Vec::new();
        }

        // Parse all signatures
        let mut bls_sigs = Vec::new();
        for sig in signatures {
            if sig.signature.len() != 96 {
                tracing::warn!(
                    validator_id = ?sig.validator_id.id,
                    sig_len = sig.signature.len(),
                    "Invalid BLS signature length, skipping"
                );
                continue;
            }

            match Signature::from_bytes(&sig.signature) {
                Ok(bls_sig) => {
                    // Validate signature format
                    if bls_sig.validate(true).is_ok() {
                        bls_sigs.push(bls_sig);
                    } else {
                        tracing::warn!(
                            validator_id = ?sig.validator_id.id,
                            "Invalid BLS signature format, skipping"
                        );
                    }
                }
                Err(e) => {
                    tracing::warn!(
                        validator_id = ?sig.validator_id.id,
                        error = ?e,
                        "Failed to parse BLS signature"
                    );
                }
            }
        }

        if bls_sigs.is_empty() {
            tracing::error!("No valid BLS signatures to aggregate");
            return Vec::new();
        }

        // Aggregate using blst
        let sig_refs: Vec<&Signature> = bls_sigs.iter().collect();
        match AggregateSignature::aggregate(&sig_refs, true) {
            Ok(agg_sig) => {
                let final_sig = agg_sig.to_signature();
                final_sig.to_bytes().to_vec()
            }
            Err(e) => {
                tracing::error!(error = ?e, "Failed to aggregate BLS signatures");
                Vec::new()
            }
        }
    }

    /// Verify aggregated BLS signature against multiple public keys.
    ///
    /// Uses pairing-based verification: e(agg_sig, G) == e(H(m), ΣPKi)
    /// This proves all validators signed the same message with O(1) verification cost.
    ///
    /// # Arguments
    /// * `aggregated` - 96-byte aggregated BLS signature
    /// * `public_keys` - Vector of 48-byte compressed BLS public keys
    /// * `message` - The message that was signed by all validators
    pub fn verify_aggregated(
        aggregated: &[u8],
        public_keys: &[Vec<u8>],
        message: &[u8],
    ) -> Result<(), BridgeError> {
        use blst::min_pk::{AggregatePublicKey, PublicKey, Signature};
        use blst::BLST_ERROR;

        if aggregated.is_empty() {
            return Err(BridgeError::InvalidSignature);
        }

        if public_keys.is_empty() {
            return Err(BridgeError::InvalidSignature);
        }

        // Parse aggregated signature (96 bytes compressed)
        if aggregated.len() != 96 {
            return Err(BridgeError::InvalidSignature);
        }

        let agg_sig =
            Signature::from_bytes(aggregated).map_err(|_| BridgeError::InvalidSignature)?;

        // Validate signature format
        agg_sig
            .validate(true)
            .map_err(|_| BridgeError::InvalidSignature)?;

        // Parse all public keys (48 bytes compressed each for min_pk)
        let mut pks = Vec::new();
        for pk_bytes in public_keys {
            if pk_bytes.len() != 48 {
                return Err(BridgeError::InvalidSignature);
            }

            let pk = PublicKey::from_bytes(pk_bytes).map_err(|_| BridgeError::InvalidSignature)?;

            // Validate public key
            pk.validate().map_err(|_| BridgeError::InvalidSignature)?;

            pks.push(pk);
        }

        // Aggregate all public keys for verification of aggregated signature
        // This is correct for the case where all signers signed the SAME message
        let pk_refs: Vec<&PublicKey> = pks.iter().collect();
        let agg_pk = AggregatePublicKey::aggregate(&pk_refs, true)
            .map_err(|_| BridgeError::InvalidSignature)?
            .to_public_key();

        // Verify aggregated signature against aggregated public key
        let result = agg_sig.verify(
            true,
            message,
            b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_",
            &[],
            &agg_pk,
            true,
        );

        if result == BLST_ERROR::BLST_SUCCESS {
            tracing::debug!(
                "Verified aggregated BLS signature for {} validators",
                public_keys.len()
            );
            Ok(())
        } else {
            Err(BridgeError::InvalidSignature)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::{Signer, SigningKey};
    use uuid::Uuid;

    /// Test helper: creates a validator with a real Ed25519 key pair
    /// Returns (ValidatorId, SigningKey) so we can sign messages
    fn create_validator_with_key(id: u8) -> (ValidatorId, SigningKey) {
        // Create deterministic signing key from seed
        let seed = [id; 32];
        let signing_key = SigningKey::from_bytes(&seed);
        let verifying_key = signing_key.verifying_key();

        let user_id = UserId::new();
        let public_key = verifying_key.as_bytes().to_vec();
        let validator = ValidatorId::new(user_id, public_key);
        (validator, signing_key)
    }

    /// Test helper: creates a validator (without exposing signing key, for legacy tests)
    fn create_validator(id: u8) -> ValidatorId {
        let (validator, _) = create_validator_with_key(id);
        validator
    }

    /// Test helper: creates a valid signature for a message
    fn create_valid_signature(
        validator: ValidatorId,
        signing_key: &SigningKey,
        message: &[u8],
    ) -> ValidatorSignature {
        let signature = signing_key.sign(message);
        ValidatorSignature {
            validator_id: validator,
            signature: signature.to_bytes().to_vec(),
            signed_at: chrono::Utc::now(),
        }
    }

    /// Test helper: creates a dummy (invalid) signature for tests that don't verify
    fn create_dummy_signature(validator: ValidatorId) -> ValidatorSignature {
        ValidatorSignature {
            validator_id: validator,
            signature: vec![0u8; 64], // Invalid but correct length
            signed_at: chrono::Utc::now(),
        }
    }

    /// Alias for create_dummy_signature (for tests that don't need verification)
    fn create_signature(validator: ValidatorId) -> ValidatorSignature {
        create_dummy_signature(validator)
    }

    #[test]
    fn test_multisig_config_creation() {
        let validators = vec![
            create_validator(1),
            create_validator(2),
            create_validator(3),
        ];

        let config = MultiSigConfig::new(2, validators).unwrap();
        assert_eq!(config.threshold, 2);
        assert_eq!(config.total_validators, 3);
    }

    #[test]
    fn test_invalid_threshold() {
        let validators = vec![create_validator(1), create_validator(2)];

        // Threshold > total
        let result = MultiSigConfig::new(3, validators.clone());
        assert!(result.is_err());

        // Threshold = 0
        let result = MultiSigConfig::new(0, validators);
        assert!(result.is_err());
    }

    #[test]
    fn test_duplicate_validator() {
        let validator = create_validator(1);
        let validators = vec![validator.clone(), validator];

        let result = MultiSigConfig::new(2, validators);
        assert!(result.is_err());
    }

    #[test]
    fn test_quorum_check() {
        let validators = vec![
            create_validator(1),
            create_validator(2),
            create_validator(3),
        ];
        let config = MultiSigConfig::new(2, validators).unwrap();

        assert!(!config.has_quorum(1));
        assert!(config.has_quorum(2));
        assert!(config.has_quorum(3));
    }

    #[test]
    fn test_add_signature() {
        let validators = vec![
            create_validator(1),
            create_validator(2),
            create_validator(3),
        ];
        let config = MultiSigConfig::new(2, validators.clone()).unwrap();

        let tx_id = Uuid::new_v4();
        let mut state = MultiSigState::new(tx_id, config);

        // Add first signature
        let sig1 = create_signature(validators[0].clone());
        let quorum_reached = state.add_signature(sig1).unwrap();
        assert!(!quorum_reached);
        assert_eq!(state.signature_count(), 1);

        // Add second signature - quorum reached
        let sig2 = create_signature(validators[1].clone());
        let quorum_reached = state.add_signature(sig2).unwrap();
        assert!(quorum_reached);
        assert_eq!(state.signature_count(), 2);
        assert!(state.quorum_reached);
    }

    #[test]
    fn test_duplicate_signature() {
        let validators = vec![create_validator(1), create_validator(2)];
        let config = MultiSigConfig::new(2, validators.clone()).unwrap();

        let tx_id = Uuid::new_v4();
        let mut state = MultiSigState::new(tx_id, config);

        let sig1 = create_signature(validators[0].clone());
        state.add_signature(sig1.clone()).unwrap();

        // Try to add same validator's signature again
        let result = state.add_signature(sig1);
        assert!(result.is_err());
    }

    #[test]
    fn test_unknown_validator() {
        let validators = vec![create_validator(1), create_validator(2)];
        let config = MultiSigConfig::new(2, validators).unwrap();

        let tx_id = Uuid::new_v4();
        let mut state = MultiSigState::new(tx_id, config);

        // Unknown validator
        let unknown = create_validator(99);
        let sig = create_signature(unknown);

        let result = state.add_signature(sig);
        assert!(result.is_err());
    }

    #[test]
    fn test_multisig_manager_init() {
        let validators = vec![
            create_validator(1),
            create_validator(2),
            create_validator(3),
        ];
        let config = MultiSigConfig::new(2, validators).unwrap();
        let manager = MultiSigManager::new(config);

        let tx_id = Uuid::new_v4();
        manager.init_transaction(tx_id).unwrap();

        assert_eq!(manager.get_signature_count(tx_id).unwrap(), 0);
        assert!(!manager.has_quorum(tx_id).unwrap());
    }

    #[test]
    fn test_multisig_manager_submit_signatures() {
        // Create validators with real key pairs
        let (validator1, sk1) = create_validator_with_key(1);
        let (validator2, sk2) = create_validator_with_key(2);
        let (validator3, _sk3) = create_validator_with_key(3);

        let validators = vec![validator1.clone(), validator2.clone(), validator3];
        let config = MultiSigConfig::new(2, validators).unwrap();
        let manager = MultiSigManager::new(config);

        let tx_id = Uuid::new_v4();
        manager.init_transaction(tx_id).unwrap();

        let message = b"transaction_data";

        // Submit first signature (valid)
        let sig1 = create_valid_signature(validator1, &sk1, message);
        let quorum = manager.submit_signature(tx_id, sig1, message).unwrap();
        assert!(!quorum);
        assert_eq!(manager.get_signature_count(tx_id).unwrap(), 1);

        // Submit second signature - quorum (valid)
        let sig2 = create_valid_signature(validator2, &sk2, message);
        let quorum = manager.submit_signature(tx_id, sig2, message).unwrap();
        assert!(quorum);
        assert!(manager.has_quorum(tx_id).unwrap());
    }

    #[test]
    fn test_validator_rotation() {
        let validators = vec![create_validator(1), create_validator(2)];
        let config = MultiSigConfig::new(2, validators).unwrap();
        let manager = MultiSigManager::new(config);

        let original_validators = manager.get_validators().unwrap();
        assert_eq!(original_validators.len(), 2);

        // Rotate to new set
        let new_validators = vec![
            create_validator(3),
            create_validator(4),
            create_validator(5),
        ];
        let new_config = MultiSigConfig::new(2, new_validators).unwrap();
        manager.rotate_validators(new_config).unwrap();

        let rotated_validators = manager.get_validators().unwrap();
        assert_eq!(rotated_validators.len(), 3);
    }

    #[test]
    fn test_cleanup_transaction() {
        let validators = vec![create_validator(1), create_validator(2)];
        let config = MultiSigConfig::new(2, validators).unwrap();
        let manager = MultiSigManager::new(config);

        let tx_id = Uuid::new_v4();
        manager.init_transaction(tx_id).unwrap();

        assert!(manager.get_state(tx_id).unwrap().is_some());

        manager.cleanup_transaction(tx_id).unwrap();
        assert!(manager.get_state(tx_id).unwrap().is_none());
    }

    #[test]
    fn test_signature_aggregation() {
        use blst::min_pk::SecretKey as BlsSecretKey;

        let message = b"transaction_data";
        let dst = b"BLS_SIG_BLS12381G2_XMD:SHA-256_SSWU_RO_NUL_";

        // Generate BLS key pairs
        let ikm1 = [1u8; 32];
        let ikm2 = [2u8; 32];
        let sk1 = BlsSecretKey::key_gen(&ikm1, &[]).unwrap();
        let sk2 = BlsSecretKey::key_gen(&ikm2, &[]).unwrap();
        let pk1 = sk1.sk_to_pk();
        let pk2 = sk2.sk_to_pk();

        // Create BLS signatures
        let bls_sig1 = sk1.sign(message, dst, &[]);
        let bls_sig2 = sk2.sign(message, dst, &[]);

        // Create validator IDs with BLS public keys (48 bytes compressed for min_pk)
        let pk1_bytes = pk1.compress().to_vec(); // 48 bytes
        let pk2_bytes = pk2.compress().to_vec(); // 48 bytes

        let validator1 = ValidatorId::new(UserId::new(), pk1_bytes.clone());
        let validator2 = ValidatorId::new(UserId::new(), pk2_bytes.clone());

        // Create ValidatorSignature with BLS signatures (96 bytes)
        let sig1 = ValidatorSignature {
            validator_id: validator1.clone(),
            signature: bls_sig1.compress().to_vec(), // 96 bytes
            signed_at: chrono::Utc::now(),
        };
        let sig2 = ValidatorSignature {
            validator_id: validator2.clone(),
            signature: bls_sig2.compress().to_vec(), // 96 bytes
            signed_at: chrono::Utc::now(),
        };

        let signatures = vec![sig1, sig2];
        let aggregated = SignatureAggregator::aggregate(&signatures);

        // BLS aggregation produces a single 96-byte signature
        assert_eq!(aggregated.len(), 96);

        let public_keys = vec![pk1_bytes, pk2_bytes];

        SignatureAggregator::verify_aggregated(&aggregated, &public_keys, message).unwrap();
    }

    #[test]
    fn test_invalid_signature_length() {
        let validator = create_validator(1);
        let mut sig = create_dummy_signature(validator.clone());
        sig.signature = vec![0u8; 32]; // Invalid length (should be 64)

        let config = MultiSigConfig::new(1, vec![validator]).unwrap();
        let tx_id = Uuid::new_v4();
        let state = MultiSigState::new(tx_id, config);

        let result = state.verify_signature(&sig, b"message");
        assert!(result.is_err());
    }
}
