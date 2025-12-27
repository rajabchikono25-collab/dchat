//! On-chain Bot Registry Program (BotFather)
//!
//! This module implements a production-grade bot registry as an on-chain program.
//! It provides:
//!
//! - **Bot Registration**: Register bots with owner and signing keys
//! - **Key Rotation**: Rotate bot signing keys without changing identity
//! - **Status Management**: Suspend/unsuspend bots
//! - **Capability Grants**: Scoped permissions for bots to act on programs
//! - **Verification**: Other programs can verify bot authorization via CPI or reads
//!
//! # Security Model
//!
//! Bots authenticate via public-key signatures only - no bearer tokens or secrets
//! are stored on-chain. Capability grants define what actions a bot can perform
//! on which programs, with optional constraints like amount limits and expiry.

use crate::account::{Account, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::events::ProgramEvent;
use crate::metering::ComputeMeter;
use crate::native_programs::BOT_REGISTRY_PROGRAM_ID;
use crate::pda::PdaDerivation;
use serde::{Deserialize, Serialize};

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum number of allowed mints in a capability grant
pub const MAX_ALLOWED_MINTS: usize = 8;

/// Bot account discriminator
pub const BOT_DISCRIMINATOR: [u8; 8] = [0x62, 0x6f, 0x74, 0x5f, 0x5f, 0x5f, 0x5f, 0x5f]; // "bot_____"

/// Capability grant discriminator
pub const CAPABILITY_DISCRIMINATOR: [u8; 8] = [0x63, 0x61, 0x70, 0x5f, 0x5f, 0x5f, 0x5f, 0x5f]; // "cap_____"

/// Bot seed prefix for PDA derivation
pub const BOT_SEED: &[u8] = b"bot";

/// Capability seed prefix for PDA derivation
pub const CAPABILITY_SEED: &[u8] = b"capability";

/// Fixed size of BotAccount for serialization
/// 8 (discriminator) + 32 (owner) + 32 (bot_signing_pubkey) + 4 (status enum variant)
/// + 8 (created_slot) + 8 (nonce) + 1 (bump) = 93 bytes
/// Note: bincode uses 4 bytes for enum variants by default, not repr(u8)
pub const BOT_ACCOUNT_SIZE: usize = 93;

/// Fixed size of BotCapabilityGrantAccount for serialization
/// 8 (discriminator) + 32 (bot_id) + 32 (target_program) + 8 (allowed_actions)
/// + 1 (has_max_amount) + 8 (max_amount) + 1 (mint_count) + 8*32 (allowed_mints)
/// + 8 (expiry_slot) + 8 (nonce) + 1 (bump) = 363 bytes
pub const CAPABILITY_ACCOUNT_SIZE: usize = 363;

// ═══════════════════════════════════════════════════════════════════════════════
// BOT STATUS
// ═══════════════════════════════════════════════════════════════════════════════

/// Bot status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum BotStatus {
    /// Bot is active and can perform authorized actions
    Active = 0,
    /// Bot is suspended and cannot perform any actions
    Suspended = 1,
}

impl BotStatus {
    /// Convert from u8
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(BotStatus::Active),
            1 => Some(BotStatus::Suspended),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT ACCOUNT
// ═══════════════════════════════════════════════════════════════════════════════

/// On-chain bot registration account
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAccount {
    /// Account type discriminator
    pub discriminator: [u8; 8],
    /// Owner's public key (can manage the bot)
    pub owner: Pubkey,
    /// Bot's signing public key (used for authentication)
    pub bot_signing_pubkey: Pubkey,
    /// Current status
    pub status: BotStatus,
    /// Slot when bot was registered
    pub created_slot: u64,
    /// Monotonic nonce for replay protection
    pub nonce: u64,
    /// PDA bump seed
    pub bump: u8,
}

impl BotAccount {
    /// Fixed serialized size
    pub const SIZE: usize = BOT_ACCOUNT_SIZE;

    /// Create a new bot account
    pub fn new(owner: Pubkey, bot_signing_pubkey: Pubkey, created_slot: u64, bump: u8) -> Self {
        Self {
            discriminator: BOT_DISCRIMINATOR,
            owner,
            bot_signing_pubkey,
            status: BotStatus::Active,
            created_slot,
            nonce: 0,
            bump,
        }
    }

    /// Deserialize from account data bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        if &data[0..8] != &BOT_DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes with fixed size padding
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }

    /// Check if bot is active
    pub fn is_active(&self) -> bool {
        self.status == BotStatus::Active
    }

    /// Increment nonce (must be called on every mutation)
    pub fn increment_nonce(&mut self) {
        self.nonce = self.nonce.saturating_add(1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// CAPABILITY GRANT ACCOUNT
// ═══════════════════════════════════════════════════════════════════════════════

/// On-chain capability grant for a bot
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCapabilityGrantAccount {
    /// Account type discriminator
    pub discriminator: [u8; 8],
    /// Bot account this grant is for
    pub bot_id: Pubkey,
    /// Target program this grant applies to
    pub target_program: Pubkey,
    /// Allowed actions as bitflags
    pub allowed_actions: u64,
    /// Whether max_amount constraint is set
    pub has_max_amount: bool,
    /// Maximum amount per action (if has_max_amount is true)
    pub max_amount: u64,
    /// Number of allowed mints (0 = any mint)
    pub mint_count: u8,
    /// Allowed mints (padded to MAX_ALLOWED_MINTS)
    pub allowed_mints: [Pubkey; MAX_ALLOWED_MINTS],
    /// Slot when grant expires (0 = no expiry)
    pub expiry_slot: u64,
    /// Monotonic nonce for replay protection
    pub nonce: u64,
    /// PDA bump seed
    pub bump: u8,
}

impl BotCapabilityGrantAccount {
    /// Fixed serialized size
    pub const SIZE: usize = CAPABILITY_ACCOUNT_SIZE;

    /// Create a new capability grant
    pub fn new(
        bot_id: Pubkey,
        target_program: Pubkey,
        allowed_actions: u64,
        max_amount: Option<u64>,
        allowed_mints: &[Pubkey],
        expiry_slot: u64,
        bump: u8,
    ) -> ProgramResult<Self> {
        if allowed_mints.len() > MAX_ALLOWED_MINTS {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut padded_mints = [Pubkey::default(); MAX_ALLOWED_MINTS];
        for (i, mint) in allowed_mints.iter().enumerate() {
            padded_mints[i] = *mint;
        }

        Ok(Self {
            discriminator: CAPABILITY_DISCRIMINATOR,
            bot_id,
            target_program,
            allowed_actions,
            has_max_amount: max_amount.is_some(),
            max_amount: max_amount.unwrap_or(0),
            mint_count: allowed_mints.len() as u8,
            allowed_mints: padded_mints,
            expiry_slot,
            nonce: 0,
            bump,
        })
    }

    /// Deserialize from account data bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        if &data[0..8] != &CAPABILITY_DISCRIMINATOR {
            return Err(ProgramError::InvalidAccountData);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes with fixed size padding
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = bincode::serialize(self).unwrap_or_default();
        data.resize(Self::SIZE, 0);
        data
    }

    /// Check if grant has expired
    pub fn is_expired(&self, current_slot: u64) -> bool {
        self.expiry_slot > 0 && current_slot >= self.expiry_slot
    }

    /// Check if action is allowed
    pub fn is_action_allowed(&self, action: u64) -> bool {
        self.allowed_actions & action != 0
    }

    /// Check if mint is allowed (empty list = all mints allowed)
    pub fn is_mint_allowed(&self, mint: &Pubkey) -> bool {
        if self.mint_count == 0 {
            return true; // No restrictions
        }
        self.allowed_mints[..self.mint_count as usize]
            .iter()
            .any(|m| m == mint)
    }

    /// Check if amount is within limit (no limit = all amounts allowed)
    pub fn is_amount_allowed(&self, amount: u64) -> bool {
        if !self.has_max_amount {
            return true;
        }
        amount <= self.max_amount
    }

    /// Increment nonce
    pub fn increment_nonce(&mut self) {
        self.nonce = self.nonce.saturating_add(1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT REGISTRY INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Bot registry program instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotRegistryInstruction {
    /// Register a new bot
    /// Accounts:
    /// 0. `[signer, writable]` Owner (payer)
    /// 1. `[writable]` Bot account PDA
    /// 2. `[]` System program
    RegisterBot {
        /// Bot's signing public key
        bot_signing_pubkey: Pubkey,
        /// Expected nonce (must be 0 for new account)
        expected_nonce: u64,
    },

    /// Rotate bot's signing key
    /// Accounts:
    /// 0. `[signer]` Owner
    /// 1. `[writable]` Bot account
    RotateBotKey {
        /// New signing public key
        new_signing_pubkey: Pubkey,
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Suspend a bot
    /// Accounts:
    /// 0. `[signer]` Owner
    /// 1. `[writable]` Bot account
    Suspend {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Unsuspend a bot
    /// Accounts:
    /// 0. `[signer]` Owner
    /// 1. `[writable]` Bot account
    Unsuspend {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Grant a capability to a bot
    /// Accounts:
    /// 0. `[signer, writable]` Owner (payer)
    /// 1. `[]` Bot account
    /// 2. `[writable]` Capability grant account PDA
    /// 3. `[]` System program
    GrantCapability {
        /// Target program for this grant
        target_program: Pubkey,
        /// Allowed actions as bitflags
        allowed_actions: u64,
        /// Maximum amount per action (None = no limit)
        max_amount: Option<u64>,
        /// Allowed mints (empty = all mints)
        allowed_mints: Vec<Pubkey>,
        /// Expiry slot (0 = no expiry)
        expiry_slot: u64,
        /// Expected nonce (must be 0 for new grant)
        expected_nonce: u64,
    },

    /// Revoke a capability from a bot
    /// Accounts:
    /// 0. `[signer]` Owner
    /// 1. `[]` Bot account
    /// 2. `[writable]` Capability grant account (to be closed)
    RevokeCapability {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Verify a bot's capability (read-only, for cross-program use)
    /// This instruction allows other programs to verify bot authorization
    /// without needing to read account data directly.
    /// Accounts:
    /// 0. `[signer]` Bot (must sign with bot_signing_pubkey)
    /// 1. `[]` Bot account
    /// 2. `[]` Capability grant account
    VerifyCapability {
        /// Target program to verify
        target_program: Pubkey,
        /// Action to verify
        action: u64,
        /// Optional: mint to verify (if applicable)
        mint: Option<Pubkey>,
        /// Optional: amount to verify (if applicable)
        amount: Option<u64>,
    },
}

impl BotRegistryInstruction {
    /// Deserialize from instruction data
    pub fn unpack(data: &[u8]) -> ProgramResult<Self> {
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)
    }

    /// Serialize to instruction data
    pub fn pack(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT REGISTRY EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Structured events emitted by the bot registry program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum BotRegistryEvent {
    /// Bot registered
    BotRegistered {
        /// The bot account pubkey
        bot: Pubkey,
        /// The owner of the bot
        owner: Pubkey,
        /// The signing pubkey for bot operations
        signing_pubkey: Pubkey,
    },
    /// Bot key rotated
    BotKeyRotated {
        /// The bot account pubkey
        bot: Pubkey,
        /// The old signing pubkey
        old_signing_pubkey: Pubkey,
        /// The new signing pubkey
        new_signing_pubkey: Pubkey,
    },
    /// Bot suspended
    BotSuspended {
        /// The bot account pubkey
        bot: Pubkey,
    },
    /// Bot unsuspended
    BotUnsuspended {
        /// The bot account pubkey
        bot: Pubkey,
    },
    /// Capability granted
    CapabilityGranted {
        /// The bot account pubkey
        bot: Pubkey,
        /// The capability grant account pubkey
        grant: Pubkey,
        /// The target program for this capability
        target_program: Pubkey,
        /// Bitmask of authorized actions
        actions: u64,
        /// Expiry slot for the capability
        expiry_slot: u64,
    },
    /// Capability revoked
    CapabilityRevoked {
        /// The bot account pubkey
        bot: Pubkey,
        /// The capability grant account pubkey
        grant: Pubkey,
    },
    /// Capability verified
    CapabilityVerified {
        /// The bot account pubkey
        bot: Pubkey,
        /// The target program for this capability
        target_program: Pubkey,
        /// The action being verified
        action: u64,
        /// Whether verification succeeded
        success: bool,
    },
}

impl BotRegistryEvent {
    /// Compute discriminator from event variant
    fn discriminator(&self) -> [u8; 8] {
        let name = match self {
            BotRegistryEvent::BotRegistered { .. } => "BotRegistered",
            BotRegistryEvent::BotKeyRotated { .. } => "BotKeyRotated",
            BotRegistryEvent::BotSuspended { .. } => "BotSuspended",
            BotRegistryEvent::BotUnsuspended { .. } => "BotUnsuspended",
            BotRegistryEvent::CapabilityGranted { .. } => "CapabilityGranted",
            BotRegistryEvent::CapabilityRevoked { .. } => "CapabilityRevoked",
            BotRegistryEvent::CapabilityVerified { .. } => "CapabilityVerified",
        };
        ProgramEvent::compute_discriminator(name)
    }

    /// Convert to ProgramEvent for emission
    pub fn to_program_event(&self) -> ProgramEvent {
        let data = bincode::serialize(self).unwrap_or_default();
        let discriminator = self.discriminator();
        // Create a minimal ProgramEvent - the runtime will fill in tx_hash, slot, index, etc.
        ProgramEvent {
            id: crate::events::EventId::zero(),
            program_id: BOT_REGISTRY_PROGRAM_ID,
            discriminator,
            data,
            slot: 0,
            index: 0,
            is_cpi: false,
            cpi_depth: 0,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT REGISTRY PROCESSOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Bot registry program processor
pub struct BotRegistryProcessor;

impl BotRegistryProcessor {
    /// Process a bot registry instruction
    pub fn process(
        data: &[u8],
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        let instruction = BotRegistryInstruction::unpack(data)?;

        match instruction {
            BotRegistryInstruction::RegisterBot {
                bot_signing_pubkey,
                expected_nonce,
            } => Self::process_register_bot(
                accounts,
                meter,
                current_slot,
                bot_signing_pubkey,
                expected_nonce,
            ),
            BotRegistryInstruction::RotateBotKey {
                new_signing_pubkey,
                expected_nonce,
            } => Self::process_rotate_key(accounts, meter, new_signing_pubkey, expected_nonce),
            BotRegistryInstruction::Suspend { expected_nonce } => {
                Self::process_suspend(accounts, meter, expected_nonce)
            }
            BotRegistryInstruction::Unsuspend { expected_nonce } => {
                Self::process_unsuspend(accounts, meter, expected_nonce)
            }
            BotRegistryInstruction::GrantCapability {
                target_program,
                allowed_actions,
                max_amount,
                allowed_mints,
                expiry_slot,
                expected_nonce,
            } => Self::process_grant_capability(
                accounts,
                meter,
                current_slot,
                target_program,
                allowed_actions,
                max_amount,
                allowed_mints,
                expiry_slot,
                expected_nonce,
            ),
            BotRegistryInstruction::RevokeCapability { expected_nonce } => {
                Self::process_revoke_capability(accounts, meter, expected_nonce)
            }
            BotRegistryInstruction::VerifyCapability {
                target_program,
                action,
                mint,
                amount,
            } => Self::process_verify_capability(
                accounts,
                meter,
                current_slot,
                target_program,
                action,
                mint,
                amount,
            ),
        }
    }

    /// Derive bot PDA address
    pub fn derive_bot_pda(owner: &Pubkey, bot_index: u64) -> ProgramResult<(Pubkey, u8)> {
        let index_bytes = bot_index.to_le_bytes();
        PdaDerivation::find_program_address(
            &[BOT_SEED, owner.as_bytes(), &index_bytes],
            &BOT_REGISTRY_PROGRAM_ID,
        )
        .map(|pda| (pda.address, pda.bump))
    }

    /// Derive capability PDA address
    pub fn derive_capability_pda(
        bot: &Pubkey,
        target_program: &Pubkey,
    ) -> ProgramResult<(Pubkey, u8)> {
        PdaDerivation::find_program_address(
            &[CAPABILITY_SEED, bot.as_bytes(), target_program.as_bytes()],
            &BOT_REGISTRY_PROGRAM_ID,
        )
        .map(|pda| (pda.address, pda.bump))
    }

    /// Verify bot capability (helper for other programs)
    /// Returns Ok(true) if bot has the specified capability, Ok(false) if not
    pub fn verify_bot_capability(
        bot_account: &Account,
        capability_account: &Account,
        bot_signer: &Pubkey,
        target_program: &Pubkey,
        action: u64,
        mint: Option<&Pubkey>,
        amount: Option<u64>,
        current_slot: u64,
    ) -> ProgramResult<bool> {
        // Load bot account
        let bot = BotAccount::from_bytes(bot_account.data.as_slice())?;

        // Verify bot is active
        if !bot.is_active() {
            return Ok(false);
        }

        // Verify signer is the bot's signing key
        if *bot_signer != bot.bot_signing_pubkey {
            return Ok(false);
        }

        // Load capability grant
        let grant = BotCapabilityGrantAccount::from_bytes(capability_account.data.as_slice())?;

        // Verify grant is for this bot
        if grant.bot_id != bot_account.key {
            return Ok(false);
        }

        // Verify grant targets the correct program
        if grant.target_program != *target_program {
            return Ok(false);
        }

        // Verify grant is not expired
        if grant.is_expired(current_slot) {
            return Ok(false);
        }

        // Verify action is allowed
        if !grant.is_action_allowed(action) {
            return Ok(false);
        }

        // Verify mint is allowed (if specified)
        if let Some(m) = mint {
            if !grant.is_mint_allowed(m) {
                return Ok(false);
            }
        }

        // Verify amount is allowed (if specified)
        if let Some(a) = amount {
            if !grant.is_amount_allowed(a) {
                return Ok(false);
            }
        }

        Ok(true)
    }

    fn process_register_bot(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        bot_signing_pubkey: Pubkey,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(5000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract owner key first (before taking mutable references)
        let owner_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs
        // Owner account (accounts[0]) must be marked as signer in the instruction's AccountMeta

        // Verify expected nonce (must be 0 for new account)
        if expected_nonce != 0 {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Now work with the bot account mutably
        let bot_account = &mut accounts[1];

        // Verify bot account is uninitialized
        if bot_account.data.len() > 0 && bot_account.data.as_slice()[0] != 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Derive and verify PDA (using index 0 for first bot)
        let (expected_pda, bump) = Self::derive_bot_pda(&owner_key, 0)?;
        if bot_account.key != expected_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        // Create bot account
        let bot = BotAccount::new(owner_key, bot_signing_pubkey, current_slot, bump);

        // Write to account
        let bot_key = bot_account.key;
        bot_account.data.set_from_bytes(bot.to_bytes());
        bot_account.owner = BOT_REGISTRY_PROGRAM_ID;

        let event = BotRegistryEvent::BotRegistered {
            bot: bot_key,
            owner: owner_key,
            signing_pubkey: bot_signing_pubkey,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_rotate_key(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        new_signing_pubkey: Pubkey,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(3000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract owner key first (before taking mutable references)
        let owner_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Now work with the bot account mutably
        let bot_account = &mut accounts[1];

        // Load bot
        let mut bot = BotAccount::from_bytes(bot_account.data.as_slice())?;

        // Verify nonce
        if bot.nonce != expected_nonce {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Verify owner
        if bot.owner != owner_key {
            return Err(ProgramError::Custom(2)); // NotOwner
        }

        let old_signing_pubkey = bot.bot_signing_pubkey;

        // Update signing key
        bot.bot_signing_pubkey = new_signing_pubkey;
        bot.increment_nonce();

        let bot_key = bot_account.key;
        bot_account.data.set_from_bytes(bot.to_bytes());

        let event = BotRegistryEvent::BotKeyRotated {
            bot: bot_key,
            old_signing_pubkey,
            new_signing_pubkey,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_suspend(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(3000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract owner key first (before taking mutable references)
        let owner_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Now work with the bot account mutably
        let bot_account = &mut accounts[1];

        // Load bot
        let mut bot = BotAccount::from_bytes(bot_account.data.as_slice())?;

        // Verify nonce
        if bot.nonce != expected_nonce {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Verify owner
        if bot.owner != owner_key {
            return Err(ProgramError::Custom(2)); // NotOwner
        }

        // Already suspended
        if bot.status == BotStatus::Suspended {
            return Err(ProgramError::Custom(3)); // AlreadySuspended
        }

        // Suspend
        bot.status = BotStatus::Suspended;
        bot.increment_nonce();

        let bot_key = bot_account.key;
        bot_account.data.set_from_bytes(bot.to_bytes());

        let event = BotRegistryEvent::BotSuspended { bot: bot_key };

        Ok(vec![event.to_program_event()])
    }

    fn process_unsuspend(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(3000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract owner key first (before taking mutable references)
        let owner_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Now work with the bot account mutably
        let bot_account = &mut accounts[1];

        // Load bot
        let mut bot = BotAccount::from_bytes(bot_account.data.as_slice())?;

        // Verify nonce
        if bot.nonce != expected_nonce {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Verify owner
        if bot.owner != owner_key {
            return Err(ProgramError::Custom(2)); // NotOwner
        }

        // Already active
        if bot.status == BotStatus::Active {
            return Err(ProgramError::Custom(4)); // AlreadyActive
        }

        // Unsuspend
        bot.status = BotStatus::Active;
        bot.increment_nonce();

        let bot_key = bot_account.key;
        bot_account.data.set_from_bytes(bot.to_bytes());

        let event = BotRegistryEvent::BotUnsuspended { bot: bot_key };

        Ok(vec![event.to_program_event()])
    }

    fn process_grant_capability(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        _current_slot: u64,
        target_program: Pubkey,
        allowed_actions: u64,
        max_amount: Option<u64>,
        allowed_mints: Vec<Pubkey>,
        expiry_slot: u64,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(8000)?;

        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract needed keys first (before taking mutable references)
        let owner_key = accounts[0].key;
        let bot_key = accounts[1].key;

        // Load bot data from immutable reference first
        let bot = BotAccount::from_bytes(accounts[1].data.as_slice())?;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Verify expected nonce (must be 0 for new grant)
        if expected_nonce != 0 {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Verify owner owns this bot
        if bot.owner != owner_key {
            return Err(ProgramError::Custom(2)); // NotOwner
        }

        // Now take mutable reference to capability account
        let capability_account = &mut accounts[2];

        // Verify capability account is uninitialized
        if capability_account.data.len() > 0 && capability_account.data.as_slice()[0] != 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Derive and verify PDA
        let (expected_pda, bump) = Self::derive_capability_pda(&bot_key, &target_program)?;
        if capability_account.key != expected_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        // Create capability grant
        let grant = BotCapabilityGrantAccount::new(
            bot_key,
            target_program,
            allowed_actions,
            max_amount,
            &allowed_mints,
            expiry_slot,
            bump,
        )?;

        // Write to account
        let grant_key = capability_account.key;
        capability_account.data.set_from_bytes(grant.to_bytes());
        capability_account.owner = BOT_REGISTRY_PROGRAM_ID;

        let event = BotRegistryEvent::CapabilityGranted {
            bot: bot_key,
            grant: grant_key,
            target_program,
            actions: allowed_actions,
            expiry_slot,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_revoke_capability(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(5000)?;

        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract needed keys first (before taking mutable references)
        let owner_key = accounts[0].key;
        let bot_key = accounts[1].key;

        // Load bot data from immutable reference first
        let bot = BotAccount::from_bytes(accounts[1].data.as_slice())?;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Verify owner owns this bot
        if bot.owner != owner_key {
            return Err(ProgramError::Custom(2)); // NotOwner
        }

        // Now take mutable reference to capability account
        let capability_account = &mut accounts[2];

        // Load capability to verify nonce
        let grant = BotCapabilityGrantAccount::from_bytes(capability_account.data.as_slice())?;

        // Verify nonce
        if grant.nonce != expected_nonce {
            return Err(ProgramError::Custom(1)); // InvalidNonce
        }

        // Verify grant is for this bot
        if grant.bot_id != bot_key {
            return Err(ProgramError::Custom(5)); // GrantBotMismatch
        }

        // Close the account by zeroing it
        let grant_key = capability_account.key;
        let zeros = vec![0u8; BotCapabilityGrantAccount::SIZE];
        capability_account.data.set_from_bytes(zeros);

        let event = BotRegistryEvent::CapabilityRevoked {
            bot: bot_key,
            grant: grant_key,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_verify_capability(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        target_program: Pubkey,
        action: u64,
        mint: Option<Pubkey>,
        amount: Option<u64>,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(5000)?;

        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        let bot_signer = &accounts[0];
        let bot_account = &accounts[1];
        let capability_account = &accounts[2];

        // NOTE: Signer verification is handled by the runtime before invoking native programs
        // The bot_signer account (accounts[0]) must be marked as signer in the instruction

        // Verify capability
        let success = Self::verify_bot_capability(
            bot_account,
            capability_account,
            &bot_signer.key,
            &target_program,
            action,
            mint.as_ref(),
            amount,
            current_slot,
        )?;

        let event = BotRegistryEvent::CapabilityVerified {
            bot: bot_account.key,
            target_program,
            action,
            success,
        };

        Ok(vec![event.to_program_event()])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_account_serialization() {
        let owner = Pubkey([1u8; 32]);
        let signing_key = Pubkey([2u8; 32]);

        let bot = BotAccount::new(owner, signing_key, 100, 255);

        let bytes = bot.to_bytes();
        assert_eq!(bytes.len(), BOT_ACCOUNT_SIZE);

        let parsed = BotAccount::from_bytes(&bytes).expect("Failed to parse bot");
        assert_eq!(parsed.owner, owner);
        assert_eq!(parsed.bot_signing_pubkey, signing_key);
        assert_eq!(parsed.status, BotStatus::Active);
        assert_eq!(parsed.created_slot, 100);
        assert_eq!(parsed.nonce, 0);
    }

    #[test]
    fn test_bot_status() {
        let owner = Pubkey([1u8; 32]);
        let signing_key = Pubkey([2u8; 32]);

        let mut bot = BotAccount::new(owner, signing_key, 100, 255);
        assert!(bot.is_active());

        bot.status = BotStatus::Suspended;
        assert!(!bot.is_active());
    }

    #[test]
    fn test_capability_grant_serialization() {
        let bot_id = Pubkey([1u8; 32]);
        let target = Pubkey([2u8; 32]);
        let mint1 = Pubkey([3u8; 32]);
        let mint2 = Pubkey([4u8; 32]);

        let grant = BotCapabilityGrantAccount::new(
            bot_id,
            target,
            0b111, // 3 actions
            Some(1000),
            &[mint1, mint2],
            500,
            255,
        )
        .expect("Failed to create grant");

        let bytes = grant.to_bytes();
        assert_eq!(bytes.len(), CAPABILITY_ACCOUNT_SIZE);

        let parsed = BotCapabilityGrantAccount::from_bytes(&bytes).expect("Failed to parse grant");
        assert_eq!(parsed.bot_id, bot_id);
        assert_eq!(parsed.target_program, target);
        assert_eq!(parsed.allowed_actions, 0b111);
        assert!(parsed.has_max_amount);
        assert_eq!(parsed.max_amount, 1000);
        assert_eq!(parsed.mint_count, 2);
        assert_eq!(parsed.expiry_slot, 500);
    }

    #[test]
    fn test_capability_expiry() {
        let grant = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[],
            500, // expires at slot 500
            255,
        )
        .expect("Failed to create grant");

        assert!(!grant.is_expired(100));
        assert!(!grant.is_expired(499));
        assert!(grant.is_expired(500));
        assert!(grant.is_expired(1000));

        // No expiry
        let grant_no_expiry = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[],
            0, // no expiry
            255,
        )
        .expect("Failed to create grant");

        assert!(!grant_no_expiry.is_expired(1000000));
    }

    #[test]
    fn test_capability_action_check() {
        let grant = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            0b101, // actions 0 and 2 allowed
            None,
            &[],
            0,
            255,
        )
        .expect("Failed to create grant");

        assert!(grant.is_action_allowed(0b001)); // action 0
        assert!(!grant.is_action_allowed(0b010)); // action 1
        assert!(grant.is_action_allowed(0b100)); // action 2
        assert!(grant.is_action_allowed(0b101)); // actions 0 and 2
        assert!(!grant.is_action_allowed(0b1000)); // action 3
    }

    #[test]
    fn test_capability_mint_check() {
        let mint1 = Pubkey([3u8; 32]);
        let mint2 = Pubkey([4u8; 32]);
        let mint3 = Pubkey([5u8; 32]);

        let grant = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[mint1, mint2],
            0,
            255,
        )
        .expect("Failed to create grant");

        assert!(grant.is_mint_allowed(&mint1));
        assert!(grant.is_mint_allowed(&mint2));
        assert!(!grant.is_mint_allowed(&mint3));

        // No restrictions
        let grant_any = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[],
            0,
            255,
        )
        .expect("Failed to create grant");

        assert!(grant_any.is_mint_allowed(&mint3));
    }

    #[test]
    fn test_capability_amount_check() {
        let grant = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            Some(1000),
            &[],
            0,
            255,
        )
        .expect("Failed to create grant");

        assert!(grant.is_amount_allowed(500));
        assert!(grant.is_amount_allowed(1000));
        assert!(!grant.is_amount_allowed(1001));

        // No limit
        let grant_any = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[],
            0,
            255,
        )
        .expect("Failed to create grant");

        assert!(grant_any.is_amount_allowed(u64::MAX));
    }

    #[test]
    fn test_nonce_increment() {
        let mut bot = BotAccount::new(Pubkey([1u8; 32]), Pubkey([2u8; 32]), 100, 255);

        assert_eq!(bot.nonce, 0);
        bot.increment_nonce();
        assert_eq!(bot.nonce, 1);
        bot.increment_nonce();
        assert_eq!(bot.nonce, 2);
    }

    #[test]
    fn test_instruction_serialization() {
        let instruction = BotRegistryInstruction::GrantCapability {
            target_program: Pubkey([1u8; 32]),
            allowed_actions: 0b111,
            max_amount: Some(1000),
            allowed_mints: vec![Pubkey([2u8; 32])],
            expiry_slot: 500,
            expected_nonce: 0,
        };

        let packed = instruction.pack();
        let unpacked = BotRegistryInstruction::unpack(&packed).expect("Failed to unpack");

        if let BotRegistryInstruction::GrantCapability {
            allowed_actions,
            max_amount,
            expiry_slot,
            ..
        } = unpacked
        {
            assert_eq!(allowed_actions, 0b111);
            assert_eq!(max_amount, Some(1000));
            assert_eq!(expiry_slot, 500);
        } else {
            panic!("Wrong instruction type");
        }
    }

    #[test]
    fn test_max_allowed_mints_validation() {
        // Valid: within limit
        let result = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &[Pubkey([3u8; 32]); MAX_ALLOWED_MINTS],
            0,
            255,
        );
        assert!(result.is_ok());

        // Invalid: exceeds limit
        let too_many_mints: Vec<Pubkey> = (0..MAX_ALLOWED_MINTS + 1)
            .map(|i| Pubkey([i as u8; 32]))
            .collect();
        let result = BotCapabilityGrantAccount::new(
            Pubkey([1u8; 32]),
            Pubkey([2u8; 32]),
            1,
            None,
            &too_many_mints,
            0,
            255,
        );
        assert!(result.is_err());
    }
}
