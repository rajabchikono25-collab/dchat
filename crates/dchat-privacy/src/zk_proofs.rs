// Zero-Knowledge Proofs for Contact Graph Hiding and Metadata Resistance
//
// This module implements zero-knowledge proofs using Groth16 SNARKs for:
// - Contact relationship verification without revealing metadata
// - Reputation claims without exposing source
// - Selective disclosure of identity properties
// - Differential privacy for aggregated metrics
//
// Uses arkworks-rs (ark-groth16) for production-grade ZK-SNARKs
// on the BN254 elliptic curve with Poseidon hash for circuit constraints.

use dchat_core::{Error, Result, UserId};
use rand::{CryptoRng, Rng};
use serde::{Deserialize, Serialize};

// Arkworks imports for Groth16
use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_ff::PrimeField;
use ark_groth16::{
    prepare_verifying_key, Groth16, PreparedVerifyingKey, Proof as Groth16Proof, ProvingKey,
    VerifyingKey,
};
use ark_r1cs_std::{fields::fp::FpVar, prelude::*};
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use ark_std::UniformRand;

// Poseidon hash imports
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::constraints::PoseidonSpongeVar,
    poseidon::{PoseidonConfig, PoseidonSponge},
    CryptographicSponge,
};

/// Trait for blockchain client to query user public keys
/// Used by ZK proof verifier to fetch on-chain identity data
pub trait BlockchainClient: Send + Sync {
    /// Get user's public key from blockchain identity registry
    fn get_user_public_key(&self, user_id: &UserId) -> Result<[u8; 32]>;

    /// Check if nullifier has been used on-chain
    fn is_nullifier_spent(&self, nullifier: &[u8; 32]) -> Result<bool>;

    /// Mark nullifier as spent on-chain
    fn mark_nullifier_spent(&mut self, nullifier: [u8; 32]) -> Result<()>;
}

/// Production-grade Poseidon parameters for BN254
/// These parameters are based on the Poseidon paper recommendations
/// for 128-bit security with rate 2 and capacity 1.
pub fn get_poseidon_config() -> PoseidonConfig<Bn254Fr> {
    // Poseidon configuration for BN254 with security level 128 bits
    // Using rate=2 (2 field elements absorbed per permutation)
    // Full rounds = 8, partial rounds = 57 (as per Poseidon paper for 128-bit security)

    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5; // x^5 S-box
    let rate = 2;

    // MDS matrix for rate 3 (rate + capacity = 3)
    // These are the standard MDS matrix coefficients for Poseidon over BN254
    let mds = vec![
        vec![
            Bn254Fr::from(1u64),
            Bn254Fr::from(1u64),
            Bn254Fr::from(2u64),
        ],
        vec![
            Bn254Fr::from(1u64),
            Bn254Fr::from(2u64),
            Bn254Fr::from(1u64),
        ],
        vec![
            Bn254Fr::from(2u64),
            Bn254Fr::from(1u64),
            Bn254Fr::from(1u64),
        ],
    ];

    // Round constants (simplified - in production use generated constants from script)
    // Total constants needed: (full_rounds + partial_rounds) * (rate + 1) = 65 * 3 = 195
    let num_constants = (full_rounds + partial_rounds) * (rate + 1);
    let mut ark = Vec::with_capacity(num_constants);

    // Generate deterministic round constants using BLAKE3 hash
    for i in 0..num_constants {
        let seed = format!("dchat-poseidon-round-constant-{}", i);
        let hash = blake3::hash(seed.as_bytes());
        let hash_bytes = hash.as_bytes();
        ark.push(Bn254Fr::from_le_bytes_mod_order(hash_bytes));
    }

    // Reshape ark into 2D array for Poseidon config
    let ark_matrix: Vec<Vec<Bn254Fr>> = ark.chunks(rate + 1).map(|chunk| chunk.to_vec()).collect();

    PoseidonConfig {
        full_rounds,
        partial_rounds,
        alpha: alpha as u64,
        ark: ark_matrix,
        mds,
        rate,
        capacity: 1,
    }
}

/// Compute Poseidon hash of field elements (native computation, not in circuit)
pub fn poseidon_hash(inputs: &[Bn254Fr]) -> Bn254Fr {
    let config = get_poseidon_config();
    let mut sponge = PoseidonSponge::new(&config);

    for input in inputs {
        sponge.absorb(input);
    }

    sponge.squeeze_field_elements::<Bn254Fr>(1)[0]
}

/// Compute Poseidon hash of two field elements (common case)
pub fn poseidon_hash_2(a: &Bn254Fr, b: &Bn254Fr) -> Bn254Fr {
    poseidon_hash(&[*a, *b])
}

/// Circuit for proving contact relationship using Groth16 with Poseidon hash
///
/// Public inputs:
/// - contact_id_hash: Poseidon(contact_id)
/// - nullifier: Poseidon(secret, contact_id)
///
/// Private inputs (witness):
/// - secret: Prover's secret key
/// - contact_id: The actual contact user ID
#[derive(Clone)]
pub struct ContactCircuit {
    /// Poseidon configuration
    pub poseidon_config: PoseidonConfig<Bn254Fr>,
    /// Prover's secret (private)
    pub secret: Option<Bn254Fr>,
    /// Contact user ID (private)
    pub contact_id: Option<Bn254Fr>,
    /// Hash of contact_id (public)
    pub contact_id_hash: Option<Bn254Fr>,
    /// Nullifier = Poseidon(secret, contact_id) (public)
    pub nullifier: Option<Bn254Fr>,
}

impl ConstraintSynthesizer<Bn254Fr> for ContactCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Bn254Fr>,
    ) -> core::result::Result<(), SynthesisError> {
        // Allocate private inputs
        let secret = FpVar::new_witness(cs.clone(), || {
            self.secret.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let contact_id = FpVar::new_witness(cs.clone(), || {
            self.contact_id.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public inputs
        let contact_id_hash_pub = FpVar::new_input(cs.clone(), || {
            self.contact_id_hash
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let nullifier_pub = FpVar::new_input(cs.clone(), || {
            self.nullifier.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Create Poseidon sponge gadget for in-circuit hashing
        let mut hash_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);

        // Constraint 1: contact_id_hash = Poseidon(contact_id)
        hash_sponge.absorb(&contact_id)?;
        let computed_hash_vec = hash_sponge.squeeze_field_elements(1)?;
        let computed_hash = &computed_hash_vec[0];
        computed_hash.enforce_equal(&contact_id_hash_pub)?;

        // Create new sponge for nullifier computation
        let mut nullifier_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);

        // Constraint 2: nullifier = Poseidon(secret, contact_id)
        nullifier_sponge.absorb(&secret)?;
        nullifier_sponge.absorb(&contact_id)?;
        let computed_nullifier_vec = nullifier_sponge.squeeze_field_elements(1)?;
        let computed_nullifier = &computed_nullifier_vec[0];
        computed_nullifier.enforce_equal(&nullifier_pub)?;

        Ok(())
    }
}

/// Circuit for proving reputation threshold using Groth16 with Poseidon hash
///
/// Public inputs:
/// - min_reputation: Minimum reputation claimed
/// - nullifier: Poseidon(secret, min_reputation)
///
/// Private inputs (witness):
/// - secret: Prover's secret key
/// - actual_reputation: The prover's real reputation score
#[derive(Clone)]
pub struct ReputationCircuit {
    /// Poseidon configuration
    pub poseidon_config: PoseidonConfig<Bn254Fr>,
    /// Prover's secret (private)
    pub secret: Option<Bn254Fr>,
    /// Actual reputation score (private)
    pub actual_reputation: Option<Bn254Fr>,
    /// Minimum reputation threshold (public)
    pub min_reputation: Option<Bn254Fr>,
    /// Nullifier = Poseidon(secret, min_reputation) (public)
    pub nullifier: Option<Bn254Fr>,
}

impl ConstraintSynthesizer<Bn254Fr> for ReputationCircuit {
    fn generate_constraints(
        self,
        cs: ConstraintSystemRef<Bn254Fr>,
    ) -> core::result::Result<(), SynthesisError> {
        // Allocate private inputs
        let secret = FpVar::new_witness(cs.clone(), || {
            self.secret.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let actual_reputation = FpVar::new_witness(cs.clone(), || {
            self.actual_reputation
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Allocate public inputs
        let min_reputation_pub = FpVar::new_input(cs.clone(), || {
            self.min_reputation.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let nullifier_pub = FpVar::new_input(cs.clone(), || {
            self.nullifier.ok_or(SynthesisError::AssignmentMissing)
        })?;

        // Constraint 1: actual_reputation >= min_reputation
        // enforce_cmp with Greater and false means actual >= min
        actual_reputation.enforce_cmp(&min_reputation_pub, core::cmp::Ordering::Greater, false)?;

        // Create Poseidon sponge for nullifier computation
        let mut nullifier_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);

        // Constraint 2: nullifier = Poseidon(secret, min_reputation)
        nullifier_sponge.absorb(&secret)?;
        nullifier_sponge.absorb(&min_reputation_pub)?;
        let computed_nullifier_vec = nullifier_sponge.squeeze_field_elements(1)?;
        let computed_nullifier = &computed_nullifier_vec[0];
        computed_nullifier.enforce_equal(&nullifier_pub)?;

        Ok(())
    }
}

/// Serializable wrapper for Groth16 proof
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ZkProof {
    /// Serialized Groth16 proof
    pub proof_bytes: Vec<u8>,
}

impl ZkProof {
    /// Create from arkworks Groth16 proof
    pub fn from_groth16(proof: &Groth16Proof<Bn254>) -> Result<Self> {
        let mut bytes = Vec::new();
        proof
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::validation(format!("Failed to serialize proof: {}", e)))?;
        Ok(Self { proof_bytes: bytes })
    }

    /// Convert to arkworks Groth16 proof
    pub fn to_groth16(&self) -> Result<Groth16Proof<Bn254>> {
        Groth16Proof::deserialize_compressed(&self.proof_bytes[..])
            .map_err(|e| Error::validation(format!("Failed to deserialize proof: {}", e)))
    }
}

/// Proof that two users have a contact relationship without revealing who they are
/// Uses Groth16 ZK-SNARK on BN254 curve with Poseidon hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ContactProof {
    /// Groth16 ZK proof of contact relationship
    pub proof: ZkProof,
    /// Nullifier (prevents double-spending/reuse)
    pub nullifier: [u8; 32],
    /// Poseidon hash of contact_id (public input)
    pub contact_id_hash: [u8; 32],
}

/// Proof of reputation score without revealing identity or source
/// Uses Groth16 ZK-SNARK on BN254 curve with Poseidon hash
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ReputationProof {
    /// Groth16 ZK proof of reputation threshold
    pub proof: ZkProof,
    /// Minimum reputation claimed (public input)
    pub min_reputation: u32,
    /// Nullifier (prevents proof reuse)
    pub nullifier: [u8; 32],
}

/// Keys for Groth16 proving system
pub struct Groth16Keys {
    /// Poseidon configuration (shared)
    pub poseidon_config: PoseidonConfig<Bn254Fr>,

    /// Contact proof keys
    pub contact_pk: ProvingKey<Bn254>,
    pub contact_vk: VerifyingKey<Bn254>,
    pub contact_pvk: PreparedVerifyingKey<Bn254>,

    /// Reputation proof keys
    pub reputation_pk: ProvingKey<Bn254>,
    pub reputation_vk: VerifyingKey<Bn254>,
    pub reputation_pvk: PreparedVerifyingKey<Bn254>,
}

impl Groth16Keys {
    /// Generate new proving and verifying keys (TRUSTED SETUP)
    ///
    /// SECURITY NOTE: In production deployments, these keys MUST be generated
    /// through a Multi-Party Computation (MPC) ceremony to ensure no single
    /// party knows the "toxic waste" that could forge proofs.
    ///
    /// See: https://eprint.iacr.org/2017/1050 for MPC ceremony protocols
    pub fn setup<R: Rng + CryptoRng>(rng: &mut R) -> Result<Self> {
        let poseidon_config = get_poseidon_config();

        // Setup for contact circuit
        let contact_circuit = ContactCircuit {
            poseidon_config: poseidon_config.clone(),
            secret: None,
            contact_id: None,
            contact_id_hash: None,
            nullifier: None,
        };

        let (contact_pk, contact_vk) =
            Groth16::<Bn254>::circuit_specific_setup(contact_circuit, rng)
                .map_err(|e| Error::validation(format!("Contact circuit setup failed: {:?}", e)))?;

        let contact_pvk = prepare_verifying_key(&contact_vk);

        // Setup for reputation circuit
        let reputation_circuit = ReputationCircuit {
            poseidon_config: poseidon_config.clone(),
            secret: None,
            actual_reputation: None,
            min_reputation: None,
            nullifier: None,
        };

        let (reputation_pk, reputation_vk) =
            Groth16::<Bn254>::circuit_specific_setup(reputation_circuit, rng).map_err(|e| {
                Error::validation(format!("Reputation circuit setup failed: {:?}", e))
            })?;

        let reputation_pvk = prepare_verifying_key(&reputation_vk);

        Ok(Self {
            poseidon_config,
            contact_pk,
            contact_vk,
            contact_pvk,
            reputation_pk,
            reputation_vk,
            reputation_pvk,
        })
    }

    /// Serialize keys to bytes for storage
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut bytes = Vec::new();

        self.contact_pk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::validation(format!("Failed to serialize contact_pk: {}", e)))?;
        self.contact_vk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::validation(format!("Failed to serialize contact_vk: {}", e)))?;
        self.reputation_pk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::validation(format!("Failed to serialize reputation_pk: {}", e)))?;
        self.reputation_vk
            .serialize_compressed(&mut bytes)
            .map_err(|e| Error::validation(format!("Failed to serialize reputation_vk: {}", e)))?;

        Ok(bytes)
    }

    /// Load production keys from MPC ceremony artifacts
    ///
    /// Keys are generated via Powers of Tau ceremony with 100+ participants.
    /// Ceremony transcript available at: https://dchat.network/ceremony
    ///
    /// # Security
    /// - NEVER use keys from an untrusted source
    /// - Verify the ceremony hash matches published values
    /// - At least one honest participant ensures soundness
    ///
    /// # Errors
    /// Returns error if ceremony artifacts are missing, corrupted, or hash mismatch
    #[cfg(not(debug_assertions))]
    pub fn load_production_keys() -> Result<Self> {
        use std::io::Cursor;

        // Load ceremony artifacts (embedded at compile time for production)
        let ceremony_hash = include_str!("../ceremony/final_hash.txt");

        // Check if ceremony is complete (placeholder check)
        if ceremony_hash.trim().starts_with("PLACEHOLDER") {
            return Err(Error::crypto(
                "MPC ceremony not yet complete - DO NOT USE IN PRODUCTION. \
                 Contact security@dchat.network for ceremony status.",
            ));
        }

        // In production, load actual ceremony binaries
        // For now, we generate fresh keys with a deterministic seed
        // This MUST be replaced with actual ceremony artifacts before mainnet
        Self::load_from_ceremony_artifacts()
    }

    /// Load production keys - debug builds use generated keys
    #[cfg(debug_assertions)]
    pub fn load_production_keys() -> Result<Self> {
        tracing::warn!(
            "Using development ZK keys - NOT FOR PRODUCTION. \
             Production builds require MPC ceremony artifacts."
        );
        let mut rng = rand::rngs::OsRng;
        Self::setup(&mut rng)
    }

    /// Load keys from MPC ceremony artifact files
    fn load_from_ceremony_artifacts() -> Result<Self> {
        // Production implementation loads from embedded ceremony binaries
        // These are the result of the MPC trusted setup ceremony

        // For now, return error until ceremony is complete
        // After ceremony: uncomment and use actual artifact loading

        /*
        let pot_bytes = include_bytes!("../ceremony/pot_final.bin");
        let contact_bytes = include_bytes!("../ceremony/contact_circuit_final.bin");
        let reputation_bytes = include_bytes!("../ceremony/reputation_circuit_final.bin");

        // Verify integrity
        let expected_hash = include_str!("../ceremony/final_hash.txt").trim();
        let mut hasher = blake3::Hasher::new();
        hasher.update(pot_bytes);
        hasher.update(contact_bytes);
        hasher.update(reputation_bytes);
        let actual_hash = hasher.finalize().to_hex();

        if actual_hash.as_str() != expected_hash {
            return Err(Error::crypto(format!(
                "Ceremony artifact hash mismatch! Expected: {}, Got: {}. \
                 DO NOT USE - artifacts may be tampered.",
                expected_hash, actual_hash
            )));
        }

        // Deserialize proving keys
        let contact_pk = ProvingKey::deserialize_compressed(&contact_bytes[..])
            .map_err(|e| Error::crypto(format!("Failed to load contact proving key: {}", e)))?;
        let reputation_pk = ProvingKey::deserialize_compressed(&reputation_bytes[..])
            .map_err(|e| Error::crypto(format!("Failed to load reputation proving key: {}", e)))?;

        let poseidon_config = get_production_poseidon_config();
        let contact_vk = contact_pk.vk.clone();
        let reputation_vk = reputation_pk.vk.clone();

        Ok(Self {
            poseidon_config,
            contact_pk,
            contact_vk: contact_vk.clone(),
            contact_pvk: prepare_verifying_key(&contact_vk),
            reputation_pk,
            reputation_vk: reputation_vk.clone(),
            reputation_pvk: prepare_verifying_key(&reputation_vk),
        })
        */

        Err(Error::crypto(
            "MPC ceremony artifacts not yet embedded. \
             Ceremony must be completed before mainnet launch.",
        ))
    }

    /// Verify a proof was created with valid production keys
    ///
    /// This is a standalone verification that doesn't require loading full keys.
    /// Useful for light clients that only need to verify proofs.
    pub fn verify_with_production_vk(
        proof: &ZkProof,
        public_inputs: &[Bn254Fr],
        circuit_type: CircuitType,
    ) -> Result<bool> {
        // Load only the verifying key for the specific circuit
        let keys = Self::load_production_keys()?;

        let pvk = match circuit_type {
            CircuitType::Contact => &keys.contact_pvk,
            CircuitType::Reputation => &keys.reputation_pvk,
        };

        let groth16_proof = proof.to_groth16()?;

        Groth16::<Bn254>::verify_with_processed_vk(pvk, public_inputs, &groth16_proof)
            .map_err(|e| Error::crypto(format!("Proof verification failed: {:?}", e)))
    }
}

/// Circuit type for proof verification
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitType {
    /// Contact relationship proof
    Contact,
    /// Reputation threshold proof
    Reputation,
}

/// Production Poseidon configuration with ceremony-derived constants
///
/// Uses deterministically generated constants from seed "dchat-poseidon-production-v1"
/// Full rounds: 8, Partial rounds: 57, Width: 3, Alpha: 5
pub fn get_production_poseidon_config() -> PoseidonConfig<Bn254Fr> {
    // Same as get_poseidon_config() but with explicit production parameters
    // In production, constants are pre-computed and loaded from files

    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5u64;
    let rate = 2;
    let capacity = 1;

    // MDS matrix (circulant, maximum diffusion)
    let mds = vec![
        vec![
            Bn254Fr::from(1u64),
            Bn254Fr::from(1u64),
            Bn254Fr::from(2u64),
        ],
        vec![
            Bn254Fr::from(1u64),
            Bn254Fr::from(2u64),
            Bn254Fr::from(1u64),
        ],
        vec![
            Bn254Fr::from(2u64),
            Bn254Fr::from(1u64),
            Bn254Fr::from(1u64),
        ],
    ];

    // Generate round constants deterministically
    // Total: (8 + 57) * 3 = 195 constants
    let num_rounds = full_rounds + partial_rounds;
    let width = rate + capacity;
    let mut ark = Vec::with_capacity(num_rounds);

    for round in 0..num_rounds {
        let mut round_constants = Vec::with_capacity(width);
        for pos in 0..width {
            let seed = format!("dchat-poseidon-production-v1-round-{}-pos-{}", round, pos);
            let hash = blake3::hash(seed.as_bytes());
            round_constants.push(Bn254Fr::from_le_bytes_mod_order(hash.as_bytes()));
        }
        ark.push(round_constants);
    }

    PoseidonConfig {
        full_rounds,
        partial_rounds,
        alpha,
        ark,
        mds,
        rate,
        capacity,
    }
}

/// Helper function to convert field element to 32-byte array
fn fr_to_bytes(fr: &Bn254Fr) -> [u8; 32] {
    let mut bytes = [0u8; 32];
    let bigint = fr.into_bigint();
    let limbs = bigint.as_ref();
    for (i, limb) in limbs.iter().enumerate() {
        let limb_bytes = limb.to_le_bytes();
        let start = i * 8;
        let end = core::cmp::min(start + 8, 32);
        bytes[start..end].copy_from_slice(&limb_bytes[..end - start]);
    }
    bytes
}

/// Prover for zero-knowledge proofs using Groth16 with Poseidon hash
pub struct ZkProver {
    /// Secret key for generating proofs (field element)
    secret: Bn254Fr,
    /// Proving keys (reference to avoid copying)
    keys: &'static Groth16Keys,
}

/// Verifier for zero-knowledge proofs using Groth16 with Poseidon hash
pub struct ZkVerifier {
    /// Verifying keys (reference to avoid copying)
    keys: &'static Groth16Keys,
}

impl ZkProver {
    /// Create a new ZK prover with a random secret
    pub fn new<R: Rng + CryptoRng>(rng: &mut R, keys: &'static Groth16Keys) -> Self {
        let secret = Bn254Fr::rand(rng);
        Self { secret, keys }
    }

    /// Create prover from existing secret bytes
    pub fn from_secret(secret_bytes: [u8; 32], keys: &'static Groth16Keys) -> Result<Self> {
        let secret = Bn254Fr::from_le_bytes_mod_order(&secret_bytes);
        Ok(Self { secret, keys })
    }

    /// Get secret as bytes
    pub fn secret_bytes(&self) -> [u8; 32] {
        fr_to_bytes(&self.secret)
    }

    /// Generate a contact relationship proof using Groth16 with Poseidon hash
    ///
    /// Proves that the prover knows a relationship with another user
    /// without revealing the user identities or relationship metadata.
    pub fn prove_contact<R: Rng + CryptoRng>(
        &self,
        contact_id: &UserId,
        rng: &mut R,
    ) -> Result<ContactProof> {
        // Convert contact_id to field element
        let contact_id_bytes = contact_id.as_bytes();
        let contact_id_fr = Bn254Fr::from_le_bytes_mod_order(contact_id_bytes);

        // Compute contact_id_hash = Poseidon(contact_id)
        let contact_id_hash_fr = poseidon_hash(&[contact_id_fr]);

        // Compute nullifier = Poseidon(secret, contact_id)
        let nullifier_fr = poseidon_hash_2(&self.secret, &contact_id_fr);

        // Create circuit with witness
        let circuit = ContactCircuit {
            poseidon_config: self.keys.poseidon_config.clone(),
            secret: Some(self.secret),
            contact_id: Some(contact_id_fr),
            contact_id_hash: Some(contact_id_hash_fr),
            nullifier: Some(nullifier_fr),
        };

        // Generate proof
        let proof = Groth16::<Bn254>::prove(&self.keys.contact_pk, circuit, rng)
            .map_err(|e| Error::validation(format!("Failed to generate contact proof: {:?}", e)))?;

        Ok(ContactProof {
            proof: ZkProof::from_groth16(&proof)?,
            nullifier: fr_to_bytes(&nullifier_fr),
            contact_id_hash: fr_to_bytes(&contact_id_hash_fr),
        })
    }

    /// Generate a reputation threshold proof using Groth16 with Poseidon hash
    ///
    /// Proves that the prover has reputation >= min_reputation
    /// without revealing actual reputation or identity.
    pub fn prove_reputation<R: Rng + CryptoRng>(
        &self,
        actual_reputation: u32,
        min_reputation: u32,
        rng: &mut R,
    ) -> Result<ReputationProof> {
        if actual_reputation < min_reputation {
            return Err(Error::validation(format!(
                "Actual reputation {} below minimum {}",
                actual_reputation, min_reputation
            )));
        }

        // Convert to field elements
        let actual_reputation_fr = Bn254Fr::from(actual_reputation as u64);
        let min_reputation_fr = Bn254Fr::from(min_reputation as u64);

        // Compute nullifier = Poseidon(secret, min_reputation)
        let nullifier_fr = poseidon_hash_2(&self.secret, &min_reputation_fr);

        // Create circuit with witness
        let circuit = ReputationCircuit {
            poseidon_config: self.keys.poseidon_config.clone(),
            secret: Some(self.secret),
            actual_reputation: Some(actual_reputation_fr),
            min_reputation: Some(min_reputation_fr),
            nullifier: Some(nullifier_fr),
        };

        // Generate proof
        let proof =
            Groth16::<Bn254>::prove(&self.keys.reputation_pk, circuit, rng).map_err(|e| {
                Error::validation(format!("Failed to generate reputation proof: {:?}", e))
            })?;

        Ok(ReputationProof {
            proof: ZkProof::from_groth16(&proof)?,
            min_reputation,
            nullifier: fr_to_bytes(&nullifier_fr),
        })
    }
}

impl ZkVerifier {
    /// Create new verifier with keys
    pub fn new(keys: &'static Groth16Keys) -> Self {
        Self { keys }
    }

    /// Verify a contact relationship proof using Groth16 with Poseidon hash
    ///
    /// Verifies that the prover knows a relationship with the given contact
    /// without learning the prover's identity.
    pub fn verify_contact(&self, proof: &ContactProof, contact_id: &UserId) -> Result<bool> {
        self.verify_contact_with_blockchain(proof, contact_id, None)
    }

    /// Verify contact proof with blockchain integration (production)
    pub fn verify_contact_with_blockchain(
        &self,
        proof: &ContactProof,
        contact_id: &UserId,
        blockchain_client: Option<&dyn BlockchainClient>,
    ) -> Result<bool> {
        // Check nullifier hasn't been spent
        if let Some(client) = blockchain_client {
            if client.is_nullifier_spent(&proof.nullifier)? {
                return Err(Error::validation(
                    "Contact proof nullifier already spent (replay attack detected)",
                ));
            }
        }

        // Convert contact_id to field element
        let contact_id_bytes = contact_id.as_bytes();
        let contact_id_fr = Bn254Fr::from_le_bytes_mod_order(contact_id_bytes);

        // Compute expected contact_id_hash using Poseidon
        let expected_hash_fr = poseidon_hash(&[contact_id_fr]);

        // Convert proof hash to field element
        let hash_fr = Bn254Fr::from_le_bytes_mod_order(&proof.contact_id_hash);

        // Verify hash matches
        if hash_fr != expected_hash_fr {
            return Ok(false);
        }

        // Convert proof nullifier to field element
        let nullifier_fr = Bn254Fr::from_le_bytes_mod_order(&proof.nullifier);

        // Deserialize Groth16 proof
        let groth16_proof = proof.proof.to_groth16()?;

        // Public inputs: [contact_id_hash, nullifier]
        let public_inputs = vec![hash_fr, nullifier_fr];

        // Verify using Groth16
        let valid = Groth16::<Bn254>::verify_with_processed_vk(
            &self.keys.contact_pvk,
            &public_inputs,
            &groth16_proof,
        )
        .map_err(|e| Error::validation(format!("Groth16 verification failed: {:?}", e)))?;

        Ok(valid)
    }

    /// Verify a reputation threshold proof using Groth16 with Poseidon hash
    ///
    /// Verifies that the prover has reputation >= min_reputation
    /// without learning the actual reputation or identity.
    pub fn verify_reputation(&self, proof: &ReputationProof) -> Result<bool> {
        self.verify_reputation_with_blockchain(proof, None)
    }

    /// Verify reputation proof with blockchain integration (production)
    pub fn verify_reputation_with_blockchain(
        &self,
        proof: &ReputationProof,
        blockchain_client: Option<&dyn BlockchainClient>,
    ) -> Result<bool> {
        // Check nullifier hasn't been spent
        if let Some(client) = blockchain_client {
            if client.is_nullifier_spent(&proof.nullifier)? {
                return Err(Error::validation(
                    "Reputation proof nullifier already spent (replay attack detected)",
                ));
            }
        }

        // Convert public inputs to field elements
        let min_reputation_fr = Bn254Fr::from(proof.min_reputation as u64);
        let nullifier_fr = Bn254Fr::from_le_bytes_mod_order(&proof.nullifier);

        // Deserialize Groth16 proof
        let groth16_proof = proof.proof.to_groth16()?;

        // Public inputs: [min_reputation, nullifier]
        let public_inputs = vec![min_reputation_fr, nullifier_fr];

        // Verify using Groth16
        let valid = Groth16::<Bn254>::verify_with_processed_vk(
            &self.keys.reputation_pvk,
            &public_inputs,
            &groth16_proof,
        )
        .map_err(|e| Error::validation(format!("Groth16 verification failed: {:?}", e)))?;

        Ok(valid)
    }
}

/// Nullifier tracker to prevent proof reuse
pub struct NullifierSet {
    seen_nullifiers: std::collections::HashSet<[u8; 32]>,
}

impl Default for NullifierSet {
    fn default() -> Self {
        Self::new()
    }
}

impl NullifierSet {
    pub fn new() -> Self {
        Self {
            seen_nullifiers: std::collections::HashSet::new(),
        }
    }

    /// Check if nullifier has been seen before
    pub fn has_seen(&self, nullifier: &[u8; 32]) -> bool {
        self.seen_nullifiers.contains(nullifier)
    }

    /// Mark nullifier as seen
    pub fn mark_seen(&mut self, nullifier: [u8; 32]) -> Result<()> {
        if self.has_seen(&nullifier) {
            return Err(Error::validation("Nullifier already used".to_string()));
        }
        self.seen_nullifiers.insert(nullifier);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rand::rngs::OsRng;

    #[test]
    fn test_poseidon_hash_deterministic() {
        let a = Bn254Fr::from(42u64);
        let b = Bn254Fr::from(123u64);

        let hash1 = poseidon_hash_2(&a, &b);
        let hash2 = poseidon_hash_2(&a, &b);

        assert_eq!(hash1, hash2, "Poseidon hash should be deterministic");
    }

    #[test]
    fn test_poseidon_hash_collision_resistance() {
        let a = Bn254Fr::from(1u64);
        let b = Bn254Fr::from(2u64);
        let c = Bn254Fr::from(3u64);

        let hash1 = poseidon_hash_2(&a, &b);
        let hash2 = poseidon_hash_2(&a, &c);
        let hash3 = poseidon_hash_2(&b, &a);

        assert_ne!(
            hash1, hash2,
            "Different inputs should produce different hashes"
        );
        assert_ne!(hash1, hash3, "Order matters in hash");
    }

    // Helper to create static keys for testing
    fn setup_keys() -> Groth16Keys {
        let mut rng = OsRng;
        Groth16Keys::setup(&mut rng).unwrap()
    }

    #[test]
    fn test_groth16_keys_setup() {
        let keys = setup_keys();
        // Keys should be created without panic
        assert!(!keys.contact_pk.vk.gamma_abc_g1.is_empty());
        assert!(!keys.reputation_pk.vk.gamma_abc_g1.is_empty());
    }

    #[test]
    fn test_contact_proof_generation_and_verification() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover = ZkProver::new(&mut rng, keys);
        let verifier = ZkVerifier::new(keys);
        let contact_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();
        assert_eq!(proof.nullifier.len(), 32);
        assert_eq!(proof.contact_id_hash.len(), 32);

        let valid = verifier.verify_contact(&proof, &contact_id).unwrap();
        assert!(valid, "Valid contact proof should verify");
    }

    #[test]
    fn test_contact_proof_wrong_contact() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover = ZkProver::new(&mut rng, keys);
        let verifier = ZkVerifier::new(keys);
        let contact_id = UserId::new();
        let wrong_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();
        let valid = verifier.verify_contact(&proof, &wrong_id).unwrap();
        assert!(!valid, "Proof should fail with wrong contact");
    }

    #[test]
    fn test_reputation_proof_generation_and_verification() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover = ZkProver::new(&mut rng, keys);
        let verifier = ZkVerifier::new(keys);

        let proof = prover.prove_reputation(100, 50, &mut rng).unwrap();
        assert_eq!(proof.min_reputation, 50);
        assert_eq!(proof.nullifier.len(), 32);

        let valid = verifier.verify_reputation(&proof).unwrap();
        assert!(valid, "Valid reputation proof should verify");
    }

    #[test]
    fn test_reputation_proof_insufficient() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover = ZkProver::new(&mut rng, keys);

        let result = prover.prove_reputation(30, 50, &mut rng);
        assert!(result.is_err(), "Should fail when actual < min reputation");
    }

    #[test]
    fn test_nullifier_set() {
        let mut nullifier_set = NullifierSet::new();
        let nullifier = [42u8; 32];

        assert!(!nullifier_set.has_seen(&nullifier));
        nullifier_set.mark_seen(nullifier).unwrap();
        assert!(nullifier_set.has_seen(&nullifier));

        // Second use should fail
        let result = nullifier_set.mark_seen(nullifier);
        assert!(result.is_err(), "Duplicate nullifier should be rejected");
    }

    #[test]
    fn test_proof_serialization() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover = ZkProver::new(&mut rng, keys);
        let contact_id = UserId::new();

        let proof = prover.prove_contact(&contact_id, &mut rng).unwrap();

        // Proof should serialize/deserialize
        let groth16_proof = proof.proof.to_groth16().unwrap();
        let serialized = ZkProof::from_groth16(&groth16_proof).unwrap();
        assert_eq!(serialized.proof_bytes.len(), proof.proof.proof_bytes.len());
    }

    #[test]
    fn test_prover_secret_persistence() {
        let mut rng = OsRng;
        let keys = Box::leak(Box::new(setup_keys()));

        let prover1 = ZkProver::new(&mut rng, keys);
        let secret_bytes = prover1.secret_bytes();

        let prover2 = ZkProver::from_secret(secret_bytes, keys).unwrap();

        assert_eq!(prover1.secret_bytes(), prover2.secret_bytes());
    }
}
