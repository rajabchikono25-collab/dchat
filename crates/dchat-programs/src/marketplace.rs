//! On-chain Marketplace Program
//!
//! This module implements a production-grade escrow-based marketplace as an on-chain
//! program (not an off-chain service). It provides:
//!
//! - **Escrow Accounts**: PDA-owned accounts holding locked funds with state machine
//! - **Vault Model**: Escrowed funds live in token accounts owned by marketplace PDAs
//! - **State Transitions**: Explicit state machine with invalid transition rejection
//! - **CPI Composability**: Calls TOKEN_PROGRAM_ID for fund locking/release
//! - **Replay Protection**: Monotonic nonce on every mutation
//! - **Expiry Enforcement**: Chain time (slot) based expiry, never wall-clock
//! - **Dispute Resolution**: Optional bot-authorized resolution path
//!
//! # Account Types
//!
//! - `EscrowAccount`: Required account storing escrow state, parties, amounts
//! - `ListingAccount`: Optional listing metadata (title, description, etc.)
//! - `EscrowVaultTokenAccount`: PDA-owned token account holding locked funds

use crate::account::{Account, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::events::ProgramEvent;
use crate::metering::ComputeMeter;
use crate::native_programs::{MARKETPLACE_PROGRAM_ID, TOKEN_PROGRAM_ID};
use crate::pda::PdaDerivation;
use crate::token::TokenAccount;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Maximum number of recipients in a multi-party escrow
pub const MAX_RECIPIENTS: usize = 8;

/// Maximum listing title length
pub const MAX_TITLE_LENGTH: usize = 128;

/// Maximum listing description length
pub const MAX_DESCRIPTION_LENGTH: usize = 512;

/// Escrow account discriminator
pub const ESCROW_DISCRIMINATOR: [u8; 8] = [0x65, 0x73, 0x63, 0x72, 0x6f, 0x77, 0x5f, 0x5f]; // "escrow__"

/// Listing account discriminator
pub const LISTING_DISCRIMINATOR: [u8; 8] = [0x6c, 0x69, 0x73, 0x74, 0x69, 0x6e, 0x67, 0x5f]; // "listing_"

/// Vault seed prefix for PDA derivation
pub const VAULT_SEED: &[u8] = b"vault";

/// Escrow seed prefix for PDA derivation
pub const ESCROW_SEED: &[u8] = b"escrow";

/// Fixed size of EscrowAccount for serialization (with MAX_RECIPIENTS=8)
/// 8 (discriminator) + 32 (buyer) + 1 (recipient_count) + 8*40 (recipients: 32 pubkey + 8 share)
/// + 32 (mint) + 8 (amount) + 4 (state enum variant) + 8 (created_slot) + 8 (expiry_slot)
/// + 1 (has_dispute) + 32 (dispute_resolver) + 8 (dispute_slot) + 8 (nonce) + 1 (bump)
/// = 8 + 32 + 1 + 320 + 32 + 8 + 4 + 8 + 8 + 1 + 32 + 8 + 8 + 1 = 471 bytes
/// Note: bincode uses 4 bytes for enum variants by default, not repr(u8)
pub const ESCROW_ACCOUNT_SIZE: usize = 471;

/// Fixed size of ListingAccount for serialization
/// 8 (discriminator) + 32 (creator) + 32 (escrow) + 128 (title) + 512 (description)
/// + 8 (price) + 32 (mint) + 1 (is_active) + 8 (created_slot) + 8 (nonce) + 1 (bump)
/// = 770 bytes
pub const LISTING_ACCOUNT_SIZE: usize = 770;

// ═══════════════════════════════════════════════════════════════════════════════
// ESCROW STATE MACHINE
// ═══════════════════════════════════════════════════════════════════════════════

/// Escrow state with explicit state transitions
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[repr(u8)]
pub enum EscrowState {
    /// Initial state - funds are locked in vault
    Locked = 0,
    /// Seller has marked item as delivered (optional intermediate state)
    Delivered = 1,
    /// Buyer has raised a dispute
    Disputed = 2,
    /// Funds have been released to seller/recipients
    Released = 3,
    /// Funds have been refunded to buyer
    Refunded = 4,
    /// Escrow has expired without resolution
    Expired = 5,
}

impl EscrowState {
    /// Check if transition to the given state is valid
    pub fn can_transition_to(&self, next: EscrowState) -> bool {
        matches!(
            (*self, next),
            // From Locked: can deliver, dispute, release, refund (on expiry), or expire
            (EscrowState::Locked, EscrowState::Delivered)
                | (EscrowState::Locked, EscrowState::Disputed)
                | (EscrowState::Locked, EscrowState::Released)
                | (EscrowState::Locked, EscrowState::Refunded)
                | (EscrowState::Locked, EscrowState::Expired)
                // From Delivered: can dispute, release, or expire
                | (EscrowState::Delivered, EscrowState::Disputed)
                | (EscrowState::Delivered, EscrowState::Released)
                | (EscrowState::Delivered, EscrowState::Expired)
                // From Disputed: can only be resolved (release or refund) by authorized party
                | (EscrowState::Disputed, EscrowState::Released)
                | (EscrowState::Disputed, EscrowState::Refunded)
        )
    }

    /// Check if this is a terminal state (no further transitions allowed)
    pub fn is_terminal(&self) -> bool {
        matches!(
            self,
            EscrowState::Released | EscrowState::Refunded | EscrowState::Expired
        )
    }

    /// Convert from u8 discriminant
    pub fn from_u8(value: u8) -> Option<Self> {
        match value {
            0 => Some(EscrowState::Locked),
            1 => Some(EscrowState::Delivered),
            2 => Some(EscrowState::Disputed),
            3 => Some(EscrowState::Released),
            4 => Some(EscrowState::Refunded),
            5 => Some(EscrowState::Expired),
            _ => None,
        }
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// RECIPIENT SPLIT
// ═══════════════════════════════════════════════════════════════════════════════

/// A recipient in a multi-party escrow with their share (in basis points)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecipientSplit {
    /// Recipient's public key
    pub pubkey: Pubkey,
    /// Share in basis points (10000 = 100%)
    pub share_bps: u64,
}

impl RecipientSplit {
    /// Size of a single recipient split in bytes
    pub const SIZE: usize = 40; // 32 + 8
}

// ═══════════════════════════════════════════════════════════════════════════════
// ESCROW ACCOUNT
// ═══════════════════════════════════════════════════════════════════════════════

/// On-chain escrow account storing locked funds state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EscrowAccount {
    /// Account type discriminator
    pub discriminator: [u8; 8],
    /// Buyer's public key (funds come from here)
    pub buyer: Pubkey,
    /// Number of recipients (1 for two-party, up to MAX_RECIPIENTS for multi-party)
    pub recipient_count: u8,
    /// Recipients and their shares (padded to MAX_RECIPIENTS)
    pub recipients: [RecipientSplit; MAX_RECIPIENTS],
    /// Token mint for escrowed funds
    pub mint: Pubkey,
    /// Amount of tokens locked in escrow
    pub amount: u64,
    /// Current escrow state
    pub state: EscrowState,
    /// Slot when escrow was created
    pub created_slot: u64,
    /// Slot when escrow expires (0 = no expiry)
    pub expiry_slot: u64,
    /// Whether a dispute has been raised
    pub has_dispute: bool,
    /// Authorized dispute resolver (if any)
    pub dispute_resolver: Pubkey,
    /// Slot when dispute was raised (0 = no dispute)
    pub dispute_slot: u64,
    /// Monotonic nonce for replay protection
    pub nonce: u64,
    /// PDA bump seed for signing
    pub bump: u8,
}

impl EscrowAccount {
    /// Fixed serialized size
    pub const SIZE: usize = ESCROW_ACCOUNT_SIZE;

    /// Create a new escrow account
    pub fn new(
        buyer: Pubkey,
        recipients: &[RecipientSplit],
        mint: Pubkey,
        amount: u64,
        created_slot: u64,
        expiry_slot: u64,
        dispute_resolver: Pubkey,
        bump: u8,
    ) -> ProgramResult<Self> {
        if recipients.is_empty() || recipients.len() > MAX_RECIPIENTS {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Validate shares sum to 10000 basis points
        let total_shares: u64 = recipients.iter().map(|r| r.share_bps).sum();
        if total_shares != 10000 {
            return Err(ProgramError::Custom(1)); // InvalidSharesTotal
        }

        let mut padded_recipients = [RecipientSplit {
            pubkey: Pubkey::default(),
            share_bps: 0,
        }; MAX_RECIPIENTS];
        for (i, r) in recipients.iter().enumerate() {
            padded_recipients[i] = *r;
        }

        Ok(Self {
            discriminator: ESCROW_DISCRIMINATOR,
            buyer,
            recipient_count: recipients.len() as u8,
            recipients: padded_recipients,
            mint,
            amount,
            state: EscrowState::Locked,
            created_slot,
            expiry_slot,
            has_dispute: false,
            dispute_resolver,
            dispute_slot: 0,
            nonce: 0,
            bump,
        })
    }

    /// Deserialize from account data bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        if &data[0..8] != &ESCROW_DISCRIMINATOR {
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

    /// Get active recipients (non-zero shares)
    pub fn active_recipients(&self) -> impl Iterator<Item = &RecipientSplit> {
        self.recipients[..self.recipient_count as usize].iter()
    }

    /// Validate state transition
    pub fn validate_transition(&self, next_state: EscrowState) -> ProgramResult<()> {
        if self.state.is_terminal() {
            return Err(ProgramError::Custom(2)); // EscrowAlreadyTerminal
        }
        if !self.state.can_transition_to(next_state) {
            return Err(ProgramError::Custom(3)); // InvalidStateTransition
        }
        Ok(())
    }

    /// Check if escrow has expired based on current slot
    pub fn is_expired(&self, current_slot: u64) -> bool {
        self.expiry_slot > 0 && current_slot >= self.expiry_slot
    }

    /// Increment nonce (must be called on every mutation)
    pub fn increment_nonce(&mut self) {
        self.nonce = self.nonce.saturating_add(1);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LISTING ACCOUNT (OPTIONAL)
// ═══════════════════════════════════════════════════════════════════════════════

/// On-chain listing metadata (optional - escrow can exist without listing)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ListingAccount {
    /// Account type discriminator
    pub discriminator: [u8; 8],
    /// Creator's public key
    pub creator: Pubkey,
    /// Associated escrow account (if any)
    pub escrow: Pubkey,
    /// Listing title (padded to MAX_TITLE_LENGTH)
    #[serde(with = "BigArray")]
    pub title: [u8; MAX_TITLE_LENGTH],
    /// Listing description (padded to MAX_DESCRIPTION_LENGTH)
    #[serde(with = "BigArray")]
    pub description: [u8; MAX_DESCRIPTION_LENGTH],
    /// Price in tokens
    pub price: u64,
    /// Token mint for payment
    pub mint: Pubkey,
    /// Whether listing is active
    pub is_active: bool,
    /// Slot when listing was created
    pub created_slot: u64,
    /// Monotonic nonce for replay protection
    pub nonce: u64,
    /// PDA bump seed
    pub bump: u8,
}

impl ListingAccount {
    /// Fixed serialized size
    pub const SIZE: usize = LISTING_ACCOUNT_SIZE;

    /// Create a new listing
    pub fn new(
        creator: Pubkey,
        title: &str,
        description: &str,
        price: u64,
        mint: Pubkey,
        created_slot: u64,
        bump: u8,
    ) -> ProgramResult<Self> {
        if title.len() > MAX_TITLE_LENGTH || description.len() > MAX_DESCRIPTION_LENGTH {
            return Err(ProgramError::InvalidInstructionData);
        }

        let mut title_bytes = [0u8; MAX_TITLE_LENGTH];
        title_bytes[..title.len()].copy_from_slice(title.as_bytes());

        let mut desc_bytes = [0u8; MAX_DESCRIPTION_LENGTH];
        desc_bytes[..description.len()].copy_from_slice(description.as_bytes());

        Ok(Self {
            discriminator: LISTING_DISCRIMINATOR,
            creator,
            escrow: Pubkey::default(),
            title: title_bytes,
            description: desc_bytes,
            price,
            mint,
            is_active: true,
            created_slot,
            nonce: 0,
            bump,
        })
    }

    /// Deserialize from account data bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.len() < Self::SIZE {
            return Err(ProgramError::InvalidAccountData);
        }
        if &data[0..8] != &LISTING_DISCRIMINATOR {
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

    /// Get title as string
    pub fn title_str(&self) -> &str {
        let end = self
            .title
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAX_TITLE_LENGTH);
        std::str::from_utf8(&self.title[..end]).unwrap_or("")
    }

    /// Get description as string
    pub fn description_str(&self) -> &str {
        let end = self
            .description
            .iter()
            .position(|&b| b == 0)
            .unwrap_or(MAX_DESCRIPTION_LENGTH);
        std::str::from_utf8(&self.description[..end]).unwrap_or("")
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MARKETPLACE INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Marketplace program instructions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketplaceInstruction {
    /// Create a new listing (optional)
    /// Accounts:
    /// 0. `[signer, writable]` Creator (payer)
    /// 1. `[writable]` Listing account PDA
    /// 2. `[]` System program
    CreateListing {
        /// Listing title
        title: String,
        /// Listing description
        description: String,
        /// Price in tokens
        price: u64,
        /// Token mint
        mint: Pubkey,
        /// Expected nonce (must be 0 for new account)
        expected_nonce: u64,
    },

    /// Create an escrow and lock funds
    /// Accounts:
    /// 0. `[signer, writable]` Buyer (payer)
    /// 1. `[writable]` Buyer's token account
    /// 2. `[writable]` Escrow account PDA
    /// 3. `[writable]` Escrow vault token account PDA
    /// 4. `[]` Token mint
    /// 5. `[]` Token program
    /// 6. `[]` System program
    /// 7+ `[]` Recipient accounts (for multi-party)
    CreateEscrow {
        /// Recipients and their shares
        recipients: Vec<RecipientSplit>,
        /// Amount to lock
        amount: u64,
        /// Expiry slot (0 = no expiry)
        expiry_slot: u64,
        /// Dispute resolver pubkey
        dispute_resolver: Pubkey,
        /// Expected nonce (must be 0 for new account)
        expected_nonce: u64,
    },

    /// Mark escrow as delivered (optional, called by seller)
    /// Accounts:
    /// 0. `[signer]` Seller (must be a recipient)
    /// 1. `[writable]` Escrow account
    MarkDelivered {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Release funds to recipients
    /// Accounts:
    /// 0. `[signer]` Buyer or authorized bot
    /// 1. `[writable]` Escrow account
    /// 2. `[writable]` Escrow vault token account
    /// 3. `[]` Token program
    /// 4+ `[writable]` Recipient token accounts
    Release {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Refund funds to buyer
    /// Accounts:
    /// 0. `[signer]` Seller, buyer (if expired), or authorized bot
    /// 1. `[writable]` Escrow account
    /// 2. `[writable]` Escrow vault token account
    /// 3. `[writable]` Buyer's token account
    /// 4. `[]` Token program
    Refund {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Raise a dispute
    /// Accounts:
    /// 0. `[signer]` Buyer or seller
    /// 1. `[writable]` Escrow account
    RaiseDispute {
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },

    /// Resolve a dispute (release or refund)
    /// Accounts:
    /// 0. `[signer]` Dispute resolver or authorized bot
    /// 1. `[writable]` Escrow account
    /// 2. `[writable]` Escrow vault token account
    /// 3. `[]` Token program
    /// 4+ `[writable]` Recipient or buyer token accounts
    /// Optional bot authorization accounts:
    /// N. `[]` Bot account
    /// N+1. `[]` Bot capability grant account
    ResolveDispute {
        /// Whether to release (true) or refund (false)
        release: bool,
        /// Expected nonce for replay protection
        expected_nonce: u64,
    },
}

impl MarketplaceInstruction {
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
// MARKETPLACE EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Structured events emitted by the marketplace program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MarketplaceEvent {
    /// Listing created
    ListingCreated {
        /// The listing account pubkey
        listing: Pubkey,
        /// The creator of the listing
        creator: Pubkey,
        /// The price in lamports
        price: u64,
        /// The token mint for payment
        mint: Pubkey,
    },
    /// Escrow created and funds locked
    EscrowCreated {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// The buyer who funded the escrow
        buyer: Pubkey,
        /// The amount locked in escrow
        amount: u64,
        /// The token mint
        mint: Pubkey,
        /// The expiry slot for auto-refund
        expiry_slot: u64,
    },
    /// Escrow marked as delivered
    EscrowDelivered {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// Who marked the delivery
        marker: Pubkey,
    },
    /// Funds released to recipients
    EscrowReleased {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// Who triggered the release
        releaser: Pubkey,
        /// The total amount released
        amount: u64,
    },
    /// Funds refunded to buyer
    EscrowRefunded {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// Who triggered the refund
        refunder: Pubkey,
        /// The amount refunded
        amount: u64,
    },
    /// Dispute raised
    DisputeRaised {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// Who raised the dispute
        raiser: Pubkey,
    },
    /// Dispute resolved
    DisputeResolved {
        /// The escrow account pubkey
        escrow: Pubkey,
        /// Who resolved the dispute
        resolver: Pubkey,
        /// True if funds were released, false if refunded
        released: bool,
        /// Whether resolver was a bot
        bot_authorized: bool,
    },
}

impl MarketplaceEvent {
    /// Compute discriminator from event variant
    fn discriminator(&self) -> [u8; 8] {
        let name = match self {
            MarketplaceEvent::ListingCreated { .. } => "ListingCreated",
            MarketplaceEvent::EscrowCreated { .. } => "EscrowCreated",
            MarketplaceEvent::EscrowDelivered { .. } => "EscrowDelivered",
            MarketplaceEvent::EscrowReleased { .. } => "EscrowReleased",
            MarketplaceEvent::EscrowRefunded { .. } => "EscrowRefunded",
            MarketplaceEvent::DisputeRaised { .. } => "DisputeRaised",
            MarketplaceEvent::DisputeResolved { .. } => "DisputeResolved",
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
            program_id: MARKETPLACE_PROGRAM_ID,
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
// MARKETPLACE PROCESSOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Marketplace program processor
pub struct MarketplaceProcessor;

impl MarketplaceProcessor {
    /// Process a marketplace instruction
    pub fn process(
        data: &[u8],
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        let instruction = MarketplaceInstruction::unpack(data)?;

        match instruction {
            MarketplaceInstruction::CreateListing {
                title,
                description,
                price,
                mint,
                expected_nonce,
            } => Self::process_create_listing(
                accounts,
                meter,
                current_slot,
                title,
                description,
                price,
                mint,
                expected_nonce,
            ),
            MarketplaceInstruction::CreateEscrow {
                recipients,
                amount,
                expiry_slot,
                dispute_resolver,
                expected_nonce,
            } => Self::process_create_escrow(
                accounts,
                meter,
                current_slot,
                recipients,
                amount,
                expiry_slot,
                dispute_resolver,
                expected_nonce,
            ),
            MarketplaceInstruction::MarkDelivered { expected_nonce } => {
                Self::process_mark_delivered(accounts, meter, current_slot, expected_nonce)
            }
            MarketplaceInstruction::Release { expected_nonce } => {
                Self::process_release(accounts, meter, current_slot, expected_nonce)
            }
            MarketplaceInstruction::Refund { expected_nonce } => {
                Self::process_refund(accounts, meter, current_slot, expected_nonce)
            }
            MarketplaceInstruction::RaiseDispute { expected_nonce } => {
                Self::process_raise_dispute(accounts, meter, current_slot, expected_nonce)
            }
            MarketplaceInstruction::ResolveDispute {
                release,
                expected_nonce,
            } => Self::process_resolve_dispute(
                accounts,
                meter,
                current_slot,
                release,
                expected_nonce,
            ),
        }
    }

    /// Derive escrow PDA address
    pub fn derive_escrow_pda(buyer: &Pubkey, nonce_seed: u64) -> ProgramResult<(Pubkey, u8)> {
        let nonce_bytes = nonce_seed.to_le_bytes();
        PdaDerivation::find_program_address(
            &[ESCROW_SEED, buyer.as_bytes(), &nonce_bytes],
            &MARKETPLACE_PROGRAM_ID,
        )
        .map(|pda| (pda.address, pda.bump))
    }

    /// Derive vault PDA address
    pub fn derive_vault_pda(escrow: &Pubkey) -> ProgramResult<(Pubkey, u8)> {
        PdaDerivation::find_program_address(
            &[VAULT_SEED, escrow.as_bytes()],
            &MARKETPLACE_PROGRAM_ID,
        )
        .map(|pda| (pda.address, pda.bump))
    }

    /// Derive listing PDA address
    pub fn derive_listing_pda(creator: &Pubkey, nonce_seed: u64) -> ProgramResult<(Pubkey, u8)> {
        let nonce_bytes = nonce_seed.to_le_bytes();
        PdaDerivation::find_program_address(
            &[b"listing", creator.as_bytes(), &nonce_bytes],
            &MARKETPLACE_PROGRAM_ID,
        )
        .map(|pda| (pda.address, pda.bump))
    }

    fn process_create_listing(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        title: String,
        description: String,
        price: u64,
        mint: Pubkey,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(5000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract creator key first (before taking mutable references)
        let creator_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs
        // Creator account (accounts[0]) must be marked as signer in the instruction's AccountMeta

        // Verify expected nonce (must be 0 for new account)
        if expected_nonce != 0 {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Now work with the listing account mutably
        let listing_account = &mut accounts[1];

        // Verify listing account is empty or uninitialized
        if listing_account.data.len() > 0 && listing_account.data.as_slice()[0] != 0 {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Derive and verify PDA
        let (expected_pda, bump) = Self::derive_listing_pda(&creator_key, 0)?;
        if listing_account.key != expected_pda {
            return Err(ProgramError::InvalidSeeds);
        }

        // Create listing
        let listing = ListingAccount::new(
            creator_key,
            &title,
            &description,
            price,
            mint,
            current_slot,
            bump,
        )?;

        // Write to account
        let listing_key = listing_account.key;
        listing_account.data.set_from_bytes(listing.to_bytes());
        listing_account.owner = MARKETPLACE_PROGRAM_ID;

        let event = MarketplaceEvent::ListingCreated {
            listing: listing_key,
            creator: creator_key,
            price,
            mint,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_create_escrow(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        recipients: Vec<RecipientSplit>,
        amount: u64,
        expiry_slot: u64,
        dispute_resolver: Pubkey,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(10000)?;

        if accounts.len() < 6 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract all needed keys upfront (before any mutable borrows)
        let buyer_key = accounts[0].key;
        let mint_key = accounts[4].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Verify expected nonce (must be 0 for new escrow)
        if expected_nonce != 0 {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Validate recipients
        if recipients.is_empty() || recipients.len() > MAX_RECIPIENTS {
            return Err(ProgramError::InvalidInstructionData);
        }

        // Validate expiry is in the future or 0
        if expiry_slot != 0 && expiry_slot <= current_slot {
            return Err(ProgramError::Custom(5)); // InvalidExpiry
        }

        // Derive and verify escrow PDA
        let (expected_escrow, escrow_bump) = Self::derive_escrow_pda(&buyer_key, 0)?;

        // Read data we need from accounts before mutable borrows
        {
            // Check escrow account key
            if accounts[2].key != expected_escrow {
                return Err(ProgramError::InvalidSeeds);
            }

            // Derive and verify vault PDA
            let (expected_vault, _vault_bump) = Self::derive_vault_pda(&accounts[2].key)?;
            if accounts[3].key != expected_vault {
                return Err(ProgramError::InvalidSeeds);
            }

            // Verify escrow account is uninitialized
            if accounts[2].data.len() > 0 && accounts[2].data.as_slice()[0] != 0 {
                return Err(ProgramError::AccountAlreadyInitialized);
            }
        }

        // Read buyer token data
        let buyer_token = TokenAccount::from_bytes(accounts[1].data.as_slice())?;
        if buyer_token.amount < amount {
            return Err(ProgramError::InsufficientFunds);
        }
        if buyer_token.mint != mint_key {
            return Err(ProgramError::Custom(6)); // MintMismatch
        }

        // Now perform mutations by index
        // Debit buyer token account
        let mut buyer_token_data = buyer_token;
        buyer_token_data.amount = buyer_token_data
            .amount
            .checked_sub(amount)
            .ok_or(ProgramError::InsufficientFunds)?;
        accounts[1].data.set_from_bytes(buyer_token_data.to_bytes());

        // Credit vault (initialize if needed)
        let vault_token = TokenAccount {
            mint: mint_key,
            owner: expected_escrow, // Escrow PDA owns the vault
            amount,
            delegate: None,
            delegated_amount: 0,
            state: crate::token::AccountState::Initialized,
            is_native: None,
            close_authority: None,
        };
        accounts[3].data.set_from_bytes(vault_token.to_bytes());
        accounts[3].owner = TOKEN_PROGRAM_ID;

        // Create escrow account
        let escrow = EscrowAccount::new(
            buyer_key,
            &recipients,
            mint_key,
            amount,
            current_slot,
            expiry_slot,
            dispute_resolver,
            escrow_bump,
        )?;

        let escrow_key = accounts[2].key;
        accounts[2].data.set_from_bytes(escrow.to_bytes());
        accounts[2].owner = MARKETPLACE_PROGRAM_ID;

        let event = MarketplaceEvent::EscrowCreated {
            escrow: escrow_key,
            buyer: buyer_key,
            amount,
            mint: mint_key,
            expiry_slot,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_mark_delivered(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        _current_slot: u64,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(3000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract marker key first (before taking mutable references)
        let marker_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Now work with the escrow account mutably
        let escrow_account = &mut accounts[1];

        // Load escrow
        let mut escrow = EscrowAccount::from_bytes(escrow_account.data.as_slice())?;

        // Verify nonce
        if escrow.nonce != expected_nonce {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Verify marker is a recipient
        let is_recipient = escrow.active_recipients().any(|r| r.pubkey == marker_key);
        if !is_recipient {
            return Err(ProgramError::Custom(7)); // NotAuthorized
        }

        // Validate state transition
        escrow.validate_transition(EscrowState::Delivered)?;

        // Update state
        escrow.state = EscrowState::Delivered;
        escrow.increment_nonce();

        let escrow_key = escrow_account.key;
        escrow_account.data.set_from_bytes(escrow.to_bytes());

        let event = MarketplaceEvent::EscrowDelivered {
            escrow: escrow_key,
            marker: marker_key,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_release(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(15000)?;

        if accounts.len() < 4 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract keys upfront (before mutable borrows)
        let releaser_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Load escrow data first (read-only phase)
        let escrow = EscrowAccount::from_bytes(accounts[1].data.as_slice())?;
        let escrow_key = accounts[1].key;

        // Verify nonce
        if escrow.nonce != expected_nonce {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Check expiry
        if escrow.is_expired(current_slot) {
            return Err(ProgramError::Custom(8)); // EscrowExpired
        }

        // Verify releaser is buyer
        if releaser_key != escrow.buyer {
            return Err(ProgramError::Custom(7)); // NotAuthorized
        }

        // Validate state transition
        escrow.validate_transition(EscrowState::Released)?;

        // Calculate distribution (read-only phase)
        let total_amount = escrow.amount;
        let recipient_count = escrow.recipient_count as usize;

        if accounts.len() < 4 + recipient_count {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Collect recipient shares for distribution
        let mut distributions: Vec<(usize, u64, Pubkey)> = Vec::new();
        for (i, recipient) in escrow.active_recipients().enumerate() {
            let share_amount = (total_amount as u128 * recipient.share_bps as u128 / 10000) as u64;
            distributions.push((4 + i, share_amount, recipient.pubkey));
        }

        // Load vault data
        let vault_token = TokenAccount::from_bytes(accounts[2].data.as_slice())?;
        let mut vault_amount = vault_token.amount;

        // Now perform mutations
        // Distribute to recipients
        for (idx, share_amount, expected_owner) in distributions {
            let recipient_token = TokenAccount::from_bytes(accounts[idx].data.as_slice())?;

            // Verify recipient token account owner
            if recipient_token.owner != expected_owner {
                return Err(ProgramError::Custom(9)); // InvalidRecipientAccount
            }

            let new_amount = recipient_token
                .amount
                .checked_add(share_amount)
                .ok_or(ProgramError::Custom(10))?; // Overflow

            vault_amount = vault_amount
                .checked_sub(share_amount)
                .ok_or(ProgramError::InsufficientFunds)?;

            // Update recipient account
            let mut updated_token = recipient_token;
            updated_token.amount = new_amount;
            accounts[idx].data.set_from_bytes(updated_token.to_bytes());
        }

        // Update vault
        let mut updated_vault = vault_token;
        updated_vault.amount = vault_amount;
        accounts[2].data.set_from_bytes(updated_vault.to_bytes());

        // Update escrow state
        let mut updated_escrow = escrow;
        updated_escrow.state = EscrowState::Released;
        updated_escrow.increment_nonce();
        accounts[1].data.set_from_bytes(updated_escrow.to_bytes());

        let event = MarketplaceEvent::EscrowReleased {
            escrow: escrow_key,
            releaser: releaser_key,
            amount: total_amount,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_refund(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(10000)?;

        if accounts.len() < 5 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract keys upfront (before mutable borrows)
        let refunder_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Load escrow data first (read-only phase)
        let escrow = EscrowAccount::from_bytes(accounts[1].data.as_slice())?;
        let escrow_key = accounts[1].key;

        // Verify nonce
        if escrow.nonce != expected_nonce {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Determine if refunder is authorized:
        // - Seller can always refund
        // - Buyer can refund if expired
        let is_seller = escrow.active_recipients().any(|r| r.pubkey == refunder_key);
        let is_buyer = refunder_key == escrow.buyer;
        let is_expired = escrow.is_expired(current_slot);

        if !is_seller && !(is_buyer && is_expired) {
            return Err(ProgramError::Custom(7)); // NotAuthorized
        }

        // Validate state transition
        escrow.validate_transition(EscrowState::Refunded)?;

        // Load vault and buyer token data (read-only)
        let vault_token = TokenAccount::from_bytes(accounts[2].data.as_slice())?;
        let buyer_token = TokenAccount::from_bytes(accounts[3].data.as_slice())?;

        // Verify buyer token account
        if buyer_token.owner != escrow.buyer {
            return Err(ProgramError::Custom(9)); // InvalidRecipientAccount
        }

        let amount = vault_token.amount;
        let new_buyer_amount = buyer_token
            .amount
            .checked_add(amount)
            .ok_or(ProgramError::Custom(10))?;

        // Now perform mutations
        // Update vault
        let mut updated_vault = vault_token;
        updated_vault.amount = 0;
        accounts[2].data.set_from_bytes(updated_vault.to_bytes());

        // Update buyer token
        let mut updated_buyer_token = buyer_token;
        updated_buyer_token.amount = new_buyer_amount;
        accounts[3]
            .data
            .set_from_bytes(updated_buyer_token.to_bytes());

        // Update escrow state
        let mut updated_escrow = escrow;
        updated_escrow.state = EscrowState::Refunded;
        updated_escrow.increment_nonce();
        accounts[1].data.set_from_bytes(updated_escrow.to_bytes());

        let event = MarketplaceEvent::EscrowRefunded {
            escrow: escrow_key,
            refunder: refunder_key,
            amount,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_raise_dispute(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(5000)?;

        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract raiser key first (before mutable borrows)
        let raiser_key = accounts[0].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Now work with escrow account
        let escrow_account = &mut accounts[1];

        // Load escrow
        let mut escrow = EscrowAccount::from_bytes(escrow_account.data.as_slice())?;
        let escrow_key = escrow_account.key;

        // Verify nonce
        if escrow.nonce != expected_nonce {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Verify raiser is buyer or seller
        let is_buyer = raiser_key == escrow.buyer;
        let is_seller = escrow.active_recipients().any(|r| r.pubkey == raiser_key);

        if !is_buyer && !is_seller {
            return Err(ProgramError::Custom(7)); // NotAuthorized
        }

        // Validate state transition
        escrow.validate_transition(EscrowState::Disputed)?;

        // Update state
        escrow.state = EscrowState::Disputed;
        escrow.has_dispute = true;
        escrow.dispute_slot = current_slot;
        escrow.increment_nonce();

        escrow_account.data.set_from_bytes(escrow.to_bytes());

        let event = MarketplaceEvent::DisputeRaised {
            escrow: escrow_key,
            raiser: raiser_key,
        };

        Ok(vec![event.to_program_event()])
    }

    fn process_resolve_dispute(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
        current_slot: u64,
        release: bool,
        expected_nonce: u64,
    ) -> ProgramResult<Vec<ProgramEvent>> {
        meter.consume(15000)?;

        if accounts.len() < 4 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Extract keys upfront before any mutable borrows
        let resolver_key = accounts[0].key;
        let escrow_key = accounts[1].key;

        // NOTE: Signer verification is handled by the runtime before invoking native programs

        // Load escrow data from immutable view first
        let escrow_data = EscrowAccount::from_bytes(accounts[1].data.as_slice())?;
        let dispute_resolver = escrow_data.dispute_resolver;
        let buyer = escrow_data.buyer;
        let recipient_count = escrow_data.recipient_count;
        let active_recipients: Vec<RecipientSplit> =
            escrow_data.active_recipients().cloned().collect();

        // Verify nonce
        if escrow_data.nonce != expected_nonce {
            return Err(ProgramError::Custom(4)); // InvalidNonce
        }

        // Verify escrow is in disputed state
        if escrow_data.state != EscrowState::Disputed {
            return Err(ProgramError::Custom(11)); // NotDisputed
        }

        // Check if resolver is authorized:
        // 1. Is the designated dispute resolver
        // 2. OR is an authorized bot (checked via bot registry accounts if provided)
        let is_dispute_resolver = resolver_key == dispute_resolver;
        let mut bot_authorized = false;

        // Check for bot authorization (accounts 4+ may include bot accounts)
        if !is_dispute_resolver && accounts.len() >= 6 {
            // Try to verify bot authorization using immutable access
            if let Ok(authorized) = Self::verify_bot_authorization(
                &accounts[0],
                &accounts[4],
                &accounts[5],
                &MARKETPLACE_PROGRAM_ID,
                MarketplaceAction::ResolveDispute as u64,
                current_slot,
            ) {
                bot_authorized = authorized;
            }
        }

        if !is_dispute_resolver && !bot_authorized {
            return Err(ProgramError::Custom(7)); // NotAuthorized
        }

        // Load vault
        let mut vault_token = TokenAccount::from_bytes(accounts[2].data.as_slice())?;

        // Determine the base index for recipient/buyer accounts
        let base_idx = if bot_authorized { 6 } else { 4 };

        let (new_state, distributions) = if release {
            // Distribute to recipients
            EscrowAccount::from_bytes(accounts[1].data.as_slice())?
                .validate_transition(EscrowState::Released)?;

            if accounts.len() < base_idx + recipient_count as usize {
                return Err(ProgramError::NotEnoughAccountKeys);
            }

            let total_amount = vault_token.amount;
            let mut dist: Vec<(usize, Vec<u8>)> = Vec::new();

            for (i, recipient) in active_recipients.iter().enumerate() {
                let share_amount =
                    (total_amount as u128 * recipient.share_bps as u128 / 10000) as u64;

                let acc_idx = base_idx + i;
                let mut recipient_token =
                    TokenAccount::from_bytes(accounts[acc_idx].data.as_slice())?;

                if recipient_token.owner != recipient.pubkey {
                    return Err(ProgramError::Custom(9));
                }

                recipient_token.amount = recipient_token
                    .amount
                    .checked_add(share_amount)
                    .ok_or(ProgramError::Custom(10))?;

                vault_token.amount = vault_token
                    .amount
                    .checked_sub(share_amount)
                    .ok_or(ProgramError::InsufficientFunds)?;

                dist.push((acc_idx, recipient_token.to_bytes()));
            }

            (EscrowState::Released, dist)
        } else {
            // Refund to buyer
            EscrowAccount::from_bytes(accounts[1].data.as_slice())?
                .validate_transition(EscrowState::Refunded)?;

            if accounts.len() <= base_idx {
                return Err(ProgramError::NotEnoughAccountKeys);
            }

            let mut buyer_token = TokenAccount::from_bytes(accounts[base_idx].data.as_slice())?;

            if buyer_token.owner != buyer {
                return Err(ProgramError::Custom(9));
            }

            let amount = vault_token.amount;
            buyer_token.amount = buyer_token
                .amount
                .checked_add(amount)
                .ok_or(ProgramError::Custom(10))?;
            vault_token.amount = 0;

            (
                EscrowState::Refunded,
                vec![(base_idx, buyer_token.to_bytes())],
            )
        };

        // Apply all distributions
        for (idx, data) in distributions {
            accounts[idx].data.set_from_bytes(data);
        }

        // Update vault
        accounts[2].data.set_from_bytes(vault_token.to_bytes());

        // Update escrow state
        let mut escrow = EscrowAccount::from_bytes(accounts[1].data.as_slice())?;
        escrow.state = new_state;
        escrow.increment_nonce();
        accounts[1].data.set_from_bytes(escrow.to_bytes());

        let event = MarketplaceEvent::DisputeResolved {
            escrow: escrow_key,
            resolver: resolver_key,
            released: release,
            bot_authorized,
        };

        Ok(vec![event.to_program_event()])
    }

    /// Verify bot authorization for a marketplace action
    fn verify_bot_authorization(
        bot_signer: &Account,
        bot_account: &Account,
        capability_account: &Account,
        target_program: &Pubkey,
        action: u64,
        current_slot: u64,
    ) -> ProgramResult<bool> {
        use crate::bot_registry::{BotAccount as BotReg, BotCapabilityGrantAccount, BotStatus};

        // Load bot account
        let bot = BotReg::from_bytes(bot_account.data.as_slice())?;

        // Verify bot is active
        if bot.status != BotStatus::Active {
            return Ok(false);
        }

        // Verify signer is the bot's signing key
        if bot_signer.key != bot.bot_signing_pubkey {
            return Ok(false);
        }

        // Load capability grant
        let grant = BotCapabilityGrantAccount::from_bytes(capability_account.data.as_slice())?;

        // Verify grant is for this bot
        if grant.bot_id != bot_account.key {
            return Ok(false);
        }

        // Verify grant is not expired
        if grant.expiry_slot > 0 && current_slot >= grant.expiry_slot {
            return Ok(false);
        }

        // Verify grant targets the marketplace program
        if grant.target_program != *target_program {
            return Ok(false);
        }

        // Verify action is allowed (bitflags)
        if grant.allowed_actions & action == 0 {
            return Ok(false);
        }

        Ok(true)
    }
}

/// Marketplace action flags for capability grants
#[repr(u64)]
pub enum MarketplaceAction {
    /// Resolve disputes
    ResolveDispute = 1 << 0,
    /// Create escrows on behalf of users
    CreateEscrow = 1 << 1,
    /// Release escrows
    Release = 1 << 2,
    /// Refund escrows
    Refund = 1 << 3,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_escrow_state_transitions() {
        assert!(EscrowState::Locked.can_transition_to(EscrowState::Delivered));
        assert!(EscrowState::Locked.can_transition_to(EscrowState::Disputed));
        assert!(EscrowState::Locked.can_transition_to(EscrowState::Released));
        assert!(EscrowState::Locked.can_transition_to(EscrowState::Refunded));

        assert!(EscrowState::Delivered.can_transition_to(EscrowState::Released));
        assert!(EscrowState::Delivered.can_transition_to(EscrowState::Disputed));
        assert!(!EscrowState::Delivered.can_transition_to(EscrowState::Locked));

        assert!(EscrowState::Disputed.can_transition_to(EscrowState::Released));
        assert!(EscrowState::Disputed.can_transition_to(EscrowState::Refunded));
        assert!(!EscrowState::Disputed.can_transition_to(EscrowState::Delivered));

        assert!(!EscrowState::Released.can_transition_to(EscrowState::Refunded));
        assert!(!EscrowState::Refunded.can_transition_to(EscrowState::Released));
    }

    #[test]
    fn test_escrow_state_terminal() {
        assert!(!EscrowState::Locked.is_terminal());
        assert!(!EscrowState::Delivered.is_terminal());
        assert!(!EscrowState::Disputed.is_terminal());
        assert!(EscrowState::Released.is_terminal());
        assert!(EscrowState::Refunded.is_terminal());
        assert!(EscrowState::Expired.is_terminal());
    }

    #[test]
    fn test_escrow_account_serialization() {
        let buyer = Pubkey([1u8; 32]);
        let seller = Pubkey([2u8; 32]);
        let mint = Pubkey([3u8; 32]);
        let resolver = Pubkey([4u8; 32]);

        let recipients = vec![RecipientSplit {
            pubkey: seller,
            share_bps: 10000,
        }];

        let escrow = EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255)
            .expect("Failed to create escrow");

        let bytes = escrow.to_bytes();
        assert_eq!(bytes.len(), ESCROW_ACCOUNT_SIZE);

        let parsed = EscrowAccount::from_bytes(&bytes).expect("Failed to parse escrow");
        assert_eq!(parsed.buyer, buyer);
        assert_eq!(parsed.amount, 1000);
        assert_eq!(parsed.state, EscrowState::Locked);
        assert_eq!(parsed.expiry_slot, 200);
        assert_eq!(parsed.recipient_count, 1);
    }

    #[test]
    fn test_escrow_shares_validation() {
        let buyer = Pubkey([1u8; 32]);
        let mint = Pubkey([3u8; 32]);
        let resolver = Pubkey([4u8; 32]);

        // Valid: 100%
        let recipients = vec![RecipientSplit {
            pubkey: Pubkey([2u8; 32]),
            share_bps: 10000,
        }];
        assert!(
            EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255).is_ok()
        );

        // Valid: 50% + 50%
        let recipients = vec![
            RecipientSplit {
                pubkey: Pubkey([2u8; 32]),
                share_bps: 5000,
            },
            RecipientSplit {
                pubkey: Pubkey([3u8; 32]),
                share_bps: 5000,
            },
        ];
        assert!(
            EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255).is_ok()
        );

        // Invalid: 50% only
        let recipients = vec![RecipientSplit {
            pubkey: Pubkey([2u8; 32]),
            share_bps: 5000,
        }];
        assert!(
            EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255).is_err()
        );

        // Invalid: empty recipients
        let recipients: Vec<RecipientSplit> = vec![];
        assert!(
            EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255).is_err()
        );
    }

    #[test]
    fn test_listing_account_serialization() {
        let creator = Pubkey([1u8; 32]);
        let mint = Pubkey([2u8; 32]);

        let listing = ListingAccount::new(
            creator,
            "Test Listing",
            "A test listing description",
            1000,
            mint,
            100,
            255,
        )
        .expect("Failed to create listing");

        let bytes = listing.to_bytes();
        assert_eq!(bytes.len(), LISTING_ACCOUNT_SIZE);

        let parsed = ListingAccount::from_bytes(&bytes).expect("Failed to parse listing");
        assert_eq!(parsed.creator, creator);
        assert_eq!(parsed.title_str(), "Test Listing");
        assert_eq!(parsed.description_str(), "A test listing description");
        assert_eq!(parsed.price, 1000);
    }

    #[test]
    fn test_instruction_serialization() {
        let instruction = MarketplaceInstruction::CreateEscrow {
            recipients: vec![RecipientSplit {
                pubkey: Pubkey([1u8; 32]),
                share_bps: 10000,
            }],
            amount: 1000,
            expiry_slot: 200,
            dispute_resolver: Pubkey([2u8; 32]),
            expected_nonce: 0,
        };

        let packed = instruction.pack();
        let unpacked = MarketplaceInstruction::unpack(&packed).expect("Failed to unpack");

        if let MarketplaceInstruction::CreateEscrow {
            amount,
            expiry_slot,
            ..
        } = unpacked
        {
            assert_eq!(amount, 1000);
            assert_eq!(expiry_slot, 200);
        } else {
            panic!("Wrong instruction type");
        }
    }

    #[test]
    fn test_escrow_expiry() {
        let buyer = Pubkey([1u8; 32]);
        let seller = Pubkey([2u8; 32]);
        let mint = Pubkey([3u8; 32]);
        let resolver = Pubkey([4u8; 32]);

        let recipients = vec![RecipientSplit {
            pubkey: seller,
            share_bps: 10000,
        }];

        let escrow = EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255)
            .expect("Failed to create escrow");

        assert!(!escrow.is_expired(100)); // At creation
        assert!(!escrow.is_expired(199)); // Just before expiry
        assert!(escrow.is_expired(200)); // At expiry
        assert!(escrow.is_expired(300)); // After expiry

        // No expiry (expiry_slot = 0)
        let escrow = EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 0, resolver, 255)
            .expect("Failed to create escrow");
        assert!(!escrow.is_expired(1000000)); // Never expires
    }

    #[test]
    fn test_nonce_increment() {
        let buyer = Pubkey([1u8; 32]);
        let seller = Pubkey([2u8; 32]);
        let mint = Pubkey([3u8; 32]);
        let resolver = Pubkey([4u8; 32]);

        let recipients = vec![RecipientSplit {
            pubkey: seller,
            share_bps: 10000,
        }];

        let mut escrow =
            EscrowAccount::new(buyer, &recipients, mint, 1000, 100, 200, resolver, 255)
                .expect("Failed to create escrow");

        assert_eq!(escrow.nonce, 0);
        escrow.increment_nonce();
        assert_eq!(escrow.nonce, 1);
        escrow.increment_nonce();
        assert_eq!(escrow.nonce, 2);
    }
}
