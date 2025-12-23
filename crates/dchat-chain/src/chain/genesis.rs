//! Genesis block initialization for both chains
//!
//! Creates the first blocks for chat chain and currency chain when the first validator comes online

use dchat_core::error::{Error, Result};
use dchat_core::motes::{DCHAT_DECIMALS, MOTES_PER_DCHAT};
use ed25519_dalek::{Signature, Signer, SigningKey, VerifyingKey};
use serde::{Deserialize, Serialize};
use tracing::info;

/// Genesis block for chat chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatGenesisBlock {
    /// Genesis block number (always 0)
    pub block_number: u64,
    /// Genesis timestamp
    pub timestamp: u64,
    /// Initial validator set
    pub initial_validators: Vec<GenesisValidator>,
    /// Genesis configuration
    pub config: ChatGenesisConfig,
    /// Block hash
    pub hash: String,
    /// Genesis signature
    pub signature: Vec<u8>,
}

/// Genesis block for currency chain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyGenesisBlock {
    /// Genesis block number (always 0)
    pub block_number: u64,
    /// Genesis timestamp
    pub timestamp: u64,
    /// Initial validator set
    pub initial_validators: Vec<GenesisValidator>,
    /// Genesis configuration
    pub config: CurrencyGenesisConfig,
    /// Initial token supply
    pub initial_supply: u64,
    /// Block hash
    pub hash: String,
    /// Genesis signature
    pub signature: Vec<u8>,
}

/// Genesis validator information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GenesisValidator {
    /// Validator public key
    pub public_key: String,
    /// Initial stake amount
    pub stake_amount: u64,
    /// Voting power
    pub voting_power: u64,
    /// Network address
    pub address: String,
}

/// Chat chain genesis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatGenesisConfig {
    /// Chain ID
    pub chain_id: String,
    /// Block time in seconds
    pub block_time_secs: u64,
    /// Maximum message size
    pub max_message_size: usize,
    /// Minimum reputation score
    pub min_reputation_score: i64,
}

/// Currency chain genesis configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CurrencyGenesisConfig {
    /// Chain ID
    pub chain_id: String,
    /// Block time in seconds
    pub block_time_secs: u64,
    /// Token name
    pub token_name: String,
    /// Token symbol
    pub token_symbol: String,
    /// Token decimals
    pub token_decimals: u8,
}

/// Genesis block builder
pub struct GenesisBuilder {
    signing_key: SigningKey,
}

impl GenesisBuilder {
    /// Create a new genesis builder with the first validator's key
    pub fn new(signing_key: SigningKey) -> Self {
        Self { signing_key }
    }

    /// Create chat chain genesis block
    pub fn create_chat_genesis(
        &self,
        initial_validators: Vec<GenesisValidator>,
        config: ChatGenesisConfig,
    ) -> Result<ChatGenesisBlock> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let verifying_key = self.signing_key.verifying_key();

        // Create genesis block data
        let mut genesis = ChatGenesisBlock {
            block_number: 0,
            timestamp,
            initial_validators,
            config,
            hash: String::new(),
            signature: Vec::new(),
        };

        // Compute hash
        let hash_input = Self::compute_chat_genesis_hash_input(&genesis);
        genesis.hash = hex::encode(blake3::hash(&hash_input).as_bytes());

        // Sign genesis block
        let signature = self.signing_key.sign(&hash_input);
        genesis.signature = signature.to_bytes().to_vec();

        info!("✅ Chat chain genesis block created");
        info!("   Block number: {}", genesis.block_number);
        info!("   Timestamp: {}", genesis.timestamp);
        info!("   Hash: {}", genesis.hash);
        info!("   Validators: {}", genesis.initial_validators.len());
        info!(
            "   Genesis validator: {}",
            hex::encode(verifying_key.as_bytes())
        );

        Ok(genesis)
    }

    /// Create currency chain genesis block
    pub fn create_currency_genesis(
        &self,
        initial_validators: Vec<GenesisValidator>,
        config: CurrencyGenesisConfig,
        initial_supply: u64,
    ) -> Result<CurrencyGenesisBlock> {
        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let verifying_key = self.signing_key.verifying_key();

        // Create genesis block data
        let mut genesis = CurrencyGenesisBlock {
            block_number: 0,
            timestamp,
            initial_validators,
            config,
            initial_supply,
            hash: String::new(),
            signature: Vec::new(),
        };

        // Compute hash
        let hash_input = Self::compute_currency_genesis_hash_input(&genesis);
        genesis.hash = hex::encode(blake3::hash(&hash_input).as_bytes());

        // Sign genesis block
        let signature = self.signing_key.sign(&hash_input);
        genesis.signature = signature.to_bytes().to_vec();

        info!("✅ Currency chain genesis block created");
        info!("   Block number: {}", genesis.block_number);
        info!("   Timestamp: {}", genesis.timestamp);
        info!("   Hash: {}", genesis.hash);
        info!("   Initial supply: {} tokens", genesis.initial_supply);
        info!("   Validators: {}", genesis.initial_validators.len());
        info!(
            "   Genesis validator: {}",
            hex::encode(verifying_key.as_bytes())
        );

        Ok(genesis)
    }

    /// Verify chat genesis block signature
    pub fn verify_chat_genesis(
        genesis: &ChatGenesisBlock,
        public_key: &VerifyingKey,
    ) -> Result<()> {
        let hash_input = Self::compute_chat_genesis_hash_input(genesis);

        let signature_bytes: [u8; 64] = genesis
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;
        let signature = Signature::from_bytes(&signature_bytes);

        public_key
            .verify_strict(&hash_input, &signature)
            .map_err(|_| Error::crypto("Genesis signature verification failed"))?;

        info!("✅ Chat genesis signature verified");
        Ok(())
    }

    /// Verify currency genesis block signature
    pub fn verify_currency_genesis(
        genesis: &CurrencyGenesisBlock,
        public_key: &VerifyingKey,
    ) -> Result<()> {
        let hash_input = Self::compute_currency_genesis_hash_input(genesis);

        let signature_bytes: [u8; 64] = genesis
            .signature
            .as_slice()
            .try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;
        let signature = Signature::from_bytes(&signature_bytes);

        public_key
            .verify_strict(&hash_input, &signature)
            .map_err(|_| Error::crypto("Genesis signature verification failed"))?;

        info!("✅ Currency genesis signature verified");
        Ok(())
    }

    /// Compute hash input for chat genesis
    fn compute_chat_genesis_hash_input(genesis: &ChatGenesisBlock) -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(&genesis.block_number.to_be_bytes());
        input.extend_from_slice(&genesis.timestamp.to_be_bytes());
        input.extend_from_slice(genesis.config.chain_id.as_bytes());
        input.extend_from_slice(&genesis.config.block_time_secs.to_be_bytes());

        for validator in &genesis.initial_validators {
            input.extend_from_slice(validator.public_key.as_bytes());
            input.extend_from_slice(&validator.stake_amount.to_be_bytes());
        }

        input
    }

    /// Compute hash input for currency genesis
    fn compute_currency_genesis_hash_input(genesis: &CurrencyGenesisBlock) -> Vec<u8> {
        let mut input = Vec::new();
        input.extend_from_slice(&genesis.block_number.to_be_bytes());
        input.extend_from_slice(&genesis.timestamp.to_be_bytes());
        input.extend_from_slice(genesis.config.chain_id.as_bytes());
        input.extend_from_slice(&genesis.config.block_time_secs.to_be_bytes());
        input.extend_from_slice(&genesis.initial_supply.to_be_bytes());

        for validator in &genesis.initial_validators {
            input.extend_from_slice(validator.public_key.as_bytes());
            input.extend_from_slice(&validator.stake_amount.to_be_bytes());
        }

        input
    }
}

/// Genesis initialization coordinator
pub struct GenesisCoordinator;

impl GenesisCoordinator {
    /// Initialize both chains with genesis blocks when first validator comes online
    pub async fn initialize_chains(
        first_validator_key: SigningKey,
        chat_rpc: &str,
        currency_rpc: &str,
    ) -> Result<(ChatGenesisBlock, CurrencyGenesisBlock)> {
        info!("🚀 Initializing genesis blocks for MAINNET LAUNCH");

        let builder = GenesisBuilder::new(first_validator_key);
        let verifying_key = builder.signing_key.verifying_key();

        // Create first validator entry
        let first_validator = GenesisValidator {
            public_key: hex::encode(verifying_key.as_bytes()),
            stake_amount: dchat_core::config::constants::MIN_VALIDATOR_STAKE,
            voting_power: 100,
            address: "validator-0".to_string(),
        };

        // Chat chain genesis config
        let chat_config = ChatGenesisConfig {
            chain_id: "dchat-mainnet-1".to_string(),
            block_time_secs: 3,
            max_message_size: 1024 * 1024, // 1 MB
            min_reputation_score: 0,
        };

        // Currency chain genesis config
        // DCHAT uses 8 decimal places: 1 DCHAT = 100,000,000 motes
        let currency_config = CurrencyGenesisConfig {
            chain_id: "dchat-currency-mainnet-1".to_string(),
            block_time_secs: 5,
            token_name: "DChat Token".to_string(),
            token_symbol: "DCHAT".to_string(),
            token_decimals: DCHAT_DECIMALS,
        };

        // Create genesis blocks
        let chat_genesis =
            builder.create_chat_genesis(vec![first_validator.clone()], chat_config)?;
        // Initial supply: 1 billion DCHAT = 1_000_000_000 * 100_000_000 motes
        let initial_supply_motes = 1_000_000_000u64.saturating_mul(MOTES_PER_DCHAT);
        let currency_genesis = builder.create_currency_genesis(
            vec![first_validator.clone()],
            currency_config,
            initial_supply_motes, // 1 billion DCHAT in motes (8 decimals)
        )?;

        // Submit to chains
        Self::submit_chat_genesis(&chat_genesis, chat_rpc).await?;
        Self::submit_currency_genesis(&currency_genesis, currency_rpc).await?;

        info!("✅ Both chains initialized with genesis blocks");
        info!("   Chat chain: {}", chat_genesis.hash);
        info!("   Currency chain: {}", currency_genesis.hash);

        Ok((chat_genesis, currency_genesis))
    }

    /// Submit chat genesis block to chain
    async fn submit_chat_genesis(genesis: &ChatGenesisBlock, rpc_endpoint: &str) -> Result<()> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "chat.submit_genesis",
            "params": {
                "genesis": genesis,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        info!("📤 Submitting chat genesis to {}", rpc_endpoint);

        let client = HttpClient::new();
        let response = client
            .post(rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to submit chat genesis: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Chat genesis submission failed: status {}",
                response.status()
            )));
        }

        info!("✅ Chat genesis accepted by chain");
        Ok(())
    }

    /// Submit currency genesis block to chain
    async fn submit_currency_genesis(
        genesis: &CurrencyGenesisBlock,
        rpc_endpoint: &str,
    ) -> Result<()> {
        use reqwest::Client as HttpClient;
        use serde_json::json;

        let payload = json!({
            "method": "currency.submit_genesis",
            "params": {
                "genesis": genesis,
            },
            "jsonrpc": "2.0",
            "id": 1,
        });

        info!("📤 Submitting currency genesis to {}", rpc_endpoint);

        let client = HttpClient::new();
        let response = client
            .post(rpc_endpoint)
            .json(&payload)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to submit currency genesis: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "Currency genesis submission failed: status {}",
                response.status()
            )));
        }

        info!("✅ Currency genesis accepted by chain");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ed25519_dalek::SigningKey;
    use rand::rngs::OsRng;

    #[test]
    fn test_chat_genesis_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let builder = GenesisBuilder::new(signing_key);

        let validator = GenesisValidator {
            public_key: hex::encode(builder.signing_key.verifying_key().as_bytes()),
            stake_amount: 1_000_000,
            voting_power: 100,
            address: "test-validator".to_string(),
        };

        let config = ChatGenesisConfig {
            chain_id: "test-chat".to_string(),
            block_time_secs: 3,
            max_message_size: 1024,
            min_reputation_score: 0,
        };

        let genesis = builder
            .create_chat_genesis(vec![validator], config)
            .unwrap();

        assert_eq!(genesis.block_number, 0);
        assert!(!genesis.hash.is_empty());
        assert!(!genesis.signature.is_empty());
    }

    #[test]
    fn test_currency_genesis_creation() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let builder = GenesisBuilder::new(signing_key);

        let validator = GenesisValidator {
            public_key: hex::encode(builder.signing_key.verifying_key().as_bytes()),
            stake_amount: 1_000_000,
            voting_power: 100,
            address: "test-validator".to_string(),
        };

        let config = CurrencyGenesisConfig {
            chain_id: "test-currency".to_string(),
            block_time_secs: 5,
            token_name: "Test Token".to_string(),
            token_symbol: "TEST".to_string(),
            token_decimals: DCHAT_DECIMALS, // 8 decimals: 1 DCHAT = 100,000,000 motes
        };

        let genesis = builder
            .create_currency_genesis(vec![validator], config, 1_000_000_000)
            .unwrap();

        assert_eq!(genesis.block_number, 0);
        assert_eq!(genesis.initial_supply, 1_000_000_000);
        assert!(!genesis.hash.is_empty());
        assert!(!genesis.signature.is_empty());
    }

    #[test]
    fn test_genesis_verification() {
        let signing_key = SigningKey::generate(&mut OsRng);
        let verifying_key = signing_key.verifying_key();
        let builder = GenesisBuilder::new(signing_key);

        let validator = GenesisValidator {
            public_key: hex::encode(verifying_key.as_bytes()),
            stake_amount: 1_000_000,
            voting_power: 100,
            address: "test-validator".to_string(),
        };

        let config = ChatGenesisConfig {
            chain_id: "test-chat".to_string(),
            block_time_secs: 3,
            max_message_size: 1024,
            min_reputation_score: 0,
        };

        let genesis = builder
            .create_chat_genesis(vec![validator], config)
            .unwrap();

        // Verify with correct key
        let result = GenesisBuilder::verify_chat_genesis(&genesis, &verifying_key);
        assert!(result.is_ok());

        // Verify with wrong key
        let wrong_key = SigningKey::generate(&mut OsRng).verifying_key();
        let result = GenesisBuilder::verify_chat_genesis(&genesis, &wrong_key);
        assert!(result.is_err());
    }
}
