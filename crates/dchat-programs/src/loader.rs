//! Program loader for deploying, upgrading, and managing programs

use std::collections::HashMap;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::account::{Account, AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::instruction::Instruction;
use crate::metering::ComputeMeter;
use crate::validation::BytecodeValidator;

/// Loader program ID
pub const LOADER_PROGRAM_ID: Pubkey = crate::native_programs::LOADER_PROGRAM_ID;

/// Upgradeable loader program ID (for upgradeable programs)
pub const UPGRADEABLE_LOADER_ID: Pubkey = Pubkey::new([
    0x02, 0x0a, 0xc4, 0x0d, 0x3d, 0x42, 0xcd, 0xd5, 0x6d, 0xab, 0xc3, 0xb8, 0x08, 0x2f, 0x5c, 0x11,
    0x1f, 0x1a, 0xc2, 0xd4, 0x3e, 0x22, 0x79, 0x01, 0x89, 0x44, 0xf5, 0x33, 0x12, 0x9d, 0x13, 0x02,
]);

/// Maximum program size
pub const MAX_PROGRAM_SIZE: usize = 10 * 1024 * 1024; // 10MB

/// Timelock duration for upgrades
pub const UPGRADE_TIMELOCK_SECONDS: u64 = 86400; // 24 hours

/// Loader instruction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LoaderInstruction {
    /// Initialize a buffer account
    InitializeBuffer,

    /// Write to a buffer account
    Write {
        /// Offset in bytes where to write
        offset: u32,
        /// Bytes to write to buffer
        bytes: Vec<u8>,
    },

    /// Deploy a program from buffer
    DeployWithMaxDataLen {
        /// Maximum data length for program
        max_data_len: usize,
    },

    /// Upgrade a program
    Upgrade,

    /// Set authority
    SetAuthority,

    /// Close a buffer or program account
    Close,

    /// Extend program data
    ExtendProgram {
        /// Number of additional bytes to add
        additional_bytes: u32,
    },

    /// Set upgrade authority checked
    SetAuthorityChecked,

    /// Initialize pending upgrade (starts timelock)
    InitializePendingUpgrade,

    /// Finalize pending upgrade (after timelock)
    FinalizePendingUpgrade,

    /// Cancel pending upgrade
    CancelPendingUpgrade,

    /// Freeze program (make non-upgradeable)
    FreezeProgram,
}

impl LoaderInstruction {
    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// Program account state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ProgramAccountState {
    /// Uninitialized
    Uninitialized,
    /// Buffer for uploading
    Buffer {
        /// Authority allowed to write to this buffer
        authority: Option<Pubkey>,
        /// Offset where bytecode data starts
        data_offset: usize,
    },
    /// Active program
    Program {
        /// Address of the program data account
        programdata_address: Pubkey,
    },
    /// Program data account
    ProgramData {
        /// Slot at which program was deployed/upgraded
        slot: u64,
        /// Authority allowed to upgrade this program
        upgrade_authority: Option<Pubkey>,
        /// Whether program is frozen (non-upgradeable)
        frozen: bool,
        /// Pending upgrade awaiting timelock
        pending_upgrade: Option<PendingUpgrade>,
    },
}

impl ProgramAccountState {
    /// Size of state header
    pub const HEADER_SIZE: usize = 45;

    /// Deserialize from bytes
    pub fn from_bytes(data: &[u8]) -> ProgramResult<Self> {
        if data.is_empty() {
            return Ok(Self::Uninitialized);
        }
        bincode::deserialize(data).map_err(|_| ProgramError::InvalidAccountData)
    }

    /// Serialize to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap_or_default()
    }
}

/// Pending upgrade info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingUpgrade {
    /// Buffer containing new program
    pub buffer: Pubkey,
    /// When upgrade can be finalized
    pub unlock_timestamp: u64,
    /// Initiator of upgrade
    pub initiator: Pubkey,
    /// Hash of new program
    pub new_program_hash: [u8; 32],
}

/// Program deployment state tracking
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentState {
    /// Program ID
    pub program_id: Pubkey,
    /// Authority
    pub authority: Pubkey,
    /// Deploy slot
    pub deploy_slot: u64,
    /// Deploy timestamp
    pub deploy_timestamp: u64,
    /// Program hash
    pub program_hash: [u8; 32],
    /// Is frozen (non-upgradeable)
    pub frozen: bool,
    /// Upgrade count
    pub upgrade_count: u32,
    /// Last upgrade slot
    pub last_upgrade_slot: Option<u64>,
}

/// Loader program implementation
pub struct LoaderProgram;

impl LoaderProgram {
    /// Initialize buffer instruction
    pub fn initialize_buffer(buffer: Pubkey, authority: Pubkey) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(buffer, false),
                AccountMeta::new_readonly(authority, false),
            ],
            data: LoaderInstruction::InitializeBuffer.to_bytes(),
        }
    }

    /// Write to buffer instruction
    pub fn write(buffer: Pubkey, authority: Pubkey, offset: u32, bytes: Vec<u8>) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(buffer, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::Write { offset, bytes }.to_bytes(),
        }
    }

    /// Deploy program instruction
    pub fn deploy_with_max_data_len(
        payer: Pubkey,
        programdata: Pubkey,
        program: Pubkey,
        buffer: Pubkey,
        authority: Pubkey,
        max_data_len: usize,
    ) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(payer, true),
                AccountMeta::new(programdata, false),
                AccountMeta::new(program, false),
                AccountMeta::new(buffer, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_CLOCK_ID, false),
                AccountMeta::new_readonly(crate::native_programs::SYSTEM_PROGRAM_ID, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::DeployWithMaxDataLen { max_data_len }.to_bytes(),
        }
    }

    /// Upgrade program instruction
    pub fn upgrade(
        programdata: Pubkey,
        program: Pubkey,
        buffer: Pubkey,
        spill: Pubkey,
        authority: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(programdata, false),
                AccountMeta::new(program, false),
                AccountMeta::new(buffer, false),
                AccountMeta::new(spill, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_RENT_ID, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_CLOCK_ID, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::Upgrade.to_bytes(),
        }
    }

    /// Set authority instruction
    pub fn set_authority(
        account: Pubkey,
        current_authority: Pubkey,
        new_authority: Option<Pubkey>,
    ) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(account, false),
            AccountMeta::new_readonly(current_authority, true),
        ];

        if let Some(new) = new_authority {
            accounts.push(AccountMeta::new_readonly(new, false));
        }

        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts,
            data: LoaderInstruction::SetAuthority.to_bytes(),
        }
    }

    /// Close buffer/program instruction
    pub fn close(
        account: Pubkey,
        recipient: Pubkey,
        authority: Pubkey,
        program: Option<Pubkey>,
    ) -> Instruction {
        let mut accounts = vec![
            AccountMeta::new(account, false),
            AccountMeta::new(recipient, false),
            AccountMeta::new_readonly(authority, true),
        ];

        if let Some(prog) = program {
            accounts.push(AccountMeta::new(prog, false));
        }

        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts,
            data: LoaderInstruction::Close.to_bytes(),
        }
    }

    /// Freeze program instruction (make non-upgradeable)
    pub fn freeze(programdata: Pubkey, authority: Pubkey) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(programdata, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::FreezeProgram.to_bytes(),
        }
    }

    /// Initialize pending upgrade instruction
    pub fn initialize_pending_upgrade(
        programdata: Pubkey,
        buffer: Pubkey,
        authority: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(programdata, false),
                AccountMeta::new_readonly(buffer, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_CLOCK_ID, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::InitializePendingUpgrade.to_bytes(),
        }
    }

    /// Finalize pending upgrade instruction
    pub fn finalize_pending_upgrade(
        programdata: Pubkey,
        program: Pubkey,
        buffer: Pubkey,
        spill: Pubkey,
        authority: Pubkey,
    ) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(programdata, false),
                AccountMeta::new(program, false),
                AccountMeta::new(buffer, false),
                AccountMeta::new(spill, false),
                AccountMeta::new_readonly(crate::native_programs::SYSVAR_CLOCK_ID, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::FinalizePendingUpgrade.to_bytes(),
        }
    }

    /// Cancel pending upgrade instruction
    pub fn cancel_pending_upgrade(programdata: Pubkey, authority: Pubkey) -> Instruction {
        Instruction {
            program_id: UPGRADEABLE_LOADER_ID,
            accounts: vec![
                AccountMeta::new(programdata, false),
                AccountMeta::new_readonly(authority, true),
            ],
            data: LoaderInstruction::CancelPendingUpgrade.to_bytes(),
        }
    }
}

/// Loader program processor
pub struct LoaderProgramProcessor;

impl LoaderProgramProcessor {
    /// Process loader instruction
    pub fn process(
        data: &[u8],
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        let instruction = LoaderInstruction::from_bytes(data)?;

        meter.consume(100)?;

        match instruction {
            LoaderInstruction::InitializeBuffer => Self::process_initialize_buffer(accounts, meter),
            LoaderInstruction::Write { offset, bytes } => {
                Self::process_write(accounts, offset, bytes, meter)
            }
            LoaderInstruction::DeployWithMaxDataLen { max_data_len } => {
                Self::process_deploy(accounts, max_data_len, meter)
            }
            LoaderInstruction::Upgrade => Self::process_upgrade(accounts, meter),
            LoaderInstruction::SetAuthority => Self::process_set_authority(accounts, meter),
            LoaderInstruction::Close => Self::process_close(accounts, meter),
            LoaderInstruction::FreezeProgram => Self::process_freeze(accounts, meter),
            LoaderInstruction::InitializePendingUpgrade => {
                Self::process_init_pending_upgrade(accounts, meter)
            }
            LoaderInstruction::FinalizePendingUpgrade => {
                Self::process_finalize_pending_upgrade(accounts, meter)
            }
            LoaderInstruction::CancelPendingUpgrade => {
                Self::process_cancel_pending_upgrade(accounts, meter)
            }
            _ => Ok(()),
        }
    }

    /// Initialize buffer
    fn process_initialize_buffer(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Read authority key first (Copy type)
        let authority_key = accounts[1].key;

        // Now work with buffer
        let buffer = &mut accounts[0];

        // Check not already initialized
        let state = ProgramAccountState::from_bytes(buffer.data.as_slice())?;
        if !matches!(state, ProgramAccountState::Uninitialized) {
            return Err(ProgramError::AccountAlreadyInitialized);
        }

        // Initialize buffer state
        let new_state = ProgramAccountState::Buffer {
            authority: Some(authority_key),
            data_offset: ProgramAccountState::HEADER_SIZE,
        };

        let state_bytes = new_state.to_bytes();
        if buffer.data.len() < state_bytes.len() {
            buffer.data.resize(state_bytes.len(), 0);
        }
        buffer.data.as_mut_slice()[..state_bytes.len()].copy_from_slice(&state_bytes);

        Ok(())
    }

    /// Write to buffer
    fn process_write(
        accounts: &mut [&mut Account],
        offset: u32,
        bytes: Vec<u8>,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Charge for data
        meter.consume(bytes.len() as u64)?;

        // Read authority key first (Copy type)
        let authority_key = accounts[1].key;

        // Now work with buffer exclusively
        let buffer = &mut accounts[0];

        // Verify buffer state
        let state = ProgramAccountState::from_bytes(buffer.data.as_slice())?;
        let (buffer_authority, data_offset) = match state {
            ProgramAccountState::Buffer {
                authority,
                data_offset,
            } => (authority, data_offset),
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Verify authority
        match buffer_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Write data
        let start = data_offset + offset as usize;
        let end = start + bytes.len();

        if end > buffer.data.len() {
            buffer.data.resize(end, 0);
        }

        buffer.data.as_mut_slice()[start..end].copy_from_slice(&bytes);

        Ok(())
    }

    /// Deploy program
    fn process_deploy(
        accounts: &mut [&mut Account],
        max_data_len: usize,
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 8 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // Charge for deployment
        meter.consume(max_data_len as u64)?;

        // First phase: read all immutable data we need (keys and buffer data)
        let authority_key = accounts[7].key;
        let programdata_key = accounts[1].key;

        // Read buffer state and validate
        let buffer_state = ProgramAccountState::from_bytes(accounts[3].data.as_slice())?;
        let (buffer_authority, data_offset) = match buffer_state {
            ProgramAccountState::Buffer {
                authority,
                data_offset,
            } => (authority, data_offset),
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Verify authority
        match buffer_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Get bytecode and validate before any mutations
        let bytecode = accounts[3].data.as_slice()[data_offset..].to_vec();

        // Validate bytecode
        let validator = BytecodeValidator::new();
        validator
            .validate(&bytecode)
            .map_err(|e| ProgramError::InvalidBytecode(e.to_string()))?;

        // Check size
        if bytecode.len() > MAX_PROGRAM_SIZE {
            return Err(ProgramError::AccountDataTooLarge);
        }

        if bytecode.len() > max_data_len {
            return Err(ProgramError::AccountDataTooLarge);
        }

        // Compute program hash
        let _program_hash: [u8; 32] = blake3::hash(&bytecode).into();

        // Second phase: prepare all new data states
        let program_state = ProgramAccountState::Program {
            programdata_address: programdata_key,
        };
        let program_state_bytes = program_state.to_bytes();

        let slot = 0u64; // Would come from sysvar
        let programdata_state = ProgramAccountState::ProgramData {
            slot,
            upgrade_authority: Some(authority_key),
            frozen: false,
            pending_upgrade: None,
        };

        let state_bytes = programdata_state.to_bytes();
        let total_size = state_bytes.len() + bytecode.len();
        let mut programdata_bytes = vec![0u8; total_size];
        programdata_bytes[..state_bytes.len()].copy_from_slice(&state_bytes);
        programdata_bytes[state_bytes.len()..].copy_from_slice(&bytecode);

        // Third phase: apply all mutations one account at a time using indices
        // Mutate program account (index 2)
        accounts[2].data.set_from_bytes(program_state_bytes);
        accounts[2].owner = UPGRADEABLE_LOADER_ID;
        accounts[2].executable = true;

        // Mutate programdata account (index 1)
        accounts[1].data.set_from_bytes(programdata_bytes);
        accounts[1].owner = UPGRADEABLE_LOADER_ID;

        // Transfer lamports from buffer to payer and close buffer
        let buffer_lamports = accounts[3].lamports;
        accounts[0].lamports += buffer_lamports;
        accounts[3].lamports = 0;
        accounts[3].data.clear();

        Ok(())
    }

    /// Upgrade program
    fn process_upgrade(accounts: &mut [&mut Account], meter: &ComputeMeter) -> ProgramResult<()> {
        if accounts.len() < 7 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read all data needed for validation
        let authority_key = accounts[6].key;

        // Verify programdata state
        let pd_state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;
        let (upgrade_authority, frozen) = match pd_state {
            ProgramAccountState::ProgramData {
                upgrade_authority,
                frozen,
                ..
            } => (upgrade_authority, frozen),
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Check not frozen
        if frozen {
            return Err(ProgramError::ProgramFrozen);
        }

        // Verify authority
        match upgrade_authority {
            Some(auth) if auth == authority_key => {}
            None => return Err(ProgramError::ProgramNotUpgradeable),
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Get new bytecode from buffer
        let buffer_state = ProgramAccountState::from_bytes(accounts[2].data.as_slice())?;
        let data_offset = match buffer_state {
            ProgramAccountState::Buffer { data_offset, .. } => data_offset,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let new_bytecode = accounts[2].data.as_slice()[data_offset..].to_vec();

        // Charge for upgrade
        meter.consume(new_bytecode.len() as u64)?;

        // Validate new bytecode
        let validator = BytecodeValidator::new();
        validator
            .validate(&new_bytecode)
            .map_err(|e| ProgramError::InvalidBytecode(e.to_string()))?;

        // Check size fits
        let state_len = ProgramAccountState::HEADER_SIZE;
        if state_len + new_bytecode.len() > accounts[0].data.len() {
            return Err(ProgramError::AccountDataTooLarge);
        }

        // Second phase: prepare new state
        let new_state = ProgramAccountState::ProgramData {
            slot: 0, // Would come from sysvar
            upgrade_authority: Some(authority_key),
            frozen: false,
            pending_upgrade: None,
        };

        let state_bytes = new_state.to_bytes();

        // Third phase: apply mutations using indices
        accounts[0].data.as_mut_slice()[..state_bytes.len()].copy_from_slice(&state_bytes);
        accounts[0].data.as_mut_slice()[state_bytes.len()..state_bytes.len() + new_bytecode.len()]
            .copy_from_slice(&new_bytecode);

        // Close buffer: transfer lamports to spill
        let buffer_lamports = accounts[2].lamports;
        accounts[3].lamports += buffer_lamports;
        accounts[2].lamports = 0;
        accounts[2].data.clear();

        Ok(())
    }

    /// Set authority
    fn process_set_authority(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read all immutable data
        let current_authority_key = accounts[1].key;
        let new_authority_key = if accounts.len() > 2 {
            Some(accounts[2].key)
        } else {
            None
        };

        // Read current state and validate authority
        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        // Second phase: validate and prepare new state
        let new_state_bytes = match state {
            ProgramAccountState::Buffer {
                authority,
                data_offset,
            } => {
                match authority {
                    Some(auth) if auth == current_authority_key => {}
                    _ => return Err(ProgramError::InvalidUpgradeAuthority),
                }

                let new_state = ProgramAccountState::Buffer {
                    authority: new_authority_key,
                    data_offset,
                };
                new_state.to_bytes()
            }
            ProgramAccountState::ProgramData {
                slot,
                frozen,
                pending_upgrade,
                upgrade_authority,
            } => {
                match upgrade_authority {
                    Some(auth) if auth == current_authority_key => {}
                    _ => return Err(ProgramError::InvalidUpgradeAuthority),
                }

                if frozen {
                    return Err(ProgramError::ProgramFrozen);
                }

                let new_state = ProgramAccountState::ProgramData {
                    slot,
                    upgrade_authority: new_authority_key,
                    frozen,
                    pending_upgrade,
                };
                new_state.to_bytes()
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Third phase: apply mutation
        accounts[0].data.as_mut_slice()[..new_state_bytes.len()].copy_from_slice(&new_state_bytes);

        Ok(())
    }

    /// Close buffer/programdata
    fn process_close(accounts: &mut [&mut Account], _meter: &ComputeMeter) -> ProgramResult<()> {
        if accounts.len() < 3 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read keys and validate
        let authority_key = accounts[2].key;
        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        // Verify authority based on account type
        match state {
            ProgramAccountState::Buffer {
                authority: Some(auth),
                ..
            } => {
                if auth != authority_key {
                    return Err(ProgramError::InvalidUpgradeAuthority);
                }
            }
            ProgramAccountState::ProgramData {
                upgrade_authority: Some(auth),
                ..
            } => {
                if auth != authority_key {
                    return Err(ProgramError::InvalidUpgradeAuthority);
                }
            }
            _ => return Err(ProgramError::InvalidAccountData),
        }

        // Second phase: transfer lamports and close
        let account_lamports = accounts[0].lamports;
        accounts[1].lamports += account_lamports;
        accounts[0].lamports = 0;
        accounts[0].data.clear();

        Ok(())
    }

    /// Freeze program (make non-upgradeable)
    fn process_freeze(accounts: &mut [&mut Account], _meter: &ComputeMeter) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read authority key
        let authority_key = accounts[1].key;

        // Read and validate state
        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        let (slot, upgrade_authority, pending_upgrade) = match state {
            ProgramAccountState::ProgramData {
                slot,
                upgrade_authority,
                pending_upgrade,
                ..
            } => (slot, upgrade_authority, pending_upgrade),
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Verify authority
        match upgrade_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Second phase: prepare and apply frozen state
        let new_state = ProgramAccountState::ProgramData {
            slot,
            upgrade_authority: None, // Remove authority
            frozen: true,
            pending_upgrade,
        };

        let bytes = new_state.to_bytes();
        accounts[0].data.as_mut_slice()[..bytes.len()].copy_from_slice(&bytes);

        Ok(())
    }

    /// Initialize pending upgrade with timelock
    fn process_init_pending_upgrade(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 4 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read all immutable data
        let authority_key = accounts[3].key;
        let buffer_key = accounts[1].key;

        // Read programdata state
        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        let (slot, upgrade_authority, frozen) = match state {
            ProgramAccountState::ProgramData {
                slot,
                upgrade_authority,
                frozen,
                pending_upgrade,
            } => {
                if pending_upgrade.is_some() {
                    return Err(ProgramError::UpgradeAlreadyPending);
                }
                (slot, upgrade_authority, frozen)
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        if frozen {
            return Err(ProgramError::ProgramFrozen);
        }

        // Verify authority
        match upgrade_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Get buffer bytecode hash
        let buffer_state = ProgramAccountState::from_bytes(accounts[1].data.as_slice())?;
        let data_offset = match buffer_state {
            ProgramAccountState::Buffer { data_offset, .. } => data_offset,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let bytecode = &accounts[1].data.as_slice()[data_offset..];
        let new_program_hash: [u8; 32] = blake3::hash(bytecode).into();

        // Set pending upgrade
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        let pending = PendingUpgrade {
            buffer: buffer_key,
            unlock_timestamp: now + UPGRADE_TIMELOCK_SECONDS,
            initiator: authority_key,
            new_program_hash,
        };

        // Second phase: apply mutation
        let new_state = ProgramAccountState::ProgramData {
            slot,
            upgrade_authority,
            frozen,
            pending_upgrade: Some(pending),
        };

        let bytes = new_state.to_bytes();
        accounts[0].data.as_mut_slice()[..bytes.len()].copy_from_slice(&bytes);

        Ok(())
    }

    /// Finalize pending upgrade after timelock
    fn process_finalize_pending_upgrade(
        accounts: &mut [&mut Account],
        meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 6 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read all immutable data and validate
        let authority_key = accounts[5].key;
        let buffer_key = accounts[2].key;

        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        let (slot, upgrade_authority, pending) = match state {
            ProgramAccountState::ProgramData {
                slot,
                upgrade_authority,
                pending_upgrade,
                ..
            } => match pending_upgrade {
                Some(p) => (slot, upgrade_authority, p),
                None => return Err(ProgramError::NoUpgradePending),
            },
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Verify authority
        match upgrade_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Check timelock has passed
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();

        if now < pending.unlock_timestamp {
            return Err(ProgramError::UpgradeTimelockActive);
        }

        // Verify buffer matches
        if buffer_key != pending.buffer {
            return Err(ProgramError::BufferMismatch);
        }

        // Get new bytecode
        let buffer_state = ProgramAccountState::from_bytes(accounts[2].data.as_slice())?;
        let data_offset = match buffer_state {
            ProgramAccountState::Buffer { data_offset, .. } => data_offset,
            _ => return Err(ProgramError::InvalidAccountData),
        };

        let new_bytecode = accounts[2].data.as_slice()[data_offset..].to_vec();

        // Verify hash
        let hash: [u8; 32] = blake3::hash(&new_bytecode).into();
        if hash != pending.new_program_hash {
            return Err(ProgramError::ProgramHashMismatch);
        }

        // Charge for upgrade
        meter.consume(new_bytecode.len() as u64)?;

        // Validate
        let validator = BytecodeValidator::new();
        validator
            .validate(&new_bytecode)
            .map_err(|e| ProgramError::InvalidBytecode(e.to_string()))?;

        // Second phase: prepare new state
        let new_state = ProgramAccountState::ProgramData {
            slot,
            upgrade_authority,
            frozen: false,
            pending_upgrade: None,
        };

        let state_bytes = new_state.to_bytes();

        // Third phase: apply mutations using indices
        accounts[0].data.as_mut_slice()[..state_bytes.len()].copy_from_slice(&state_bytes);
        accounts[0].data.as_mut_slice()[state_bytes.len()..state_bytes.len() + new_bytecode.len()]
            .copy_from_slice(&new_bytecode);

        // Close buffer: transfer lamports to spill
        let buffer_lamports = accounts[2].lamports;
        accounts[3].lamports += buffer_lamports;
        accounts[2].lamports = 0;
        accounts[2].data.clear();

        Ok(())
    }

    /// Cancel pending upgrade
    fn process_cancel_pending_upgrade(
        accounts: &mut [&mut Account],
        _meter: &ComputeMeter,
    ) -> ProgramResult<()> {
        if accounts.len() < 2 {
            return Err(ProgramError::NotEnoughAccountKeys);
        }

        // First phase: read authority key and validate
        let authority_key = accounts[1].key;

        let state = ProgramAccountState::from_bytes(accounts[0].data.as_slice())?;

        let (slot, upgrade_authority, frozen) = match state {
            ProgramAccountState::ProgramData {
                slot,
                upgrade_authority,
                frozen,
                pending_upgrade,
            } => {
                if pending_upgrade.is_none() {
                    return Err(ProgramError::NoUpgradePending);
                }
                (slot, upgrade_authority, frozen)
            }
            _ => return Err(ProgramError::InvalidAccountData),
        };

        // Verify authority
        match upgrade_authority {
            Some(auth) if auth == authority_key => {}
            _ => return Err(ProgramError::InvalidUpgradeAuthority),
        }

        // Second phase: cancel by removing pending upgrade
        let new_state = ProgramAccountState::ProgramData {
            slot,
            upgrade_authority,
            frozen,
            pending_upgrade: None,
        };

        let bytes = new_state.to_bytes();
        accounts[0].data.as_mut_slice()[..bytes.len()].copy_from_slice(&bytes);

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_loader_instruction_serialization() {
        let ix = LoaderInstruction::Write {
            offset: 100,
            bytes: vec![1, 2, 3, 4],
        };

        let bytes = ix.to_bytes();
        let recovered = LoaderInstruction::from_bytes(&bytes).unwrap();

        match recovered {
            LoaderInstruction::Write { offset, bytes } => {
                assert_eq!(offset, 100);
                assert_eq!(bytes, vec![1, 2, 3, 4]);
            }
            _ => panic!("Wrong instruction type"),
        }
    }

    #[test]
    fn test_program_account_state() {
        let state = ProgramAccountState::Buffer {
            authority: Some(Pubkey::new([1u8; 32])),
            data_offset: 45,
        };

        let bytes = state.to_bytes();
        let recovered = ProgramAccountState::from_bytes(&bytes).unwrap();

        match recovered {
            ProgramAccountState::Buffer {
                authority,
                data_offset,
            } => {
                assert_eq!(authority, Some(Pubkey::new([1u8; 32])));
                assert_eq!(data_offset, 45);
            }
            _ => panic!("Wrong state type"),
        }
    }

    #[test]
    fn test_pending_upgrade() {
        let pending = PendingUpgrade {
            buffer: Pubkey::new([1u8; 32]),
            unlock_timestamp: 1000000,
            initiator: Pubkey::new([2u8; 32]),
            new_program_hash: [3u8; 32],
        };

        let state = ProgramAccountState::ProgramData {
            slot: 100,
            upgrade_authority: Some(Pubkey::new([4u8; 32])),
            frozen: false,
            pending_upgrade: Some(pending.clone()),
        };

        let bytes = state.to_bytes();
        let recovered = ProgramAccountState::from_bytes(&bytes).unwrap();

        match recovered {
            ProgramAccountState::ProgramData {
                pending_upgrade, ..
            } => {
                assert!(pending_upgrade.is_some());
                let p = pending_upgrade.unwrap();
                assert_eq!(p.buffer, pending.buffer);
                assert_eq!(p.unlock_timestamp, pending.unlock_timestamp);
            }
            _ => panic!("Wrong state type"),
        }
    }

    #[test]
    fn test_deploy_instruction() {
        let ix = LoaderProgram::deploy_with_max_data_len(
            Pubkey::new([1u8; 32]),
            Pubkey::new([2u8; 32]),
            Pubkey::new([3u8; 32]),
            Pubkey::new([4u8; 32]),
            Pubkey::new([5u8; 32]),
            1000000,
        );

        assert_eq!(ix.program_id, UPGRADEABLE_LOADER_ID);
        assert_eq!(ix.accounts.len(), 8);
        assert!(ix.accounts[0].is_signer);
        assert!(ix.accounts[7].is_signer);
    }
}
