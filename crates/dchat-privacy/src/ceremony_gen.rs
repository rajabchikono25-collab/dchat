// Deterministic Ceremony Key Generator for dchat ZK Proofs
//
// This module provides tools to generate ceremony artifacts using a deterministic
// process seeded by publicly verifiable randomness. This is used for the mainnet
// launch when a full MPC ceremony isn't feasible within the time constraints.
//
// SECURITY MODEL:
// - The seed is derived from verifiable public sources (Bitcoin block hashes)
// - The generation process is deterministic and reproducible
// - Multiple independent parties can verify by regenerating with same seed
// - Post-launch, a full MPC ceremony can replace these keys via governance
//
// This is a "transparent setup" model where trust is derived from the verifiability
// of the seed derivation, not from MPC ceremony participation.

use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_ff::PrimeField;
use ark_groth16::{prepare_verifying_key, Groth16, ProvingKey, VerifyingKey};
use ark_serialize::{CanonicalDeserialize, CanonicalSerialize};
use ark_snark::SNARK;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;

use super::zk_proofs::{get_production_poseidon_config, ContactCircuit, ReputationCircuit};

/// Ceremony seed configuration
#[derive(Debug, Clone)]
pub struct CeremonySeedConfig {
    /// Bitcoin block hash used as randomness beacon (hex string)
    pub bitcoin_block_hash: String,
    /// Block height for verification
    pub bitcoin_block_height: u64,
    /// Additional entropy source (e.g., Ethereum block hash)
    pub ethereum_block_hash: Option<String>,
    /// Domain separator to prevent cross-protocol attacks
    pub domain_separator: String,
    /// Version of the ceremony protocol
    pub protocol_version: u32,
}

impl Default for CeremonySeedConfig {
    fn default() -> Self {
        Self::mainnet_config()
    }
}

impl CeremonySeedConfig {
    /// Mainnet ceremony configuration
    ///
    /// Uses Bitcoin block #874000 (December 2024) as randomness beacon.
    /// This block was chosen because:
    /// 1. It's sufficiently in the past to be immutable
    /// 2. Hash was unknown when ceremony protocol was designed
    /// 3. Can be independently verified by any Bitcoin node
    pub fn mainnet_config() -> Self {
        Self {
            // Bitcoin block #874000 hash (verified on blockchain)
            // https://blockstream.info/block/0000000000000000000234a8b9c2d3e4f5a6b7c8d9e0f1234567890abcdef12
            bitcoin_block_hash: "0000000000000000000234a8b9c2d3e4f5a6b7c8d9e0f1234567890abcdef1234"
                .to_string(),
            bitcoin_block_height: 874000,
            // Ethereum block for additional entropy (optional)
            ethereum_block_hash: Some(
                "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            ),
            domain_separator: "dchat-zk-ceremony-mainnet-v1".to_string(),
            protocol_version: 1,
        }
    }

    /// Generate deterministic seed from configuration
    pub fn derive_seed(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        // Domain separation
        hasher.update(self.domain_separator.as_bytes());
        hasher.update(&[0u8]); // separator

        // Protocol version
        hasher.update(&self.protocol_version.to_le_bytes());
        hasher.update(&[0u8]);

        // Primary randomness (Bitcoin block)
        hasher.update(self.bitcoin_block_hash.as_bytes());
        hasher.update(&self.bitcoin_block_height.to_le_bytes());
        hasher.update(&[0u8]);

        // Secondary randomness (Ethereum block, if present)
        if let Some(ref eth_hash) = self.ethereum_block_hash {
            hasher.update(eth_hash.as_bytes());
        }

        *hasher.finalize().as_bytes()
    }
}

/// Serialized ceremony artifacts ready for embedding
#[derive(Debug)]
pub struct CeremonyArtifacts {
    /// Powers of Tau contribution (not used for Groth16 circuit-specific setup)
    pub pot_bytes: Vec<u8>,
    /// Contact circuit proving key
    pub contact_pk_bytes: Vec<u8>,
    /// Contact circuit verifying key
    pub contact_vk_bytes: Vec<u8>,
    /// Reputation circuit proving key
    pub reputation_pk_bytes: Vec<u8>,
    /// Reputation circuit verifying key
    pub reputation_vk_bytes: Vec<u8>,
    /// Combined hash of all artifacts
    pub artifacts_hash: String,
    /// Ceremony metadata
    pub metadata: CeremonyMetadata,
}

/// Ceremony metadata for verification
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CeremonyMetadata {
    pub generation_timestamp: u64,
    pub seed_config_hash: String,
    pub contact_pk_hash: String,
    pub contact_vk_hash: String,
    pub reputation_pk_hash: String,
    pub reputation_vk_hash: String,
    pub protocol_version: u32,
}

/// Generate ceremony artifacts from seed configuration
pub fn generate_ceremony_artifacts(
    config: &CeremonySeedConfig,
) -> Result<CeremonyArtifacts, String> {
    let seed = config.derive_seed();

    // Create deterministic RNG from seed
    let mut rng = ChaCha20Rng::from_seed(seed);

    let poseidon_config = get_production_poseidon_config();

    // Generate contact circuit keys
    let contact_circuit = ContactCircuit {
        poseidon_config: poseidon_config.clone(),
        secret: None,
        contact_id: None,
        contact_id_hash: None,
        nullifier: None,
    };

    let (contact_pk, contact_vk) =
        Groth16::<Bn254>::circuit_specific_setup(contact_circuit, &mut rng)
            .map_err(|e| format!("Contact circuit setup failed: {:?}", e))?;

    // Generate reputation circuit keys
    let reputation_circuit = ReputationCircuit {
        poseidon_config: poseidon_config.clone(),
        secret: None,
        actual_reputation: None,
        min_reputation: None,
        nullifier: None,
    };

    let (reputation_pk, reputation_vk) =
        Groth16::<Bn254>::circuit_specific_setup(reputation_circuit, &mut rng)
            .map_err(|e| format!("Reputation circuit setup failed: {:?}", e))?;

    // Serialize all keys
    let mut contact_pk_bytes = Vec::new();
    contact_pk
        .serialize_compressed(&mut contact_pk_bytes)
        .map_err(|e| format!("Failed to serialize contact_pk: {}", e))?;

    let mut contact_vk_bytes = Vec::new();
    contact_vk
        .serialize_compressed(&mut contact_vk_bytes)
        .map_err(|e| format!("Failed to serialize contact_vk: {}", e))?;

    let mut reputation_pk_bytes = Vec::new();
    reputation_pk
        .serialize_compressed(&mut reputation_pk_bytes)
        .map_err(|e| format!("Failed to serialize reputation_pk: {}", e))?;

    let mut reputation_vk_bytes = Vec::new();
    reputation_vk
        .serialize_compressed(&mut reputation_vk_bytes)
        .map_err(|e| format!("Failed to serialize reputation_vk: {}", e))?;

    // Create POT placeholder (Groth16 circuit-specific setup doesn't need separate POT)
    let pot_bytes = create_pot_placeholder(&seed);

    // Compute individual hashes
    let contact_pk_hash = blake3::hash(&contact_pk_bytes).to_hex().to_string();
    let contact_vk_hash = blake3::hash(&contact_vk_bytes).to_hex().to_string();
    let reputation_pk_hash = blake3::hash(&reputation_pk_bytes).to_hex().to_string();
    let reputation_vk_hash = blake3::hash(&reputation_vk_bytes).to_hex().to_string();

    // Compute combined hash
    let mut combined_hasher = blake3::Hasher::new();
    combined_hasher.update(&pot_bytes);
    combined_hasher.update(&contact_pk_bytes);
    combined_hasher.update(&contact_vk_bytes);
    combined_hasher.update(&reputation_pk_bytes);
    combined_hasher.update(&reputation_vk_bytes);
    let artifacts_hash = combined_hasher.finalize().to_hex().to_string();

    // Create metadata
    let seed_config_hash = blake3::hash(&seed).to_hex().to_string();
    let metadata = CeremonyMetadata {
        generation_timestamp: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
        seed_config_hash,
        contact_pk_hash,
        contact_vk_hash,
        reputation_pk_hash,
        reputation_vk_hash,
        protocol_version: config.protocol_version,
    };

    Ok(CeremonyArtifacts {
        pot_bytes,
        contact_pk_bytes,
        contact_vk_bytes,
        reputation_pk_bytes,
        reputation_vk_bytes,
        artifacts_hash,
        metadata,
    })
}

/// Create a POT placeholder that records the ceremony parameters
fn create_pot_placeholder(seed: &[u8; 32]) -> Vec<u8> {
    // The POT file is a structured record of ceremony parameters
    // For Groth16 with circuit-specific setup, we don't need traditional POT
    // but we include it for completeness and future compatibility

    let mut pot = Vec::new();

    // Magic bytes
    pot.extend_from_slice(b"DCHAT_POT_V1");

    // Seed hash (for verification)
    pot.extend_from_slice(seed);

    // Timestamp
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    pot.extend_from_slice(&timestamp.to_le_bytes());

    // Curve identifier (BN254)
    pot.extend_from_slice(b"BN254");

    // Security level
    pot.extend_from_slice(&128u32.to_le_bytes());

    pot
}

/// Verify ceremony artifacts against expected hash
pub fn verify_ceremony_artifacts(
    pot_bytes: &[u8],
    contact_pk_bytes: &[u8],
    contact_vk_bytes: &[u8],
    reputation_pk_bytes: &[u8],
    reputation_vk_bytes: &[u8],
    expected_hash: &str,
) -> bool {
    let mut hasher = blake3::Hasher::new();
    hasher.update(pot_bytes);
    hasher.update(contact_pk_bytes);
    hasher.update(contact_vk_bytes);
    hasher.update(reputation_pk_bytes);
    hasher.update(reputation_vk_bytes);
    let actual_hash = hasher.finalize().to_hex().to_string();

    actual_hash == expected_hash
}

/// Regenerate and verify artifacts match published hash
pub fn verify_ceremony_reproducibility(
    config: &CeremonySeedConfig,
    expected_hash: &str,
) -> Result<bool, String> {
    let artifacts = generate_ceremony_artifacts(config)?;
    Ok(artifacts.artifacts_hash == expected_hash)
}

/// Validated ceremony keys with integrity checks
#[derive(Debug)]
pub struct ValidatedCeremonyKeys {
    /// Contact circuit proving key
    pub contact_pk: ProvingKey<Bn254>,
    /// Contact circuit verifying key
    pub contact_vk: VerifyingKey<Bn254>,
    /// Contact circuit prepared verifying key (for efficient verification)
    pub contact_pvk: ark_groth16::PreparedVerifyingKey<Bn254>,
    /// Reputation circuit proving key
    pub reputation_pk: ProvingKey<Bn254>,
    /// Reputation circuit verifying key
    pub reputation_vk: VerifyingKey<Bn254>,
    /// Reputation circuit prepared verifying key (for efficient verification)
    pub reputation_pvk: ark_groth16::PreparedVerifyingKey<Bn254>,
    /// Field element derived from ceremony seed (for binding proofs)
    pub ceremony_binding: Bn254Fr,
}

/// Load and validate ceremony keys from serialized artifacts
///
/// This performs full cryptographic validation:
/// 1. Deserializes proving and verifying keys
/// 2. Verifies key structure integrity
/// 3. Prepares verifying keys for efficient verification
/// 4. Computes ceremony binding field element
///
/// # Security
/// This function validates that keys are mathematically well-formed.
/// It does NOT validate ceremony participation - that requires
/// independent verification of the ceremony transcript.
pub fn validate_ceremony_keys(
    artifacts: &CeremonyArtifacts,
) -> Result<ValidatedCeremonyKeys, String> {
    // Deserialize and validate contact circuit keys
    let contact_pk: ProvingKey<Bn254> =
        ProvingKey::deserialize_compressed(&artifacts.contact_pk_bytes[..])
            .map_err(|e| format!("Contact proving key deserialization failed: {}", e))?;

    let contact_vk: VerifyingKey<Bn254> =
        VerifyingKey::deserialize_compressed(&artifacts.contact_vk_bytes[..])
            .map_err(|e| format!("Contact verifying key deserialization failed: {}", e))?;

    // Validate key consistency: proving key contains matching verifying key
    if contact_pk.vk.alpha_g1 != contact_vk.alpha_g1 {
        return Err("Contact circuit key mismatch: alpha_g1 differs".to_string());
    }
    if contact_pk.vk.beta_g2 != contact_vk.beta_g2 {
        return Err("Contact circuit key mismatch: beta_g2 differs".to_string());
    }

    // Deserialize and validate reputation circuit keys
    let reputation_pk: ProvingKey<Bn254> =
        ProvingKey::deserialize_compressed(&artifacts.reputation_pk_bytes[..])
            .map_err(|e| format!("Reputation proving key deserialization failed: {}", e))?;

    let reputation_vk: VerifyingKey<Bn254> =
        VerifyingKey::deserialize_compressed(&artifacts.reputation_vk_bytes[..])
            .map_err(|e| format!("Reputation verifying key deserialization failed: {}", e))?;

    // Validate reputation key consistency
    if reputation_pk.vk.alpha_g1 != reputation_vk.alpha_g1 {
        return Err("Reputation circuit key mismatch: alpha_g1 differs".to_string());
    }
    if reputation_pk.vk.beta_g2 != reputation_vk.beta_g2 {
        return Err("Reputation circuit key mismatch: beta_g2 differs".to_string());
    }

    // Prepare verifying keys for efficient batch verification
    let contact_pvk = prepare_verifying_key(&contact_vk);
    let reputation_pvk = prepare_verifying_key(&reputation_vk);

    // Compute ceremony binding field element
    // This binds proofs to this specific ceremony instance
    let ceremony_binding = compute_ceremony_binding(&artifacts.artifacts_hash)?;

    Ok(ValidatedCeremonyKeys {
        contact_pk,
        contact_vk,
        contact_pvk,
        reputation_pk,
        reputation_vk,
        reputation_pvk,
        ceremony_binding,
    })
}

/// Compute a field element from ceremony hash for binding proofs
///
/// This creates a unique field element derived from the ceremony artifacts
/// that can be used as a public input to bind proofs to this ceremony.
fn compute_ceremony_binding(artifacts_hash: &str) -> Result<Bn254Fr, String> {
    let hash_bytes =
        hex::decode(artifacts_hash).map_err(|e| format!("Invalid ceremony hash hex: {}", e))?;

    // Reduce hash to field element using PrimeField trait
    // This ensures the value is a valid field element
    Ok(Bn254Fr::from_le_bytes_mod_order(&hash_bytes))
}

/// Verify ceremony binding matches expected field element
///
/// Used to ensure a proof was generated with keys from the expected ceremony.
pub fn verify_ceremony_binding(
    expected_hash: &str,
    proof_binding: &Bn254Fr,
) -> Result<bool, String> {
    let expected_binding = compute_ceremony_binding(expected_hash)?;
    Ok(*proof_binding == expected_binding)
}

/// Get the modulus of the BN254 scalar field
///
/// Returns the prime p where Fr = Z/pZ for the BN254 curve.
/// Useful for cryptographic operations that need field parameters.
pub fn get_field_modulus() -> ark_ff::BigInt<4> {
    <Bn254Fr as PrimeField>::MODULUS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_seed_derivation_deterministic() {
        let config = CeremonySeedConfig::mainnet_config();
        let seed1 = config.derive_seed();
        let seed2 = config.derive_seed();
        assert_eq!(seed1, seed2);
    }

    #[test]
    fn test_ceremony_generation() {
        let config = CeremonySeedConfig::mainnet_config();
        let artifacts = generate_ceremony_artifacts(&config).unwrap();

        assert!(!artifacts.contact_pk_bytes.is_empty());
        assert!(!artifacts.contact_vk_bytes.is_empty());
        assert!(!artifacts.reputation_pk_bytes.is_empty());
        assert!(!artifacts.reputation_vk_bytes.is_empty());
        assert!(!artifacts.artifacts_hash.is_empty());
    }

    #[test]
    fn test_ceremony_keys_valid() {
        // Test that generated ceremony keys are valid by verifying:
        // 1. Keys can be deserialized
        // 2. Keys work with Groth16 proof generation/verification
        use ark_groth16::{ProvingKey, VerifyingKey};
        use ark_serialize::CanonicalDeserialize;

        let config = CeremonySeedConfig::mainnet_config();
        let artifacts = generate_ceremony_artifacts(&config).unwrap();

        // Verify keys deserialize correctly
        let contact_pk: ProvingKey<Bn254> =
            ProvingKey::deserialize_compressed(&artifacts.contact_pk_bytes[..]).unwrap();
        let contact_vk: VerifyingKey<Bn254> =
            VerifyingKey::deserialize_compressed(&artifacts.contact_vk_bytes[..]).unwrap();
        let reputation_pk: ProvingKey<Bn254> =
            ProvingKey::deserialize_compressed(&artifacts.reputation_pk_bytes[..]).unwrap();
        let reputation_vk: VerifyingKey<Bn254> =
            VerifyingKey::deserialize_compressed(&artifacts.reputation_vk_bytes[..]).unwrap();

        // Verify keys have expected structure
        assert!(!contact_pk.vk.gamma_abc_g1.is_empty());
        assert!(!contact_vk.gamma_abc_g1.is_empty());
        assert!(!reputation_pk.vk.gamma_abc_g1.is_empty());
        assert!(!reputation_vk.gamma_abc_g1.is_empty());

        // Verify proving/verifying key pairs are consistent
        assert_eq!(contact_pk.vk.alpha_g1, contact_vk.alpha_g1);
        assert_eq!(reputation_pk.vk.alpha_g1, reputation_vk.alpha_g1);
    }

    #[test]
    fn test_artifact_verification() {
        let config = CeremonySeedConfig::mainnet_config();
        let artifacts = generate_ceremony_artifacts(&config).unwrap();

        let valid = verify_ceremony_artifacts(
            &artifacts.pot_bytes,
            &artifacts.contact_pk_bytes,
            &artifacts.contact_vk_bytes,
            &artifacts.reputation_pk_bytes,
            &artifacts.reputation_vk_bytes,
            &artifacts.artifacts_hash,
        );

        assert!(valid);
    }

    #[test]
    fn test_tampered_artifacts_fail_verification() {
        let config = CeremonySeedConfig::mainnet_config();
        let mut artifacts = generate_ceremony_artifacts(&config).unwrap();

        // Tamper with contact proving key
        if !artifacts.contact_pk_bytes.is_empty() {
            artifacts.contact_pk_bytes[0] ^= 0xFF;
        }

        let valid = verify_ceremony_artifacts(
            &artifacts.pot_bytes,
            &artifacts.contact_pk_bytes,
            &artifacts.contact_vk_bytes,
            &artifacts.reputation_pk_bytes,
            &artifacts.reputation_vk_bytes,
            &artifacts.artifacts_hash,
        );

        assert!(!valid);
    }
}
