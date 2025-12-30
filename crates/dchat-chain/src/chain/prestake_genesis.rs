//! Pre-stake Genesis Implementation
//!
//! Solves the chicken-and-egg problem of mainnet launch by pre-staking validators
//! at genesis using cryptographic bond commitments. This eliminates the need for
//! a functioning RPC to stake before the chain is live.
//!
//! ## Flow
//! 1. Freeze code and collect validator public keys (offline)
//! 2. Validators submit cryptographic bond commitments (offline)
//! 3. Generate genesis file with pre-staked validator set
//! 4. All validators start with the same genesis file
//! 5. Chain starts with active validators immediately
//!
//! ## Security
//! - Bond commitments are cryptographically signed
//! - Validators cannot repudiate their commitments
//! - Stakes are locked from block 0
//! - No RPC required until chain is live

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::motes::MOTES_PER_DCHAT;
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::Path;
use tracing::info;

use super::genesis::{
    ChatGenesisBlock, ChatGenesisConfig, CurrencyGenesisBlock, CurrencyGenesisConfig,
    GenesisValidator,
};

/// Minimum validators required to launch mainnet
pub const MIN_GENESIS_VALIDATORS: usize = 4;

/// Maximum validators in genesis set
pub const MAX_GENESIS_VALIDATORS: usize = 100;

/// Default minimum stake for genesis validators (10,000 DCHAT)
pub const DEFAULT_MIN_GENESIS_STAKE: u64 = 10_000 * MOTES_PER_DCHAT;

/// Cryptographic bond commitment from a validator
///
/// This represents a validator's irrevocable commitment to stake tokens
/// at genesis. The commitment is signed with the validator's private key
/// and can be verified by anyone with the public key.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BondCommitment {
    /// Validator's Ed25519 public key (hex-encoded)
    pub validator_pubkey: String,

    /// Amount committed to stake (in motes - smallest unit)
    pub stake_amount: u64,

    /// Validator's human-readable name/identifier
    pub validator_name: String,

    /// Network endpoint (IP:port or DNS name)
    pub network_address: String,

    /// Geographic region for diversity requirements
    pub region: String,

    /// Timestamp when commitment was made
    pub committed_at: DateTime<Utc>,

    /// Lockup period in days (minimum 7 days)
    pub lockup_days: u64,

    /// Hash of the genesis chain ID this commitment is for
    pub chain_id_hash: String,

    /// Ed25519 signature over the commitment data
    pub signature: String,
}

impl BondCommitment {
    /// Create a new bond commitment (must be signed separately)
    pub fn new(
        validator_pubkey: String,
        stake_amount: u64,
        validator_name: String,
        network_address: String,
        region: String,
        lockup_days: u64,
        chain_id: &str,
    ) -> Self {
        let chain_id_hash = hex::encode(blake3::hash(chain_id.as_bytes()).as_bytes());

        Self {
            validator_pubkey,
            stake_amount,
            validator_name,
            network_address,
            region,
            committed_at: Utc::now(),
            lockup_days,
            chain_id_hash,
            signature: String::new(),
        }
    }

    /// Create the message bytes that will be signed
    pub fn signing_message(&self) -> Vec<u8> {
        let mut message = Vec::new();
        message.extend_from_slice(b"DCHAT_BOND_COMMITMENT_V1:");
        message.extend_from_slice(self.validator_pubkey.as_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(&self.stake_amount.to_be_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(self.validator_name.as_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(self.network_address.as_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(self.region.as_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(&self.lockup_days.to_be_bytes());
        message.extend_from_slice(b":");
        message.extend_from_slice(self.chain_id_hash.as_bytes());
        message
    }

    /// Sign the commitment with the validator's private key
    pub fn sign(&mut self, signing_key: &SigningKey) -> Result<()> {
        let message = self.signing_message();
        let signature = signing_key.sign(&message);
        self.signature = hex::encode(signature.to_bytes());
        Ok(())
    }

    /// Verify the commitment signature
    pub fn verify(&self) -> Result<()> {
        if self.signature.is_empty() {
            return Err(Error::validation("Commitment is not signed"));
        }

        let pubkey_bytes = hex::decode(&self.validator_pubkey)
            .map_err(|e| Error::validation(format!("Invalid public key hex: {}", e)))?;

        let pubkey_array: [u8; 32] = pubkey_bytes
            .try_into()
            .map_err(|_| Error::validation("Public key must be 32 bytes"))?;

        let verifying_key = VerifyingKey::from_bytes(&pubkey_array)
            .map_err(|e| Error::validation(format!("Invalid public key: {}", e)))?;

        let sig_bytes = hex::decode(&self.signature)
            .map_err(|e| Error::validation(format!("Invalid signature hex: {}", e)))?;

        let sig_array: [u8; 64] = sig_bytes
            .try_into()
            .map_err(|_| Error::validation("Signature must be 64 bytes"))?;

        let signature = Signature::from_bytes(&sig_array);

        let message = self.signing_message();
        verifying_key
            .verify(&message, &signature)
            .map_err(|_| Error::validation("Bond commitment signature verification failed"))?;

        Ok(())
    }

    /// Validate commitment parameters
    pub fn validate(&self, min_stake: u64) -> Result<()> {
        // Verify signature first
        self.verify()?;

        // Check minimum stake
        if self.stake_amount < min_stake {
            return Err(Error::validation(format!(
                "Stake {} below minimum {}",
                self.stake_amount, min_stake
            )));
        }

        // Check lockup period (minimum 7 days)
        if self.lockup_days < 7 {
            return Err(Error::validation(format!(
                "Lockup {} days below minimum 7 days",
                self.lockup_days
            )));
        }

        // Validate network address format (basic check)
        if self.network_address.is_empty() {
            return Err(Error::validation("Network address cannot be empty"));
        }

        // Validate region
        if self.region.is_empty() {
            return Err(Error::validation("Region cannot be empty"));
        }

        Ok(())
    }

    /// Convert to GenesisValidator
    pub fn to_genesis_validator(&self) -> GenesisValidator {
        GenesisValidator {
            public_key: self.validator_pubkey.clone(),
            stake_amount: self.stake_amount,
            voting_power: self.calculate_voting_power(),
            address: self.network_address.clone(),
        }
    }

    /// Calculate voting power based on stake
    fn calculate_voting_power(&self) -> u64 {
        // Voting power is proportional to stake, capped at 5% of total
        // For genesis, we use a simple linear formula
        // 1 voting power per 1000 DCHAT staked
        self.stake_amount / (1000 * MOTES_PER_DCHAT)
    }
}

/// Collection of bond commitments from all genesis validators
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreStakeManifest {
    /// Chain ID for mainnet
    pub chain_id: String,

    /// Target genesis timestamp (UTC)
    pub genesis_time: DateTime<Utc>,

    /// Initial token supply (in motes)
    pub initial_supply: u64,

    /// Minimum stake required (in motes)
    pub min_stake: u64,

    /// Bond commitments from validators
    pub commitments: Vec<BondCommitment>,

    /// Token allocations
    pub allocations: TokenAllocations,

    /// Manifest hash (computed after all commitments added)
    pub manifest_hash: String,

    /// Coordinator signature (optional - for centralized genesis)
    pub coordinator_signature: Option<String>,
}

/// Token allocation pools at genesis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TokenAllocations {
    /// Foundation allocation (governance treasury)
    pub foundation: u64,

    /// Validator staking pool
    pub validator_pool: u64,

    /// Community rewards pool
    pub community_pool: u64,

    /// Liquidity pool
    pub liquidity_pool: u64,
}

impl Default for TokenAllocations {
    fn default() -> Self {
        // Default allocation based on 1 billion DCHAT total supply
        // All values in motes (8 decimal places)
        let total_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;
        Self {
            foundation: (total_supply * 20) / 100,     // 20%
            validator_pool: (total_supply * 24) / 100, // 24%
            community_pool: (total_supply * 56) / 100, // 56%
            liquidity_pool: 0,                         // Computed from remainder
        }
    }
}

impl PreStakeManifest {
    /// Create a new pre-stake manifest
    pub fn new(chain_id: String, genesis_time: DateTime<Utc>, initial_supply: u64) -> Self {
        Self {
            chain_id,
            genesis_time,
            initial_supply,
            min_stake: DEFAULT_MIN_GENESIS_STAKE,
            commitments: Vec::new(),
            allocations: TokenAllocations::default(),
            manifest_hash: String::new(),
            coordinator_signature: None,
        }
    }

    /// Add a bond commitment from a validator
    pub fn add_commitment(&mut self, commitment: BondCommitment) -> Result<()> {
        // Validate the commitment
        commitment.validate(self.min_stake)?;

        // Check for duplicate validator
        if self
            .commitments
            .iter()
            .any(|c| c.validator_pubkey == commitment.validator_pubkey)
        {
            return Err(Error::validation(format!(
                "Duplicate commitment from validator {}",
                commitment.validator_pubkey
            )));
        }

        // Check max validators
        if self.commitments.len() >= MAX_GENESIS_VALIDATORS {
            return Err(Error::validation(format!(
                "Maximum {} genesis validators exceeded",
                MAX_GENESIS_VALIDATORS
            )));
        }

        // Verify chain ID matches
        let expected_chain_id_hash = hex::encode(blake3::hash(self.chain_id.as_bytes()).as_bytes());
        if commitment.chain_id_hash != expected_chain_id_hash {
            return Err(Error::validation(
                "Commitment chain ID does not match manifest",
            ));
        }

        info!(
            "✅ Added bond commitment: {} ({} DCHAT)",
            commitment.validator_name,
            commitment.stake_amount / MOTES_PER_DCHAT
        );

        self.commitments.push(commitment);
        self.recompute_hash();
        Ok(())
    }

    /// Validate the manifest is ready for genesis
    pub fn validate(&self) -> Result<()> {
        // Check minimum validators
        if self.commitments.len() < MIN_GENESIS_VALIDATORS {
            return Err(Error::validation(format!(
                "Minimum {} validators required, have {}",
                MIN_GENESIS_VALIDATORS,
                self.commitments.len()
            )));
        }

        // Validate all commitments
        for commitment in &self.commitments {
            commitment.validate(self.min_stake)?;
        }

        // Check geographic diversity (minimum 3 regions)
        let regions: std::collections::HashSet<_> =
            self.commitments.iter().map(|c| &c.region).collect();
        if regions.len() < 3 {
            return Err(Error::validation(format!(
                "Minimum 3 geographic regions required, have {}",
                regions.len()
            )));
        }

        // Check total staked doesn't exceed validator pool allocation
        let total_staked: u64 = self.commitments.iter().map(|c| c.stake_amount).sum();
        if total_staked > self.allocations.validator_pool {
            return Err(Error::validation(format!(
                "Total staked {} exceeds validator pool {}",
                total_staked, self.allocations.validator_pool
            )));
        }

        info!("✅ Pre-stake manifest validated");
        info!("   Validators: {}", self.commitments.len());
        info!("   Regions: {}", regions.len());
        info!("   Total staked: {} DCHAT", total_staked / MOTES_PER_DCHAT);

        Ok(())
    }

    /// Recompute manifest hash
    fn recompute_hash(&mut self) {
        let mut hasher_input = Vec::new();
        hasher_input.extend_from_slice(self.chain_id.as_bytes());
        hasher_input.extend_from_slice(&self.initial_supply.to_be_bytes());

        for commitment in &self.commitments {
            hasher_input.extend_from_slice(commitment.validator_pubkey.as_bytes());
            hasher_input.extend_from_slice(&commitment.stake_amount.to_be_bytes());
        }

        self.manifest_hash = hex::encode(blake3::hash(&hasher_input).as_bytes());
    }

    /// Get total stake across all validators
    pub fn total_staked(&self) -> u64 {
        self.commitments.iter().map(|c| c.stake_amount).sum()
    }

    /// Get validators grouped by region
    pub fn validators_by_region(&self) -> HashMap<String, Vec<&BondCommitment>> {
        let mut by_region: HashMap<String, Vec<&BondCommitment>> = HashMap::new();
        for commitment in &self.commitments {
            by_region
                .entry(commitment.region.clone())
                .or_default()
                .push(commitment);
        }
        by_region
    }

    /// Save manifest to JSON file
    pub fn save_to_file(&self, path: &Path) -> Result<()> {
        let json = serde_json::to_string_pretty(self)
            .map_err(|e| Error::serialization(format!("Failed to serialize manifest: {}", e)))?;

        fs::write(path, json)
            .map_err(|e| Error::io(format!("Failed to write manifest file: {}", e)))?;

        info!("📄 Saved pre-stake manifest to {:?}", path);
        Ok(())
    }

    /// Load manifest from JSON file
    pub fn load_from_file(path: &Path) -> Result<Self> {
        let json = fs::read_to_string(path)
            .map_err(|e| Error::io(format!("Failed to read manifest file: {}", e)))?;

        let manifest: Self = serde_json::from_str(&json)
            .map_err(|e| Error::serialization(format!("Failed to parse manifest: {}", e)))?;

        info!("📄 Loaded pre-stake manifest from {:?}", path);
        Ok(manifest)
    }
}

/// Genesis file generator from pre-stake manifest
pub struct PreStakeGenesisBuilder {
    manifest: PreStakeManifest,
    signing_key: SigningKey,
}

impl PreStakeGenesisBuilder {
    /// Create a new genesis builder from a pre-stake manifest
    pub fn new(manifest: PreStakeManifest, signing_key: SigningKey) -> Self {
        Self {
            manifest,
            signing_key,
        }
    }

    /// Build both chat and currency chain genesis blocks
    pub fn build(&self) -> Result<(ChatGenesisBlock, CurrencyGenesisBlock)> {
        // Validate manifest first
        self.manifest.validate()?;

        // Convert commitments to genesis validators
        let genesis_validators: Vec<GenesisValidator> = self
            .manifest
            .commitments
            .iter()
            .map(|c| c.to_genesis_validator())
            .collect();

        // Build chat genesis
        let chat_genesis = self.build_chat_genesis(&genesis_validators)?;

        // Build currency genesis
        let currency_genesis = self.build_currency_genesis(&genesis_validators)?;

        info!("🎉 Pre-stake genesis blocks created!");
        info!("   Chat genesis hash: {}", chat_genesis.hash);
        info!("   Currency genesis hash: {}", currency_genesis.hash);

        Ok((chat_genesis, currency_genesis))
    }

    /// Build chat chain genesis block
    fn build_chat_genesis(&self, validators: &[GenesisValidator]) -> Result<ChatGenesisBlock> {
        use super::genesis::GenesisBuilder;

        let config = ChatGenesisConfig {
            chain_id: format!("{}-chat", self.manifest.chain_id),
            block_time_secs: 3,
            max_message_size: 1024 * 1024, // 1 MB
            min_reputation_score: 0,
        };

        let builder = GenesisBuilder::new(self.signing_key.clone());
        builder.create_chat_genesis(validators.to_vec(), config)
    }

    /// Build currency chain genesis block
    fn build_currency_genesis(
        &self,
        validators: &[GenesisValidator],
    ) -> Result<CurrencyGenesisBlock> {
        use super::genesis::GenesisBuilder;

        let config = CurrencyGenesisConfig {
            chain_id: format!("{}-currency", self.manifest.chain_id),
            block_time_secs: 5,
            token_name: "DChat Token".to_string(),
            token_symbol: "DCHAT".to_string(),
            token_decimals: 8,
        };

        let builder = GenesisBuilder::new(self.signing_key.clone());
        builder.create_currency_genesis(validators.to_vec(), config, self.manifest.initial_supply)
    }

    /// Generate all genesis files and save to directory
    pub fn generate_files(&self, output_dir: &Path) -> Result<()> {
        // Create output directory if it doesn't exist
        fs::create_dir_all(output_dir)
            .map_err(|e| Error::io(format!("Failed to create output directory: {}", e)))?;

        // Build genesis blocks
        let (chat_genesis, currency_genesis) = self.build()?;

        // Save chat genesis
        let chat_path = output_dir.join("chat_chain_genesis.json");
        let chat_json = serde_json::to_string_pretty(&chat_genesis).map_err(|e| {
            Error::serialization(format!("Failed to serialize chat genesis: {}", e))
        })?;
        fs::write(&chat_path, chat_json)
            .map_err(|e| Error::io(format!("Failed to write chat genesis: {}", e)))?;
        info!("📄 Saved chat genesis to {:?}", chat_path);

        // Save currency genesis
        let currency_path = output_dir.join("currency_chain_genesis.json");
        let currency_json = serde_json::to_string_pretty(&currency_genesis).map_err(|e| {
            Error::serialization(format!("Failed to serialize currency genesis: {}", e))
        })?;
        fs::write(&currency_path, currency_json)
            .map_err(|e| Error::io(format!("Failed to write currency genesis: {}", e)))?;
        info!("📄 Saved currency genesis to {:?}", currency_path);

        // Save combined genesis summary
        let summary = GenesisSummary {
            chain_id: self.manifest.chain_id.clone(),
            genesis_time: self.manifest.genesis_time,
            chat_genesis_hash: chat_genesis.hash.clone(),
            currency_genesis_hash: currency_genesis.hash.clone(),
            validator_count: self.manifest.commitments.len(),
            total_staked: self.manifest.total_staked(),
            initial_supply: self.manifest.initial_supply,
        };

        let summary_path = output_dir.join("genesis.json");
        let summary_json = serde_json::to_string_pretty(&summary).map_err(|e| {
            Error::serialization(format!("Failed to serialize genesis summary: {}", e))
        })?;
        fs::write(&summary_path, summary_json)
            .map_err(|e| Error::io(format!("Failed to write genesis summary: {}", e)))?;
        info!("📄 Saved genesis summary to {:?}", summary_path);

        Ok(())
    }
}

/// Summary of generated genesis files
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisSummary {
    pub chain_id: String,
    pub genesis_time: DateTime<Utc>,
    pub chat_genesis_hash: String,
    pub currency_genesis_hash: String,
    pub validator_count: usize,
    pub total_staked: u64,
    pub initial_supply: u64,
}

/// Helper function to create and sign a bond commitment
pub fn create_signed_commitment(
    signing_key: &SigningKey,
    validator_name: String,
    stake_amount: u64,
    network_address: String,
    region: String,
    lockup_days: u64,
    chain_id: &str,
) -> Result<BondCommitment> {
    let pubkey = hex::encode(signing_key.verifying_key().as_bytes());

    let mut commitment = BondCommitment::new(
        pubkey,
        stake_amount,
        validator_name,
        network_address,
        region,
        lockup_days,
        chain_id,
    );

    commitment.sign(signing_key)?;
    Ok(commitment)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    fn generate_test_key() -> SigningKey {
        SigningKey::generate(&mut OsRng)
    }

    #[test]
    fn test_bond_commitment_signing() {
        let key = generate_test_key();
        let pubkey = hex::encode(key.verifying_key().as_bytes());

        let mut commitment = BondCommitment::new(
            pubkey,
            DEFAULT_MIN_GENESIS_STAKE,
            "test-validator".to_string(),
            "127.0.0.1:8080".to_string(),
            "us-east".to_string(),
            30,
            "dchat-mainnet-1",
        );

        // Should fail before signing
        assert!(commitment.verify().is_err());

        // Sign and verify
        commitment.sign(&key).unwrap();
        assert!(commitment.verify().is_ok());
    }

    #[test]
    fn test_bond_commitment_validation() {
        let key = generate_test_key();

        let commitment = create_signed_commitment(
            &key,
            "test-validator".to_string(),
            DEFAULT_MIN_GENESIS_STAKE,
            "127.0.0.1:8080".to_string(),
            "us-east".to_string(),
            30,
            "dchat-mainnet-1",
        )
        .unwrap();

        assert!(commitment.validate(DEFAULT_MIN_GENESIS_STAKE).is_ok());
    }

    #[test]
    fn test_insufficient_stake_rejected() {
        let key = generate_test_key();

        let commitment = create_signed_commitment(
            &key,
            "test-validator".to_string(),
            DEFAULT_MIN_GENESIS_STAKE - 1, // Below minimum
            "127.0.0.1:8080".to_string(),
            "us-east".to_string(),
            30,
            "dchat-mainnet-1",
        )
        .unwrap();

        assert!(commitment.validate(DEFAULT_MIN_GENESIS_STAKE).is_err());
    }

    #[test]
    fn test_prestake_manifest() {
        let chain_id = "dchat-mainnet-1".to_string();
        let genesis_time = Utc::now();
        let initial_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;

        let mut manifest = PreStakeManifest::new(chain_id.clone(), genesis_time, initial_supply);

        // Create 4 validators in 3 regions
        let regions = ["us-east", "eu-west", "ap-south", "us-west"];
        for (i, region) in regions.iter().enumerate() {
            let key = generate_test_key();
            let commitment = create_signed_commitment(
                &key,
                format!("validator-{}", i),
                DEFAULT_MIN_GENESIS_STAKE,
                format!("192.168.1.{}:8080", i + 1),
                region.to_string(),
                30,
                &chain_id,
            )
            .unwrap();

            manifest.add_commitment(commitment).unwrap();
        }

        assert!(manifest.validate().is_ok());
        assert_eq!(manifest.commitments.len(), 4);
    }

    #[test]
    fn test_genesis_generation() {
        let chain_id = "dchat-test-1".to_string();
        let genesis_time = Utc::now();
        let initial_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;

        let mut manifest = PreStakeManifest::new(chain_id.clone(), genesis_time, initial_supply);

        // Create minimum validators
        let regions = ["us-east", "eu-west", "ap-south", "us-west"];
        for (i, region) in regions.iter().enumerate() {
            let key = generate_test_key();
            let commitment = create_signed_commitment(
                &key,
                format!("validator-{}", i),
                DEFAULT_MIN_GENESIS_STAKE,
                format!("192.168.1.{}:8080", i + 1),
                region.to_string(),
                30,
                &chain_id,
            )
            .unwrap();

            manifest.add_commitment(commitment).unwrap();
        }

        // Build genesis
        let coordinator_key = generate_test_key();
        let builder = PreStakeGenesisBuilder::new(manifest, coordinator_key);
        let (chat_genesis, currency_genesis) = builder.build().unwrap();

        assert_eq!(chat_genesis.block_number, 0);
        assert_eq!(currency_genesis.block_number, 0);
        assert_eq!(chat_genesis.initial_validators.len(), 4);
        assert_eq!(currency_genesis.initial_validators.len(), 4);
    }

    #[test]
    fn test_duplicate_validator_rejected() {
        let chain_id = "dchat-mainnet-1".to_string();
        let genesis_time = Utc::now();
        let initial_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;

        let mut manifest = PreStakeManifest::new(chain_id.clone(), genesis_time, initial_supply);

        let key = generate_test_key();
        let commitment1 = create_signed_commitment(
            &key,
            "validator-1".to_string(),
            DEFAULT_MIN_GENESIS_STAKE,
            "127.0.0.1:8080".to_string(),
            "us-east".to_string(),
            30,
            &chain_id,
        )
        .unwrap();

        let commitment2 = create_signed_commitment(
            &key, // Same key!
            "validator-1-duplicate".to_string(),
            DEFAULT_MIN_GENESIS_STAKE,
            "127.0.0.1:8081".to_string(),
            "us-east".to_string(),
            30,
            &chain_id,
        )
        .unwrap();

        manifest.add_commitment(commitment1).unwrap();
        assert!(manifest.add_commitment(commitment2).is_err());
    }

    #[test]
    fn test_insufficient_validators_rejected() {
        let chain_id = "dchat-mainnet-1".to_string();
        let genesis_time = Utc::now();
        let initial_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;

        let mut manifest = PreStakeManifest::new(chain_id.clone(), genesis_time, initial_supply);

        // Only add 3 validators (minimum is 4)
        for i in 0..3 {
            let key = generate_test_key();
            let commitment = create_signed_commitment(
                &key,
                format!("validator-{}", i),
                DEFAULT_MIN_GENESIS_STAKE,
                format!("192.168.1.{}:8080", i + 1),
                format!("region-{}", i),
                30,
                &chain_id,
            )
            .unwrap();

            manifest.add_commitment(commitment).unwrap();
        }

        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_insufficient_regions_rejected() {
        let chain_id = "dchat-mainnet-1".to_string();
        let genesis_time = Utc::now();
        let initial_supply = 1_000_000_000u64 * MOTES_PER_DCHAT;

        let mut manifest = PreStakeManifest::new(chain_id.clone(), genesis_time, initial_supply);

        // All validators in same region (need minimum 3 regions)
        for i in 0..4 {
            let key = generate_test_key();
            let commitment = create_signed_commitment(
                &key,
                format!("validator-{}", i),
                DEFAULT_MIN_GENESIS_STAKE,
                format!("192.168.1.{}:8080", i + 1),
                "us-east".to_string(), // Same region!
                30,
                &chain_id,
            )
            .unwrap();

            manifest.add_commitment(commitment).unwrap();
        }

        assert!(manifest.validate().is_err());
    }
}
