//! Dchat Program Language v1 - Guest SDK
//!
//! This module provides the contract authoring syntax and safety rails for
//! programs compiled to wasm32-unknown-unknown target.
//!
//! # Features
//! - Required entrypoint signature with compatibility shim
//! - Instruction router (tag → handler)
//! - Typed account schema parsing via AccountsCursor
//! - Account wrappers: Signer, Writable, Readonly, ProgramOwned, Pda
//! - Deterministic state load/store helpers
//! - Logging, return data, and event emission
//!
//! # Security
//! - All bounds checks are explicit
//! - No unwrap() in production paths
//! - Deterministic parsing and encoding
//! - PDA derivation matches host exactly

use crate::abi::{
    AbiError, AccountTocEntry, AccountsBlob, EventEnvelope, IxEnvelope, ABI_VERSION,
    ACCOUNTS_BLOB_HEADER_SIZE, MAX_EVENT_DATA_SIZE, TOC_ENTRY_SIZE,
};
use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};
use crate::pda::PdaDerivation;

// ═══════════════════════════════════════════════════════════════════════════════
// ENTRYPOINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result code for successful execution
pub const SUCCESS: u32 = 0;

/// Result code for failure (base - add error code)
pub const ERROR_BASE: u32 = 1;

/// Entrypoint function signature
///
/// Programs must export a function with this signature as "entrypoint" or
/// "process_instruction". The runtime will call this with:
/// - accounts_ptr: pointer to AccountsBlob in linear memory
/// - accounts_len: length of the accounts blob
/// - ix_ptr: pointer to IxEnvelope in linear memory
/// - ix_len: length of the instruction envelope
///
/// Returns 0 on success, non-zero error code on failure.
pub type Entrypoint = fn(accounts_ptr: u32, accounts_len: u32, ix_ptr: u32, ix_len: u32) -> u32;

/// Entrypoint context parsed from raw memory pointers
#[derive(Debug)]
pub struct EntrypointContext<'a> {
    /// Parsed accounts blob
    pub accounts: AccountsCursor<'a>,
    /// Parsed instruction envelope
    pub instruction: ParsedInstruction<'a>,
}

impl<'a> EntrypointContext<'a> {
    /// Parse entrypoint context from raw memory
    ///
    /// # Safety
    /// This function requires valid memory regions at the specified pointers.
    /// In a WASM context, these are provided by the host runtime.
    pub fn parse(accounts_data: &'a mut [u8], ix_data: &'a [u8]) -> ProgramResult<Self> {
        let accounts = AccountsCursor::parse(accounts_data)?;
        let instruction = ParsedInstruction::parse(ix_data)?;

        Ok(Self {
            accounts,
            instruction,
        })
    }
}

/// Parsed instruction from IxEnvelope
#[derive(Debug, Clone)]
pub struct ParsedInstruction<'a> {
    /// Instruction tag/discriminator
    pub tag: u16,
    /// Instruction payload
    pub payload: &'a [u8],
    /// ABI version
    pub version: u16,
}

impl<'a> ParsedInstruction<'a> {
    /// Parse from raw bytes
    pub fn parse(data: &'a [u8]) -> ProgramResult<Self> {
        let envelope = IxEnvelope::decode(data)?;
        Ok(Self {
            tag: envelope.tag,
            payload: &data[12..12 + envelope.payload.len()],
            version: envelope.version,
        })
    }

    /// Deserialize payload to typed data using deterministic format
    pub fn deserialize_payload<T: PayloadDeserialize>(&self) -> ProgramResult<T> {
        T::deserialize(self.payload)
    }
}

/// Trait for deserializing instruction payloads
pub trait PayloadDeserialize: Sized {
    /// Deserialize from bytes
    fn deserialize(data: &[u8]) -> ProgramResult<Self>;
}

/// Trait for serializing instruction payloads
pub trait PayloadSerialize {
    /// Serialize to bytes
    fn serialize(&self) -> Vec<u8>;
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION ROUTER
// ═══════════════════════════════════════════════════════════════════════════════

/// Instruction handler function type
pub type InstructionHandler<Ctx> =
    fn(&mut Ctx, &mut AccountsCursor<'_>, &[u8]) -> ProgramResult<()>;

/// Instruction router for tag-based dispatch
///
/// Maps instruction tags to handler functions for clean program structure.
#[derive(Default)]
pub struct InstructionRouter<Ctx> {
    /// Tag → handler mappings
    handlers: Vec<(u16, InstructionHandler<Ctx>)>,
    /// Fallback handler for unknown tags
    fallback: Option<fn(u16) -> ProgramError>,
}

impl<Ctx> InstructionRouter<Ctx> {
    /// Create a new router
    pub fn new() -> Self {
        Self {
            handlers: Vec::new(),
            fallback: None,
        }
    }

    /// Register a handler for a specific tag
    pub fn register(mut self, tag: u16, handler: InstructionHandler<Ctx>) -> Self {
        self.handlers.push((tag, handler));
        self
    }

    /// Set fallback error for unknown tags (optional)
    pub fn fallback(mut self, f: fn(u16) -> ProgramError) -> Self {
        self.fallback = Some(f);
        self
    }

    /// Dispatch an instruction to the appropriate handler
    pub fn dispatch(
        &self,
        ctx: &mut Ctx,
        accounts: &mut AccountsCursor<'_>,
        tag: u16,
        payload: &[u8],
    ) -> ProgramResult<()> {
        for (registered_tag, handler) in &self.handlers {
            if *registered_tag == tag {
                return handler(ctx, accounts, payload);
            }
        }

        // Unknown tag
        if let Some(fallback) = self.fallback {
            Err(fallback(tag))
        } else {
            Err(ProgramError::Custom(AbiError::UnknownTag as u32))
        }
    }

    /// Get list of registered tags (for introspection)
    pub fn registered_tags(&self) -> Vec<u16> {
        self.handlers.iter().map(|(t, _)| *t).collect()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNTS CURSOR
// ═══════════════════════════════════════════════════════════════════════════════

/// Cursor for iterating and accessing accounts from AccountsBlob
#[derive(Debug)]
pub struct AccountsCursor<'a> {
    /// Raw blob data
    data: &'a mut [u8],
    /// Parsed TOC entries
    entries: Vec<AccountTocEntry>,
    /// Current position for iteration
    position: usize,
    /// Data section offset
    data_section_offset: usize,
    /// ABI version
    pub version: u16,
}

impl<'a> AccountsCursor<'a> {
    /// Parse accounts from raw blob data
    pub fn parse(data: &'a mut [u8]) -> ProgramResult<Self> {
        // Decode the blob structure
        let blob = AccountsBlob::decode(data)?;

        // Calculate data section offset
        let account_count = blob.entries.len();
        let offsets_size = account_count * 4;
        let toc_size = account_count * TOC_ENTRY_SIZE;
        let data_section_offset = ACCOUNTS_BLOB_HEADER_SIZE + offsets_size + toc_size;

        Ok(Self {
            data,
            entries: blob.entries,
            position: 0,
            data_section_offset,
            version: blob.version,
        })
    }

    /// Get the number of accounts
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Reset cursor position
    pub fn reset(&mut self) {
        self.position = 0;
    }

    /// Get account at index (immutable view)
    pub fn get(&self, index: usize) -> Option<AccountView<'_>> {
        let entry = self.entries.get(index)?;
        let data_start = self.data_section_offset + entry.data_off as usize;
        let data_end = data_start + entry.data_len as usize;

        if data_end > self.data.len() {
            return None;
        }

        Some(AccountView {
            entry,
            data: &self.data[data_start..data_end],
        })
    }

    /// Get mutable view of account at index
    ///
    /// This method uses the complex split_at_mut approach to safely provide
    /// non-overlapping mutable references to both the TOC entry (for motes)
    /// and the account data section. This prevents undefined behavior from
    /// overlapping mutable borrows.
    ///
    /// # Safety Approach
    ///
    /// The blob layout is: [header][offsets][TOC entries][data section]
    ///
    /// For account at index N:
    /// - TOC entry is at: header_size + offsets_size + (N * TOC_ENTRY_SIZE)
    /// - Data is at: data_section_offset + entry.data_off
    ///
    /// We use split_at_mut to partition the blob into non-overlapping regions:
    /// 1. If TOC region comes before data region: split at data_start
    /// 2. If data region comes before TOC region: split at toc_start
    ///
    /// This ensures no aliasing occurs.
    pub fn get_mut(&mut self, index: usize) -> Option<AccountViewMut<'_>> {
        let entry = self.entries.get(index)?;
        let toc_start = self.toc_offset(index);
        let toc_end = toc_start + TOC_ENTRY_SIZE;
        let data_start = self.data_section_offset + entry.data_off as usize;
        let data_end = data_start + entry.data_len as usize;

        // Validate bounds
        if data_end > self.data.len() || toc_end > self.data.len() {
            return None;
        }

        // Capture entry values before borrowing data
        let is_writable = entry.is_writable;
        let data_len = entry.data_len;
        let entry_pubkey = entry.pubkey;
        let entry_owner = entry.owner;
        let entry_motes = entry.motes;
        let entry_is_signer = entry.is_signer;
        let entry_executable = entry.executable;
        let entry_rent_epoch = entry.rent_epoch;

        // COMPLEX APPROACH: Use split_at_mut to get non-overlapping mutable slices
        // The TOC is always before the data section in the blob layout
        // Layout: [header(8)][offsets(n*4)][TOC(n*89)][data_section]
        // So toc_end <= data_section_offset <= data_start

        // Verify TOC and data don't overlap (they shouldn't by construction)
        if toc_end > data_start {
            // This would mean the layout is corrupted or our calculation is wrong
            return None;
        }

        // Split at the boundary between TOC region and data region
        // We need: toc_data = data[toc_start..toc_end], account_data = data[data_start..data_end]
        //
        // Split strategy:
        // 1. Split at toc_start → (before_toc, from_toc_onwards)
        // 2. Split from_toc_onwards at (toc_end - toc_start) → (toc_slice, after_toc)
        // 3. Split after_toc at (data_start - toc_end) → (between, from_data)
        // 4. Split from_data at (data_end - data_start) → (data_slice, _)

        let (before_toc, from_toc) = self.data.split_at_mut(toc_start);
        let toc_len = TOC_ENTRY_SIZE;
        let (toc_slice, after_toc) = from_toc.split_at_mut(toc_len);

        // Calculate offset from after_toc to data_start
        let gap_to_data = data_start - toc_end;
        let (_, from_data) = after_toc.split_at_mut(gap_to_data);

        let data_slice_len = entry.data_len as usize;
        let (data_slice, _) = from_data.split_at_mut(data_slice_len);

        Some(AccountViewMut {
            data: data_slice,
            toc_data: toc_slice,
            is_writable,
            data_len,
            pubkey: entry_pubkey,
            owner: entry_owner,
            motes: entry_motes,
            is_signer: entry_is_signer,
            executable: entry_executable,
            rent_epoch: entry_rent_epoch,
        })
    }

    /// Update motes for an account by index
    ///
    /// This is the safe way to update motes - through the cursor which
    /// owns the entire blob. Uses explicit offset calculation to avoid
    /// overlapping mutable borrows.
    pub fn update_motes(&mut self, index: usize, motes: u64) -> ProgramResult<()> {
        let entry = self
            .entries
            .get(index)
            .ok_or(ProgramError::AccountNotFound)?;
        if !entry.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }

        // Update cached entry
        if let Some(e) = self.entries.get_mut(index) {
            e.motes = motes;
        }

        // Write to TOC (motes is at offset 64 in TOC entry: 32 pubkey + 32 owner)
        let toc_offset = self.toc_offset(index);
        let motes_offset = toc_offset + 64;
        if motes_offset + 8 > self.data.len() {
            return Err(ProgramError::Custom(AbiError::BufferTooSmall as u32));
        }
        self.data[motes_offset..motes_offset + 8].copy_from_slice(&motes.to_le_bytes());

        Ok(())
    }

    /// Calculate TOC offset for an account index
    fn toc_offset(&self, index: usize) -> usize {
        let offsets_size = self.entries.len() * 4;
        ACCOUNTS_BLOB_HEADER_SIZE + offsets_size + (index * TOC_ENTRY_SIZE)
    }

    /// Next account as typed wrapper (signer required)
    pub fn next_signer(&mut self) -> ProgramResult<Signer<'_>> {
        let index = self.position;
        self.position += 1;

        let view = self.get(index).ok_or(ProgramError::AccountNotFound)?;
        if !view.entry.is_signer {
            return Err(ProgramError::MissingRequiredSignature);
        }

        Ok(Signer {
            pubkey: view.entry.pubkey,
            motes: view.entry.motes,
            _lifetime: std::marker::PhantomData,
            data: view.data.to_vec(),
            owner: view.entry.owner,
        })
    }

    /// Next account as writable
    ///
    /// Uses the complex split_at_mut approach to safely provide non-overlapping
    /// mutable references to TOC and data sections.
    pub fn next_writable(&mut self) -> ProgramResult<Writable<'_>> {
        let index = self.position;
        self.position += 1;

        let entry = self
            .entries
            .get(index)
            .ok_or(ProgramError::AccountNotFound)?;
        if !entry.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }

        let toc_start = self.toc_offset(index);
        let toc_end = toc_start + TOC_ENTRY_SIZE;
        let data_start = self.data_section_offset + entry.data_off as usize;
        let data_end = data_start + entry.data_len as usize;

        // Capture entry values
        let entry_pubkey = entry.pubkey;
        let entry_motes = entry.motes;
        let entry_owner = entry.owner;
        let entry_data_len = entry.data_len as usize;

        // Validate layout: TOC must come before data
        if toc_end > data_start || data_end > self.data.len() {
            return Err(ProgramError::InvalidAccountData);
        }

        // Complex split_at_mut approach for non-overlapping access
        let (_, from_toc) = self.data.split_at_mut(toc_start);
        let (toc_slice, after_toc) = from_toc.split_at_mut(TOC_ENTRY_SIZE);
        let gap_to_data = data_start - toc_end;
        let (_, from_data) = after_toc.split_at_mut(gap_to_data);
        let (data_slice, _) = from_data.split_at_mut(entry_data_len);

        Ok(Writable {
            pubkey: entry_pubkey,
            motes: entry_motes,
            owner: entry_owner,
            data: data_slice,
            toc_data: toc_slice,
            index,
        })
    }

    /// Next account as readonly
    pub fn next_readonly(&mut self) -> ProgramResult<Readonly<'_>> {
        let index = self.position;
        self.position += 1;

        let view = self.get(index).ok_or(ProgramError::AccountNotFound)?;

        Ok(Readonly {
            pubkey: view.entry.pubkey,
            motes: view.entry.motes,
            data: view.data.to_vec(),
            owner: view.entry.owner,
            _lifetime: std::marker::PhantomData,
        })
    }

    /// Next account that must be owned by given program
    ///
    /// Uses the complex split_at_mut approach for safe mutable access.
    pub fn next_program_owned(
        &mut self,
        expected_owner: &Pubkey,
    ) -> ProgramResult<ProgramOwned<'_>> {
        let index = self.position;
        self.position += 1;

        let entry = self
            .entries
            .get(index)
            .ok_or(ProgramError::AccountNotFound)?;
        if entry.owner != *expected_owner {
            return Err(ProgramError::InvalidAccountOwner);
        }

        let toc_start = self.toc_offset(index);
        let toc_end = toc_start + TOC_ENTRY_SIZE;
        let data_start = self.data_section_offset + entry.data_off as usize;
        let data_end = data_start + entry.data_len as usize;

        // Capture entry values
        let entry_pubkey = entry.pubkey;
        let entry_motes = entry.motes;
        let entry_owner = entry.owner;
        let entry_data_len = entry.data_len as usize;
        let is_writable = entry.is_writable;

        // Validate layout
        if toc_end > data_start || data_end > self.data.len() {
            return Err(ProgramError::InvalidAccountData);
        }

        // Complex split_at_mut approach
        let (_, from_toc) = self.data.split_at_mut(toc_start);
        let (toc_slice, after_toc) = from_toc.split_at_mut(TOC_ENTRY_SIZE);
        let gap_to_data = data_start - toc_end;
        let (_, from_data) = after_toc.split_at_mut(gap_to_data);
        let (data_slice, _) = from_data.split_at_mut(entry_data_len);

        Ok(ProgramOwned {
            pubkey: entry_pubkey,
            motes: entry_motes,
            owner: entry_owner,
            data: data_slice,
            toc_data: toc_slice,
            is_writable,
            index,
        })
    }

    /// Next account as PDA (verifies derivation matches host)
    ///
    /// Verifies the account matches the PDA derivation exactly as the host would
    /// compute it. Uses the complex split_at_mut approach for safe mutable access.
    pub fn next_pda(&mut self, seeds: &[&[u8]], program_id: &Pubkey) -> ProgramResult<Pda<'_>> {
        let index = self.position;
        self.position += 1;

        let entry = self
            .entries
            .get(index)
            .ok_or(ProgramError::AccountNotFound)?;

        // Derive PDA and verify it matches - MUST match host PdaDerivation exactly
        let derived = PdaDerivation::find_program_address(seeds, program_id)?;
        if derived.address != entry.pubkey {
            return Err(ProgramError::InvalidSeeds);
        }

        let toc_start = self.toc_offset(index);
        let toc_end = toc_start + TOC_ENTRY_SIZE;
        let data_start = self.data_section_offset + entry.data_off as usize;
        let data_end = data_start + entry.data_len as usize;

        // Capture entry values
        let entry_pubkey = entry.pubkey;
        let entry_motes = entry.motes;
        let entry_owner = entry.owner;
        let entry_data_len = entry.data_len as usize;
        let is_writable = entry.is_writable;
        let bump = derived.bump;

        // Validate layout
        if toc_end > data_start || data_end > self.data.len() {
            return Err(ProgramError::InvalidAccountData);
        }

        // Complex split_at_mut approach
        let (_, from_toc) = self.data.split_at_mut(toc_start);
        let (toc_slice, after_toc) = from_toc.split_at_mut(TOC_ENTRY_SIZE);
        let gap_to_data = data_start - toc_end;
        let (_, from_data) = after_toc.split_at_mut(gap_to_data);
        let (data_slice, _) = from_data.split_at_mut(entry_data_len);

        Ok(Pda {
            pubkey: entry_pubkey,
            motes: entry_motes,
            owner: entry_owner,
            data: data_slice,
            toc_data: toc_slice,
            bump,
            is_writable,
            index,
        })
    }

    /// Get remaining account count
    pub fn remaining(&self) -> usize {
        self.entries.len().saturating_sub(self.position)
    }

    /// Skip n accounts
    pub fn skip(&mut self, n: usize) {
        self.position = (self.position + n).min(self.entries.len());
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT VIEWS
// ═══════════════════════════════════════════════════════════════════════════════

/// Immutable view of an account
#[derive(Debug)]
pub struct AccountView<'a> {
    /// TOC entry
    pub entry: &'a AccountTocEntry,
    /// Account data
    pub data: &'a [u8],
}

impl<'a> AccountView<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.entry.pubkey
    }

    /// Get motes
    pub fn motes(&self) -> u64 {
        self.entry.motes
    }

    /// Get owner
    pub fn owner(&self) -> &Pubkey {
        &self.entry.owner
    }

    /// Is signer
    pub fn is_signer(&self) -> bool {
        self.entry.is_signer
    }

    /// Is writable
    pub fn is_writable(&self) -> bool {
        self.entry.is_writable
    }

    /// Is executable
    pub fn is_executable(&self) -> bool {
        self.entry.executable
    }
}

/// Mutable view of an account
#[derive(Debug)]
pub struct AccountViewMut<'a> {
    /// Account data (mutable)
    pub data: &'a mut [u8],
    /// TOC data (for updating motes)
    toc_data: &'a mut [u8],
    /// Is writable
    pub is_writable: bool,
    /// Data length
    pub data_len: u32,
    /// Public key
    pub pubkey: Pubkey,
    /// Owner
    pub owner: Pubkey,
    /// Motes
    pub motes: u64,
    /// Is signer
    pub is_signer: bool,
    /// Is executable
    pub executable: bool,
    /// Rent epoch
    pub rent_epoch: u64,
}

impl<'a> AccountViewMut<'a> {
    /// Update motes in the TOC (writes back to blob)
    pub fn set_motes(&mut self, motes: u64) -> ProgramResult<()> {
        if !self.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }

        // Update in-memory value
        self.motes = motes;

        // Write to TOC (motes is at offset 64 in TOC entry)
        let motes_offset = 64; // 32 (pubkey) + 32 (owner)
        self.toc_data[motes_offset..motes_offset + 8].copy_from_slice(&motes.to_le_bytes());

        Ok(())
    }

    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TYPED ACCOUNT WRAPPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Account that is a signer
#[derive(Debug)]
pub struct Signer<'a> {
    /// Public key
    pub pubkey: Pubkey,
    /// Motes
    pub motes: u64,
    /// Data (copied)
    pub data: Vec<u8>,
    /// Owner
    pub owner: Pubkey,
    #[doc(hidden)]
    pub _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> Signer<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }
}

/// Writable account with mutation support
#[derive(Debug)]
pub struct Writable<'a> {
    /// Public key
    pub pubkey: Pubkey,
    /// Motes
    pub motes: u64,
    /// Owner
    pub owner: Pubkey,
    /// Mutable data reference
    pub data: &'a mut [u8],
    /// TOC data for motes updates
    toc_data: &'a mut [u8],
    /// Index in cursor
    index: usize,
}

impl<'a> Writable<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }

    /// Set motes (writes to blob)
    pub fn set_motes(&mut self, motes: u64) -> ProgramResult<()> {
        self.motes = motes;
        let motes_offset = 64;
        self.toc_data[motes_offset..motes_offset + 8].copy_from_slice(&motes.to_le_bytes());
        Ok(())
    }

    /// Subtract motes with underflow check
    pub fn sub_motes(&mut self, amount: u64) -> ProgramResult<()> {
        let new_balance = self
            .motes
            .checked_sub(amount)
            .ok_or(ProgramError::InsufficientFunds)?;
        self.set_motes(new_balance)
    }

    /// Add motes with overflow check
    pub fn add_motes(&mut self, amount: u64) -> ProgramResult<()> {
        let new_balance = self
            .motes
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        self.set_motes(new_balance)
    }

    /// Get account index
    pub fn index(&self) -> usize {
        self.index
    }
}

/// Readonly account
#[derive(Debug)]
pub struct Readonly<'a> {
    /// Public key
    pub pubkey: Pubkey,
    /// Motes
    pub motes: u64,
    /// Data (copied for safety)
    pub data: Vec<u8>,
    /// Owner
    pub owner: Pubkey,
    #[doc(hidden)]
    pub _lifetime: std::marker::PhantomData<&'a ()>,
}

impl<'a> Readonly<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }
}

/// Account owned by a specific program
#[derive(Debug)]
pub struct ProgramOwned<'a> {
    /// Public key
    pub pubkey: Pubkey,
    /// Motes
    pub motes: u64,
    /// Owner
    pub owner: Pubkey,
    /// Mutable data reference
    pub data: &'a mut [u8],
    /// TOC data
    toc_data: &'a mut [u8],
    /// Is writable
    pub is_writable: bool,
    /// Index
    index: usize,
}

impl<'a> ProgramOwned<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }

    /// Set motes if writable
    pub fn set_motes(&mut self, motes: u64) -> ProgramResult<()> {
        if !self.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }
        self.motes = motes;
        let motes_offset = 64;
        self.toc_data[motes_offset..motes_offset + 8].copy_from_slice(&motes.to_le_bytes());
        Ok(())
    }

    /// Get account index
    pub fn index(&self) -> usize {
        self.index
    }
}

/// Program Derived Address account
#[derive(Debug)]
pub struct Pda<'a> {
    /// Public key
    pub pubkey: Pubkey,
    /// Motes
    pub motes: u64,
    /// Owner
    pub owner: Pubkey,
    /// Mutable data reference
    pub data: &'a mut [u8],
    /// TOC data
    toc_data: &'a mut [u8],
    /// Bump seed used in derivation
    pub bump: u8,
    /// Is writable
    pub is_writable: bool,
    /// Index
    index: usize,
}

impl<'a> Pda<'a> {
    /// Get the public key
    pub fn key(&self) -> &Pubkey {
        &self.pubkey
    }

    /// Get bump seed
    pub fn bump(&self) -> u8 {
        self.bump
    }

    /// Set motes if writable
    pub fn set_motes(&mut self, motes: u64) -> ProgramResult<()> {
        if !self.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }
        self.motes = motes;
        let motes_offset = 64;
        self.toc_data[motes_offset..motes_offset + 8].copy_from_slice(&motes.to_le_bytes());
        Ok(())
    }

    /// Get signing seeds (for CPI)
    pub fn signer_seeds<'b>(
        &self,
        base_seeds: &'b [&'b [u8]],
        bump_storage: &'b [u8; 1],
    ) -> Vec<&'b [u8]> {
        let mut seeds = base_seeds.to_vec();
        seeds.push(bump_storage);
        seeds
    }

    /// Get account index
    pub fn index(&self) -> usize {
        self.index
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// STATE HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// State loading and storing with deterministic serialization
pub mod state {
    use super::*;

    /// Load state from account data
    ///
    /// Uses deterministic little-endian encoding for fixed-size types.
    pub fn load_u64(data: &[u8], offset: usize) -> ProgramResult<u64> {
        if offset + 8 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        let bytes: [u8; 8] = data[offset..offset + 8]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(u64::from_le_bytes(bytes))
    }

    /// Store u64 to account data
    pub fn store_u64(data: &mut [u8], offset: usize, value: u64) -> ProgramResult<()> {
        if offset + 8 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Load u32
    pub fn load_u32(data: &[u8], offset: usize) -> ProgramResult<u32> {
        if offset + 4 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        let bytes: [u8; 4] = data[offset..offset + 4]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(u32::from_le_bytes(bytes))
    }

    /// Store u32
    pub fn store_u32(data: &mut [u8], offset: usize, value: u32) -> ProgramResult<()> {
        if offset + 4 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Load u16
    pub fn load_u16(data: &[u8], offset: usize) -> ProgramResult<u16> {
        if offset + 2 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        let bytes: [u8; 2] = data[offset..offset + 2]
            .try_into()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Ok(u16::from_le_bytes(bytes))
    }

    /// Store u16
    pub fn store_u16(data: &mut [u8], offset: usize, value: u16) -> ProgramResult<()> {
        if offset + 2 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
        Ok(())
    }

    /// Load u8
    pub fn load_u8(data: &[u8], offset: usize) -> ProgramResult<u8> {
        data.get(offset)
            .copied()
            .ok_or(ProgramError::InvalidAccountData)
    }

    /// Store u8
    pub fn store_u8(data: &mut [u8], offset: usize, value: u8) -> ProgramResult<()> {
        if offset >= data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[offset] = value;
        Ok(())
    }

    /// Load bool (0 = false, non-zero = true)
    pub fn load_bool(data: &[u8], offset: usize) -> ProgramResult<bool> {
        Ok(load_u8(data, offset)? != 0)
    }

    /// Store bool
    pub fn store_bool(data: &mut [u8], offset: usize, value: bool) -> ProgramResult<()> {
        store_u8(data, offset, value as u8)
    }

    /// Load pubkey
    pub fn load_pubkey(data: &[u8], offset: usize) -> ProgramResult<Pubkey> {
        if offset + 32 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(&data[offset..offset + 32]);
        Ok(Pubkey(bytes))
    }

    /// Store pubkey
    pub fn store_pubkey(data: &mut [u8], offset: usize, pubkey: &Pubkey) -> ProgramResult<()> {
        if offset + 32 > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[offset..offset + 32].copy_from_slice(&pubkey.0);
        Ok(())
    }

    /// Load bytes with length prefix (u32)
    pub fn load_bytes(data: &[u8], offset: usize) -> ProgramResult<Vec<u8>> {
        let len = load_u32(data, offset)? as usize;
        let start = offset + 4;
        let end = start + len;
        if end > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        Ok(data[start..end].to_vec())
    }

    /// Store bytes with length prefix
    pub fn store_bytes(data: &mut [u8], offset: usize, bytes: &[u8]) -> ProgramResult<usize> {
        let len = bytes.len() as u32;
        store_u32(data, offset, len)?;
        let start = offset + 4;
        let end = start + bytes.len();
        if end > data.len() {
            return Err(ProgramError::InvalidAccountData);
        }
        data[start..end].copy_from_slice(bytes);
        Ok(end)
    }

    /// Calculate hash of account data (for receipts)
    pub fn hash_data(data: &[u8]) -> [u8; 32] {
        *blake3::hash(data).as_bytes()
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOGGING AND RETURN DATA
// ═══════════════════════════════════════════════════════════════════════════════

/// Logging helpers for guest programs
///
/// These wrap the host syscalls with safe interfaces.
pub mod log {
    /// Log a message (calls sol_log_)
    ///
    /// In WASM context, this writes to the message buffer and calls the host.
    /// In native test context, this writes to stderr.
    pub fn msg(message: &str) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            extern "C" {
                fn sol_log_(ptr: *const u8, len: u64);
            }
            sol_log_(message.as_ptr(), message.len() as u64);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("[LOG] {}", message);
        }
    }

    /// Log formatted values
    pub fn values(v1: u64, v2: u64, v3: u64, v4: u64, v5: u64) {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            extern "C" {
                fn sol_log_64_(v1: u64, v2: u64, v3: u64, v4: u64, v5: u64);
            }
            sol_log_64_(v1, v2, v3, v4, v5);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("[LOG64] {} {} {} {} {}", v1, v2, v3, v4, v5);
        }
    }

    /// Log remaining compute units
    pub fn compute_units() {
        #[cfg(target_arch = "wasm32")]
        unsafe {
            extern "C" {
                fn sol_log_compute_units_();
            }
            sol_log_compute_units_();
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!("[LOG] Compute units check (native mode)");
        }
    }
}

/// Return data helpers
pub mod return_data {
    use super::*;

    /// Set return data for the caller
    pub fn set(data: &[u8]) -> ProgramResult<()> {
        if data.len() > 1024 {
            return Err(ProgramError::Custom(
                super::AbiError::PayloadTooLarge as u32,
            ));
        }

        #[cfg(target_arch = "wasm32")]
        unsafe {
            extern "C" {
                fn sol_set_return_data(ptr: *const u8, len: u64);
            }
            sol_set_return_data(data.as_ptr(), data.len() as u64);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            let _ = data; // Suppress warning
        }

        Ok(())
    }

    /// Set return data with typed serialization
    pub fn set_typed<T: super::PayloadSerialize>(value: &T) -> ProgramResult<()> {
        let bytes = value.serialize();
        set(&bytes)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT EMISSION
// ═══════════════════════════════════════════════════════════════════════════════

/// Event emission for guest programs
pub mod events {
    use super::*;

    /// Emit an event with typed data
    ///
    /// Events are logged using the deterministic EventEnvelope format
    /// and collected by the host for inclusion in receipts.
    pub fn emit(program_id: &Pubkey, name: &str, data: &[u8]) -> ProgramResult<()> {
        if data.len() > MAX_EVENT_DATA_SIZE as usize {
            return Err(ProgramError::Custom(AbiError::PayloadTooLarge as u32));
        }

        let envelope = EventEnvelope::new(*program_id, name, data.to_vec())?;
        let encoded = envelope.encode();

        // Emit via syscall
        #[cfg(target_arch = "wasm32")]
        unsafe {
            extern "C" {
                fn sol_emit_event_(ptr: *const u8, len: u64);
            }
            sol_emit_event_(encoded.as_ptr(), encoded.len() as u64);
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            eprintln!(
                "[EVENT] program={} name={} data_len={}",
                program_id.to_base58(),
                name,
                data.len()
            );
            let _ = encoded; // Suppress warning
        }

        Ok(())
    }

    /// Emit event with discriminator only (no additional data)
    pub fn emit_simple(program_id: &Pubkey, name: &str) -> ProgramResult<()> {
        emit(program_id, name, &[])
    }

    /// Compute event discriminator for matching
    pub fn discriminator(name: &str) -> [u8; 8] {
        EventEnvelope::compute_discriminator(name)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROGRAM MACRO HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Helper to create a standard entrypoint wrapper
#[macro_export]
macro_rules! entrypoint {
    ($handler:ident) => {
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub unsafe extern "C" fn entrypoint(
            accounts_ptr: u32,
            accounts_len: u32,
            ix_ptr: u32,
            ix_len: u32,
        ) -> u32 {
            // Create slices from pointers
            let accounts_data =
                std::slice::from_raw_parts_mut(accounts_ptr as *mut u8, accounts_len as usize);
            let ix_data = std::slice::from_raw_parts(ix_ptr as *const u8, ix_len as usize);

            // Parse and dispatch
            match $crate::guest::EntrypointContext::parse(accounts_data, ix_data) {
                Ok(mut ctx) => match $handler(&mut ctx) {
                    Ok(()) => $crate::guest::SUCCESS,
                    Err(e) => e.to_code(),
                },
                Err(e) => e.to_code(),
            }
        }

        // Also export as process_instruction for compatibility
        #[cfg(target_arch = "wasm32")]
        #[no_mangle]
        pub unsafe extern "C" fn process_instruction(
            accounts_ptr: u32,
            accounts_len: u32,
            ix_ptr: u32,
            ix_len: u32,
        ) -> u32 {
            entrypoint(accounts_ptr, accounts_len, ix_ptr, ix_len)
        }
    };
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::abi::SerializableAccount;

    fn create_test_accounts() -> Vec<SerializableAccount> {
        vec![
            SerializableAccount {
                pubkey: Pubkey::new([1u8; 32]),
                owner: Pubkey::new([10u8; 32]),
                motes: 1000,
                data: vec![1, 2, 3, 4],
                is_signer: true,
                is_writable: true,
                executable: false,
                rent_epoch: 0,
            },
            SerializableAccount {
                pubkey: Pubkey::new([2u8; 32]),
                owner: Pubkey::new([20u8; 32]),
                motes: 2000,
                data: vec![5, 6, 7, 8],
                is_signer: false,
                is_writable: true,
                executable: false,
                rent_epoch: 0,
            },
            SerializableAccount {
                pubkey: Pubkey::new([3u8; 32]),
                owner: Pubkey::new([30u8; 32]),
                motes: 3000,
                data: vec![9, 10],
                is_signer: false,
                is_writable: false,
                executable: false,
                rent_epoch: 0,
            },
        ]
    }

    #[test]
    fn test_accounts_cursor_parsing() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let cursor = AccountsCursor::parse(&mut encoded).unwrap();
        assert_eq!(cursor.len(), 3);
        assert_eq!(cursor.version, ABI_VERSION);
    }

    #[test]
    fn test_accounts_cursor_get() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let cursor = AccountsCursor::parse(&mut encoded).unwrap();

        let acc0 = cursor.get(0).unwrap();
        assert_eq!(acc0.entry.pubkey, Pubkey::new([1u8; 32]));
        assert_eq!(acc0.motes(), 1000);
        assert!(acc0.is_signer());
        assert_eq!(acc0.data, &[1, 2, 3, 4]);

        let acc2 = cursor.get(2).unwrap();
        assert!(!acc2.is_signer());
        assert!(!acc2.is_writable());
    }

    #[test]
    fn test_accounts_cursor_next_signer() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

        let signer = cursor.next_signer().unwrap();
        assert_eq!(signer.pubkey, Pubkey::new([1u8; 32]));
        assert_eq!(signer.motes, 1000);
    }

    #[test]
    fn test_accounts_cursor_next_signer_fails_on_non_signer() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

        // Skip the signer
        cursor.skip(1);

        // Should fail - account 1 is not a signer
        let result = cursor.next_signer();
        assert!(matches!(
            result,
            Err(ProgramError::MissingRequiredSignature)
        ));
    }

    #[test]
    fn test_accounts_cursor_next_writable() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

        let mut writable = cursor.next_writable().unwrap();
        assert_eq!(writable.pubkey, Pubkey::new([1u8; 32]));
        assert_eq!(writable.motes, 1000);

        // Modify motes
        writable.set_motes(500).unwrap();
        assert_eq!(writable.motes, 500);
    }

    #[test]
    fn test_accounts_cursor_next_readonly() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();
        cursor.skip(2); // Skip to readonly account

        let readonly = cursor.next_readonly().unwrap();
        assert_eq!(readonly.pubkey, Pubkey::new([3u8; 32]));
        assert_eq!(readonly.motes, 3000);
    }

    #[test]
    fn test_instruction_router() {
        struct TestCtx {
            called_tag: Option<u16>,
        }

        fn handler1(
            ctx: &mut TestCtx,
            _accounts: &mut AccountsCursor<'_>,
            _payload: &[u8],
        ) -> ProgramResult<()> {
            ctx.called_tag = Some(1);
            Ok(())
        }

        fn handler2(
            ctx: &mut TestCtx,
            _accounts: &mut AccountsCursor<'_>,
            _payload: &[u8],
        ) -> ProgramResult<()> {
            ctx.called_tag = Some(2);
            Ok(())
        }

        let router = InstructionRouter::<TestCtx>::new()
            .register(1, handler1)
            .register(2, handler2);

        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();
        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();

        let mut ctx = TestCtx { called_tag: None };

        router.dispatch(&mut ctx, &mut cursor, 1, &[]).unwrap();
        assert_eq!(ctx.called_tag, Some(1));

        ctx.called_tag = None;
        router.dispatch(&mut ctx, &mut cursor, 2, &[]).unwrap();
        assert_eq!(ctx.called_tag, Some(2));
    }

    #[test]
    fn test_instruction_router_unknown_tag() {
        struct TestCtx;

        let router = InstructionRouter::<TestCtx>::new();

        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();
        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();
        let mut ctx = TestCtx;

        let result = router.dispatch(&mut ctx, &mut cursor, 999, &[]);
        assert!(result.is_err());
    }

    #[test]
    fn test_state_helpers() {
        let mut data = vec![0u8; 100];

        // u64
        state::store_u64(&mut data, 0, 12345678901234567890).unwrap();
        assert_eq!(state::load_u64(&data, 0).unwrap(), 12345678901234567890);

        // u32
        state::store_u32(&mut data, 8, 1234567890).unwrap();
        assert_eq!(state::load_u32(&data, 8).unwrap(), 1234567890);

        // u16
        state::store_u16(&mut data, 12, 65535).unwrap();
        assert_eq!(state::load_u16(&data, 12).unwrap(), 65535);

        // u8
        state::store_u8(&mut data, 14, 255).unwrap();
        assert_eq!(state::load_u8(&data, 14).unwrap(), 255);

        // bool
        state::store_bool(&mut data, 15, true).unwrap();
        assert!(state::load_bool(&data, 15).unwrap());

        // pubkey
        let pk = Pubkey::new([42u8; 32]);
        state::store_pubkey(&mut data, 16, &pk).unwrap();
        assert_eq!(state::load_pubkey(&data, 16).unwrap(), pk);

        // bytes
        let bytes = vec![1, 2, 3, 4, 5];
        state::store_bytes(&mut data, 48, &bytes).unwrap();
        assert_eq!(state::load_bytes(&data, 48).unwrap(), bytes);
    }

    #[test]
    fn test_state_bounds_checking() {
        let mut data = vec![0u8; 4];

        // Should fail - not enough space
        assert!(state::load_u64(&data, 0).is_err());
        assert!(state::store_u64(&mut data, 0, 0).is_err());

        // Should work
        assert!(state::load_u32(&data, 0).is_ok());
        assert!(state::store_u32(&mut data, 0, 0).is_ok());
    }

    #[test]
    fn test_parsed_instruction() {
        let envelope = IxEnvelope::new(42, vec![1, 2, 3]).unwrap();
        let encoded = envelope.encode();

        let parsed = ParsedInstruction::parse(&encoded).unwrap();
        assert_eq!(parsed.tag, 42);
        assert_eq!(parsed.version, ABI_VERSION);
        assert_eq!(parsed.payload, &[1, 2, 3]);
    }

    #[test]
    fn test_event_emission() {
        let program_id = Pubkey::new([1u8; 32]);

        // Should not panic in non-WASM context
        events::emit(&program_id, "TestEvent", &[1, 2, 3]).unwrap();
        events::emit_simple(&program_id, "SimpleEvent").unwrap();
    }

    #[test]
    fn test_log_functions() {
        // Should not panic in non-WASM context
        log::msg("Test message");
        log::values(1, 2, 3, 4, 5);
        log::compute_units();
    }

    #[test]
    fn test_writable_sub_add_motes() {
        let accounts = create_test_accounts();
        let blob = AccountsBlob::new(&accounts).unwrap();
        let mut encoded = blob.encode();

        let mut cursor = AccountsCursor::parse(&mut encoded).unwrap();
        let mut writable = cursor.next_writable().unwrap();

        assert_eq!(writable.motes, 1000);

        writable.sub_motes(100).unwrap();
        assert_eq!(writable.motes, 900);

        writable.add_motes(50).unwrap();
        assert_eq!(writable.motes, 950);

        // Should fail - underflow
        let result = writable.sub_motes(1000);
        assert!(matches!(result, Err(ProgramError::InsufficientFunds)));
    }
}
