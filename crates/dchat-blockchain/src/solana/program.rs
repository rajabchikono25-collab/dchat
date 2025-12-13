//! dchat Bridge Program Interface
//!
//! Solana program interface for the dchat-Solana bridge.
//! Handles locking/unlocking wDCHAT tokens and bridge state management.

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};

use super::spl_token::SplToken;
use super::transaction::{AccountMeta, Instruction};
use crate::wallet::solana_compat::SolanaAddress;

/// Bridge program seed for PDA derivation
pub const BRIDGE_SEED: &[u8] = b"dchat_bridge";

/// Bridge state seed
pub const BRIDGE_STATE_SEED: &[u8] = b"bridge_state";

/// Deposit record seed  
pub const DEPOSIT_SEED: &[u8] = b"deposit";

/// Withdrawal record seed
pub const WITHDRAWAL_SEED: &[u8] = b"withdrawal";

/// Bridge program interface
pub struct BridgeProgram {
    /// Program ID
    pub program_id: SolanaAddress,
    /// wDCHAT token mint
    pub wdchat_mint: SolanaAddress,
    /// Bridge authority (PDA)
    pub bridge_authority: SolanaAddress,
    /// Bridge authority bump seed
    pub authority_bump: u8,
}

impl BridgeProgram {
    /// Create a new bridge program interface
    pub fn new(program_id: SolanaAddress, wdchat_mint: SolanaAddress) -> Result<Self> {
        // Derive bridge authority PDA
        let seeds: &[&[u8]] = &[BRIDGE_SEED, program_id.as_bytes()];
        let (bridge_authority, authority_bump) = SolanaAddress::derive_pda(seeds, &program_id)?;

        Ok(Self {
            program_id,
            wdchat_mint,
            bridge_authority,
            authority_bump,
        })
    }

    /// Get bridge state PDA
    pub fn get_bridge_state_address(&self) -> Result<SolanaAddress> {
        let seeds: &[&[u8]] = &[BRIDGE_STATE_SEED];
        let (pda, _) = SolanaAddress::derive_pda(seeds, &self.program_id)?;
        Ok(pda)
    }

    /// Get deposit record PDA for a transfer ID
    pub fn get_deposit_address(&self, transfer_id: &[u8; 16]) -> Result<SolanaAddress> {
        let seeds: &[&[u8]] = &[DEPOSIT_SEED, transfer_id];
        let (pda, _) = SolanaAddress::derive_pda(seeds, &self.program_id)?;
        Ok(pda)
    }

    /// Get withdrawal record PDA for a transfer ID
    pub fn get_withdrawal_address(&self, transfer_id: &[u8; 16]) -> Result<SolanaAddress> {
        let seeds: &[&[u8]] = &[WITHDRAWAL_SEED, transfer_id];
        let (pda, _) = SolanaAddress::derive_pda(seeds, &self.program_id)?;
        Ok(pda)
    }

    // ============ Bridge Instructions ============

    /// Initialize the bridge
    pub fn initialize(
        &self,
        admin: &SolanaAddress,
        fee_collector: &SolanaAddress,
        fee_bps: u16,
        min_transfer: u64,
        max_transfer: u64,
        required_confirmations: u32,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;
        let system_program = SolanaAddress::from_base58(super::SYSTEM_PROGRAM_ID)?;

        let data = BridgeInstruction::Initialize {
            fee_bps,
            min_transfer,
            max_transfer,
            required_confirmations,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::signer_writable(admin.clone()),
            AccountMeta::writable(bridge_state),
            AccountMeta::readonly(self.wdchat_mint.clone()),
            AccountMeta::readonly(fee_collector.clone()),
            AccountMeta::readonly(self.bridge_authority.clone()),
            AccountMeta::readonly(system_program),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Lock tokens on Solana (dchat → Solana direction)
    /// This mints wDCHAT after receiving proof from dchat chain
    pub fn mint_wrapped(
        &self,
        transfer_id: [u8; 16],
        recipient: &SolanaAddress,
        amount: u64,
        dchat_tx_hash: [u8; 32],
        validator_signatures: Vec<[u8; 64]>,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;
        let deposit_record = self.get_deposit_address(&transfer_id)?;
        let recipient_ata = SplToken::get_associated_token_address(recipient, &self.wdchat_mint)?;
        let token_program = SplToken::program_id()?;
        let system_program = SolanaAddress::from_base58(super::SYSTEM_PROGRAM_ID)?;

        let data = BridgeInstruction::MintWrapped {
            transfer_id,
            amount,
            dchat_tx_hash,
            signatures: validator_signatures,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::writable(bridge_state),
            AccountMeta::writable(deposit_record),
            AccountMeta::writable(self.wdchat_mint.clone()),
            AccountMeta::writable(recipient_ata),
            AccountMeta::readonly(recipient.clone()),
            AccountMeta::readonly(self.bridge_authority.clone()),
            AccountMeta::readonly(token_program),
            AccountMeta::readonly(system_program),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Burn wrapped tokens (Solana → dchat direction)
    /// Burns wDCHAT and creates a withdrawal record for dchat chain
    pub fn burn_wrapped(
        &self,
        user: &SolanaAddress,
        amount: u64,
        dchat_recipient: [u8; 32], // dchat address as bytes
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;
        let user_ata = SplToken::get_associated_token_address(user, &self.wdchat_mint)?;
        let token_program = SplToken::program_id()?;

        // Generate transfer ID from user + timestamp
        let transfer_id = generate_transfer_id(user, dchat_recipient);
        let withdrawal_record = self.get_withdrawal_address(&transfer_id)?;
        let system_program = SolanaAddress::from_base58(super::SYSTEM_PROGRAM_ID)?;

        let data = BridgeInstruction::BurnWrapped {
            amount,
            dchat_recipient,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::signer_writable(user.clone()),
            AccountMeta::writable(bridge_state),
            AccountMeta::writable(withdrawal_record),
            AccountMeta::writable(user_ata),
            AccountMeta::writable(self.wdchat_mint.clone()),
            AccountMeta::readonly(self.bridge_authority.clone()),
            AccountMeta::readonly(token_program),
            AccountMeta::readonly(system_program),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Finalize withdrawal after dchat chain confirmation
    pub fn finalize_withdrawal(
        &self,
        transfer_id: [u8; 16],
        dchat_tx_hash: [u8; 32],
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;
        let withdrawal_record = self.get_withdrawal_address(&transfer_id)?;

        let data = BridgeInstruction::FinalizeWithdrawal {
            transfer_id,
            dchat_tx_hash,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::writable(bridge_state),
            AccountMeta::writable(withdrawal_record),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Add a validator to the bridge
    pub fn add_validator(
        &self,
        admin: &SolanaAddress,
        validator: &SolanaAddress,
        weight: u32,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;

        let data = BridgeInstruction::AddValidator {
            validator: *validator.as_bytes(),
            weight,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::signer_readonly(admin.clone()),
            AccountMeta::writable(bridge_state),
            AccountMeta::readonly(validator.clone()),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Remove a validator from the bridge
    pub fn remove_validator(
        &self,
        admin: &SolanaAddress,
        validator: &SolanaAddress,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;

        let data = BridgeInstruction::RemoveValidator {
            validator: *validator.as_bytes(),
        }
        .serialize();

        let accounts = vec![
            AccountMeta::signer_readonly(admin.clone()),
            AccountMeta::writable(bridge_state),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Update bridge configuration
    pub fn update_config(
        &self,
        admin: &SolanaAddress,
        fee_bps: Option<u16>,
        min_transfer: Option<u64>,
        max_transfer: Option<u64>,
        paused: Option<bool>,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;

        let data = BridgeInstruction::UpdateConfig {
            fee_bps,
            min_transfer,
            max_transfer,
            paused,
        }
        .serialize();

        let accounts = vec![
            AccountMeta::signer_readonly(admin.clone()),
            AccountMeta::writable(bridge_state),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }

    /// Emergency pause the bridge
    pub fn pause(&self, admin: &SolanaAddress) -> Result<Instruction> {
        self.update_config(admin, None, None, None, Some(true))
    }

    /// Unpause the bridge
    pub fn unpause(&self, admin: &SolanaAddress) -> Result<Instruction> {
        self.update_config(admin, None, None, None, Some(false))
    }

    /// Collect accumulated fees
    pub fn collect_fees(
        &self,
        admin: &SolanaAddress,
        fee_collector: &SolanaAddress,
    ) -> Result<Instruction> {
        let bridge_state = self.get_bridge_state_address()?;
        let fee_ata = SplToken::get_associated_token_address(fee_collector, &self.wdchat_mint)?;
        let bridge_fee_ata =
            SplToken::get_associated_token_address(&self.bridge_authority, &self.wdchat_mint)?;
        let token_program = SplToken::program_id()?;

        let data = BridgeInstruction::CollectFees.serialize();

        let accounts = vec![
            AccountMeta::signer_readonly(admin.clone()),
            AccountMeta::writable(bridge_state),
            AccountMeta::writable(bridge_fee_ata),
            AccountMeta::writable(fee_ata),
            AccountMeta::readonly(self.bridge_authority.clone()),
            AccountMeta::readonly(token_program),
        ];

        Ok(Instruction::new(self.program_id.clone(), accounts, data))
    }
}

/// Bridge instruction types
#[derive(Debug, Clone)]
pub enum BridgeInstruction {
    /// Initialize the bridge
    Initialize {
        fee_bps: u16,
        min_transfer: u64,
        max_transfer: u64,
        required_confirmations: u32,
    },
    /// Mint wrapped tokens (dchat → Solana)
    MintWrapped {
        transfer_id: [u8; 16],
        amount: u64,
        dchat_tx_hash: [u8; 32],
        signatures: Vec<[u8; 64]>,
    },
    /// Burn wrapped tokens (Solana → dchat)
    BurnWrapped {
        amount: u64,
        dchat_recipient: [u8; 32],
    },
    /// Finalize a withdrawal
    FinalizeWithdrawal {
        transfer_id: [u8; 16],
        dchat_tx_hash: [u8; 32],
    },
    /// Add validator
    AddValidator { validator: [u8; 32], weight: u32 },
    /// Remove validator
    RemoveValidator { validator: [u8; 32] },
    /// Update configuration
    UpdateConfig {
        fee_bps: Option<u16>,
        min_transfer: Option<u64>,
        max_transfer: Option<u64>,
        paused: Option<bool>,
    },
    /// Collect fees
    CollectFees,
}

impl BridgeInstruction {
    /// Serialize instruction to bytes
    pub fn serialize(&self) -> Vec<u8> {
        let mut data = Vec::new();

        match self {
            BridgeInstruction::Initialize {
                fee_bps,
                min_transfer,
                max_transfer,
                required_confirmations,
            } => {
                data.push(0); // Instruction discriminator
                data.extend_from_slice(&fee_bps.to_le_bytes());
                data.extend_from_slice(&min_transfer.to_le_bytes());
                data.extend_from_slice(&max_transfer.to_le_bytes());
                data.extend_from_slice(&required_confirmations.to_le_bytes());
            }
            BridgeInstruction::MintWrapped {
                transfer_id,
                amount,
                dchat_tx_hash,
                signatures,
            } => {
                data.push(1);
                data.extend_from_slice(transfer_id);
                data.extend_from_slice(&amount.to_le_bytes());
                data.extend_from_slice(dchat_tx_hash);
                data.push(signatures.len() as u8);
                for sig in signatures {
                    data.extend_from_slice(sig);
                }
            }
            BridgeInstruction::BurnWrapped {
                amount,
                dchat_recipient,
            } => {
                data.push(2);
                data.extend_from_slice(&amount.to_le_bytes());
                data.extend_from_slice(dchat_recipient);
            }
            BridgeInstruction::FinalizeWithdrawal {
                transfer_id,
                dchat_tx_hash,
            } => {
                data.push(3);
                data.extend_from_slice(transfer_id);
                data.extend_from_slice(dchat_tx_hash);
            }
            BridgeInstruction::AddValidator { validator, weight } => {
                data.push(4);
                data.extend_from_slice(validator);
                data.extend_from_slice(&weight.to_le_bytes());
            }
            BridgeInstruction::RemoveValidator { validator } => {
                data.push(5);
                data.extend_from_slice(validator);
            }
            BridgeInstruction::UpdateConfig {
                fee_bps,
                min_transfer,
                max_transfer,
                paused,
            } => {
                data.push(6);

                // Option encoding
                if let Some(v) = fee_bps {
                    data.push(1);
                    data.extend_from_slice(&v.to_le_bytes());
                } else {
                    data.push(0);
                }

                if let Some(v) = min_transfer {
                    data.push(1);
                    data.extend_from_slice(&v.to_le_bytes());
                } else {
                    data.push(0);
                }

                if let Some(v) = max_transfer {
                    data.push(1);
                    data.extend_from_slice(&v.to_le_bytes());
                } else {
                    data.push(0);
                }

                if let Some(v) = paused {
                    data.push(1);
                    data.push(if *v { 1 } else { 0 });
                } else {
                    data.push(0);
                }
            }
            BridgeInstruction::CollectFees => {
                data.push(7);
            }
        }

        data
    }
}

/// Bridge state account data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeState {
    /// Is initialized
    pub is_initialized: bool,
    /// Admin authority
    pub admin: [u8; 32],
    /// wDCHAT mint
    pub wdchat_mint: [u8; 32],
    /// Fee collector
    pub fee_collector: [u8; 32],
    /// Bridge authority (PDA)
    pub bridge_authority: [u8; 32],
    /// Authority bump seed
    pub authority_bump: u8,
    /// Fee in basis points
    pub fee_bps: u16,
    /// Minimum transfer amount
    pub min_transfer: u64,
    /// Maximum transfer amount
    pub max_transfer: u64,
    /// Required confirmations from validators
    pub required_confirmations: u32,
    /// Is paused
    pub is_paused: bool,
    /// Total deposited (dchat → Solana)
    pub total_deposited: u64,
    /// Total withdrawn (Solana → dchat)
    pub total_withdrawn: u64,
    /// Total fees collected
    pub total_fees: u64,
    /// Number of deposits
    pub deposit_count: u64,
    /// Number of withdrawals
    pub withdrawal_count: u64,
    /// Validators
    pub validators: Vec<ValidatorInfo>,
}

impl BridgeState {
    /// Account size
    pub const LEN: usize = 512; // Base size + space for validators

    /// Parse from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() < 200 {
            return Err(Error::validation("Bridge state data too short"));
        }

        // Parse fixed fields
        let is_initialized = data[0] == 1;
        let admin: [u8; 32] = data[1..33].try_into().unwrap();
        let wdchat_mint: [u8; 32] = data[33..65].try_into().unwrap();
        let fee_collector: [u8; 32] = data[65..97].try_into().unwrap();
        let bridge_authority: [u8; 32] = data[97..129].try_into().unwrap();
        let authority_bump = data[129];
        let fee_bps = u16::from_le_bytes(data[130..132].try_into().unwrap());
        let min_transfer = u64::from_le_bytes(data[132..140].try_into().unwrap());
        let max_transfer = u64::from_le_bytes(data[140..148].try_into().unwrap());
        let required_confirmations = u32::from_le_bytes(data[148..152].try_into().unwrap());
        let is_paused = data[152] == 1;
        let total_deposited = u64::from_le_bytes(data[153..161].try_into().unwrap());
        let total_withdrawn = u64::from_le_bytes(data[161..169].try_into().unwrap());
        let total_fees = u64::from_le_bytes(data[169..177].try_into().unwrap());
        let deposit_count = u64::from_le_bytes(data[177..185].try_into().unwrap());
        let withdrawal_count = u64::from_le_bytes(data[185..193].try_into().unwrap());

        // Parse validators (simplified)
        let validators = Vec::new();

        Ok(Self {
            is_initialized,
            admin,
            wdchat_mint,
            fee_collector,
            bridge_authority,
            authority_bump,
            fee_bps,
            min_transfer,
            max_transfer,
            required_confirmations,
            is_paused,
            total_deposited,
            total_withdrawn,
            total_fees,
            deposit_count,
            withdrawal_count,
            validators,
        })
    }
}

/// Validator info in bridge state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatorInfo {
    pub pubkey: [u8; 32],
    pub weight: u32,
    pub is_active: bool,
}

/// Deposit record account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DepositRecord {
    /// Transfer ID
    pub transfer_id: [u8; 16],
    /// Recipient on Solana
    pub recipient: [u8; 32],
    /// Amount minted
    pub amount: u64,
    /// Fee deducted
    pub fee: u64,
    /// dchat transaction hash
    pub dchat_tx_hash: [u8; 32],
    /// Timestamp
    pub timestamp: i64,
    /// Is finalized
    pub is_finalized: bool,
}

impl DepositRecord {
    pub const LEN: usize = 137;
}

/// Withdrawal record account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WithdrawRecord {
    /// Transfer ID
    pub transfer_id: [u8; 16],
    /// User on Solana
    pub user: [u8; 32],
    /// Recipient on dchat
    pub dchat_recipient: [u8; 32],
    /// Amount burned
    pub amount: u64,
    /// Fee deducted
    pub fee: u64,
    /// dchat transaction hash (set when finalized)
    pub dchat_tx_hash: [u8; 32],
    /// Timestamp
    pub timestamp: i64,
    /// Is finalized
    pub is_finalized: bool,
}

impl WithdrawRecord {
    pub const LEN: usize = 169;
}

/// Lock accounts for dchat → Solana bridge
#[derive(Debug, Clone)]
pub struct LockAccounts {
    pub bridge_state: SolanaAddress,
    pub deposit_record: SolanaAddress,
    pub wdchat_mint: SolanaAddress,
    pub recipient_ata: SolanaAddress,
    pub recipient: SolanaAddress,
    pub bridge_authority: SolanaAddress,
}

/// Unlock accounts for Solana → dchat bridge
#[derive(Debug, Clone)]
pub struct UnlockAccounts {
    pub user: SolanaAddress,
    pub bridge_state: SolanaAddress,
    pub withdrawal_record: SolanaAddress,
    pub user_ata: SolanaAddress,
    pub wdchat_mint: SolanaAddress,
    pub bridge_authority: SolanaAddress,
}

/// Generate a transfer ID
fn generate_transfer_id(user: &SolanaAddress, dchat_recipient: [u8; 32]) -> [u8; 16] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(user.as_bytes());
    hasher.update(&dchat_recipient);
    hasher.update(&chrono::Utc::now().timestamp().to_le_bytes());

    let hash = hasher.finalize();
    let mut id = [0u8; 16];
    id.copy_from_slice(&hash.as_bytes()[..16]);
    id
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bridge_program_creation() {
        let program_id = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        let mint = SolanaAddress::from_bytes(&[2u8; 32]).unwrap();

        let bridge = BridgeProgram::new(program_id, mint);
        assert!(bridge.is_ok());
    }

    #[test]
    fn test_instruction_serialization() {
        let ix = BridgeInstruction::Initialize {
            fee_bps: 30,
            min_transfer: 1_000_000,
            max_transfer: 1_000_000_000,
            required_confirmations: 12,
        };

        let data = ix.serialize();
        assert_eq!(data[0], 0); // Initialize discriminator
    }

    #[test]
    fn test_burn_wrapped_instruction() {
        let ix = BridgeInstruction::BurnWrapped {
            amount: 1_000_000_000,
            dchat_recipient: [3u8; 32],
        };

        let data = ix.serialize();
        assert_eq!(data[0], 2); // BurnWrapped discriminator
    }
}
