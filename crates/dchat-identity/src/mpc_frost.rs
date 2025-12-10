// FROST (Flexible Round-Optimized Schnorr Threshold) Signatures
// Production-grade threshold signature implementation for dchat MPC
//
// This module provides threshold signature functionality for dchat using the
// audited frost-ed25519 crate (NCC Group Security Audit, April 2023).
//
// FROST Protocol Overview:
// - Two-round signing protocol (preprocessing + signing)
// - t-of-n threshold (default: 2-of-3 for user device, cloud, recovery)
// - Compatible with standard Ed25519 public keys and signatures
//
// Security Properties:
// - Existentially unforgeable under chosen message attack (EU-CMA)
// - Robustness against malicious signers (abort detection)
// - Forward secrecy via nonce commitment
//
// PRODUCTION STATUS: ✅ Uses audited frost-ed25519 v2.x library
//
// Paper: "FROST: Flexible Round-Optimized Schnorr Threshold Signatures" (Komlo & Goldberg, 2020)

use frost_ed25519 as frost;
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use thiserror::Error;

/// Helper function to convert frost::Identifier to ParticipantId (u16)
fn identifier_to_u16(id: &frost::Identifier) -> Result<u16, FrostError> {
    // Serialize and convert from the serialized bytes
    let bytes = id.serialize();
    // frost::Identifier is a scalar, we use the first 2 bytes as the u16
    // In practice, participant IDs are small positive integers
    if bytes.len() >= 2 {
        Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
    } else if !bytes.is_empty() {
        Ok(bytes[0] as u16)
    } else {
        Err(FrostError::DkgFailed("Invalid identifier".to_string()))
    }
}

/// Helper function to convert ParticipantId (u16) to frost::Identifier
fn u16_to_identifier(pid: u16) -> Result<frost::Identifier, FrostError> {
    frost::Identifier::try_from(pid)
        .map_err(|e| FrostError::SigningFailed(format!("Invalid identifier: {:?}", e)))
}

/// FROST-specific errors
#[derive(Error, Debug)]
pub enum FrostError {
    #[error("Insufficient signers: need {required}, have {available}")]
    InsufficientSigners { required: u16, available: u16 },

    #[error("Invalid signature share from signer {0}")]
    InvalidSignatureShare(u16),

    #[error("DKG failed: {0}")]
    DkgFailed(String),

    #[error("Signing failed: {0}")]
    SigningFailed(String),

    #[error("Signer {0} not found")]
    SignerNotFound(u16),

    #[error("Round 1 commitment missing for signer {0}")]
    Round1Missing(u16),

    #[error("Round 2 signature share missing for signer {0}")]
    Round2Missing(u16),

    #[error("Serialization error: {0}")]
    SerializationError(String),

    #[error("Timeout waiting for signers")]
    Timeout,

    #[error("Invalid configuration: {0}")]
    InvalidConfig(String),
}

/// FROST configuration for threshold signatures
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostConfig {
    /// Minimum number of signers required (threshold)
    pub min_signers: u16,
    /// Maximum number of signers (total)
    pub max_signers: u16,
    /// Timeout for signature collection (seconds)
    pub timeout_seconds: u64,
}

impl Default for FrostConfig {
    fn default() -> Self {
        Self {
            min_signers: 2,
            max_signers: 3,
            timeout_seconds: 30,
        }
    }
}

impl FrostConfig {
    /// Validate configuration
    pub fn validate(&self) -> Result<(), FrostError> {
        if self.min_signers == 0 {
            return Err(FrostError::InvalidConfig(
                "Threshold must be at least 1".to_string(),
            ));
        }
        if self.min_signers > self.max_signers {
            return Err(FrostError::InvalidConfig(format!(
                "Threshold ({}) cannot exceed max signers ({})",
                self.min_signers, self.max_signers
            )));
        }
        if self.max_signers > 255 {
            return Err(FrostError::InvalidConfig(
                "Max signers cannot exceed 255".to_string(),
            ));
        }
        Ok(())
    }
}

/// Participant identifier (1-indexed, as per FROST spec)
pub type ParticipantId = u16;

/// DKG Round 1 package (commitment to secret polynomial)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgRound1Package {
    pub sender_id: ParticipantId,
    /// Serialized commitment (proof of secret sharing)
    pub commitment_data: Vec<u8>,
}

/// DKG Round 2 package (secret shares for each participant)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DkgRound2Package {
    pub sender_id: ParticipantId,
    /// Map of participant_id -> encrypted secret share
    pub shares: BTreeMap<ParticipantId, Vec<u8>>,
}

/// Completed DKG result with key share and public verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostKeyShare {
    /// Participant's unique identifier
    pub participant_id: ParticipantId,
    /// This participant's secret key share (KEEP SECRET!)
    pub secret_key_share: Vec<u8>,
    /// Group public key (same for all participants)
    pub group_public_key: Vec<u8>,
    /// Public key shares of all participants (for verification)
    pub public_key_shares: BTreeMap<ParticipantId, Vec<u8>>,
    /// Verification key (group public key in different format)
    pub verifying_key: Vec<u8>,
}

/// Signing Round 1 commitment (nonce commitment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningRound1Package {
    pub signer_id: ParticipantId,
    /// Nonce commitment (hiding + binding)
    pub commitment_data: Vec<u8>,
}

/// Signing Round 2 signature share
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SigningRound2Package {
    pub signer_id: ParticipantId,
    /// Signature share (response to challenge)
    pub signature_share_data: Vec<u8>,
}

/// Aggregated FROST signature (compatible with Ed25519)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrostSignature {
    /// Final signature bytes (64 bytes for Ed25519)
    pub signature: Vec<u8>,
    /// Participant IDs that contributed to this signature
    pub signers: Vec<ParticipantId>,
}

/// FROST coordinator managing the signing protocol
///
/// Uses the audited frost-ed25519 library for all cryptographic operations.
/// Each participant should run their own coordinator instance in production.
pub struct FrostCoordinator {
    config: FrostConfig,
    /// Key shares for all participants (in production, each only has their own)
    key_shares: BTreeMap<ParticipantId, FrostKeyShare>,
    /// Active signing sessions
    signing_sessions: BTreeMap<String, FrostSigningSession>,
}

/// State for an active FROST signing session
#[derive(Debug, Clone)]
pub struct FrostSigningSession {
    /// Unique session identifier
    pub session_id: String,
    /// Message to be signed
    pub message: Vec<u8>,
    /// Round 1 packages from participants
    pub round1_packages: BTreeMap<ParticipantId, SigningRound1Package>,
    /// Round 2 packages from participants
    pub round2_packages: BTreeMap<ParticipantId, SigningRound2Package>,
    /// When the session started
    pub started_at: i64,
    /// Current session status
    pub status: SessionStatus,
}

/// FROST signing session status
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SessionStatus {
    /// Waiting for round 1 packages
    WaitingForRound1,
    /// Waiting for round 2 packages  
    WaitingForRound2,
    /// Session completed successfully
    Complete,
    /// Session failed
    Failed,
}

impl FrostCoordinator {
    /// Create a new FROST coordinator
    pub fn new(config: FrostConfig) -> Result<Self, FrostError> {
        config.validate()?;
        Ok(Self {
            config,
            key_shares: BTreeMap::new(),
            signing_sessions: BTreeMap::new(),
        })
    }

    /// Perform Distributed Key Generation (DKG) using frost-ed25519 trusted dealer
    ///
    /// Uses frost-ed25519's generate_with_dealer for secure key generation.
    /// For P2P DKG without a trusted dealer, use frost::keys::dkg protocol.
    pub async fn perform_dkg(
        &mut self,
        participant_ids: Vec<ParticipantId>,
    ) -> Result<BTreeMap<ParticipantId, FrostKeyShare>, FrostError> {
        if participant_ids.len() != self.config.max_signers as usize {
            return Err(FrostError::InvalidConfig(format!(
                "Expected {} participants, got {}",
                self.config.max_signers,
                participant_ids.len()
            )));
        }

        // Validate participant IDs are 1-indexed and unique
        for (idx, pid) in participant_ids.iter().enumerate() {
            if *pid == 0 {
                return Err(FrostError::InvalidConfig(
                    "Participant IDs must be 1-indexed (not 0)".to_string(),
                ));
            }
            if participant_ids[..idx].contains(pid) {
                return Err(FrostError::InvalidConfig(format!(
                    "Duplicate participant ID: {}",
                    pid
                )));
            }
        }

        use rand::rngs::OsRng;
        let mut rng = OsRng;
        let max_signers = self.config.max_signers;
        let min_signers = self.config.min_signers;

        // Use frost-ed25519's trusted dealer key generation (audited by NCC Group)
        let (shares, pubkey_package) = frost::keys::generate_with_dealer(
            max_signers,
            min_signers,
            frost::keys::IdentifierList::Default,
            &mut rng,
        ).map_err(|e| FrostError::DkgFailed(format!("FROST DKG failed: {:?}", e)))?;

        // Convert to our FrostKeyShare format
        let mut key_shares = BTreeMap::new();
        let group_public_key = pubkey_package.verifying_key().serialize()
            .map_err(|e| FrostError::DkgFailed(format!("Serialization failed: {:?}", e)))?;

        // Build public key shares map
        let mut public_key_shares: BTreeMap<ParticipantId, Vec<u8>> = BTreeMap::new();
        for (id, _share) in shares.iter() {
            let pid = identifier_to_u16(id)?;
            let verifying_share = pubkey_package.verifying_shares()
                .get(id)
                .ok_or_else(|| FrostError::DkgFailed("Missing verifying share".to_string()))?;
            let serialized = verifying_share.serialize()
                .map_err(|e| FrostError::DkgFailed(format!("Serialization failed: {:?}", e)))?;
            public_key_shares.insert(pid, serialized);
        }

        // Store shares for each participant
        for (participant_id, share) in shares {
            let pid = identifier_to_u16(&participant_id)?;
            
            // Serialize the signing share securely
            let key_package = frost::keys::KeyPackage::try_from(share)
                .map_err(|e| FrostError::DkgFailed(format!("KeyPackage creation failed: {:?}", e)))?;
            let secret_share = key_package.signing_share().serialize();
            
            let key_share = FrostKeyShare {
                participant_id: pid,
                secret_key_share: secret_share,
                group_public_key: group_public_key.clone(),
                public_key_shares: public_key_shares.clone(),
                verifying_key: group_public_key.clone(),
            };

            key_shares.insert(pid, key_share.clone());
            self.key_shares.insert(pid, key_share);
        }

        tracing::info!(
            "✅ FROST DKG complete (frost-ed25519): {} participants, threshold {}",
            key_shares.len(),
            min_signers
        );

        Ok(key_shares)
    }

    /// Start a new signing session for P2P FROST signing
    /// 
    /// This creates a session that tracks the multi-round FROST protocol
    /// when participants communicate over the network.
    pub async fn start_signing(
        &mut self,
        message: Vec<u8>,
    ) -> Result<String, FrostError> {
        let available_count = self.key_shares.len() as u16;
        if available_count < self.config.min_signers {
            return Err(FrostError::InsufficientSigners {
                required: self.config.min_signers,
                available: available_count,
            });
        }

        let session_id = self.generate_session_id(&message);

        let session = FrostSigningSession {
            session_id: session_id.clone(),
            message,
            round1_packages: BTreeMap::new(),
            round2_packages: BTreeMap::new(),
            started_at: chrono::Utc::now().timestamp(),
            status: SessionStatus::WaitingForRound1,
        };

        self.signing_sessions.insert(session_id.clone(), session);

        tracing::debug!("Started FROST signing session: {}", session_id);

        Ok(session_id)
    }

    /// Add Round 1 commitment from a signer (for P2P mode)
    pub async fn add_round1_commitment(
        &mut self,
        session_id: &str,
        package: SigningRound1Package,
    ) -> Result<(), FrostError> {
        let session = self
            .signing_sessions
            .get_mut(session_id)
            .ok_or_else(|| FrostError::SignerNotFound(0))?;

        if session.status != SessionStatus::WaitingForRound1 {
            return Err(FrostError::SigningFailed(
                "Not in Round 1 phase".to_string(),
            ));
        }

        session
            .round1_packages
            .insert(package.signer_id, package);

        // Check if we have enough Round 1 packages
        if session.round1_packages.len() >= self.config.min_signers as usize {
            session.status = SessionStatus::WaitingForRound2;
            tracing::debug!(
                "Session {} progressed to Round 2 ({} commitments)",
                session_id,
                session.round1_packages.len()
            );
        }

        Ok(())
    }

    /// Add Round 2 signature share from a signer (for P2P mode)
    pub async fn add_round2_share(
        &mut self,
        session_id: &str,
        package: SigningRound2Package,
    ) -> Result<(), FrostError> {
        let session = self
            .signing_sessions
            .get_mut(session_id)
            .ok_or_else(|| FrostError::SignerNotFound(0))?;

        if session.status != SessionStatus::WaitingForRound2 {
            return Err(FrostError::SigningFailed(
                "Not in Round 2 phase".to_string(),
            ));
        }

        // Verify this signer contributed to Round 1
        if !session.round1_packages.contains_key(&package.signer_id) {
            return Err(FrostError::Round1Missing(package.signer_id));
        }

        session
            .round2_packages
            .insert(package.signer_id, package);

        // Check if we have enough Round 2 packages
        if session.round2_packages.len() >= self.config.min_signers as usize {
            session.status = SessionStatus::Complete;
            tracing::debug!(
                "Session {} complete ({} signature shares)",
                session_id,
                session.round2_packages.len()
            );
        }

        Ok(())
    }

    /// Aggregate signature shares into final FROST signature (for P2P mode)
    /// 
    /// This method aggregates the Round 2 packages collected from participants
    /// into a final FROST signature using the frost-ed25519 library.
    /// 
    /// Note: The round packages contain serialized frost-ed25519 types.
    pub async fn aggregate_signature(
        &self,
        session_id: &str,
    ) -> Result<FrostSignature, FrostError> {
        let session = self
            .signing_sessions
            .get(session_id)
            .ok_or_else(|| FrostError::SignerNotFound(0))?;

        if session.status != SessionStatus::Complete {
            return Err(FrostError::SigningFailed(format!(
                "Session not complete (status: {:?})",
                session.status
            )));
        }

        if session.round2_packages.len() < self.config.min_signers as usize {
            return Err(FrostError::InsufficientSigners {
                required: self.config.min_signers,
                available: session.round2_packages.len() as u16,
            });
        }

        // Collect signer IDs
        let signer_ids: Vec<ParticipantId> = session.round2_packages.keys().copied().collect();

        // Deserialize round1 commitments into frost types
        let mut commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> = BTreeMap::new();
        for (&pid, round1_pkg) in &session.round1_packages {
            let identifier = u16_to_identifier(pid)?;
            let commitments = frost::round1::SigningCommitments::deserialize(&round1_pkg.commitment_data)
                .map_err(|e| FrostError::SigningFailed(format!("Invalid commitments: {:?}", e)))?;
            commitments_map.insert(identifier, commitments);
        }

        // Create signing package
        let signing_package = frost::SigningPackage::new(commitments_map, &session.message);

        // Deserialize signature shares into frost types
        let mut signature_shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare> = BTreeMap::new();
        for (&pid, round2_pkg) in &session.round2_packages {
            let identifier = u16_to_identifier(pid)?;
            let sig_share = frost::round2::SignatureShare::deserialize(&round2_pkg.signature_share_data)
                .map_err(|e| FrostError::SigningFailed(format!("Invalid signature share: {:?}", e)))?;
            signature_shares.insert(identifier, sig_share);
        }

        // Build public key package for aggregation
        let first_key_share = self.key_shares.values().next()
            .ok_or_else(|| FrostError::SigningFailed("No key shares".to_string()))?;
        
        let verifying_key = frost::VerifyingKey::deserialize(&first_key_share.group_public_key)
            .map_err(|e| FrostError::SigningFailed(format!("Invalid verifying key: {:?}", e)))?;
        
        let mut verifying_shares: BTreeMap<frost::Identifier, frost::keys::VerifyingShare> = BTreeMap::new();
        for &pid in &signer_ids {
            let key_share = self.key_shares.get(&pid)
                .ok_or(FrostError::SignerNotFound(pid))?;
            let identifier = u16_to_identifier(pid)?;
            let verifying_share = frost::keys::VerifyingShare::deserialize(
                key_share.public_key_shares.get(&pid)
                    .ok_or_else(|| FrostError::SigningFailed("Missing verifying share".to_string()))?
            ).map_err(|e| FrostError::SigningFailed(format!("Invalid verifying share: {:?}", e)))?;
            verifying_shares.insert(identifier, verifying_share);
        }
        
        let pubkey_package = frost::keys::PublicKeyPackage::new(verifying_shares, verifying_key);

        // Aggregate signature shares using frost-ed25519
        let signature = frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
            .map_err(|e| FrostError::SigningFailed(format!("Aggregation failed: {:?}", e)))?;

        let signature_bytes = signature.serialize()
            .map_err(|e| FrostError::SigningFailed(format!("Serialization failed: {:?}", e)))?;

        tracing::info!(
            "✅ FROST signature aggregated (frost-ed25519): {} signers",
            signer_ids.len()
        );

        Ok(FrostSignature {
            signature: signature_bytes,
            signers: signer_ids,
        })
    }

    /// Generate Round 1 commitment for a participant (for P2P mode)
    ///
    /// Uses frost-ed25519 to generate cryptographically secure nonces
    /// and commitments. Returns serialized commitment data.
    pub async fn generate_round1_commitment(
        &self,
        participant_id: ParticipantId,
        _message: &[u8],
    ) -> Result<(SigningRound1Package, frost::round1::SigningNonces), FrostError> {
        use rand::rngs::OsRng;
        
        let key_share = self
            .key_shares
            .get(&participant_id)
            .ok_or(FrostError::SignerNotFound(participant_id))?;

        let mut rng = OsRng;

        // Reconstruct signing share from stored bytes
        let signing_share = frost::keys::SigningShare::deserialize(&key_share.secret_key_share)
            .map_err(|e| FrostError::SigningFailed(format!("Invalid signing share: {:?}", e)))?;

        // Generate nonces and commitments using frost-ed25519
        let (nonces, commitments) = frost::round1::commit(&signing_share, &mut rng);

        // Serialize commitments for transmission
        let commitment_data = commitments.serialize()
            .map_err(|e| FrostError::SigningFailed(format!("Serialization failed: {:?}", e)))?;

        Ok((SigningRound1Package {
            signer_id: participant_id,
            commitment_data,
        }, nonces))
    }

    /// Generate Round 2 signature share for a participant (for P2P mode)
    /// 
    /// Uses frost-ed25519 to generate the signature share.
    pub async fn generate_round2_share(
        &self,
        participant_id: ParticipantId,
        session_id: &str,
        nonces: &frost::round1::SigningNonces,
    ) -> Result<SigningRound2Package, FrostError> {
        let key_share = self
            .key_shares
            .get(&participant_id)
            .ok_or(FrostError::SignerNotFound(participant_id))?;

        let session = self
            .signing_sessions
            .get(session_id)
            .ok_or_else(|| FrostError::SignerNotFound(0))?;

        // Reconstruct key package
        let identifier = u16_to_identifier(participant_id)?;
        
        let signing_share = frost::keys::SigningShare::deserialize(&key_share.secret_key_share)
            .map_err(|e| FrostError::SigningFailed(format!("Invalid signing share: {:?}", e)))?;
        
        let verifying_share = frost::keys::VerifyingShare::deserialize(
            key_share.public_key_shares.get(&participant_id)
                .ok_or_else(|| FrostError::SigningFailed("Missing verifying share".to_string()))?
        ).map_err(|e| FrostError::SigningFailed(format!("Invalid verifying share: {:?}", e)))?;
        
        let verifying_key = frost::VerifyingKey::deserialize(&key_share.group_public_key)
            .map_err(|e| FrostError::SigningFailed(format!("Invalid verifying key: {:?}", e)))?;
        
        let key_package = frost::keys::KeyPackage::new(
            identifier,
            signing_share,
            verifying_share,
            verifying_key,
            self.config.min_signers,
        );

        // Build commitments map from round1 packages
        let mut commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> = BTreeMap::new();
        for (&pid, round1_pkg) in &session.round1_packages {
            let id = u16_to_identifier(pid)?;
            let commitments = frost::round1::SigningCommitments::deserialize(&round1_pkg.commitment_data)
                .map_err(|e| FrostError::SigningFailed(format!("Invalid commitments: {:?}", e)))?;
            commitments_map.insert(id, commitments);
        }

        // Create signing package
        let signing_package = frost::SigningPackage::new(commitments_map, &session.message);

        // Generate signature share using frost-ed25519
        let signature_share = frost::round2::sign(&signing_package, nonces, &key_package)
            .map_err(|e| FrostError::SigningFailed(format!("Round 2 signing failed: {:?}", e)))?;

        // Serialize signature share for transmission
        let signature_share_data = signature_share.serialize();

        Ok(SigningRound2Package {
            signer_id: participant_id,
            signature_share_data,
        })
    }

    /// Sign a message using FROST (full two-round protocol)
    /// 
    /// PRODUCTION: Uses the audited frost-ed25519 library for signing.
    /// Implements the complete FROST signing protocol with round 1 (commitment)
    /// and round 2 (signature share generation) followed by aggregation.
    pub async fn sign(&mut self, message: Vec<u8>) -> Result<FrostSignature, FrostError> {
        use rand::rngs::OsRng;

        // Select participants (use min_signers, which is the threshold)
        let participant_ids: Vec<ParticipantId> = self
            .key_shares
            .keys()
            .take(self.config.min_signers as usize)
            .copied()
            .collect();

        if participant_ids.len() < self.config.min_signers as usize {
            return Err(FrostError::InsufficientSigners {
                required: self.config.min_signers,
                available: participant_ids.len() as u16,
            });
        }

        let mut rng = OsRng;

        // Build key packages for each participant
        let mut key_packages: BTreeMap<frost::Identifier, frost::keys::KeyPackage> = BTreeMap::new();
        
        for &pid in &participant_ids {
            let key_share = self.key_shares.get(&pid)
                .ok_or(FrostError::SignerNotFound(pid))?;
            
            let identifier = u16_to_identifier(pid)?;
            
            // Reconstruct signing share from stored bytes
            let signing_share = frost::keys::SigningShare::deserialize(&key_share.secret_key_share)
                .map_err(|e| FrostError::SigningFailed(format!("Invalid signing share: {:?}", e)))?;
            
            // Get verifying share
            let verifying_share = frost::keys::VerifyingShare::deserialize(
                key_share.public_key_shares.get(&pid)
                    .ok_or_else(|| FrostError::SigningFailed("Missing verifying share".to_string()))?
            ).map_err(|e| FrostError::SigningFailed(format!("Invalid verifying share: {:?}", e)))?;
            
            // Get group verifying key
            let verifying_key = frost::VerifyingKey::deserialize(&key_share.group_public_key)
                .map_err(|e| FrostError::SigningFailed(format!("Invalid verifying key: {:?}", e)))?;
            
            let key_package = frost::keys::KeyPackage::new(
                identifier,
                signing_share,
                verifying_share,
                verifying_key,
                self.config.min_signers,
            );
            
            key_packages.insert(identifier, key_package);
        }

        // ========================================
        // Round 1: Generate nonces and commitments
        // ========================================
        let mut nonces_map: BTreeMap<frost::Identifier, frost::round1::SigningNonces> = BTreeMap::new();
        let mut commitments_map: BTreeMap<frost::Identifier, frost::round1::SigningCommitments> = BTreeMap::new();

        for (&identifier, key_package) in &key_packages {
            // Generate nonces and commitments using signing share
            let (nonces, commitments) = frost::round1::commit(
                key_package.signing_share(),
                &mut rng,
            );
            nonces_map.insert(identifier, nonces);
            commitments_map.insert(identifier, commitments);
        }

        // ========================================
        // Create the signing package
        // ========================================
        let signing_package = frost::SigningPackage::new(commitments_map, &message);

        // ========================================
        // Round 2: Generate signature shares
        // ========================================
        let mut signature_shares: BTreeMap<frost::Identifier, frost::round2::SignatureShare> = BTreeMap::new();
        
        for (&identifier, key_package) in &key_packages {
            let nonces = nonces_map.get(&identifier)
                .ok_or_else(|| FrostError::SigningFailed("Missing nonces".to_string()))?;
            
            let signature_share = frost::round2::sign(&signing_package, nonces, key_package)
                .map_err(|e| FrostError::SigningFailed(format!("Round 2 signing failed: {:?}", e)))?;
            
            signature_shares.insert(identifier, signature_share);
        }

        // ========================================
        // Aggregate signature shares
        // ========================================
        let first_key_share = self.key_shares.values().next()
            .ok_or_else(|| FrostError::SigningFailed("No key shares".to_string()))?;
        
        // Build verifying key
        let verifying_key = frost::VerifyingKey::deserialize(&first_key_share.group_public_key)
            .map_err(|e| FrostError::SigningFailed(format!("Invalid verifying key: {:?}", e)))?;
        
        // Build verifying shares map for all participants
        let mut verifying_shares: BTreeMap<frost::Identifier, frost::keys::VerifyingShare> = BTreeMap::new();
        for &pid in &participant_ids {
            let key_share = self.key_shares.get(&pid)
                .ok_or(FrostError::SignerNotFound(pid))?;
            let identifier = u16_to_identifier(pid)?;
            let verifying_share = frost::keys::VerifyingShare::deserialize(
                key_share.public_key_shares.get(&pid)
                    .ok_or_else(|| FrostError::SigningFailed("Missing verifying share".to_string()))?
            ).map_err(|e| FrostError::SigningFailed(format!("Invalid verifying share: {:?}", e)))?;
            verifying_shares.insert(identifier, verifying_share);
        }
        
        // Create public key package for aggregation
        let pubkey_package = frost::keys::PublicKeyPackage::new(verifying_shares, verifying_key);
        
        // Aggregate all signature shares into final signature
        let signature = frost::aggregate(&signing_package, &signature_shares, &pubkey_package)
            .map_err(|e| FrostError::SigningFailed(format!("Aggregation failed: {:?}", e)))?;

        // Serialize signature
        let signature_bytes = signature.serialize()
            .map_err(|e| FrostError::SigningFailed(format!("Serialization failed: {:?}", e)))?;

        tracing::info!(
            "✅ FROST signature complete (frost-ed25519): {} signers, threshold {}",
            participant_ids.len(),
            self.config.min_signers
        );

        Ok(FrostSignature {
            signature: signature_bytes,
            signers: participant_ids,
        })
    }

    /// Verify a FROST signature (standard Ed25519 verification)
    /// 
    /// PRODUCTION: Uses frost-ed25519 verification which is compatible
    /// with standard Ed25519 signatures.
    pub fn verify_signature(
        &self,
        signature: &[u8],
        message: &[u8],
        public_key: &[u8],
    ) -> Result<bool, FrostError> {
        if signature.len() != 64 {
            return Ok(false);
        }
        if public_key.len() != 32 {
            return Ok(false);
        }

        // Use frost-ed25519's verification
        let sig = frost::Signature::deserialize(signature)
            .map_err(|_| FrostError::SigningFailed("Invalid signature format".to_string()))?;
        
        let vk = frost::VerifyingKey::deserialize(public_key)
            .map_err(|_| FrostError::SigningFailed("Invalid public key format".to_string()))?;
        
        Ok(vk.verify(message, &sig).is_ok())
    }

    fn generate_session_id(&self, message: &[u8]) -> String {
        use sha2::{Digest, Sha256};
        let mut hasher = Sha256::new();
        hasher.update(b"frost-signing-session");
        hasher.update(message);
        hasher.update(chrono::Utc::now().timestamp().to_le_bytes());
        format!("{:x}", hasher.finalize())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_frost_dkg() {
        let config = FrostConfig::default();
        let mut coordinator = FrostCoordinator::new(config).unwrap();

        let participants = vec![1, 2, 3];
        let key_shares = coordinator.perform_dkg(participants).await.unwrap();

        assert_eq!(key_shares.len(), 3);
        // Verify all key shares have the same group public key
        let group_pk = &key_shares[&1].group_public_key;
        assert!(key_shares.values().all(|ks| &ks.group_public_key == group_pk));
    }

    #[tokio::test]
    async fn test_frost_threshold_signing() {
        let config = FrostConfig::default();
        let mut coordinator = FrostCoordinator::new(config).unwrap();

        // Setup participants
        let participants = vec![1, 2, 3];
        let _key_shares = coordinator.perform_dkg(participants).await.unwrap();

        // Sign a message
        let message = b"Hello, FROST!".to_vec();
        let signature = coordinator.sign(message.clone()).await.unwrap();

        assert_eq!(signature.signature.len(), 64);
        assert_eq!(signature.signers.len(), 2); // threshold = 2

        // Verify signature
        let group_pk = &coordinator.key_shares[&1].group_public_key;
        let valid = coordinator
            .verify_signature(&signature.signature, &message, group_pk)
            .unwrap();
        assert!(valid);
    }

    #[tokio::test]
    async fn test_frost_insufficient_signers() {
        let config = FrostConfig {
            min_signers: 3,
            max_signers: 5,
            timeout_seconds: 30,
        };
        let mut coordinator = FrostCoordinator::new(config).unwrap();

        // Setup all 5 participants for DKG
        let participants = vec![1, 2, 3, 4, 5];
        coordinator.perform_dkg(participants).await.unwrap();

        // Remove some key shares to simulate only 2 available signers
        coordinator.key_shares.remove(&3);
        coordinator.key_shares.remove(&4);
        coordinator.key_shares.remove(&5);

        // Now try to sign with only 2 signers (below threshold of 3)
        let message = b"Test".to_vec();
        let result = coordinator.sign(message).await;

        assert!(matches!(result, Err(FrostError::InsufficientSigners { .. })));
    }

    #[tokio::test]
    async fn test_frost_config_validation() {
        // Invalid: threshold = 0
        let config = FrostConfig {
            min_signers: 0,
            max_signers: 3,
            timeout_seconds: 30,
        };
        assert!(config.validate().is_err());

        // Invalid: threshold > max
        let config = FrostConfig {
            min_signers: 5,
            max_signers: 3,
            timeout_seconds: 30,
        };
        assert!(config.validate().is_err());

        // Valid
        let config = FrostConfig::default();
        assert!(config.validate().is_ok());
    }
}
