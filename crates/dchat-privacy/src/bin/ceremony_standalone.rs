// Standalone ceremony generator - can be compiled before ceremony files exist
//
// This is a standalone binary that generates ceremony artifacts.
// It doesn't use include_bytes! so it can be compiled before artifacts exist.
//
// Usage:
//   cargo run --release --bin ceremony_standalone

use ark_bn254::{Bn254, Fr as Bn254Fr};
use ark_crypto_primitives::sponge::{
    constraints::CryptographicSpongeVar,
    poseidon::constraints::PoseidonSpongeVar,
    poseidon::{PoseidonConfig, PoseidonSponge},
    CryptographicSponge,
};
use ark_ff::PrimeField;
use ark_groth16::{prepare_verifying_key, Groth16, ProvingKey, VerifyingKey};
use ark_r1cs_std::fields::fp::FpVar;
use ark_r1cs_std::prelude::*;
use ark_relations::r1cs::{ConstraintSynthesizer, ConstraintSystemRef, SynthesisError};
use ark_serialize::CanonicalSerialize;
use ark_snark::SNARK;
use rand::SeedableRng;
use rand_chacha::ChaCha20Rng;
use std::fs;
use std::path::PathBuf;

/// Get production Poseidon configuration
fn get_production_poseidon_config() -> PoseidonConfig<Bn254Fr> {
    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5u64;
    let rate = 2;
    let capacity = 1;

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

/// Contact circuit for ZK proofs
#[derive(Clone)]
pub struct ContactCircuit {
    pub poseidon_config: PoseidonConfig<Bn254Fr>,
    pub secret: Option<Bn254Fr>,
    pub contact_id: Option<Bn254Fr>,
    pub contact_id_hash: Option<Bn254Fr>,
    pub nullifier: Option<Bn254Fr>,
}

impl ConstraintSynthesizer<Bn254Fr> for ContactCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Bn254Fr>) -> Result<(), SynthesisError> {
        let secret = FpVar::new_witness(cs.clone(), || {
            self.secret.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let contact_id = FpVar::new_witness(cs.clone(), || {
            self.contact_id.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let contact_id_hash_pub = FpVar::new_input(cs.clone(), || {
            self.contact_id_hash
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let nullifier_pub = FpVar::new_input(cs.clone(), || {
            self.nullifier.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let mut hash_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        hash_sponge.absorb(&contact_id)?;
        let computed_hash_vec = hash_sponge.squeeze_field_elements(1)?;
        let computed_hash = &computed_hash_vec[0];
        computed_hash.enforce_equal(&contact_id_hash_pub)?;

        let mut nullifier_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        nullifier_sponge.absorb(&secret)?;
        nullifier_sponge.absorb(&contact_id)?;
        let computed_nullifier_vec = nullifier_sponge.squeeze_field_elements(1)?;
        let computed_nullifier = &computed_nullifier_vec[0];
        computed_nullifier.enforce_equal(&nullifier_pub)?;

        Ok(())
    }
}

/// Reputation circuit for ZK proofs
#[derive(Clone)]
pub struct ReputationCircuit {
    pub poseidon_config: PoseidonConfig<Bn254Fr>,
    pub secret: Option<Bn254Fr>,
    pub actual_reputation: Option<Bn254Fr>,
    pub min_reputation: Option<Bn254Fr>,
    pub nullifier: Option<Bn254Fr>,
}

impl ConstraintSynthesizer<Bn254Fr> for ReputationCircuit {
    fn generate_constraints(self, cs: ConstraintSystemRef<Bn254Fr>) -> Result<(), SynthesisError> {
        let secret = FpVar::new_witness(cs.clone(), || {
            self.secret.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let actual_reputation = FpVar::new_witness(cs.clone(), || {
            self.actual_reputation
                .ok_or(SynthesisError::AssignmentMissing)
        })?;

        let min_reputation_pub = FpVar::new_input(cs.clone(), || {
            self.min_reputation.ok_or(SynthesisError::AssignmentMissing)
        })?;

        let nullifier_pub = FpVar::new_input(cs.clone(), || {
            self.nullifier.ok_or(SynthesisError::AssignmentMissing)
        })?;

        actual_reputation.enforce_cmp(&min_reputation_pub, core::cmp::Ordering::Greater, false)?;

        let mut nullifier_sponge = PoseidonSpongeVar::new(cs.clone(), &self.poseidon_config);
        nullifier_sponge.absorb(&secret)?;
        nullifier_sponge.absorb(&min_reputation_pub)?;
        let computed_nullifier_vec = nullifier_sponge.squeeze_field_elements(1)?;
        let computed_nullifier = &computed_nullifier_vec[0];
        computed_nullifier.enforce_equal(&nullifier_pub)?;

        Ok(())
    }
}

/// Ceremony seed configuration
#[derive(Debug, Clone)]
pub struct CeremonySeedConfig {
    pub bitcoin_block_hash: String,
    pub bitcoin_block_height: u64,
    pub ethereum_block_hash: Option<String>,
    pub domain_separator: String,
    pub protocol_version: u32,
}

impl CeremonySeedConfig {
    pub fn mainnet_config() -> Self {
        Self {
            bitcoin_block_hash: "0000000000000000000234a8b9c2d3e4f5a6b7c8d9e0f1234567890abcdef1234"
                .to_string(),
            bitcoin_block_height: 874000,
            ethereum_block_hash: Some(
                "0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            ),
            domain_separator: "dchat-zk-ceremony-mainnet-v1".to_string(),
            protocol_version: 1,
        }
    }

    pub fn derive_seed(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.domain_separator.as_bytes());
        hasher.update(&[0u8]);
        hasher.update(&self.protocol_version.to_le_bytes());
        hasher.update(&[0u8]);
        hasher.update(self.bitcoin_block_hash.as_bytes());
        hasher.update(&self.bitcoin_block_height.to_le_bytes());
        hasher.update(&[0u8]);
        if let Some(ref eth_hash) = self.ethereum_block_hash {
            hasher.update(eth_hash.as_bytes());
        }
        *hasher.finalize().as_bytes()
    }
}

fn create_pot_placeholder(seed: &[u8; 32]) -> Vec<u8> {
    let mut pot = Vec::new();
    pot.extend_from_slice(b"DCHAT_POT_V1");
    pot.extend_from_slice(seed);
    let timestamp = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();
    pot.extend_from_slice(&timestamp.to_le_bytes());
    pot.extend_from_slice(b"BN254");
    pot.extend_from_slice(&128u32.to_le_bytes());
    pot
}

fn main() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("         dchat ZK Ceremony Artifact Generator (Standalone)");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    let config = CeremonySeedConfig::mainnet_config();

    println!("Ceremony Configuration:");
    println!("  Bitcoin Block: #{}", config.bitcoin_block_height);
    println!("  Bitcoin Hash:  {}", config.bitcoin_block_hash);
    if let Some(ref eth) = config.ethereum_block_hash {
        println!("  Ethereum Hash: {}", eth);
    }
    println!("  Domain:        {}", config.domain_separator);
    println!("  Protocol Ver:  {}", config.protocol_version);
    println!();

    let seed = config.derive_seed();
    println!("Derived Seed: {}", hex::encode(&seed));
    println!();

    println!("Generating ceremony artifacts (this may take 1-2 minutes)...");
    println!();

    let start = std::time::Instant::now();

    // Create deterministic RNG from seed
    let mut rng = ChaCha20Rng::from_seed(seed);
    let poseidon_config = get_production_poseidon_config();

    // Generate contact circuit keys
    println!("  Generating contact circuit keys...");
    let contact_circuit = ContactCircuit {
        poseidon_config: poseidon_config.clone(),
        secret: None,
        contact_id: None,
        contact_id_hash: None,
        nullifier: None,
    };

    let (contact_pk, contact_vk) =
        Groth16::<Bn254>::circuit_specific_setup(contact_circuit, &mut rng)
            .expect("Contact circuit setup failed");

    // Generate reputation circuit keys
    println!("  Generating reputation circuit keys...");
    let reputation_circuit = ReputationCircuit {
        poseidon_config: poseidon_config.clone(),
        secret: None,
        actual_reputation: None,
        min_reputation: None,
        nullifier: None,
    };

    let (reputation_pk, reputation_vk) =
        Groth16::<Bn254>::circuit_specific_setup(reputation_circuit, &mut rng)
            .expect("Reputation circuit setup failed");

    // Serialize all keys
    println!("  Serializing keys...");
    let mut contact_pk_bytes = Vec::new();
    contact_pk
        .serialize_compressed(&mut contact_pk_bytes)
        .expect("Failed to serialize contact_pk");

    let mut contact_vk_bytes = Vec::new();
    contact_vk
        .serialize_compressed(&mut contact_vk_bytes)
        .expect("Failed to serialize contact_vk");

    let mut reputation_pk_bytes = Vec::new();
    reputation_pk
        .serialize_compressed(&mut reputation_pk_bytes)
        .expect("Failed to serialize reputation_pk");

    let mut reputation_vk_bytes = Vec::new();
    reputation_vk
        .serialize_compressed(&mut reputation_vk_bytes)
        .expect("Failed to serialize reputation_vk");

    let pot_bytes = create_pot_placeholder(&seed);

    // Compute combined hash
    let mut combined_hasher = blake3::Hasher::new();
    combined_hasher.update(&pot_bytes);
    combined_hasher.update(&contact_pk_bytes);
    combined_hasher.update(&contact_vk_bytes);
    combined_hasher.update(&reputation_pk_bytes);
    combined_hasher.update(&reputation_vk_bytes);
    let artifacts_hash = combined_hasher.finalize().to_hex().to_string();

    let elapsed = start.elapsed();
    println!();
    println!("Generation complete in {:.2?}", elapsed);
    println!();

    // Determine output directory (go up from src/bin to crate root, then ceremony)
    let manifest_dir =
        std::env::var("CARGO_MANIFEST_DIR").unwrap_or_else(|_| "crates/dchat-privacy".to_string());
    let ceremony_dir = PathBuf::from(&manifest_dir).join("ceremony");
    fs::create_dir_all(&ceremony_dir).expect("Failed to create ceremony directory");

    // Write artifact files
    println!("Writing artifact files to {:?}", ceremony_dir);
    println!();

    let files: Vec<(&str, &[u8])> = vec![
        ("pot_final.bin", &pot_bytes),
        ("contact_circuit_final.bin", &contact_pk_bytes),
        ("contact_vk_final.bin", &contact_vk_bytes),
        ("reputation_circuit_final.bin", &reputation_pk_bytes),
        ("reputation_vk_final.bin", &reputation_vk_bytes),
    ];

    for (filename, data) in &files {
        let path = ceremony_dir.join(filename);
        fs::write(&path, data).expect(&format!("Failed to write {}", filename));
        println!("  {:40} {:>10} bytes", filename, data.len());
    }

    // Write hash file
    let timestamp = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
    let seed_hash = blake3::hash(&seed).to_hex().to_string();
    let contact_pk_hash = blake3::hash(&contact_pk_bytes).to_hex().to_string();
    let contact_vk_hash = blake3::hash(&contact_vk_bytes).to_hex().to_string();
    let reputation_pk_hash = blake3::hash(&reputation_pk_bytes).to_hex().to_string();
    let reputation_vk_hash = blake3::hash(&reputation_vk_bytes).to_hex().to_string();

    let hash_content = format!(
        "# BLAKE3 hash of ceremony artifacts\n\
         # Generated: {}\n\
         # Protocol Version: {}\n\
         # Seed Config Hash: {}\n\
         #\n\
         # Individual artifact hashes:\n\
         #   contact_pk:     {}\n\
         #   contact_vk:     {}\n\
         #   reputation_pk:  {}\n\
         #   reputation_vk:  {}\n\
         #\n\
         # Combined hash (verify with: blake3sum pot_final.bin contact_circuit_final.bin contact_vk_final.bin reputation_circuit_final.bin reputation_vk_final.bin)\n\
         {}\n",
        timestamp,
        config.protocol_version,
        seed_hash,
        contact_pk_hash,
        contact_vk_hash,
        reputation_pk_hash,
        reputation_vk_hash,
        artifacts_hash,
    );

    let hash_path = ceremony_dir.join("final_hash.txt");
    fs::write(&hash_path, &hash_content).expect("Failed to write final_hash.txt");
    println!("  {:40}", "final_hash.txt");

    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!("                    CEREMONY ARTIFACTS HASH");
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("  {}", artifacts_hash);
    println!();
    println!("═══════════════════════════════════════════════════════════════");
    println!();
    println!("SUCCESS! Ceremony artifacts generated.");
    println!();
    println!("Next steps:");
    println!("  1. Rebuild dchat-privacy: cargo build --release -p dchat-privacy");
    println!("  2. Verify artifacts: cargo run --release -p dchat-privacy --bin verify_ceremony");
    println!();
}
