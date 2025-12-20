//! Receipt system - cross-chain receipts with threshold attestations

use std::collections::{HashMap, HashSet};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::intent::IntentId;
use crate::{ATTESTATION_THRESHOLD_PERCENT, MIN_ATTESTATIONS};

/// Receipt ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ReceiptId(pub Uuid);

impl ReceiptId {
    /// Generate new ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Parse from string
    pub fn parse(s: &str) -> MiniAppResult<Self> {
        Uuid::parse_str(s)
            .map(Self)
            .map_err(|_| MiniAppError::ReceiptNotFound("invalid receipt ID".to_string()))
    }
}

impl Default for ReceiptId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ReceiptId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Receipt status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ReceiptStatus {
    /// Receipt pending attestations
    Pending,
    /// Receipt has sufficient attestations
    Confirmed,
    /// Receipt rejected (conflicting attestations)
    Rejected,
    /// Receipt finalized on chain
    Finalized,
}

/// Execution result data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionResult {
    /// Whether execution was successful
    pub success: bool,
    /// Return data (if any)
    pub return_data: Option<Vec<u8>>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Logs emitted during execution
    pub logs: Vec<String>,
    /// Account changes
    pub account_changes: Vec<AccountChange>,
    /// Compute units consumed
    pub compute_units: u64,
    /// Fee paid
    pub fee: u64,
}

/// Account change record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountChange {
    /// Account address
    pub address: [u8; 32],
    /// Previous lamports
    pub prev_lamports: u64,
    /// New lamports
    pub new_lamports: u64,
    /// Data changed
    pub data_changed: bool,
    /// Owner changed
    pub owner_changed: bool,
}

/// Cross-chain receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Receipt {
    /// Receipt ID
    pub id: ReceiptId,
    /// Intent this receipt is for
    pub intent_id: IntentId,
    /// Execution result
    pub result: ExecutionResult,
    /// Currency chain slot where executed
    pub execution_slot: u64,
    /// Currency chain transaction hash
    pub transaction_hash: [u8; 32],
    /// Block hash
    pub block_hash: [u8; 32],
    /// Receipt status
    pub status: ReceiptStatus,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Finalized timestamp (when enough attestations)
    pub finalized_at: Option<DateTime<Utc>>,
}

impl Receipt {
    /// Create new receipt
    pub fn new(
        intent_id: IntentId,
        result: ExecutionResult,
        execution_slot: u64,
        transaction_hash: [u8; 32],
        block_hash: [u8; 32],
    ) -> Self {
        Self {
            id: ReceiptId::new(),
            intent_id,
            result,
            execution_slot,
            transaction_hash,
            block_hash,
            status: ReceiptStatus::Pending,
            created_at: Utc::now(),
            finalized_at: None,
        }
    }

    /// Compute receipt hash for signing
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&self.id.0.as_bytes());
        hasher.update(&self.intent_id.0.as_bytes());
        hasher.update(&self.transaction_hash);
        hasher.update(&self.block_hash);
        hasher.update(&self.execution_slot.to_le_bytes());
        hasher.update(if self.result.success { &[1u8] } else { &[0u8] });
        hasher.finalize().into()
    }

    /// Mark as confirmed
    pub fn confirm(&mut self) {
        self.status = ReceiptStatus::Confirmed;
    }

    /// Mark as rejected
    pub fn reject(&mut self) {
        self.status = ReceiptStatus::Rejected;
    }

    /// Mark as finalized
    pub fn finalize(&mut self) {
        self.status = ReceiptStatus::Finalized;
        self.finalized_at = Some(Utc::now());
    }
}

/// Attestation from a signer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Attestation {
    /// Signer's public key
    pub signer: [u8; 32],
    /// Receipt hash that was signed
    pub receipt_hash: [u8; 32],
    /// Signature
    pub signature: [u8; 64],
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Signer's stake (for weighted voting)
    pub stake: u64,
}

impl Attestation {
    /// Create new attestation
    pub fn create(signing_key: &SigningKey, receipt_hash: [u8; 32], stake: u64) -> Self {
        let signature = signing_key.sign(&receipt_hash);
        Self {
            signer: signing_key.verifying_key().to_bytes(),
            receipt_hash,
            signature: signature.to_bytes(),
            timestamp: Utc::now(),
            stake,
        }
    }

    /// Verify attestation
    pub fn verify(&self) -> MiniAppResult<()> {
        let verifying_key = VerifyingKey::from_bytes(&self.signer)
            .map_err(|_| MiniAppError::InvalidPublicKey("invalid signer key".to_string()))?;

        let signature = Signature::from_bytes(&self.signature);
        verifying_key
            .verify(&self.receipt_hash, &signature)
            .map_err(|_| MiniAppError::AttestationSignatureInvalid)
    }
}

/// Set of attestations for a receipt
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AttestationSet {
    /// Attestations by signer
    attestations: HashMap<[u8; 32], Attestation>,
    /// Total stake of attestors
    pub total_stake: u64,
}

impl AttestationSet {
    /// Create new set
    pub fn new() -> Self {
        Self::default()
    }

    /// Add attestation
    pub fn add(&mut self, attestation: Attestation) -> MiniAppResult<()> {
        // Verify attestation
        attestation.verify()?;

        // Check for duplicate
        if self.attestations.contains_key(&attestation.signer) {
            return Err(MiniAppError::DuplicateAttestation(hex::encode(
                &attestation.signer[..8],
            )));
        }

        self.total_stake += attestation.stake;
        self.attestations.insert(attestation.signer, attestation);
        Ok(())
    }

    /// Get attestation count
    pub fn count(&self) -> usize {
        self.attestations.len()
    }

    /// Check if has attestation from signer
    pub fn has_attestation(&self, signer: &[u8; 32]) -> bool {
        self.attestations.contains_key(signer)
    }

    /// Get all attestations
    pub fn attestations(&self) -> Vec<&Attestation> {
        self.attestations.values().collect()
    }

    /// Get signers
    pub fn signers(&self) -> Vec<[u8; 32]> {
        self.attestations.keys().cloned().collect()
    }

    /// Check if all attestations agree on receipt hash
    pub fn is_consistent(&self) -> bool {
        if self.attestations.is_empty() {
            return true;
        }

        let first_hash = self.attestations.values().next().map(|a| a.receipt_hash);
        self.attestations
            .values()
            .all(|a| Some(a.receipt_hash) == first_hash)
    }
}

/// Cross-chain receipt (includes attestations)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrossChainReceipt {
    /// The receipt
    pub receipt: Receipt,
    /// Attestations from signers
    pub attestations: AttestationSet,
}

impl CrossChainReceipt {
    /// Create new cross-chain receipt
    pub fn new(receipt: Receipt) -> Self {
        Self {
            receipt,
            attestations: AttestationSet::new(),
        }
    }

    /// Add attestation
    pub fn add_attestation(&mut self, attestation: Attestation) -> MiniAppResult<()> {
        // Verify attestation matches receipt
        let receipt_hash = self.receipt.hash();
        if attestation.receipt_hash != receipt_hash {
            return Err(MiniAppError::InvalidAttestation(
                "receipt hash mismatch".to_string(),
            ));
        }

        self.attestations.add(attestation)
    }
}

/// Signer set for attestations
#[derive(Debug, Clone)]
pub struct SignerSet {
    /// Active signers
    signers: HashMap<[u8; 32], SignerInfo>,
    /// Total stake
    pub total_stake: u64,
    /// Threshold percentage
    threshold_percent: u8,
    /// Minimum attestations
    min_attestations: usize,
}

/// Signer information
#[derive(Debug, Clone)]
pub struct SignerInfo {
    /// Public key
    pub public_key: [u8; 32],
    /// Stake
    pub stake: u64,
    /// Whether active
    pub active: bool,
    /// Added timestamp
    pub added_at: DateTime<Utc>,
}

impl SignerSet {
    /// Create new signer set
    pub fn new(threshold_percent: u8, min_attestations: usize) -> Self {
        Self {
            signers: HashMap::new(),
            total_stake: 0,
            threshold_percent,
            min_attestations,
        }
    }

    /// Create with default thresholds
    pub fn default_thresholds() -> Self {
        Self::new(ATTESTATION_THRESHOLD_PERCENT, MIN_ATTESTATIONS)
    }

    /// Add signer
    pub fn add_signer(&mut self, public_key: [u8; 32], stake: u64) {
        let info = SignerInfo {
            public_key,
            stake,
            active: true,
            added_at: Utc::now(),
        };
        self.total_stake += stake;
        self.signers.insert(public_key, info);
    }

    /// Remove signer
    pub fn remove_signer(&mut self, public_key: &[u8; 32]) {
        if let Some(info) = self.signers.remove(public_key) {
            self.total_stake -= info.stake;
        }
    }

    /// Check if public key is a valid signer
    pub fn is_valid_signer(&self, public_key: &[u8; 32]) -> bool {
        self.signers
            .get(public_key)
            .map(|s| s.active)
            .unwrap_or(false)
    }

    /// Get required stake for confirmation
    pub fn required_stake(&self) -> u64 {
        (self.total_stake as u128 * self.threshold_percent as u128 / 100) as u64
    }

    /// Check if attestation set meets threshold
    pub fn meets_threshold(&self, attestations: &AttestationSet) -> bool {
        // Must have minimum attestations
        if attestations.count() < self.min_attestations {
            return false;
        }

        // Check stake threshold
        attestations.total_stake >= self.required_stake()
    }

    /// Get signer count
    pub fn count(&self) -> usize {
        self.signers.len()
    }
}

/// Receipt verifier
pub struct ReceiptVerifier {
    /// Signer set
    signer_set: Arc<RwLock<SignerSet>>,
}

impl ReceiptVerifier {
    /// Create new verifier
    pub fn new(signer_set: SignerSet) -> Self {
        Self {
            signer_set: Arc::new(RwLock::new(signer_set)),
        }
    }

    /// Verify receipt has sufficient attestations
    pub fn verify(&self, receipt: &CrossChainReceipt) -> MiniAppResult<()> {
        let signer_set = self.signer_set.read();

        // Check consistency
        if !receipt.attestations.is_consistent() {
            return Err(MiniAppError::ReceiptVerificationFailed(
                "inconsistent attestations".to_string(),
            ));
        }

        // Verify all attestors are valid signers
        for signer in receipt.attestations.signers() {
            if !signer_set.is_valid_signer(&signer) {
                return Err(MiniAppError::InvalidAttestation(format!(
                    "unknown signer: {}",
                    hex::encode(&signer[..8])
                )));
            }
        }

        // Check threshold
        if !signer_set.meets_threshold(&receipt.attestations) {
            return Err(MiniAppError::InsufficientAttestations {
                count: receipt.attestations.count(),
                required: signer_set.min_attestations,
            });
        }

        Ok(())
    }

    /// Add signer
    pub fn add_signer(&self, public_key: [u8; 32], stake: u64) {
        self.signer_set.write().add_signer(public_key, stake);
    }

    /// Remove signer
    pub fn remove_signer(&self, public_key: &[u8; 32]) {
        self.signer_set.write().remove_signer(public_key);
    }

    /// Get required attestations count
    pub fn required_attestations(&self) -> usize {
        self.signer_set.read().min_attestations
    }

    /// Get threshold percentage
    pub fn threshold_percent(&self) -> u8 {
        self.signer_set.read().threshold_percent
    }
}

/// Receipt store
pub struct ReceiptStore {
    /// Receipts by ID
    receipts: RwLock<HashMap<ReceiptId, CrossChainReceipt>>,
    /// Receipts by intent ID
    by_intent: RwLock<HashMap<IntentId, ReceiptId>>,
    /// Verifier
    verifier: ReceiptVerifier,
}

impl ReceiptStore {
    /// Create new store
    pub fn new(signer_set: SignerSet) -> Self {
        Self {
            receipts: RwLock::new(HashMap::new()),
            by_intent: RwLock::new(HashMap::new()),
            verifier: ReceiptVerifier::new(signer_set),
        }
    }

    /// Submit new receipt
    pub fn submit(&self, receipt: Receipt) -> MiniAppResult<ReceiptId> {
        let id = receipt.id;
        let intent_id = receipt.intent_id;

        // Check for duplicate
        if self.by_intent.read().contains_key(&intent_id) {
            return Err(MiniAppError::ReceiptVerificationFailed(
                "receipt already exists for intent".to_string(),
            ));
        }

        let cross_chain = CrossChainReceipt::new(receipt);

        self.receipts.write().insert(id, cross_chain);
        self.by_intent.write().insert(intent_id, id);

        Ok(id)
    }

    /// Add attestation to receipt
    pub fn add_attestation(
        &self,
        receipt_id: &ReceiptId,
        attestation: Attestation,
    ) -> MiniAppResult<()> {
        let mut receipts = self.receipts.write();
        let receipt = receipts
            .get_mut(receipt_id)
            .ok_or_else(|| MiniAppError::ReceiptNotFound(receipt_id.to_string()))?;

        receipt.add_attestation(attestation)?;

        // Check if now verified
        if self.verifier.verify(receipt).is_ok() {
            receipt.receipt.confirm();
        }

        Ok(())
    }

    /// Get receipt by ID
    pub fn get(&self, id: &ReceiptId) -> Option<CrossChainReceipt> {
        self.receipts.read().get(id).cloned()
    }

    /// Get receipt by intent ID
    pub fn get_by_intent(&self, intent_id: &IntentId) -> Option<CrossChainReceipt> {
        let receipt_id = self.by_intent.read().get(intent_id).cloned()?;
        self.get(&receipt_id)
    }

    /// Verify receipt
    pub fn verify(&self, receipt_id: &ReceiptId) -> MiniAppResult<()> {
        let receipts = self.receipts.read();
        let receipt = receipts
            .get(receipt_id)
            .ok_or_else(|| MiniAppError::ReceiptNotFound(receipt_id.to_string()))?;

        self.verifier.verify(receipt)
    }

    /// Get pending receipts
    pub fn get_pending(&self) -> Vec<CrossChainReceipt> {
        self.receipts
            .read()
            .values()
            .filter(|r| r.receipt.status == ReceiptStatus::Pending)
            .cloned()
            .collect()
    }

    /// Get confirmed receipts
    pub fn get_confirmed(&self) -> Vec<CrossChainReceipt> {
        self.receipts
            .read()
            .values()
            .filter(|r| r.receipt.status == ReceiptStatus::Confirmed)
            .cloned()
            .collect()
    }

    /// Finalize receipt (after on-chain confirmation)
    pub fn finalize(&self, receipt_id: &ReceiptId) -> MiniAppResult<()> {
        let mut receipts = self.receipts.write();
        let receipt = receipts
            .get_mut(receipt_id)
            .ok_or_else(|| MiniAppError::ReceiptNotFound(receipt_id.to_string()))?;

        if receipt.receipt.status != ReceiptStatus::Confirmed {
            return Err(MiniAppError::ReceiptVerificationFailed(
                "receipt not confirmed".to_string(),
            ));
        }

        receipt.receipt.finalize();
        Ok(())
    }

    /// Get verifier
    pub fn verifier(&self) -> &ReceiptVerifier {
        &self.verifier
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn create_test_receipt() -> Receipt {
        let intent_id = IntentId::new();
        let result = ExecutionResult {
            success: true,
            return_data: None,
            error: None,
            logs: vec!["test log".to_string()],
            account_changes: vec![],
            compute_units: 1000,
            fee: 5000,
        };

        Receipt::new(intent_id, result, 100, [1u8; 32], [2u8; 32])
    }

    #[test]
    fn test_receipt_hash() {
        let receipt = create_test_receipt();
        let hash1 = receipt.hash();
        let hash2 = receipt.hash();
        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_attestation_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let receipt = create_test_receipt();
        let receipt_hash = receipt.hash();

        let attestation = Attestation::create(&signing_key, receipt_hash, 1000);
        assert!(attestation.verify().is_ok());
    }

    #[test]
    fn test_attestation_set() {
        let mut set = AttestationSet::new();

        // Add attestations
        for i in 0..3 {
            let signing_key = SigningKey::generate(&mut OsRng);
            let attestation = Attestation::create(&signing_key, [i; 32], 1000);
            set.add(attestation).unwrap();
        }

        assert_eq!(set.count(), 3);
        assert_eq!(set.total_stake, 3000);
    }

    #[test]
    fn test_duplicate_attestation() {
        let mut set = AttestationSet::new();
        let signing_key = SigningKey::generate(&mut OsRng);

        let attestation1 = Attestation::create(&signing_key, [1u8; 32], 1000);
        let attestation2 = Attestation::create(&signing_key, [1u8; 32], 1000);

        set.add(attestation1).unwrap();
        assert!(set.add(attestation2).is_err());
    }

    #[test]
    fn test_signer_set() {
        let mut signer_set = SignerSet::new(67, 3);

        // Add signers
        for i in 0..5 {
            signer_set.add_signer([i; 32], 1000);
        }

        assert_eq!(signer_set.count(), 5);
        assert_eq!(signer_set.total_stake, 5000);

        // Required stake is 67% of 5000 = 3350
        assert_eq!(signer_set.required_stake(), 3350);
    }

    #[test]
    fn test_receipt_verification() {
        // Create signer set
        let mut signer_set = SignerSet::new(67, 3);
        let mut signing_keys = Vec::new();

        for _ in 0..5 {
            let signing_key = SigningKey::generate(&mut OsRng);
            let pubkey = signing_key.verifying_key().to_bytes();
            signer_set.add_signer(pubkey, 1000);
            signing_keys.push(signing_key);
        }

        let verifier = ReceiptVerifier::new(signer_set);

        // Create receipt
        let receipt = create_test_receipt();
        let receipt_hash = receipt.hash();
        let mut cross_chain = CrossChainReceipt::new(receipt);

        // Add 3 attestations (should not meet 67% threshold)
        for i in 0..3 {
            let attestation = Attestation::create(&signing_keys[i], receipt_hash, 1000);
            cross_chain.add_attestation(attestation).unwrap();
        }

        // Should not meet threshold yet (3000/5000 = 60%)
        assert!(verifier.verify(&cross_chain).is_err());

        // Add 4th attestation
        let attestation = Attestation::create(&signing_keys[3], receipt_hash, 1000);
        cross_chain.add_attestation(attestation).unwrap();

        // Now should meet threshold (4000/5000 = 80%)
        assert!(verifier.verify(&cross_chain).is_ok());
    }

    #[test]
    fn test_receipt_store() {
        let signer_set = SignerSet::default_thresholds();
        let store = ReceiptStore::new(signer_set);

        let receipt = create_test_receipt();
        let intent_id = receipt.intent_id;

        let receipt_id = store.submit(receipt).unwrap();

        // Get by ID
        let loaded = store.get(&receipt_id);
        assert!(loaded.is_some());

        // Get by intent ID
        let by_intent = store.get_by_intent(&intent_id);
        assert!(by_intent.is_some());
        assert_eq!(by_intent.unwrap().receipt.id, receipt_id);
    }
}
