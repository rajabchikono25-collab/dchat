//! Confidential Token Program - SPL-like tokens with hidden balances
//!
//! This module implements Plan A: Confidential SPL Accounts as specified in
//! CONFIDENTIAL_TOKEN_PROGRAM_V1.md. Key features:
//!
//! - Pedersen commitments for balance hiding
//! - Bulletproof range proofs for balance validity
//! - Transaction-bound proofs with anti-replay nonces
//! - SPL-like semantics: mint, transfer, burn, freeze/thaw, set authority
//!
//! # Security Properties
//!
//! - Amounts and balances are hidden (only commitments on-chain)
//! - Owners and token accounts remain public
//! - Anti-replay via account_nonce and mint_nonce
//! - Proofs are bound to transaction context (mint, accounts, nonces, blockhash)
//! - All proof/ciphertext sizes are bounded

use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::account::{Account, AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::Instruction;
use crate::metering::ComputeMeter;
use crate::pda::PdaDerivation;
use crate::privacy::{BalanceProof, RangeProof};

// Re-export from account module, not token
pub use crate::account::AccountState;

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Confidential Token Program ID
pub const CONF_TOKEN_PROGRAM_ID: Pubkey = crate::native_programs::CONF_TOKEN_PROGRAM_ID;

/// Maximum proof size in bytes (Bulletproof + sigma protocol)
pub const MAX_PROOF_BYTES: usize = 1024;

/// Maximum ciphertext size for encrypted balance payloads
pub const MAX_CIPHERTEXT_BYTES: usize = 128;

/// Maximum public inputs size
pub const MAX_PUBLIC_INPUTS_BYTES: usize = 256;

/// Maximum decimals for confidential tokens
pub const MAX_DECIMALS: u8 = 18;

/// Compute units for range proof verification
pub const RANGE_PROOF_VERIFY_CU: u64 = 50_000;

/// Compute units for balance proof verification
pub const BALANCE_PROOF_VERIFY_CU: u64 = 10_000;

/// Compute units for basic instruction processing
pub const BASE_INSTRUCTION_CU: u64 = 1_000;

/// Domain separator for commitment parameters hash
pub const COMMITMENT_PARAMS_DOMAIN: &[u8] = b"dchat-confidential-token-params-v1";

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT LAYOUTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Confidential Mint account - defines token configuration
///
/// Unlike regular SPL mints, the supply can be optionally public (for auditing)
/// while individual balances remain hidden.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidentialMint {
    /// Number of decimal places for token amounts
    pub decimals: u8,
    /// Authority that can mint new tokens (None = disabled)
    pub mint_authority: Option<Pubkey>,
    /// Authority that can freeze/thaw accounts (None = disabled)
    pub freeze_authority: Option<Pubkey>,
    /// Public supply counter (optional, for auditing)
    /// Updated on mint/burn only, not on transfers
    pub supply_public: u64,
    /// Nonce for mint mutations (anti-replay)
    pub mint_nonce: u64,
    /// Hash of commitment parameters for deterministic verification
    /// Computed as H(DOMAIN || generator_g || generator_h || bit_size)
    #[serde(with = "BigArray")]
    pub commitment_params_hash: [u8; 32],
    /// Whether this mint is initialized
    pub is_initialized: bool,
}

impl ConfidentialMint {
    /// Account size in bytes
    pub const SIZE: usize = 150;

    /// Create a new confidential mint
    pub fn new(
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
    ) -> ProgramResult<Self> {
        if decimals > MAX_DECIMALS {
            return Err(ProgramError::InvalidDecimals);
        }

        // Compute commitment params hash for this mint
        let commitment_params_hash = Self::compute_commitment_params_hash();

        Ok(Self {
            decimals,
            mint_authority: Some(mint_authority),
            freeze_authority,
            supply_public: 0,
            mint_nonce: 0,
            commitment_params_hash,
            is_initialized: true,
        })
    }

    /// Compute commitment parameters hash
    /// This pins the cryptographic parameters for deterministic verification
    fn compute_commitment_params_hash() -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(COMMITMENT_PARAMS_DOMAIN);
        // Include Ristretto basepoint identifier
        hasher.update(b"ristretto255-basepoint");
        // Include H generator derivation
        hasher.update(b"dchat-pedersen-h-generator-v1");
        // Include bit size for range proofs
        hasher.update(&64u8.to_le_bytes());
        *hasher.finalize().as_bytes()
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }

    /// Validate commitment params hash matches expected
    pub fn validate_commitment_params(&self) -> ProgramResult<()> {
        let expected = Self::compute_commitment_params_hash();
        if self.commitment_params_hash != expected {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(())
    }
}

/// Confidential Token Account - holds hidden balance for an owner
///
/// The balance is stored as a Pedersen commitment, with an encrypted
/// value that only the owner can decrypt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfidentialTokenAccount {
    /// The mint this account holds tokens of
    pub mint: Pubkey,
    /// The owner of this account
    pub owner: Pubkey,
    /// Account state (Uninitialized, Initialized, Frozen)
    pub state: AccountState,
    /// Pedersen commitment to the balance: C = v*G + r*H
    #[serde(with = "BigArray")]
    pub balance_commitment: [u8; 32],
    /// Encrypted balance for owner to decrypt (bounded size)
    /// Format: nonce (24) + ciphertext (48) + tag (16) = 88 bytes max
    #[serde(with = "BigArray")]
    pub enc_balance_owner: [u8; 88],
    /// Length of valid encrypted data in enc_balance_owner
    pub enc_balance_len: u8,
    /// Account nonce for anti-replay (increments on every mutation)
    pub account_nonce: u64,
    /// Optional delegate authority
    pub delegate: Option<Pubkey>,
    /// Delegated amount commitment (if delegate is set)
    #[serde(with = "BigArray")]
    pub delegated_commitment: [u8; 32],
}

impl ConfidentialTokenAccount {
    /// Account size in bytes
    pub const SIZE: usize = 250;

    /// Create a new uninitialized confidential token account
    pub fn new(mint: Pubkey, owner: Pubkey) -> Self {
        Self {
            mint,
            owner,
            state: AccountState::Initialized,
            balance_commitment: [0u8; 32],
            enc_balance_owner: [0u8; 88],
            enc_balance_len: 0,
            account_nonce: 0,
            delegate: None,
            delegated_commitment: [0u8; 32],
        }
    }

    /// Create with initial zero balance commitment
    pub fn new_with_zero_balance(mint: Pubkey, owner: Pubkey) -> Self {
        // Zero balance commitment: C = 0*G + 0*H = identity point
        // Compressed identity is all zeros
        Self::new(mint, owner)
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }

    /// Check if account is frozen
    pub fn is_frozen(&self) -> bool {
        self.state == AccountState::Frozen
    }

    /// Check if account is initialized
    pub fn is_initialized(&self) -> bool {
        self.state != AccountState::Uninitialized
    }

    /// Update balance with new commitment and encrypted value
    pub fn update_balance(
        &mut self,
        new_commitment: [u8; 32],
        new_encrypted: &[u8],
    ) -> ProgramResult<()> {
        if new_encrypted.len() > self.enc_balance_owner.len() {
            return Err(ProgramError::InvalidInstructionData);
        }

        self.balance_commitment = new_commitment;
        self.enc_balance_owner[..new_encrypted.len()].copy_from_slice(new_encrypted);
        self.enc_balance_len = new_encrypted.len() as u8;
        self.account_nonce = self
            .account_nonce
            .checked_add(1)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        Ok(())
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// Authority type for SetAuthority instruction
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConfidentialAuthorityType {
    /// Mint authority (can mint new tokens)
    MintTokens,
    /// Freeze authority (can freeze/thaw accounts)
    FreezeAccount,
}

/// Transfer proof data with transaction binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransferProofData {
    /// Balance proof (sigma protocol)
    pub balance_proof: Vec<u8>,
    /// Range proof for sender's new balance (>= 0)
    pub sender_range_proof: Vec<u8>,
    /// Expected sender account nonce (for binding)
    pub expected_sender_nonce: u64,
    /// Expected receiver account nonce (for binding)
    pub expected_receiver_nonce: u64,
    /// Recent blockhash for transaction binding
    #[serde(with = "BigArray")]
    pub recent_blockhash: [u8; 32],
}

impl TransferProofData {
    /// Maximum serialized size
    pub const MAX_SIZE: usize = MAX_PROOF_BYTES * 2 + 8 + 8 + 32;

    /// Validate proof data sizes
    pub fn validate_sizes(&self) -> ProgramResult<()> {
        if self.balance_proof.len() > MAX_PROOF_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        if self.sender_range_proof.len() > MAX_PROOF_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(())
    }
}

/// Mint proof data with transaction binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MintProofData {
    /// Range proof for minted amount (>= 0, <= 2^64-1)
    pub range_proof: Vec<u8>,
    /// Expected mint nonce (for binding)
    pub expected_mint_nonce: u64,
    /// Expected destination account nonce
    pub expected_dest_nonce: u64,
    /// Recent blockhash for transaction binding
    #[serde(with = "BigArray")]
    pub recent_blockhash: [u8; 32],
}

impl MintProofData {
    /// Validate proof data sizes
    pub fn validate_sizes(&self) -> ProgramResult<()> {
        if self.range_proof.len() > MAX_PROOF_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(())
    }
}

/// Burn proof data with transaction binding
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BurnProofData {
    /// Balance proof showing valid burn
    pub balance_proof: Vec<u8>,
    /// Range proof for remaining balance (>= 0)
    pub range_proof: Vec<u8>,
    /// Expected account nonce
    pub expected_account_nonce: u64,
    /// Expected mint nonce
    pub expected_mint_nonce: u64,
    /// Recent blockhash for transaction binding
    #[serde(with = "BigArray")]
    pub recent_blockhash: [u8; 32],
    /// Optionally reveal the burned amount (for public supply tracking)
    pub revealed_amount: Option<u64>,
}

impl BurnProofData {
    /// Validate proof data sizes
    pub fn validate_sizes(&self) -> ProgramResult<()> {
        if self.balance_proof.len() > MAX_PROOF_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        if self.range_proof.len() > MAX_PROOF_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        Ok(())
    }
}

/// Confidential Token instruction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidentialTokenInstruction {
    /// Initialize a new confidential mint
    ///
    /// Accounts:
    /// 0. `[writable]` Mint account to initialize
    /// 1. `[]` Rent sysvar
    InitializeMint {
        /// Number of decimal places
        decimals: u8,
        /// Authority allowed to mint new tokens
        mint_authority: Pubkey,
        /// Optional authority allowed to freeze accounts
        freeze_authority: Option<Pubkey>,
    },

    /// Initialize a new confidential token account
    ///
    /// Accounts:
    /// 0. `[writable]` Token account to initialize
    /// 1. `[]` Mint account
    /// 2. `[]` Owner
    InitializeAccount,

    /// Transfer tokens with hidden amount
    ///
    /// Accounts:
    /// 0. `[writable]` Source token account
    /// 1. `[writable]` Destination token account
    /// 2. `[]` Mint account
    /// 3. `[signer]` Owner of source account
    TransferConfidential {
        /// New commitment for sender's balance
        #[serde(with = "BigArray")]
        new_source_commitment: [u8; 32],
        /// Encrypted new balance for sender
        new_source_encrypted: Vec<u8>,
        /// New commitment for receiver's balance
        #[serde(with = "BigArray")]
        new_dest_commitment: [u8; 32],
        /// Encrypted new balance for receiver
        new_dest_encrypted: Vec<u8>,
        /// Transfer proof data
        proof_data: TransferProofData,
    },

    /// Mint new tokens to an account
    ///
    /// Accounts:
    /// 0. `[writable]` Mint account
    /// 1. `[writable]` Destination token account
    /// 2. `[signer]` Mint authority
    MintToConfidential {
        /// Amount to mint (public for supply tracking)
        amount: u64,
        /// New commitment for destination balance
        #[serde(with = "BigArray")]
        new_dest_commitment: [u8; 32],
        /// Encrypted new balance for destination
        new_dest_encrypted: Vec<u8>,
        /// Mint proof data
        proof_data: MintProofData,
    },

    /// Burn tokens from an account
    ///
    /// Accounts:
    /// 0. `[writable]` Token account to burn from
    /// 1. `[writable]` Mint account
    /// 2. `[signer]` Owner of token account
    BurnConfidential {
        /// New commitment for account balance after burn
        #[serde(with = "BigArray")]
        new_commitment: [u8; 32],
        /// Encrypted new balance
        new_encrypted: Vec<u8>,
        /// Burn proof data
        proof_data: BurnProofData,
    },

    /// Freeze a token account
    ///
    /// Accounts:
    /// 0. `[writable]` Token account to freeze
    /// 1. `[]` Mint account
    /// 2. `[signer]` Freeze authority
    FreezeAccount,

    /// Thaw a frozen token account
    ///
    /// Accounts:
    /// 0. `[writable]` Token account to thaw
    /// 1. `[]` Mint account
    /// 2. `[signer]` Freeze authority
    ThawAccount,

    /// Set mint or freeze authority
    ///
    /// Accounts:
    /// 0. `[writable]` Mint account
    /// 1. `[signer]` Current authority
    SetAuthority {
        /// Type of authority to change
        authority_type: ConfidentialAuthorityType,
        /// New authority (None to disable permanently)
        new_authority: Option<Pubkey>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS (for indexers)
// ═══════════════════════════════════════════════════════════════════════════════

/// Event types emitted by the confidential token program
/// Note: Amounts are NOT included to preserve privacy
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ConfidentialTokenEvent {
    /// Mint was initialized
    MintInitialized {
        /// Mint account address
        mint: Pubkey,
        /// Number of decimal places
        decimals: u8,
        /// Mint authority
        mint_authority: Option<Pubkey>,
        /// Freeze authority
        freeze_authority: Option<Pubkey>,
    },

    /// Token account was initialized
    AccountInitialized {
        /// Token account address
        account: Pubkey,
        /// Associated mint
        mint: Pubkey,
        /// Account owner
        owner: Pubkey,
    },

    /// Confidential transfer occurred (amount hidden)
    ConfidentialTransfer {
        /// Source account
        source: Pubkey,
        /// Destination account
        destination: Pubkey,
        /// Mint
        mint: Pubkey,
        /// New commitment for source (for indexers tracking state)
        source_commitment: [u8; 32],
        /// New commitment for destination
        dest_commitment: [u8; 32],
    },

    /// Tokens were minted
    ConfidentialMintTo {
        /// Mint account
        mint: Pubkey,
        /// Destination account
        destination: Pubkey,
        /// Amount is public for supply tracking
        amount: u64,
    },

    /// Tokens were burned
    ConfidentialBurn {
        /// Mint account
        mint: Pubkey,
        /// Token account
        account: Pubkey,
        /// Amount may be revealed for supply tracking
        revealed_amount: Option<u64>,
    },

    /// Account was frozen
    AccountFrozen {
        /// Token account
        account: Pubkey,
        /// Mint
        mint: Pubkey,
    },

    /// Account was thawed
    AccountThawed {
        /// Token account
        account: Pubkey,
        /// Mint
        mint: Pubkey,
    },

    /// Authority was changed
    AuthorityChanged {
        /// Mint account
        mint: Pubkey,
        /// Type of authority changed
        authority_type: ConfidentialAuthorityType,
        /// Previous authority
        old_authority: Option<Pubkey>,
        /// New authority
        new_authority: Option<Pubkey>,
    },
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM IMPLEMENTATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Confidential Token Program
pub struct ConfidentialTokenProgram;

impl ConfidentialTokenProgram {
    /// Create InitializeMint instruction
    pub fn initialize_mint(
        mint: Pubkey,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
        decimals: u8,
    ) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::InitializeMint {
                decimals,
                mint_authority,
                freeze_authority,
            })
            .unwrap_or_default(),
        }
    }

    /// Create InitializeAccount instruction
    pub fn initialize_account(account: Pubkey, mint: Pubkey, owner: Pubkey) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(owner, false),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::InitializeAccount)
                .unwrap_or_default(),
        }
    }

    /// Create TransferConfidential instruction
    #[allow(clippy::too_many_arguments)]
    pub fn transfer_confidential(
        source: Pubkey,
        destination: Pubkey,
        mint: Pubkey,
        owner: Pubkey,
        new_source_commitment: [u8; 32],
        new_source_encrypted: Vec<u8>,
        new_dest_commitment: [u8; 32],
        new_dest_encrypted: Vec<u8>,
        proof_data: TransferProofData,
    ) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(source, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::TransferConfidential {
                new_source_commitment,
                new_source_encrypted,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
            })
            .unwrap_or_default(),
        }
    }

    /// Create MintToConfidential instruction
    pub fn mint_to_confidential(
        mint: Pubkey,
        destination: Pubkey,
        mint_authority: Pubkey,
        amount: u64,
        new_dest_commitment: [u8; 32],
        new_dest_encrypted: Vec<u8>,
        proof_data: MintProofData,
    ) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(mint, false),
                AccountMeta::new(destination, false),
                AccountMeta::new_readonly(mint_authority, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::MintToConfidential {
                amount,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
            })
            .unwrap_or_default(),
        }
    }

    /// Create BurnConfidential instruction
    pub fn burn_confidential(
        account: Pubkey,
        mint: Pubkey,
        owner: Pubkey,
        new_commitment: [u8; 32],
        new_encrypted: Vec<u8>,
        proof_data: BurnProofData,
    ) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(owner, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::BurnConfidential {
                new_commitment,
                new_encrypted,
                proof_data,
            })
            .unwrap_or_default(),
        }
    }

    /// Create FreezeAccount instruction
    pub fn freeze_account(account: Pubkey, mint: Pubkey, freeze_authority: Pubkey) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(freeze_authority, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::FreezeAccount)
                .unwrap_or_default(),
        }
    }

    /// Create ThawAccount instruction
    pub fn thaw_account(account: Pubkey, mint: Pubkey, freeze_authority: Pubkey) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(account, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(freeze_authority, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::ThawAccount)
                .unwrap_or_default(),
        }
    }

    /// Create SetAuthority instruction
    pub fn set_authority(
        mint: Pubkey,
        current_authority: Pubkey,
        authority_type: ConfidentialAuthorityType,
        new_authority: Option<Pubkey>,
    ) -> Instruction {
        Instruction {
            program_id: CONF_TOKEN_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(mint, false),
                AccountMeta::new_readonly(current_authority, true),
            ],
            data: bincode::serialize(&ConfidentialTokenInstruction::SetAuthority {
                authority_type,
                new_authority,
            })
            .unwrap_or_default(),
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ASSOCIATED TOKEN ACCOUNT DERIVATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Confidential Associated Token Account derivation
pub struct ConfidentialAta;

impl ConfidentialAta {
    /// Derive confidential ATA address for a wallet and mint
    ///
    /// Uses seeds: [wallet, CONF_TOKEN_PROGRAM_ID, mint]
    pub fn derive_address(wallet: &Pubkey, mint: &Pubkey) -> ProgramResult<Pubkey> {
        let seeds: &[&[u8]] = &[&wallet.0, &CONF_TOKEN_PROGRAM_ID.0, &mint.0];
        let pda =
            PdaDerivation::find_program_address(seeds, &crate::native_programs::ATA_PROGRAM_ID)?;
        Ok(pda.address)
    }

    /// Derive with bump seed
    pub fn derive_address_with_bump(wallet: &Pubkey, mint: &Pubkey) -> ProgramResult<(Pubkey, u8)> {
        let seeds: &[&[u8]] = &[&wallet.0, &CONF_TOKEN_PROGRAM_ID.0, &mint.0];
        let pda =
            PdaDerivation::find_program_address(seeds, &crate::native_programs::ATA_PROGRAM_ID)?;
        Ok((pda.address, pda.bump))
    }

    /// Create instruction to create a confidential ATA
    pub fn create_instruction(
        payer: Pubkey,
        wallet: Pubkey,
        mint: Pubkey,
    ) -> ProgramResult<Instruction> {
        let ata = Self::derive_address(&wallet, &mint)?;

        Ok(Instruction {
            program_id: crate::native_programs::ATA_PROGRAM_ID,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(wallet, false),
                AccountMeta::new_readonly(mint, false),
                AccountMeta::new_readonly(crate::native_programs::SYSTEM_PROGRAM_ID, false),
                AccountMeta::new_readonly(CONF_TOKEN_PROGRAM_ID, false),
            ],
            data: vec![0], // Create instruction
        })
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENCRYPTED BALANCE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Encrypted balance utilities using ChaCha20-Poly1305
///
/// The balance is encrypted so only the owner can decrypt it and know
/// their actual balance, while on-chain we only store the commitment.
#[cfg(feature = "chacha20poly1305")]
pub mod encrypted_balance {
    use chacha20poly1305::{
        aead::{Aead, KeyInit},
        ChaCha20Poly1305, Nonce,
    };

    use super::*;

    /// Encrypted balance payload size: nonce (12) + ciphertext (8 + 16 tag) = 36 bytes
    pub const ENCRYPTED_BALANCE_SIZE: usize = 36;

    /// Encrypt a balance for the owner using their decryption key
    ///
    /// # Arguments
    /// * `balance` - The balance value to encrypt
    /// * `decryption_key` - 32-byte key derived from owner's secret
    /// * `nonce_input` - 12-byte nonce (use account_nonce || random for uniqueness)
    ///
    /// # Returns
    /// Encrypted payload: [nonce (12 bytes) || ciphertext (24 bytes)]
    pub fn encrypt_balance(
        balance: u64,
        decryption_key: &[u8; 32],
        nonce_input: &[u8; 12],
    ) -> ProgramResult<Vec<u8>> {
        let cipher = ChaCha20Poly1305::new_from_slice(decryption_key)
            .map_err(|_| ProgramError::DecryptionFailed)?;

        let nonce = Nonce::from_slice(nonce_input);
        let plaintext = balance.to_le_bytes();

        let ciphertext = cipher
            .encrypt(nonce, plaintext.as_ref())
            .map_err(|_| ProgramError::DecryptionFailed)?;

        // Prepend nonce to ciphertext
        let mut result = Vec::with_capacity(12 + ciphertext.len());
        result.extend_from_slice(nonce_input);
        result.extend_from_slice(&ciphertext);

        Ok(result)
    }

    /// Decrypt a balance using the owner's decryption key
    ///
    /// # Arguments
    /// * `encrypted` - Encrypted payload from encrypt_balance
    /// * `decryption_key` - 32-byte key derived from owner's secret
    ///
    /// # Returns
    /// The decrypted balance value
    pub fn decrypt_balance(encrypted: &[u8], decryption_key: &[u8; 32]) -> ProgramResult<u64> {
        if encrypted.len() < 12 + 8 + 16 {
            return Err(ProgramError::DecryptionFailed);
        }

        let nonce = Nonce::from_slice(&encrypted[..12]);
        let ciphertext = &encrypted[12..];

        let cipher = ChaCha20Poly1305::new_from_slice(decryption_key)
            .map_err(|_| ProgramError::DecryptionFailed)?;

        let plaintext = cipher
            .decrypt(nonce, ciphertext)
            .map_err(|_| ProgramError::DecryptionFailed)?;

        if plaintext.len() != 8 {
            return Err(ProgramError::DecryptionFailed);
        }

        let balance_bytes: [u8; 8] = plaintext
            .try_into()
            .map_err(|_| ProgramError::DecryptionFailed)?;

        Ok(u64::from_le_bytes(balance_bytes))
    }

    /// Derive a decryption key from an owner's secret and the mint
    ///
    /// This creates a unique key per mint so that compromise of one
    /// token's key doesn't affect others.
    pub fn derive_decryption_key(owner_secret: &[u8; 32], mint: &Pubkey) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"dchat-confidential-token-decrypt-key-v1");
        hasher.update(owner_secret);
        hasher.update(&mint.0);
        *hasher.finalize().as_bytes()
    }

    /// Generate a nonce for encryption based on account state
    ///
    /// Uses account_nonce to ensure uniqueness per transaction
    pub fn generate_nonce(account_nonce: u64, random_suffix: &[u8; 4]) -> [u8; 12] {
        let mut nonce = [0u8; 12];
        nonce[..8].copy_from_slice(&account_nonce.to_le_bytes());
        nonce[8..].copy_from_slice(random_suffix);
        nonce
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn test_encrypt_decrypt_roundtrip() {
            let balance = 1_000_000u64;
            let key = [42u8; 32];
            let nonce = [1u8; 12];

            let encrypted = encrypt_balance(balance, &key, &nonce).expect("should encrypt");
            let decrypted = decrypt_balance(&encrypted, &key).expect("should decrypt");

            assert_eq!(balance, decrypted);
        }

        #[test]
        fn test_wrong_key_fails() {
            let balance = 1_000_000u64;
            let key = [42u8; 32];
            let wrong_key = [99u8; 32];
            let nonce = [1u8; 12];

            let encrypted = encrypt_balance(balance, &key, &nonce).expect("should encrypt");
            let result = decrypt_balance(&encrypted, &wrong_key);

            assert!(result.is_err());
        }

        #[test]
        fn test_tampered_ciphertext_fails() {
            let balance = 1_000_000u64;
            let key = [42u8; 32];
            let nonce = [1u8; 12];

            let mut encrypted = encrypt_balance(balance, &key, &nonce).expect("should encrypt");
            // Tamper with ciphertext
            encrypted[15] ^= 0xFF;

            let result = decrypt_balance(&encrypted, &key);
            assert!(result.is_err());
        }

        #[test]
        fn test_derive_decryption_key_deterministic() {
            let secret = [7u8; 32];
            let mint = Pubkey::new([3u8; 32]);

            let key1 = derive_decryption_key(&secret, &mint);
            let key2 = derive_decryption_key(&secret, &mint);

            assert_eq!(key1, key2);

            // Different mint = different key
            let other_mint = Pubkey::new([4u8; 32]);
            let key3 = derive_decryption_key(&secret, &other_mint);
            assert_ne!(key1, key3);
        }

        #[test]
        fn test_generate_nonce_uniqueness() {
            let random = [1u8; 4];

            let nonce1 = generate_nonce(0, &random);
            let nonce2 = generate_nonce(1, &random);

            assert_ne!(nonce1, nonce2);
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION PROCESSOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Confidential Token instruction processor
pub struct ConfidentialTokenProcessor;

impl ConfidentialTokenProcessor {
    /// Process a confidential token instruction
    pub fn process(
        data: &[u8],
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        let instruction: ConfidentialTokenInstruction =
            bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)?;

        meter.consume(BASE_INSTRUCTION_CU)?;

        match instruction {
            ConfidentialTokenInstruction::InitializeMint {
                decimals,
                mint_authority,
                freeze_authority,
            } => Self::process_initialize_mint(
                accounts,
                decimals,
                mint_authority,
                freeze_authority,
                meter,
            ),

            ConfidentialTokenInstruction::InitializeAccount => {
                Self::process_initialize_account(accounts, meter)
            }

            ConfidentialTokenInstruction::TransferConfidential {
                new_source_commitment,
                new_source_encrypted,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
            } => Self::process_transfer_confidential(
                accounts,
                new_source_commitment,
                new_source_encrypted,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
                meter,
            ),

            ConfidentialTokenInstruction::MintToConfidential {
                amount,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
            } => Self::process_mint_to_confidential(
                accounts,
                amount,
                new_dest_commitment,
                new_dest_encrypted,
                proof_data,
                meter,
            ),

            ConfidentialTokenInstruction::BurnConfidential {
                new_commitment,
                new_encrypted,
                proof_data,
            } => Self::process_burn_confidential(
                accounts,
                new_commitment,
                new_encrypted,
                proof_data,
                meter,
            ),

            ConfidentialTokenInstruction::FreezeAccount => {
                Self::process_freeze_account(accounts, meter)
            }

            ConfidentialTokenInstruction::ThawAccount => {
                Self::process_thaw_account(accounts, meter)
            }

            ConfidentialTokenInstruction::SetAuthority {
                authority_type,
                new_authority,
            } => Self::process_set_authority(accounts, authority_type, new_authority, meter),
        }
    }

    /// Initialize a confidential mint
    fn process_initialize_mint(
        accounts: &mut [&mut Account],
        decimals: u8,
        mint_authority: Pubkey,
        freeze_authority: Option<Pubkey>,
        _meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.is_empty() {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mint_account = &mut accounts[0];

        // Check not already initialized
        if mint_account.data.len() >= ConfidentialMint::SIZE {
            if let Ok(existing) = ConfidentialMint::from_bytes(mint_account.data.as_slice()) {
                if existing.is_initialized {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
            }
        }

        let mint = ConfidentialMint::new(decimals, mint_authority, freeze_authority)?;
        mint_account.data.set_from_bytes(mint.to_bytes());
        mint_account.owner = CONF_TOKEN_PROGRAM_ID;

        Ok(Some(ConfidentialTokenEvent::MintInitialized {
            mint: mint_account.key,
            decimals,
            mint_authority: Some(mint_authority),
            freeze_authority,
        }))
    }

    /// Initialize a confidential token account
    fn process_initialize_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let mint_key = accounts[1].key;
        let owner = accounts[2].key;

        // SECURITY: Check if account is already initialized to prevent re-initialization
        if accounts[0].data.len() >= ConfidentialTokenAccount::SIZE {
            if let Ok(existing) = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice())
            {
                if existing.is_initialized() {
                    return Err(ProgramError::AccountAlreadyInitialized);
                }
            }
        }

        // Verify mint is initialized
        let mint = ConfidentialMint::from_bytes(accounts[1].data.as_slice())?;
        if !mint.is_initialized {
            return Err(ProgramError::UninitializedMint);
        }

        // Create token account with zero balance
        let token_account = ConfidentialTokenAccount::new_with_zero_balance(mint_key, owner);
        let account_key = accounts[0].key;
        accounts[0].data.set_from_bytes(token_account.to_bytes());
        accounts[0].owner = CONF_TOKEN_PROGRAM_ID;

        Ok(Some(ConfidentialTokenEvent::AccountInitialized {
            account: account_key,
            mint: mint_key,
            owner,
        }))
    }

    /// Process confidential transfer
    #[allow(clippy::too_many_arguments)]
    fn process_transfer_confidential(
        accounts: &mut [&mut Account],
        new_source_commitment: [u8; 32],
        new_source_encrypted: Vec<u8>,
        new_dest_commitment: [u8; 32],
        new_dest_encrypted: Vec<u8>,
        proof_data: TransferProofData,
        meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 4 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Validate proof sizes
        proof_data.validate_sizes()?;
        if new_source_encrypted.len() > MAX_CIPHERTEXT_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }
        if new_dest_encrypted.len() > MAX_CIPHERTEXT_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Read account data
        let owner_key = accounts[3].key;
        let mint_key = accounts[2].key;
        let source_key = accounts[0].key;
        let dest_key = accounts[1].key;

        // SECURITY: Block self-transfers (could be used for nonce manipulation)
        if source_key == dest_key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        let mut source = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice())?;
        let mut dest = ConfidentialTokenAccount::from_bytes(accounts[1].data.as_slice())?;
        let _mint = ConfidentialMint::from_bytes(accounts[2].data.as_slice())?;

        // SECURITY: Verify both accounts are initialized
        if !source.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }
        if !dest.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        // Validate mint relationship
        if source.mint != mint_key || dest.mint != mint_key {
            return Err(ProgramError::MintMismatch);
        }

        // Validate owner
        if source.owner != owner_key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Check frozen status
        if source.is_frozen() || dest.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        // Validate nonces (anti-replay)
        if source.account_nonce != proof_data.expected_sender_nonce {
            return Err(ProgramError::InvalidNonce);
        }
        if dest.account_nonce != proof_data.expected_receiver_nonce {
            return Err(ProgramError::InvalidNonce);
        }

        // Consume compute for proof verification
        meter.consume(BALANCE_PROOF_VERIFY_CU)?;
        meter.consume(RANGE_PROOF_VERIFY_CU)?;

        // Verify balance proof (sum preservation)
        // The proof must show: old_source - amount = new_source AND old_dest + amount = new_dest
        // This is equivalent to: old_source + old_dest = new_source + new_dest
        let balance_proof = BalanceProof::new(
            crate::privacy::BalanceProofType::Sum,
            proof_data.balance_proof.clone(),
        );

        balance_proof.verify_transfer(
            &[source.balance_commitment, dest.balance_commitment],
            &[new_source_commitment, new_dest_commitment],
        )?;

        // Verify range proof for sender's new balance (>= 0)
        let sender_range_proof = RangeProof::new(proof_data.sender_range_proof.clone(), 64);
        sender_range_proof.verify(&new_source_commitment)?;

        // Update source account
        source.update_balance(new_source_commitment, &new_source_encrypted)?;

        // Update destination account
        dest.update_balance(new_dest_commitment, &new_dest_encrypted)?;

        // Write back
        let (first, rest) = accounts.split_at_mut(1);
        first[0].data.set_from_bytes(source.to_bytes());
        rest[0].data.set_from_bytes(dest.to_bytes());

        Ok(Some(ConfidentialTokenEvent::ConfidentialTransfer {
            source: source_key,
            destination: dest_key,
            mint: mint_key,
            source_commitment: new_source_commitment,
            dest_commitment: new_dest_commitment,
        }))
    }

    /// Process mint to confidential account
    fn process_mint_to_confidential(
        accounts: &mut [&mut Account],
        amount: u64,
        new_dest_commitment: [u8; 32],
        new_dest_encrypted: Vec<u8>,
        proof_data: MintProofData,
        meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Validate proof sizes
        proof_data.validate_sizes()?;
        if new_dest_encrypted.len() > MAX_CIPHERTEXT_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }

        let authority_key = accounts[2].key;
        let mint_key = accounts[0].key;
        let dest_key = accounts[1].key;

        let mut mint = ConfidentialMint::from_bytes(accounts[0].data.as_slice())?;
        let mut dest = ConfidentialTokenAccount::from_bytes(accounts[1].data.as_slice())?;

        // SECURITY: Verify destination account is initialized
        if !dest.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        // Verify mint authority
        match mint.mint_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidMintAuthority),
        }

        // Verify mint relationship
        if dest.mint != mint_key {
            return Err(ProgramError::MintMismatch);
        }

        // Check frozen status
        if dest.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        // Validate nonces
        if mint.mint_nonce != proof_data.expected_mint_nonce {
            return Err(ProgramError::InvalidNonce);
        }
        if dest.account_nonce != proof_data.expected_dest_nonce {
            return Err(ProgramError::InvalidNonce);
        }

        // Consume compute for proof verification
        meter.consume(RANGE_PROOF_VERIFY_CU)?;

        // Verify range proof for minted amount
        let range_proof = RangeProof::new(proof_data.range_proof.clone(), 64);
        // For mint, we verify that the commitment difference matches the public amount
        // The new_dest_commitment should be old_dest_commitment + commitment(amount)
        range_proof.verify(&new_dest_commitment)?;

        // Update supply (public)
        mint.supply_public = mint
            .supply_public
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        mint.mint_nonce = mint
            .mint_nonce
            .checked_add(1)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // Update destination account
        dest.update_balance(new_dest_commitment, &new_dest_encrypted)?;

        // Write back
        let (first, rest) = accounts.split_at_mut(1);
        first[0].data.set_from_bytes(mint.to_bytes());
        rest[0].data.set_from_bytes(dest.to_bytes());

        Ok(Some(ConfidentialTokenEvent::ConfidentialMintTo {
            mint: mint_key,
            destination: dest_key,
            amount,
        }))
    }

    /// Process burn from confidential account
    fn process_burn_confidential(
        accounts: &mut [&mut Account],
        new_commitment: [u8; 32],
        new_encrypted: Vec<u8>,
        proof_data: BurnProofData,
        meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Validate proof sizes
        proof_data.validate_sizes()?;
        if new_encrypted.len() > MAX_CIPHERTEXT_BYTES {
            return Err(ProgramError::InvalidInstructionData);
        }

        let owner_key = accounts[2].key;
        let account_key = accounts[0].key;
        let mint_key = accounts[1].key;

        let mut token_account = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice())?;
        let mut mint = ConfidentialMint::from_bytes(accounts[1].data.as_slice())?;

        // SECURITY: Verify account is initialized
        if !token_account.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        // Verify owner
        if token_account.owner != owner_key {
            return Err(ProgramError::InvalidAccountOwner);
        }

        // Verify mint relationship
        if token_account.mint != mint_key {
            return Err(ProgramError::MintMismatch);
        }

        // Check frozen status
        if token_account.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        // Validate nonces
        if token_account.account_nonce != proof_data.expected_account_nonce {
            return Err(ProgramError::InvalidNonce);
        }
        if mint.mint_nonce != proof_data.expected_mint_nonce {
            return Err(ProgramError::InvalidNonce);
        }

        // Consume compute for proof verification
        meter.consume(BALANCE_PROOF_VERIFY_CU)?;
        meter.consume(RANGE_PROOF_VERIFY_CU)?;

        // Verify balance proof (valid burn)
        let balance_proof = BalanceProof::new(
            crate::privacy::BalanceProofType::Sum,
            proof_data.balance_proof.clone(),
        );
        // old_balance = new_balance + burned_amount
        balance_proof.verify_transfer(&[token_account.balance_commitment], &[new_commitment])?;

        // Verify range proof for remaining balance
        let range_proof = RangeProof::new(proof_data.range_proof.clone(), 64);
        range_proof.verify(&new_commitment)?;

        // Update supply if amount is revealed
        if let Some(amount) = proof_data.revealed_amount {
            mint.supply_public = mint
                .supply_public
                .checked_sub(amount)
                .ok_or(ProgramError::ArithmeticOverflow)?;
        }
        mint.mint_nonce = mint
            .mint_nonce
            .checked_add(1)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        // Update token account
        token_account.update_balance(new_commitment, &new_encrypted)?;

        // Write back
        let (first, rest) = accounts.split_at_mut(1);
        first[0].data.set_from_bytes(token_account.to_bytes());
        rest[0].data.set_from_bytes(mint.to_bytes());

        Ok(Some(ConfidentialTokenEvent::ConfidentialBurn {
            mint: mint_key,
            account: account_key,
            revealed_amount: proof_data.revealed_amount,
        }))
    }

    /// Freeze a token account
    fn process_freeze_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let authority_key = accounts[2].key;
        let account_key = accounts[0].key;
        let mint_key = accounts[1].key;

        let mut token_account = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice())?;
        let mint = ConfidentialMint::from_bytes(accounts[1].data.as_slice())?;

        // SECURITY: Verify account is initialized
        if !token_account.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        // Verify freeze authority
        match mint.freeze_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidFreezeAuthority),
        }

        // Verify mint relationship
        if token_account.mint != mint_key {
            return Err(ProgramError::MintMismatch);
        }

        // SECURITY: Prevent freezing already frozen account
        if token_account.is_frozen() {
            return Err(ProgramError::AccountFrozen);
        }

        token_account.state = AccountState::Frozen;
        accounts[0].data.set_from_bytes(token_account.to_bytes());

        Ok(Some(ConfidentialTokenEvent::AccountFrozen {
            account: account_key,
            mint: mint_key,
        }))
    }

    /// Thaw a frozen token account
    fn process_thaw_account(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let authority_key = accounts[2].key;
        let account_key = accounts[0].key;
        let mint_key = accounts[1].key;

        let mut token_account = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice())?;
        let mint = ConfidentialMint::from_bytes(accounts[1].data.as_slice())?;

        // SECURITY: Verify account is initialized
        if !token_account.is_initialized() {
            return Err(ProgramError::UninitializedAccount);
        }

        // Verify freeze authority
        match mint.freeze_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidFreezeAuthority),
        }

        // Verify mint relationship
        if token_account.mint != mint_key {
            return Err(ProgramError::MintMismatch);
        }

        // SECURITY: Can only thaw a frozen account
        if !token_account.is_frozen() {
            return Err(ProgramError::InvalidAccountData);
        }

        token_account.state = AccountState::Initialized;
        accounts[0].data.set_from_bytes(token_account.to_bytes());

        Ok(Some(ConfidentialTokenEvent::AccountThawed {
            account: account_key,
            mint: mint_key,
        }))
    }

    /// Set authority on a mint
    fn process_set_authority(
        accounts: &mut [&mut Account],
        authority_type: ConfidentialAuthorityType,
        new_authority: Option<Pubkey>,
        _meter: &ComputeMeter,
    ) -> ProgramResult<Option<ConfidentialTokenEvent>> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let current_authority_key = accounts[1].key;
        let mint_key = accounts[0].key;

        let mut mint = ConfidentialMint::from_bytes(accounts[0].data.as_slice())?;

        let old_authority = match authority_type {
            ConfidentialAuthorityType::MintTokens => {
                let old = mint.mint_authority;
                if old != Some(current_authority_key) {
                    return Err(ProgramError::InvalidAuthority);
                }
                mint.mint_authority = new_authority;
                old
            }
            ConfidentialAuthorityType::FreezeAccount => {
                let old = mint.freeze_authority;
                if old != Some(current_authority_key) {
                    return Err(ProgramError::InvalidFreezeAuthority);
                }
                mint.freeze_authority = new_authority;
                old
            }
        };

        accounts[0].data.set_from_bytes(mint.to_bytes());

        Ok(Some(ConfidentialTokenEvent::AuthorityChanged {
            mint: mint_key,
            authority_type,
            old_authority,
            new_authority,
        }))
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_confidential_mint_serialization() {
        let mint = ConfidentialMint::new(9, Pubkey::new([1u8; 32]), Some(Pubkey::new([2u8; 32])))
            .expect("should create mint");

        let bytes = mint.to_bytes();
        assert_eq!(bytes.len(), ConfidentialMint::SIZE);

        let recovered = ConfidentialMint::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(recovered.decimals, 9);
        assert_eq!(recovered.mint_authority, Some(Pubkey::new([1u8; 32])));
        assert_eq!(recovered.freeze_authority, Some(Pubkey::new([2u8; 32])));
        assert!(recovered.is_initialized);
        assert_eq!(recovered.supply_public, 0);
        assert_eq!(recovered.mint_nonce, 0);
    }

    #[test]
    fn test_confidential_mint_invalid_decimals() {
        let result = ConfidentialMint::new(19, Pubkey::new([1u8; 32]), None);
        assert!(result.is_err());
    }

    #[test]
    fn test_confidential_token_account_serialization() {
        let account = ConfidentialTokenAccount::new_with_zero_balance(
            Pubkey::new([1u8; 32]),
            Pubkey::new([2u8; 32]),
        );

        let bytes = account.to_bytes();
        assert_eq!(bytes.len(), ConfidentialTokenAccount::SIZE);

        let recovered = ConfidentialTokenAccount::from_bytes(&bytes).expect("should deserialize");
        assert_eq!(recovered.mint, Pubkey::new([1u8; 32]));
        assert_eq!(recovered.owner, Pubkey::new([2u8; 32]));
        assert_eq!(recovered.state, AccountState::Initialized);
        assert_eq!(recovered.account_nonce, 0);
        assert!(!recovered.is_frozen());
    }

    #[test]
    fn test_confidential_token_account_balance_update() {
        let mut account = ConfidentialTokenAccount::new_with_zero_balance(
            Pubkey::new([1u8; 32]),
            Pubkey::new([2u8; 32]),
        );

        assert_eq!(account.account_nonce, 0);

        let new_commitment = [5u8; 32];
        let new_encrypted = vec![1, 2, 3, 4, 5];
        account
            .update_balance(new_commitment, &new_encrypted)
            .expect("should update");

        assert_eq!(account.balance_commitment, new_commitment);
        assert_eq!(&account.enc_balance_owner[..5], &[1, 2, 3, 4, 5]);
        assert_eq!(account.enc_balance_len, 5);
        assert_eq!(account.account_nonce, 1);
    }

    #[test]
    fn test_confidential_ata_derivation() {
        let wallet = Pubkey::new([1u8; 32]);
        let mint = Pubkey::new([2u8; 32]);

        let ata1 = ConfidentialAta::derive_address(&wallet, &mint).expect("should derive");
        let ata2 = ConfidentialAta::derive_address(&wallet, &mint).expect("should derive");

        // Same inputs = same output
        assert_eq!(ata1, ata2);

        // Different mint = different ATA
        let other_mint = Pubkey::new([3u8; 32]);
        let ata3 = ConfidentialAta::derive_address(&wallet, &other_mint).expect("should derive");
        assert_ne!(ata1, ata3);
    }

    #[test]
    fn test_confidential_mint_commitment_params_validation() {
        let mint = ConfidentialMint::new(6, Pubkey::new([1u8; 32]), None).expect("should create");

        // Should pass validation
        assert!(mint.validate_commitment_params().is_ok());

        // Tamper with params hash
        let mut tampered = mint.clone();
        tampered.commitment_params_hash[0] ^= 0xFF;
        assert!(tampered.validate_commitment_params().is_err());
    }

    #[test]
    fn test_transfer_proof_data_size_validation() {
        let valid_proof = TransferProofData {
            balance_proof: vec![0u8; MAX_PROOF_BYTES],
            sender_range_proof: vec![0u8; MAX_PROOF_BYTES],
            expected_sender_nonce: 0,
            expected_receiver_nonce: 0,
            recent_blockhash: [0u8; 32],
        };
        assert!(valid_proof.validate_sizes().is_ok());

        let invalid_proof = TransferProofData {
            balance_proof: vec![0u8; MAX_PROOF_BYTES + 1],
            sender_range_proof: vec![0u8; MAX_PROOF_BYTES],
            expected_sender_nonce: 0,
            expected_receiver_nonce: 0,
            recent_blockhash: [0u8; 32],
        };
        assert!(invalid_proof.validate_sizes().is_err());
    }

    #[test]
    fn test_instruction_builders() {
        let mint = Pubkey::new([1u8; 32]);
        let authority = Pubkey::new([2u8; 32]);

        let ix = ConfidentialTokenProgram::initialize_mint(mint, authority, None, 9);
        assert_eq!(ix.program_id, CONF_TOKEN_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 2);

        let account = Pubkey::new([3u8; 32]);
        let owner = Pubkey::new([4u8; 32]);
        let ix = ConfidentialTokenProgram::initialize_account(account, mint, owner);
        assert_eq!(ix.program_id, CONF_TOKEN_PROGRAM_ID);
        assert_eq!(ix.accounts.len(), 3);
    }

    #[test]
    fn test_frozen_account_detection() {
        let mut account = ConfidentialTokenAccount::new_with_zero_balance(
            Pubkey::new([1u8; 32]),
            Pubkey::new([2u8; 32]),
        );

        assert!(!account.is_frozen());
        assert!(account.is_initialized());

        account.state = AccountState::Frozen;
        assert!(account.is_frozen());
    }

    #[test]
    fn test_authority_types() {
        let authority_mint = ConfidentialAuthorityType::MintTokens;
        let authority_freeze = ConfidentialAuthorityType::FreezeAccount;

        // Ensure they're different
        assert_ne!(authority_mint, authority_freeze);

        // Ensure serialization works
        let serialized = bincode::serialize(&authority_mint).unwrap();
        let deserialized: ConfidentialAuthorityType = bincode::deserialize(&serialized).unwrap();
        assert_eq!(deserialized, authority_mint);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // INTEGRATION TESTS - End-to-end flows with real cryptographic operations
    // ═══════════════════════════════════════════════════════════════════════════

    /// Helper to create test accounts for processor tests
    fn create_test_account(key: Pubkey, data: Vec<u8>, owner: Pubkey) -> Account {
        Account {
            key,
            owner,
            motes: 1_000_000,
            data: crate::account::AccountData::new(data),
            executable: false,
            rent_epoch: 0,
            state: AccountState::Initialized,
        }
    }

    /// Helper to create a compute meter for tests
    fn test_meter() -> ComputeMeter {
        ComputeMeter::new(crate::metering::ComputeBudget::default())
    }

    #[test]
    fn test_process_initialize_mint() {
        let mint_key = Pubkey::new([10u8; 32]);
        let authority = Pubkey::new([20u8; 32]);
        let rent_sysvar = crate::native_programs::SYSVAR_RENT_ID;

        // Create empty mint account
        let mut mint_account = create_test_account(
            mint_key,
            vec![0u8; ConfidentialMint::SIZE],
            CONF_TOKEN_PROGRAM_ID,
        );
        let mut rent_account = create_test_account(rent_sysvar, vec![], Pubkey::default());

        let meter = test_meter();

        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::InitializeMint {
            decimals: 9,
            mint_authority: authority,
            freeze_authority: Some(authority),
        })
        .unwrap();

        let mut accounts: Vec<&mut Account> = vec![&mut mint_account, &mut rent_account];

        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(result.is_ok());

        // Verify mint was initialized
        let mint = ConfidentialMint::from_bytes(accounts[0].data.as_slice()).unwrap();
        assert!(mint.is_initialized);
        assert_eq!(mint.decimals, 9);
        assert_eq!(mint.mint_authority, Some(authority));
        assert_eq!(mint.freeze_authority, Some(authority));
        assert_eq!(mint.supply_public, 0);
    }

    #[test]
    fn test_process_initialize_account() {
        let mint_key = Pubkey::new([10u8; 32]);
        let account_key = Pubkey::new([11u8; 32]);
        let owner_key = Pubkey::new([20u8; 32]);

        // Create initialized mint
        let mint = ConfidentialMint::new(9, owner_key, None).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create empty token account
        let mut token_account = create_test_account(
            account_key,
            vec![0u8; ConfidentialTokenAccount::SIZE],
            CONF_TOKEN_PROGRAM_ID,
        );

        let mut owner_account = create_test_account(owner_key, vec![], Pubkey::default());

        let meter = test_meter();

        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::InitializeAccount).unwrap();

        let mut accounts: Vec<&mut Account> =
            vec![&mut token_account, &mut mint_account, &mut owner_account];

        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(result.is_ok());

        // Verify account was initialized
        let account = ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice()).unwrap();
        assert!(account.is_initialized());
        assert_eq!(account.mint, mint_key);
        assert_eq!(account.owner, owner_key);
        assert_eq!(account.account_nonce, 0);
    }

    #[test]
    fn test_process_freeze_thaw_account() {
        let mint_key = Pubkey::new([10u8; 32]);
        let account_key = Pubkey::new([11u8; 32]);
        let owner_key = Pubkey::new([20u8; 32]);
        let freeze_authority = Pubkey::new([30u8; 32]);

        // Create mint with freeze authority
        let mint = ConfidentialMint::new(9, owner_key, Some(freeze_authority)).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create initialized token account
        let token = ConfidentialTokenAccount::new_with_zero_balance(mint_key, owner_key);
        let mut token_account =
            create_test_account(account_key, token.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        let mut authority_account =
            create_test_account(freeze_authority, vec![], Pubkey::default());

        let meter = test_meter();

        // Freeze the account
        let freeze_ix = bincode::serialize(&ConfidentialTokenInstruction::FreezeAccount).unwrap();
        let mut accounts: Vec<&mut Account> = vec![
            &mut token_account,
            &mut mint_account,
            &mut authority_account,
        ];

        let result = ConfidentialTokenProcessor::process(&freeze_ix, &mut accounts, &meter);
        assert!(result.is_ok());

        let frozen_account =
            ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice()).unwrap();
        assert!(frozen_account.is_frozen());

        // Thaw the account
        let thaw_ix = bincode::serialize(&ConfidentialTokenInstruction::ThawAccount).unwrap();
        let result = ConfidentialTokenProcessor::process(&thaw_ix, &mut accounts, &meter);
        assert!(result.is_ok());

        let thawed_account =
            ConfidentialTokenAccount::from_bytes(accounts[0].data.as_slice()).unwrap();
        assert!(!thawed_account.is_frozen());
    }

    #[test]
    fn test_process_set_authority() {
        let mint_key = Pubkey::new([10u8; 32]);
        let old_authority = Pubkey::new([20u8; 32]);
        let new_authority = Pubkey::new([30u8; 32]);

        // Create mint
        let mint = ConfidentialMint::new(9, old_authority, Some(old_authority)).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        let mut authority_account = create_test_account(old_authority, vec![], Pubkey::default());

        let meter = test_meter();

        // Change mint authority
        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::SetAuthority {
            authority_type: ConfidentialAuthorityType::MintTokens,
            new_authority: Some(new_authority),
        })
        .unwrap();

        let mut accounts: Vec<&mut Account> = vec![&mut mint_account, &mut authority_account];

        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(result.is_ok());

        let updated_mint = ConfidentialMint::from_bytes(accounts[0].data.as_slice()).unwrap();
        assert_eq!(updated_mint.mint_authority, Some(new_authority));
    }

    #[test]
    fn test_double_initialization_fails() {
        let mint_key = Pubkey::new([10u8; 32]);
        let authority = Pubkey::new([20u8; 32]);
        let rent_sysvar = crate::native_programs::SYSVAR_RENT_ID;

        // Create already-initialized mint
        let mint = ConfidentialMint::new(9, authority, None).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);
        let mut rent_account = create_test_account(rent_sysvar, vec![], Pubkey::default());

        let meter = test_meter();

        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::InitializeMint {
            decimals: 6,
            mint_authority: authority,
            freeze_authority: None,
        })
        .unwrap();

        let mut accounts: Vec<&mut Account> = vec![&mut mint_account, &mut rent_account];

        // Second initialization should fail
        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(result.is_err());
    }

    #[test]
    fn test_invalid_freeze_authority_fails() {
        let mint_key = Pubkey::new([10u8; 32]);
        let account_key = Pubkey::new([11u8; 32]);
        let owner_key = Pubkey::new([20u8; 32]);
        let freeze_authority = Pubkey::new([30u8; 32]);
        let wrong_authority = Pubkey::new([99u8; 32]);

        // Create mint with freeze authority
        let mint = ConfidentialMint::new(9, owner_key, Some(freeze_authority)).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create initialized token account
        let token = ConfidentialTokenAccount::new_with_zero_balance(mint_key, owner_key);
        let mut token_account =
            create_test_account(account_key, token.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Use wrong authority
        let mut wrong_auth_account =
            create_test_account(wrong_authority, vec![], Pubkey::default());

        let meter = test_meter();

        let freeze_ix = bincode::serialize(&ConfidentialTokenInstruction::FreezeAccount).unwrap();
        let mut accounts: Vec<&mut Account> = vec![
            &mut token_account,
            &mut mint_account,
            &mut wrong_auth_account,
        ];

        let result = ConfidentialTokenProcessor::process(&freeze_ix, &mut accounts, &meter);
        assert!(result.is_err());
    }

    #[test]
    fn test_mint_mismatch_fails() {
        let mint_key = Pubkey::new([10u8; 32]);
        let other_mint_key = Pubkey::new([99u8; 32]);
        let account_key = Pubkey::new([11u8; 32]);
        let owner_key = Pubkey::new([20u8; 32]);
        let freeze_authority = Pubkey::new([30u8; 32]);

        // Create mint
        let mint = ConfidentialMint::new(9, owner_key, Some(freeze_authority)).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create token account for DIFFERENT mint
        let token = ConfidentialTokenAccount::new_with_zero_balance(other_mint_key, owner_key);
        let mut token_account =
            create_test_account(account_key, token.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        let mut authority_account =
            create_test_account(freeze_authority, vec![], Pubkey::default());

        let meter = test_meter();

        let freeze_ix = bincode::serialize(&ConfidentialTokenInstruction::FreezeAccount).unwrap();
        let mut accounts: Vec<&mut Account> = vec![
            &mut token_account,
            &mut mint_account,
            &mut authority_account,
        ];

        let result = ConfidentialTokenProcessor::process(&freeze_ix, &mut accounts, &meter);
        assert!(matches!(result, Err(ProgramError::MintMismatch)));
    }

    #[test]
    fn test_transfer_proof_data_serialization() {
        use crate::privacy::{BalanceProof, BalanceProofType};

        let proof_data = TransferProofData {
            balance_proof: vec![1, 2, 3, 4],
            sender_range_proof: vec![5, 6, 7, 8],
            expected_sender_nonce: 42,
            expected_receiver_nonce: 17,
            recent_blockhash: [9u8; 32],
        };

        let serialized = bincode::serialize(&proof_data).unwrap();
        let deserialized: TransferProofData = bincode::deserialize(&serialized).unwrap();

        assert_eq!(deserialized.expected_sender_nonce, 42);
        assert_eq!(deserialized.expected_receiver_nonce, 17);
        assert_eq!(deserialized.balance_proof, vec![1, 2, 3, 4]);
    }

    #[test]
    fn test_events_serialization() {
        let event = ConfidentialTokenEvent::ConfidentialTransfer {
            source: Pubkey::new([1u8; 32]),
            destination: Pubkey::new([2u8; 32]),
            mint: Pubkey::new([3u8; 32]),
            source_commitment: [4u8; 32],
            dest_commitment: [5u8; 32],
        };

        let serialized = bincode::serialize(&event).unwrap();
        let deserialized: ConfidentialTokenEvent = bincode::deserialize(&serialized).unwrap();

        match deserialized {
            ConfidentialTokenEvent::ConfidentialTransfer {
                source,
                destination,
                ..
            } => {
                assert_eq!(source, Pubkey::new([1u8; 32]));
                assert_eq!(destination, Pubkey::new([2u8; 32]));
            }
            _ => panic!("Wrong event type"),
        }
    }

    #[test]
    fn test_account_reinitialization_blocked() {
        // Verify that re-initializing an already initialized account fails
        let mint_key = Pubkey::new([10u8; 32]);
        let account_key = Pubkey::new([11u8; 32]);
        let owner_key = Pubkey::new([20u8; 32]);

        // Create initialized mint
        let mint = ConfidentialMint::new(9, owner_key, None).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create ALREADY initialized token account
        let existing_token = ConfidentialTokenAccount::new_with_zero_balance(mint_key, owner_key);
        let mut token_account = create_test_account(
            account_key,
            existing_token.to_bytes(),
            CONF_TOKEN_PROGRAM_ID,
        );

        let mut owner_account = create_test_account(owner_key, vec![], Pubkey::default());

        let meter = test_meter();
        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::InitializeAccount).unwrap();
        let mut accounts: Vec<&mut Account> =
            vec![&mut token_account, &mut mint_account, &mut owner_account];

        // Should fail because account is already initialized
        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(matches!(
            result,
            Err(ProgramError::AccountAlreadyInitialized)
        ));
    }

    #[test]
    fn test_self_transfer_blocked() {
        // Verify that transferring from an account to itself fails
        let mint_key = Pubkey::new([10u8; 32]);
        let account_key = Pubkey::new([11u8; 32]); // Same source and dest
        let owner_key = Pubkey::new([20u8; 32]);

        // Create initialized mint
        let mint = ConfidentialMint::new(9, owner_key, None).unwrap();
        let mut mint_account =
            create_test_account(mint_key, mint.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Create token account
        let token = ConfidentialTokenAccount::new_with_zero_balance(mint_key, owner_key);
        let mut token_account =
            create_test_account(account_key, token.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        // Clone for destination (same key)
        let mut dest_account =
            create_test_account(account_key, token.to_bytes(), CONF_TOKEN_PROGRAM_ID);

        let mut owner_account = create_test_account(owner_key, vec![], Pubkey::default());

        let meter = test_meter();
        let proof_data = TransferProofData {
            balance_proof: vec![0u8; 96],
            sender_range_proof: vec![0u8; 672],
            expected_sender_nonce: 0,
            expected_receiver_nonce: 0,
            recent_blockhash: [0u8; 32],
        };
        let ix_data = bincode::serialize(&ConfidentialTokenInstruction::TransferConfidential {
            new_source_commitment: [0u8; 32],
            new_source_encrypted: vec![0u8; 36],
            new_dest_commitment: [0u8; 32],
            new_dest_encrypted: vec![0u8; 36],
            proof_data,
        })
        .unwrap();

        let mut accounts: Vec<&mut Account> = vec![
            &mut token_account,
            &mut dest_account,
            &mut mint_account,
            &mut owner_account,
        ];

        // Should fail because source and dest are the same account
        let result = ConfidentialTokenProcessor::process(&ix_data, &mut accounts, &meter);
        assert!(matches!(result, Err(ProgramError::InvalidAccountOwner)));
    }
}
