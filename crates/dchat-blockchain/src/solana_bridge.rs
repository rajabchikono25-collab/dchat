//! Solana Bridge Integration
//!
//! Production-ready bridge connecting dchat wallets to Solana ecosystem:
//! - Cross-chain asset transfers (DCHAT ↔ wDCHAT)
//! - SPL token support
//! - Multi-sig bridge security
//! - Transaction verification

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

use crate::wallet::solana_compat::{SolanaAddress, TokenMint};
use crate::wallet::address::{AddressMapping, UniversalAddress};

/// Solana bridge configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SolanaBridgeConfig {
    /// Solana RPC endpoint
    pub rpc_url: String,
    /// WebSocket endpoint for subscriptions
    pub ws_url: Option<String>,
    /// Bridge program ID on Solana
    pub bridge_program_id: SolanaAddress,
    /// Wrapped DCHAT token mint
    pub wdchat_mint: SolanaAddress,
    /// Required confirmations on dchat
    pub dchat_confirmations: u32,
    /// Required confirmations on Solana
    pub solana_confirmations: u32,
    /// Bridge fee (basis points)
    pub fee_bps: u16,
    /// Minimum transfer amount
    pub min_transfer: u64,
    /// Maximum transfer amount (per tx)
    pub max_transfer: u64,
    /// Bridge validators (multi-sig)
    pub validators: Vec<BridgeValidator>,
    /// Required validator signatures
    pub validator_threshold: usize,
}

impl Default for SolanaBridgeConfig {
    fn default() -> Self {
        // Default to devnet for safety
        Self {
            rpc_url: "https://api.devnet.solana.com".to_string(),
            ws_url: Some("wss://api.devnet.solana.com".to_string()),
            bridge_program_id: SolanaAddress::from_bytes(&[0u8; 32]).unwrap(),
            wdchat_mint: SolanaAddress::from_bytes(&[0u8; 32]).unwrap(),
            dchat_confirmations: 12,
            solana_confirmations: 32,
            fee_bps: 30, // 0.3%
            min_transfer: 1_000_000_000, // 1 DCHAT
            max_transfer: 1_000_000_000_000_000, // 1M DCHAT
            validators: Vec::new(),
            validator_threshold: 2,
        }
    }
}

/// Bridge validator info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeValidator {
    /// Validator ID
    pub id: UserId,
    /// Validator's Solana address
    pub solana_address: SolanaAddress,
    /// Validator's dchat address
    pub dchat_address: String,
    /// Stake amount
    pub stake: u64,
    /// Is active
    pub is_active: bool,
}

/// Direction of bridge transfer
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeDirection {
    /// dchat → Solana (mint wDCHAT)
    DchatToSolana,
    /// Solana → dchat (burn wDCHAT)
    SolanaToDchat,
}

/// Bridge transfer status
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum BridgeTransferStatus {
    /// Transfer initiated, waiting for source chain confirmation
    Pending,
    /// Source chain confirmed, waiting for validator signatures
    AwaitingValidation,
    /// Validators have signed, executing on destination
    Executing,
    /// Transfer completed successfully
    Completed {
        source_tx: String,
        dest_tx: String,
    },
    /// Transfer failed
    Failed {
        reason: String,
    },
    /// Transfer refunded (e.g., timeout)
    Refunded {
        refund_tx: String,
    },
}

/// A cross-chain bridge transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeTransfer {
    /// Unique transfer ID
    pub id: Uuid,
    /// Transfer direction
    pub direction: BridgeDirection,
    /// Source address
    pub source_address: UniversalAddress,
    /// Destination address
    pub destination_address: UniversalAddress,
    /// Amount (in smallest unit)
    pub amount: u64,
    /// Bridge fee
    pub fee: u64,
    /// Current status
    pub status: BridgeTransferStatus,
    /// Initiated at
    pub initiated_at: DateTime<Utc>,
    /// Last updated
    pub updated_at: DateTime<Utc>,
    /// Source chain transaction hash
    pub source_tx_hash: Option<String>,
    /// Destination chain transaction hash
    pub dest_tx_hash: Option<String>,
    /// Validator signatures collected
    pub validator_signatures: Vec<ValidatorSignature>,
    /// Timeout timestamp
    pub timeout_at: DateTime<Utc>,
}

/// Validator signature on bridge transfer
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorSignature {
    pub validator_id: UserId,
    pub signature: Vec<u8>,
    pub signed_at: DateTime<Utc>,
}

impl BridgeTransfer {
    /// Create new transfer dchat → Solana
    pub fn dchat_to_solana(
        source: UniversalAddress,
        dest_solana: SolanaAddress,
        amount: u64,
        fee: u64,
        timeout_hours: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            direction: BridgeDirection::DchatToSolana,
            source_address: source,
            destination_address: UniversalAddress::from_solana(&dest_solana),
            amount,
            fee,
            status: BridgeTransferStatus::Pending,
            initiated_at: Utc::now(),
            updated_at: Utc::now(),
            source_tx_hash: None,
            dest_tx_hash: None,
            validator_signatures: Vec::new(),
            timeout_at: Utc::now() + chrono::Duration::hours(timeout_hours as i64),
        }
    }

    /// Create new transfer Solana → dchat
    pub fn solana_to_dchat(
        source_solana: SolanaAddress,
        dest: UniversalAddress,
        amount: u64,
        fee: u64,
        timeout_hours: u64,
    ) -> Self {
        Self {
            id: Uuid::new_v4(),
            direction: BridgeDirection::SolanaToDchat,
            source_address: UniversalAddress::from_solana(&source_solana),
            destination_address: dest,
            amount,
            fee,
            status: BridgeTransferStatus::Pending,
            initiated_at: Utc::now(),
            updated_at: Utc::now(),
            source_tx_hash: None,
            dest_tx_hash: None,
            validator_signatures: Vec::new(),
            timeout_at: Utc::now() + chrono::Duration::hours(timeout_hours as i64),
        }
    }

    /// Check if transfer has timed out
    pub fn is_timed_out(&self) -> bool {
        Utc::now() > self.timeout_at
    }

    /// Get net amount after fee
    pub fn net_amount(&self) -> u64 {
        self.amount.saturating_sub(self.fee)
    }

    /// Check if enough validator signatures
    pub fn has_quorum(&self, threshold: usize) -> bool {
        self.validator_signatures.len() >= threshold
    }
}

/// Solana Bridge Manager
pub struct SolanaBridge {
    /// Configuration
    config: SolanaBridgeConfig,
    /// Active transfers
    transfers: HashMap<Uuid, BridgeTransfer>,
    /// Address mappings (dchat ↔ Solana)
    address_mappings: HashMap<String, AddressMapping>,
    /// Token mints supported
    supported_tokens: Vec<TokenMint>,
    /// Total volume bridged (for statistics)
    total_volume_dchat_to_solana: u64,
    total_volume_solana_to_dchat: u64,
}

impl SolanaBridge {
    /// Create a new Solana bridge
    pub fn new(config: SolanaBridgeConfig) -> Result<Self> {
        // Validate config
        if config.validator_threshold > config.validators.len() {
            return Err(Error::validation(
                "Validator threshold exceeds number of validators"
            ));
        }

        if config.validator_threshold == 0 {
            return Err(Error::validation("Validator threshold must be at least 1"));
        }

        let mut bridge = Self {
            config,
            transfers: HashMap::new(),
            address_mappings: HashMap::new(),
            supported_tokens: Vec::new(),
            total_volume_dchat_to_solana: 0,
            total_volume_solana_to_dchat: 0,
        };

        // Add wDCHAT token
        bridge.supported_tokens.push(TokenMint::dchat_wrapped()?);

        Ok(bridge)
    }

    /// Get configuration
    pub fn config(&self) -> &SolanaBridgeConfig {
        &self.config
    }

    /// Calculate bridge fee
    pub fn calculate_fee(&self, amount: u64) -> u64 {
        (amount as u128 * self.config.fee_bps as u128 / 10000) as u64
    }

    /// Initiate a transfer from dchat to Solana
    pub fn initiate_dchat_to_solana(
        &mut self,
        source_address: UniversalAddress,
        dest_solana_address: &str,
        amount: u64,
    ) -> Result<Uuid> {
        // Validate amount
        if amount < self.config.min_transfer {
            return Err(Error::validation(format!(
                "Amount {} below minimum {}",
                amount, self.config.min_transfer
            )));
        }

        if amount > self.config.max_transfer {
            return Err(Error::validation(format!(
                "Amount {} exceeds maximum {}",
                amount, self.config.max_transfer
            )));
        }

        // Parse destination address
        let dest = SolanaAddress::from_base58(dest_solana_address)?;

        // Calculate fee
        let fee = self.calculate_fee(amount);

        // Create transfer
        let transfer = BridgeTransfer::dchat_to_solana(
            source_address,
            dest,
            amount,
            fee,
            24, // 24 hour timeout
        );

        let transfer_id = transfer.id;
        self.transfers.insert(transfer_id, transfer);

        tracing::info!(
            "Bridge transfer {} initiated: dchat → Solana, {} tokens (fee: {})",
            transfer_id,
            amount,
            fee
        );

        Ok(transfer_id)
    }

    /// Initiate a transfer from Solana to dchat
    pub fn initiate_solana_to_dchat(
        &mut self,
        source_solana_address: &str,
        dest_address: UniversalAddress,
        amount: u64,
    ) -> Result<Uuid> {
        // Validate amount
        if amount < self.config.min_transfer {
            return Err(Error::validation("Amount below minimum"));
        }

        if amount > self.config.max_transfer {
            return Err(Error::validation("Amount exceeds maximum"));
        }

        // Parse source address
        let source = SolanaAddress::from_base58(source_solana_address)?;

        // Calculate fee
        let fee = self.calculate_fee(amount);

        // Create transfer
        let transfer = BridgeTransfer::solana_to_dchat(
            source,
            dest_address,
            amount,
            fee,
            24,
        );

        let transfer_id = transfer.id;
        self.transfers.insert(transfer_id, transfer);

        tracing::info!(
            "Bridge transfer {} initiated: Solana → dchat, {} tokens",
            transfer_id,
            amount
        );

        Ok(transfer_id)
    }

    /// Record source chain transaction confirmation
    pub fn record_source_confirmation(
        &mut self,
        transfer_id: Uuid,
        tx_hash: String,
    ) -> Result<()> {
        let transfer = self.transfers.get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        if transfer.source_tx_hash.is_some() {
            return Err(Error::validation("Source tx already recorded"));
        }

        transfer.source_tx_hash = Some(tx_hash);
        transfer.status = BridgeTransferStatus::AwaitingValidation;
        transfer.updated_at = Utc::now();

        Ok(())
    }

    /// Submit validator signature
    pub fn submit_validator_signature(
        &mut self,
        transfer_id: Uuid,
        validator_id: UserId,
        signature: Vec<u8>,
    ) -> Result<bool> {
        // Validate signature format upfront
        if signature.len() != 64 {
            return Err(Error::crypto("Invalid signature length"));
        }

        // Check if validator is valid
        let validator = self.config.validators.iter()
            .find(|v| v.id == validator_id && v.is_active)
            .ok_or_else(|| Error::validation("Unknown or inactive validator"))?
            .clone();

        // Get transfer and create signing message
        let transfer = self.transfers.get(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        // Check if already signed
        if transfer.validator_signatures.iter().any(|s| s.validator_id == validator_id) {
            return Err(Error::validation("Validator already signed"));
        }

        // Verify the signature against the transfer data
        let signing_message = self.create_signing_message(transfer);
        self.verify_validator_signature(
            &validator.solana_address,
            &signing_message,
            &signature,
        )?;

        // Now get mutable reference and add the verified signature
        let transfer = self.transfers.get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;
        
        // Add signature
        transfer.validator_signatures.push(ValidatorSignature {
            validator_id: validator.id,
            signature,
            signed_at: Utc::now(),
        });
        transfer.updated_at = Utc::now();

        let has_quorum = transfer.has_quorum(self.config.validator_threshold);

        if has_quorum && transfer.status == BridgeTransferStatus::AwaitingValidation {
            transfer.status = BridgeTransferStatus::Executing;
            tracing::info!(
                "Transfer {} reached quorum ({} signatures), executing",
                transfer_id,
                transfer.validator_signatures.len()
            );
        }

        Ok(has_quorum)
    }

    /// Verify a validator's signature on the signing message
    fn verify_validator_signature(
        &self,
        validator_address: &SolanaAddress,
        message: &[u8],
        signature: &[u8],
    ) -> Result<()> {
        let sig_bytes: [u8; 64] = signature.try_into()
            .map_err(|_| Error::crypto("Invalid signature length"))?;
        
        let verifying_key = VerifyingKey::from_bytes(validator_address.as_bytes())
            .map_err(|e| Error::crypto(format!("Invalid validator public key: {}", e)))?;
        
        let sig = Signature::from_bytes(&sig_bytes);
        
        verifying_key.verify_strict(message, &sig)
            .map_err(|e| Error::crypto(format!("Validator signature verification failed: {}", e)))?;
        
        Ok(())
    }

    /// Create signing message for validators
    /// 
    /// Generates a deterministic message hash that validators must sign
    /// to approve a bridge transfer. The message includes all relevant
    /// transfer details to prevent replay attacks.
    fn create_signing_message(&self, transfer: &BridgeTransfer) -> Vec<u8> {
        let mut msg = Vec::new();
        msg.extend_from_slice(transfer.id.as_bytes());
        msg.extend_from_slice(&(transfer.direction as u8).to_le_bytes());
        msg.extend_from_slice(&transfer.amount.to_le_bytes());
        msg.extend_from_slice(&transfer.fee.to_le_bytes());
        if let Some(ref hash) = transfer.source_tx_hash {
            msg.extend_from_slice(hash.as_bytes());
        }
        blake3::hash(&msg).as_bytes().to_vec()
    }

    /// Get the signing message for a transfer (for validators to sign)
    /// 
    /// This public method allows validators to retrieve the message they
    /// need to sign to approve a transfer.
    pub fn get_signing_message(&self, transfer_id: Uuid) -> Result<Vec<u8>> {
        let transfer = self.transfers.get(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;
        
        Ok(self.create_signing_message(transfer))
    }

    /// Complete transfer (record destination tx)
    pub fn complete_transfer(
        &mut self,
        transfer_id: Uuid,
        dest_tx_hash: String,
    ) -> Result<()> {
        let transfer = self.transfers.get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        if !matches!(transfer.status, BridgeTransferStatus::Executing) {
            return Err(Error::validation("Transfer not in executing state"));
        }

        let source_tx = transfer.source_tx_hash.clone()
            .ok_or_else(|| Error::validation("Source tx not recorded"))?;

        transfer.dest_tx_hash = Some(dest_tx_hash.clone());
        transfer.status = BridgeTransferStatus::Completed {
            source_tx,
            dest_tx: dest_tx_hash,
        };
        transfer.updated_at = Utc::now();

        // Update volume stats
        match transfer.direction {
            BridgeDirection::DchatToSolana => {
                self.total_volume_dchat_to_solana += transfer.amount;
            }
            BridgeDirection::SolanaToDchat => {
                self.total_volume_solana_to_dchat += transfer.amount;
            }
        }

        tracing::info!("Transfer {} completed successfully", transfer_id);

        Ok(())
    }

    /// Fail transfer
    pub fn fail_transfer(&mut self, transfer_id: Uuid, reason: String) -> Result<()> {
        let transfer = self.transfers.get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        transfer.status = BridgeTransferStatus::Failed { reason };
        transfer.updated_at = Utc::now();

        Ok(())
    }

    /// Refund timed-out transfer
    pub fn refund_transfer(&mut self, transfer_id: Uuid, refund_tx: String) -> Result<()> {
        let transfer = self.transfers.get_mut(&transfer_id)
            .ok_or_else(|| Error::validation("Transfer not found"))?;

        if !transfer.is_timed_out() {
            return Err(Error::validation("Transfer has not timed out"));
        }

        transfer.status = BridgeTransferStatus::Refunded { refund_tx };
        transfer.updated_at = Utc::now();

        Ok(())
    }

    /// Get transfer by ID
    pub fn get_transfer(&self, transfer_id: Uuid) -> Option<&BridgeTransfer> {
        self.transfers.get(&transfer_id)
    }

    /// Get pending transfers
    pub fn pending_transfers(&self) -> Vec<&BridgeTransfer> {
        self.transfers.values()
            .filter(|t| matches!(
                t.status,
                BridgeTransferStatus::Pending | BridgeTransferStatus::AwaitingValidation
            ))
            .collect()
    }

    /// Register address mapping
    pub fn register_address_mapping(&mut self, mapping: AddressMapping) {
        if let Some(ref addr) = mapping.dchat {
            self.address_mappings.insert(addr.to_hex(), mapping);
        }
    }

    /// Get address mapping
    pub fn get_address_mapping(&self, dchat_address: &str) -> Option<&AddressMapping> {
        self.address_mappings.get(dchat_address)
    }

    /// Cleanup timed-out transfers
    pub fn cleanup_timed_out(&mut self) -> Vec<Uuid> {
        let timed_out: Vec<Uuid> = self.transfers.iter()
            .filter(|(_, t)| {
                t.is_timed_out() && matches!(
                    t.status,
                    BridgeTransferStatus::Pending | BridgeTransferStatus::AwaitingValidation
                )
            })
            .map(|(id, _)| *id)
            .collect();

        for id in &timed_out {
            if let Some(transfer) = self.transfers.get_mut(id) {
                transfer.status = BridgeTransferStatus::Failed {
                    reason: "Transfer timed out".to_string(),
                };
            }
        }

        timed_out
    }

    /// Get bridge statistics
    pub fn statistics(&self) -> BridgeStatistics {
        let pending_count = self.transfers.values()
            .filter(|t| matches!(
                t.status,
                BridgeTransferStatus::Pending | BridgeTransferStatus::AwaitingValidation | BridgeTransferStatus::Executing
            ))
            .count();

        let completed_count = self.transfers.values()
            .filter(|t| matches!(t.status, BridgeTransferStatus::Completed { .. }))
            .count();

        let failed_count = self.transfers.values()
            .filter(|t| matches!(t.status, BridgeTransferStatus::Failed { .. }))
            .count();

        BridgeStatistics {
            total_transfers: self.transfers.len(),
            pending_count,
            completed_count,
            failed_count,
            total_volume_dchat_to_solana: self.total_volume_dchat_to_solana,
            total_volume_solana_to_dchat: self.total_volume_solana_to_dchat,
            active_validators: self.config.validators.iter().filter(|v| v.is_active).count(),
        }
    }
}

/// Bridge statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeStatistics {
    pub total_transfers: usize,
    pub pending_count: usize,
    pub completed_count: usize,
    pub failed_count: usize,
    pub total_volume_dchat_to_solana: u64,
    pub total_volume_solana_to_dchat: u64,
    pub active_validators: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;
    use ed25519_dalek::Signer;

    /// Test validator with signing key for test signature creation
    struct TestValidator {
        info: BridgeValidator,
        signing_key: ed25519_dalek::SigningKey,
    }

    fn create_test_validators(count: usize) -> Vec<TestValidator> {
        (0..count).map(|_| {
            let keypair = KeyPair::try_generate().unwrap();
            let signing_key = ed25519_dalek::SigningKey::from_bytes(
                keypair.private_key().as_bytes()
            );
            
            TestValidator {
                info: BridgeValidator {
                    id: UserId::new(),
                    solana_address: SolanaAddress::from_public_key(keypair.public_key()),
                    dchat_address: keypair.to_address().to_hex(),
                    stake: 10000,
                    is_active: true,
                },
                signing_key,
            }
        }).collect()
    }

    fn create_test_config() -> SolanaBridgeConfig {
        let mut config = SolanaBridgeConfig::default();
        
        // Add test validators
        for _ in 0..3 {
            let keypair = KeyPair::try_generate().unwrap();
            config.validators.push(BridgeValidator {
                id: UserId::new(),
                solana_address: SolanaAddress::from_public_key(keypair.public_key()),
                dchat_address: keypair.to_address().to_hex(),
                stake: 10000,
                is_active: true,
            });
        }
        
        config.validator_threshold = 2;
        config
    }

    fn create_test_config_with_signers() -> (SolanaBridgeConfig, Vec<TestValidator>) {
        let validators = create_test_validators(3);
        
        let mut config = SolanaBridgeConfig::default();
        config.validators = validators.iter().map(|v| v.info.clone()).collect();
        config.validator_threshold = 2;
        
        (config, validators)
    }

    #[test]
    fn test_bridge_creation() {
        let config = create_test_config();
        let bridge = SolanaBridge::new(config).unwrap();
        
        assert_eq!(bridge.config().validator_threshold, 2);
        assert!(!bridge.supported_tokens.is_empty());
    }

    #[test]
    fn test_fee_calculation() {
        let config = create_test_config();
        let bridge = SolanaBridge::new(config).unwrap();
        
        // 30 bps = 0.3%
        let fee = bridge.calculate_fee(1_000_000_000);
        assert_eq!(fee, 3_000_000); // 0.3% of 1B
    }

    #[test]
    fn test_transfer_initiation() {
        let config = create_test_config();
        let mut bridge = SolanaBridge::new(config).unwrap();
        
        let keypair = KeyPair::try_generate().unwrap();
        let source = UniversalAddress::from_public_key(
            keypair.public_key(),
            crate::wallet::AddressFormat::DchatNative,
        );
        
        let dest_keypair = KeyPair::try_generate().unwrap();
        let dest = SolanaAddress::from_public_key(dest_keypair.public_key());
        
        let transfer_id = bridge.initiate_dchat_to_solana(
            source,
            &dest.to_base58(),
            1_000_000_000,
        ).unwrap();
        
        let transfer = bridge.get_transfer(transfer_id).unwrap();
        assert_eq!(transfer.direction, BridgeDirection::DchatToSolana);
        assert!(matches!(transfer.status, BridgeTransferStatus::Pending));
    }

    #[test]
    fn test_transfer_validation_flow() {
        let (config, validators) = create_test_config_with_signers();
        let mut bridge = SolanaBridge::new(config).unwrap();
        
        let keypair = KeyPair::try_generate().unwrap();
        let source = UniversalAddress::from_public_key(
            keypair.public_key(),
            crate::wallet::AddressFormat::DchatNative,
        );
        
        let dest_keypair = KeyPair::try_generate().unwrap();
        let dest = SolanaAddress::from_public_key(dest_keypair.public_key());
        
        let transfer_id = bridge.initiate_dchat_to_solana(
            source,
            &dest.to_base58(),
            1_000_000_000,
        ).unwrap();
        
        // Record source confirmation
        bridge.record_source_confirmation(transfer_id, "tx_hash_123".to_string()).unwrap();
        
        let transfer = bridge.get_transfer(transfer_id).unwrap();
        assert!(matches!(transfer.status, BridgeTransferStatus::AwaitingValidation));
        
        // Get the signing message and create real signatures
        let signing_message = bridge.get_signing_message(transfer_id).unwrap();
        
        // Submit first validator signature
        let sig1 = validators[0].signing_key.sign(&signing_message);
        let has_quorum = bridge.submit_validator_signature(
            transfer_id,
            validators[0].info.id.clone(),
            sig1.to_bytes().to_vec(),
        ).unwrap();
        assert!(!has_quorum);
        
        // Submit second validator signature - reaches quorum
        let sig2 = validators[1].signing_key.sign(&signing_message);
        let has_quorum = bridge.submit_validator_signature(
            transfer_id,
            validators[1].info.id.clone(),
            sig2.to_bytes().to_vec(),
        ).unwrap();
        assert!(has_quorum);
        
        let transfer = bridge.get_transfer(transfer_id).unwrap();
        assert!(matches!(transfer.status, BridgeTransferStatus::Executing));
    }

    #[test]
    fn test_invalid_signature_rejected() {
        let (config, validators) = create_test_config_with_signers();
        let mut bridge = SolanaBridge::new(config).unwrap();
        
        let keypair = KeyPair::try_generate().unwrap();
        let source = UniversalAddress::from_public_key(
            keypair.public_key(),
            crate::wallet::AddressFormat::DchatNative,
        );
        
        let dest_keypair = KeyPair::try_generate().unwrap();
        let dest = SolanaAddress::from_public_key(dest_keypair.public_key());
        
        let transfer_id = bridge.initiate_dchat_to_solana(
            source,
            &dest.to_base58(),
            1_000_000_000,
        ).unwrap();
        
        bridge.record_source_confirmation(transfer_id, "tx_hash_123".to_string()).unwrap();
        
        // Try to submit an invalid signature (all zeros)
        let result = bridge.submit_validator_signature(
            transfer_id,
            validators[0].info.id.clone(),
            vec![0u8; 64],
        );
        
        // Should fail signature verification
        assert!(result.is_err());
    }

    #[test]
    fn test_min_transfer_limit() {
        let mut config = create_test_config();
        config.min_transfer = 1_000_000_000;
        let mut bridge = SolanaBridge::new(config).unwrap();
        
        let keypair = KeyPair::try_generate().unwrap();
        let source = UniversalAddress::from_public_key(
            keypair.public_key(),
            crate::wallet::AddressFormat::DchatNative,
        );
        
        let dest_keypair = KeyPair::try_generate().unwrap();
        let dest = SolanaAddress::from_public_key(dest_keypair.public_key());
        
        // Below minimum
        let result = bridge.initiate_dchat_to_solana(
            source,
            &dest.to_base58(),
            100, // Too small
        );
        assert!(result.is_err());
    }
}
