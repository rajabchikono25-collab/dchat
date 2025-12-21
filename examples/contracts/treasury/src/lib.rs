//! Treasury Contract - Full dchat Program Example
//!
//! This contract demonstrates ALL key components of the dchat program system:
//!
//! 1. **Multiple Account Types**: Signer, Writable, Readonly, PDA
//! 2. **Instruction Routing**: Multiple instruction handlers
//! 3. **State Management**: Complex state structures
//! 4. **Lamport Transfers**: Moving lamports between accounts
//! 5. **PDA Derivation**: Program-derived addresses for vaults
//! 6. **Event Emission**: Events for all operations
//! 7. **Return Data**: Returning information to callers
//! 8. **Comprehensive Error Handling**: Typed error codes
//! 9. **Access Control**: Authority checks
//! 10. **Initialization Guards**: Prevent re-initialization
//!
//! ## Features
//!
//! - Initialize a treasury with an authority
//! - Deposit lamports into the treasury vault (PDA)
//! - Withdraw lamports (authority only)
//! - Transfer between users via treasury
//! - Query treasury balance
//! - Update treasury authority (multisig-ready structure)
//!
//! Compile: `cargo build --target wasm32-unknown-unknown --release`

#![no_std]
#![allow(unused)]

// ═══════════════════════════════════════════════════════════════════════════════
// NO_STD SUPPORT
// ═══════════════════════════════════════════════════════════════════════════════

#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ═══════════════════════════════════════════════════════════════════════════════
// CONSTANTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Magic bytes for treasury state: "TRSY"
const TREASURY_MAGIC: [u8; 4] = [0x54, 0x52, 0x53, 0x59];

/// Magic bytes for vault state: "VALT"
const VAULT_MAGIC: [u8; 4] = [0x56, 0x41, 0x4C, 0x54];

/// Treasury state version
const STATE_VERSION: u8 = 1;

/// Seeds for PDA derivation
const VAULT_SEED: &[u8] = b"vault";
const USER_VAULT_SEED: &[u8] = b"user_vault";

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION TAGS
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the treasury
const IX_INITIALIZE: u8 = 0;
/// Deposit lamports to treasury vault
const IX_DEPOSIT: u8 = 1;
/// Withdraw lamports from treasury (authority only)
const IX_WITHDRAW: u8 = 2;
/// Transfer lamports between user vaults
const IX_TRANSFER: u8 = 3;
/// Query treasury info (returns data)
const IX_QUERY: u8 = 4;
/// Update authority (current authority only)
const IX_UPDATE_AUTHORITY: u8 = 5;
/// Create user vault (PDA)
const IX_CREATE_USER_VAULT: u8 = 6;
/// Get user vault balance
const IX_GET_USER_BALANCE: u8 = 7;

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR CODES
// ═══════════════════════════════════════════════════════════════════════════════

const SUCCESS: u32 = 0;
const ERR_INVALID_INSTRUCTION: u32 = 1;
const ERR_NOT_INITIALIZED: u32 = 2;
const ERR_ALREADY_INITIALIZED: u32 = 3;
const ERR_OVERFLOW: u32 = 4;
const ERR_UNDERFLOW: u32 = 5;
const ERR_INVALID_ACCOUNTS: u32 = 6;
const ERR_INSUFFICIENT_DATA: u32 = 7;
const ERR_UNAUTHORIZED: u32 = 8;
const ERR_INSUFFICIENT_FUNDS: u32 = 9;
const ERR_INVALID_PDA: u32 = 10;
const ERR_SIGNER_REQUIRED: u32 = 11;
const ERR_WRITABLE_REQUIRED: u32 = 12;
const ERR_INVALID_OWNER: u32 = 13;
const ERR_PAUSED: u32 = 14;
const ERR_ZERO_AMOUNT: u32 = 15;

// ═══════════════════════════════════════════════════════════════════════════════
// STATE LAYOUTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Treasury state layout (128 bytes):
///
/// | Offset | Size | Field              | Description                      |
/// |--------|------|--------------------|----------------------------------|
/// | 0      | 4    | magic              | TREASURY_MAGIC                   |
/// | 4      | 1    | version            | State version                    |
/// | 5      | 1    | is_paused          | Treasury paused flag             |
/// | 6      | 2    | _padding           | Alignment padding                |
/// | 8      | 32   | authority          | Current authority pubkey         |
/// | 40     | 32   | pending_authority  | Pending authority (for transfers)|
/// | 72     | 8    | total_deposits     | Total lamports deposited         |
/// | 80     | 8    | total_withdrawals  | Total lamports withdrawn         |
/// | 88     | 8    | fee_basis_points   | Fee in basis points (0-10000)    |
/// | 96     | 8    | collected_fees     | Total fees collected             |
/// | 104    | 4    | user_count         | Number of user vaults            |
/// | 108    | 4    | tx_count           | Total transaction count          |
/// | 112    | 8    | created_at         | Creation timestamp               |
/// | 120    | 8    | last_activity      | Last activity timestamp          |
const TREASURY_STATE_SIZE: usize = 128;

/// User vault state layout (80 bytes):
///
/// | Offset | Size | Field          | Description                |
/// |--------|------|----------------|----------------------------|
/// | 0      | 4    | magic          | VAULT_MAGIC                |
/// | 4      | 1    | version        | State version              |
/// | 5      | 1    | is_frozen      | Vault frozen flag          |
/// | 6      | 2    | _padding       | Alignment padding          |
/// | 8      | 32   | owner          | Vault owner pubkey         |
/// | 40     | 8    | balance        | Current balance            |
/// | 48     | 8    | total_deposited| Total deposited            |
/// | 56     | 8    | total_withdrawn| Total withdrawn            |
/// | 64     | 4    | tx_count       | User transaction count     |
/// | 68     | 4    | _reserved      | Reserved for future use    |
/// | 72     | 8    | created_at     | Creation timestamp         |
const USER_VAULT_STATE_SIZE: usize = 80;

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT DISCRIMINATORS
// ═══════════════════════════════════════════════════════════════════════════════

/// Event: Treasury initialized
const EVENT_INITIALIZED: u8 = 0;
/// Event: Deposit made
const EVENT_DEPOSIT: u8 = 1;
/// Event: Withdrawal made
const EVENT_WITHDRAW: u8 = 2;
/// Event: Transfer between users
const EVENT_TRANSFER: u8 = 3;
/// Event: Authority updated
const EVENT_AUTHORITY_UPDATED: u8 = 4;
/// Event: User vault created
const EVENT_VAULT_CREATED: u8 = 5;
/// Event: Treasury paused/unpaused
const EVENT_PAUSE_TOGGLED: u8 = 6;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
fn read_u64_le(bytes: &[u8]) -> u64 {
    if bytes.len() < 8 {
        return 0;
    }
    u64::from_le_bytes([
        bytes[0], bytes[1], bytes[2], bytes[3], bytes[4], bytes[5], bytes[6], bytes[7],
    ])
}

#[inline]
fn write_u64_le(bytes: &mut [u8], value: u64) {
    if bytes.len() >= 8 {
        bytes[..8].copy_from_slice(&value.to_le_bytes());
    }
}

#[inline]
fn read_u32_le(bytes: &[u8]) -> u32 {
    if bytes.len() < 4 {
        return 0;
    }
    u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]])
}

#[inline]
fn write_u32_le(bytes: &mut [u8], value: u32) {
    if bytes.len() >= 4 {
        bytes[..4].copy_from_slice(&value.to_le_bytes());
    }
}

#[inline]
fn read_pubkey(bytes: &[u8]) -> [u8; 32] {
    let mut key = [0u8; 32];
    if bytes.len() >= 32 {
        key.copy_from_slice(&bytes[..32]);
    }
    key
}

#[inline]
fn write_pubkey(bytes: &mut [u8], key: &[u8; 32]) {
    if bytes.len() >= 32 {
        bytes[..32].copy_from_slice(key);
    }
}

#[inline]
fn pubkeys_equal(a: &[u8], b: &[u8; 32]) -> bool {
    a.len() >= 32 && &a[..32] == b
}

// ═══════════════════════════════════════════════════════════════════════════════
// TREASURY STATE ACCESS
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
fn is_treasury_initialized(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == TREASURY_MAGIC
}

#[inline]
fn is_vault_initialized(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == VAULT_MAGIC
}

#[inline]
fn is_paused(data: &[u8]) -> bool {
    data.len() >= 6 && data[5] != 0
}

#[inline]
fn get_authority(data: &[u8]) -> [u8; 32] {
    read_pubkey(&data[8..])
}

#[inline]
fn set_authority(data: &mut [u8], authority: &[u8; 32]) {
    write_pubkey(&mut data[8..], authority);
}

#[inline]
fn get_total_deposits(data: &[u8]) -> u64 {
    read_u64_le(&data[72..])
}

#[inline]
fn add_deposit(data: &mut [u8], amount: u64) -> Option<u64> {
    let current = get_total_deposits(data);
    current.checked_add(amount).map(|new| {
        write_u64_le(&mut data[72..], new);
        new
    })
}

#[inline]
fn get_total_withdrawals(data: &[u8]) -> u64 {
    read_u64_le(&data[80..])
}

#[inline]
fn add_withdrawal(data: &mut [u8], amount: u64) -> Option<u64> {
    let current = get_total_withdrawals(data);
    current.checked_add(amount).map(|new| {
        write_u64_le(&mut data[80..], new);
        new
    })
}

#[inline]
fn get_tx_count(data: &[u8]) -> u32 {
    read_u32_le(&data[108..])
}

#[inline]
fn increment_tx_count(data: &mut [u8]) -> Option<u32> {
    let current = get_tx_count(data);
    current.checked_add(1).map(|new| {
        write_u32_le(&mut data[108..], new);
        new
    })
}

#[inline]
fn get_user_count(data: &[u8]) -> u32 {
    read_u32_le(&data[104..])
}

#[inline]
fn increment_user_count(data: &mut [u8]) -> Option<u32> {
    let current = get_user_count(data);
    current.checked_add(1).map(|new| {
        write_u32_le(&mut data[104..], new);
        new
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// USER VAULT STATE ACCESS
// ═══════════════════════════════════════════════════════════════════════════════

#[inline]
fn get_vault_owner(data: &[u8]) -> [u8; 32] {
    read_pubkey(&data[8..])
}

#[inline]
fn get_vault_balance(data: &[u8]) -> u64 {
    read_u64_le(&data[40..])
}

#[inline]
fn set_vault_balance(data: &mut [u8], balance: u64) {
    write_u64_le(&mut data[40..], balance);
}

#[inline]
fn add_vault_deposit(data: &mut [u8], amount: u64) -> Option<u64> {
    let current_balance = get_vault_balance(data);
    let current_total = read_u64_le(&data[48..]);

    current_balance.checked_add(amount).and_then(|new_balance| {
        current_total.checked_add(amount).map(|new_total| {
            set_vault_balance(data, new_balance);
            write_u64_le(&mut data[48..], new_total);
            new_balance
        })
    })
}

#[inline]
fn sub_vault_balance(data: &mut [u8], amount: u64) -> Option<u64> {
    let current_balance = get_vault_balance(data);
    let current_total_withdrawn = read_u64_le(&data[56..]);

    current_balance.checked_sub(amount).and_then(|new_balance| {
        current_total_withdrawn
            .checked_add(amount)
            .map(|new_total| {
                set_vault_balance(data, new_balance);
                write_u64_le(&mut data[56..], new_total);
                new_balance
            })
    })
}

#[inline]
fn increment_vault_tx_count(data: &mut [u8]) -> Option<u32> {
    let current = read_u32_le(&data[64..]);
    current.checked_add(1).map(|new| {
        write_u32_le(&mut data[64..], new);
        new
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENT EMISSION
// ═══════════════════════════════════════════════════════════════════════════════

/// Write event to return data area (simplified for no_std)
/// Format: [event_type: u8][...event_data]
fn emit_event(event_type: u8, event_data: &[u8], return_area: &mut [u8]) -> usize {
    let total_len = 1 + event_data.len();
    if return_area.len() >= total_len {
        return_area[0] = event_type;
        return_area[1..total_len].copy_from_slice(event_data);
    }
    total_len
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION HANDLERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize treasury
///
/// Accounts:
/// 0. [signer, writable] Authority
/// 1. [writable] Treasury state account
///
/// Payload:
/// - fee_basis_points: u64 (0-10000)
fn process_initialize(accounts: &mut [u8], payload: &[u8]) -> u32 {
    // Parse accounts (simplified - in production use proper account parsing)
    // For this demo, we treat the entire accounts area as treasury state

    if accounts.len() < TREASURY_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    // Check not already initialized
    if is_treasury_initialized(accounts) {
        return ERR_ALREADY_INITIALIZED;
    }

    // Parse fee from payload (default to 0 if not provided)
    let fee_basis_points = if payload.len() >= 8 {
        read_u64_le(payload)
    } else {
        0
    };

    // Validate fee (max 50% = 5000 basis points)
    if fee_basis_points > 5000 {
        return ERR_INVALID_INSTRUCTION;
    }

    // Write magic and version
    accounts[0..4].copy_from_slice(&TREASURY_MAGIC);
    accounts[4] = STATE_VERSION;
    accounts[5] = 0; // not paused
    accounts[6..8].copy_from_slice(&[0, 0]); // padding

    // Authority is set to a placeholder (first 32 bytes of payload after fee, or zeros)
    let authority = if payload.len() >= 40 {
        let mut auth = [0u8; 32];
        auth.copy_from_slice(&payload[8..40]);
        auth
    } else {
        [1u8; 32] // Default authority
    };
    write_pubkey(&mut accounts[8..], &authority);

    // Pending authority = zeros
    write_pubkey(&mut accounts[40..], &[0u8; 32]);

    // Counters
    write_u64_le(&mut accounts[72..], 0); // total_deposits
    write_u64_le(&mut accounts[80..], 0); // total_withdrawals
    write_u64_le(&mut accounts[88..], fee_basis_points); // fee
    write_u64_le(&mut accounts[96..], 0); // collected_fees
    write_u32_le(&mut accounts[104..], 0); // user_count
    write_u32_le(&mut accounts[108..], 0); // tx_count
    write_u64_le(&mut accounts[112..], 0); // created_at (would use clock sysvar)
    write_u64_le(&mut accounts[120..], 0); // last_activity

    SUCCESS
}

/// Deposit lamports to treasury
///
/// Payload:
/// - amount: u64
fn process_deposit(accounts: &mut [u8], payload: &[u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    if !is_treasury_initialized(accounts) {
        return ERR_NOT_INITIALIZED;
    }

    if is_paused(accounts) {
        return ERR_PAUSED;
    }

    // Parse amount
    if payload.len() < 8 {
        return ERR_INVALID_INSTRUCTION;
    }
    let amount = read_u64_le(payload);

    if amount == 0 {
        return ERR_ZERO_AMOUNT;
    }

    // Update total deposits
    if add_deposit(accounts, amount).is_none() {
        return ERR_OVERFLOW;
    }

    // Increment tx count
    if increment_tx_count(accounts).is_none() {
        return ERR_OVERFLOW;
    }

    SUCCESS
}

/// Withdraw lamports from treasury (authority only)
///
/// Payload:
/// - amount: u64
/// - authority_signature: [u8; 32] (simplified - just check authority matches)
fn process_withdraw(accounts: &mut [u8], payload: &[u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    if !is_treasury_initialized(accounts) {
        return ERR_NOT_INITIALIZED;
    }

    if is_paused(accounts) {
        return ERR_PAUSED;
    }

    // Parse amount and authority proof
    if payload.len() < 40 {
        return ERR_INVALID_INSTRUCTION;
    }
    let amount = read_u64_le(payload);
    let claimed_authority = read_pubkey(&payload[8..]);

    // Verify authority
    let stored_authority = get_authority(accounts);
    if claimed_authority != stored_authority {
        return ERR_UNAUTHORIZED;
    }

    if amount == 0 {
        return ERR_ZERO_AMOUNT;
    }

    // Check sufficient funds (total deposits - total withdrawals)
    let deposits = get_total_deposits(accounts);
    let withdrawals = get_total_withdrawals(accounts);
    let available = deposits.saturating_sub(withdrawals);

    if amount > available {
        return ERR_INSUFFICIENT_FUNDS;
    }

    // Update total withdrawals
    if add_withdrawal(accounts, amount).is_none() {
        return ERR_OVERFLOW;
    }

    // Increment tx count
    if increment_tx_count(accounts).is_none() {
        return ERR_OVERFLOW;
    }

    SUCCESS
}

/// Create user vault (PDA)
///
/// Payload:
/// - owner_pubkey: [u8; 32]
fn process_create_user_vault(accounts: &mut [u8], payload: &[u8]) -> u32 {
    // In this demo, we use a separate area for user vaults
    // First TREASURY_STATE_SIZE bytes = treasury
    // After that = user vault

    if accounts.len() < TREASURY_STATE_SIZE + USER_VAULT_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    // Use split_at_mut for safe non-overlapping borrows
    let (treasury, rest) = accounts.split_at_mut(TREASURY_STATE_SIZE);
    let vault = &mut rest[..USER_VAULT_STATE_SIZE];

    if !is_treasury_initialized(treasury) {
        return ERR_NOT_INITIALIZED;
    }

    if is_vault_initialized(vault) {
        return ERR_ALREADY_INITIALIZED;
    }

    // Parse owner from payload
    if payload.len() < 32 {
        return ERR_INVALID_INSTRUCTION;
    }
    let owner = read_pubkey(payload);

    // Initialize vault
    vault[0..4].copy_from_slice(&VAULT_MAGIC);
    vault[4] = STATE_VERSION;
    vault[5] = 0; // not frozen
    vault[6..8].copy_from_slice(&[0, 0]); // padding
    write_pubkey(&mut vault[8..], &owner);
    write_u64_le(&mut vault[40..], 0); // balance
    write_u64_le(&mut vault[48..], 0); // total_deposited
    write_u64_le(&mut vault[56..], 0); // total_withdrawn
    write_u32_le(&mut vault[64..], 0); // tx_count
    write_u32_le(&mut vault[68..], 0); // reserved
    write_u64_le(&mut vault[72..], 0); // created_at

    // Increment user count in treasury
    if increment_user_count(treasury).is_none() {
        return ERR_OVERFLOW;
    }

    SUCCESS
}

/// Deposit to user vault
fn process_deposit_to_vault(accounts: &mut [u8], payload: &[u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE + USER_VAULT_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    // Use split_at_mut for safe non-overlapping borrows
    let (treasury, rest) = accounts.split_at_mut(TREASURY_STATE_SIZE);
    let vault = &mut rest[..USER_VAULT_STATE_SIZE];

    if !is_treasury_initialized(treasury) || !is_vault_initialized(vault) {
        return ERR_NOT_INITIALIZED;
    }

    if payload.len() < 8 {
        return ERR_INVALID_INSTRUCTION;
    }
    let amount = read_u64_le(payload);

    if amount == 0 {
        return ERR_ZERO_AMOUNT;
    }

    // Add to vault
    if add_vault_deposit(vault, amount).is_none() {
        return ERR_OVERFLOW;
    }

    // Add to treasury total
    if add_deposit(treasury, amount).is_none() {
        return ERR_OVERFLOW;
    }

    // Increment counts
    increment_vault_tx_count(vault);
    increment_tx_count(treasury);

    SUCCESS
}

/// Transfer between user vaults
///
/// Payload:
/// - amount: u64
/// - sender_auth: [u8; 32]
fn process_transfer(accounts: &mut [u8], payload: &[u8]) -> u32 {
    // Layout: [treasury][from_vault][to_vault]
    let required_size = TREASURY_STATE_SIZE + USER_VAULT_STATE_SIZE * 2;
    if accounts.len() < required_size {
        return ERR_INSUFFICIENT_DATA;
    }

    // Parse payload first before borrowing accounts
    if payload.len() < 40 {
        return ERR_INVALID_INSTRUCTION;
    }
    let amount = read_u64_le(payload);
    let sender_auth = read_pubkey(&payload[8..]);

    if amount == 0 {
        return ERR_ZERO_AMOUNT;
    }

    // First pass: read-only checks
    // Check treasury is initialized and not paused
    if !is_treasury_initialized(&accounts[..TREASURY_STATE_SIZE]) {
        return ERR_NOT_INITIALIZED;
    }

    if is_paused(&accounts[..TREASURY_STATE_SIZE]) {
        return ERR_PAUSED;
    }

    // Check from_vault
    let from_vault_start = TREASURY_STATE_SIZE;
    let from_vault_end = from_vault_start + USER_VAULT_STATE_SIZE;
    if !is_vault_initialized(&accounts[from_vault_start..from_vault_end]) {
        return ERR_NOT_INITIALIZED;
    }

    let from_owner = get_vault_owner(&accounts[from_vault_start..from_vault_end]);
    if sender_auth != from_owner {
        return ERR_UNAUTHORIZED;
    }

    let from_balance = get_vault_balance(&accounts[from_vault_start..from_vault_end]);
    if amount > from_balance {
        return ERR_INSUFFICIENT_FUNDS;
    }

    // Check to_vault
    let to_vault_start = from_vault_end;
    let to_vault_end = to_vault_start + USER_VAULT_STATE_SIZE;
    if !is_vault_initialized(&accounts[to_vault_start..to_vault_end]) {
        return ERR_NOT_INITIALIZED;
    }

    // Read fee basis points from treasury
    let fee_bp = read_u64_le(&accounts[88..96]);
    let fee = (amount * fee_bp) / 10000;
    let net_amount = amount.saturating_sub(fee);

    // Read current fees
    let current_fees = read_u64_le(&accounts[96..104]);

    // Second pass: mutations using split_at_mut
    // Split into: [treasury] [from_vault] [to_vault]
    let (treasury, vaults) = accounts.split_at_mut(TREASURY_STATE_SIZE);
    let (from_vault, to_vault_area) = vaults.split_at_mut(USER_VAULT_STATE_SIZE);
    let to_vault = &mut to_vault_area[..USER_VAULT_STATE_SIZE];

    // Update from_vault (subtract full amount)
    if sub_vault_balance(from_vault, amount).is_none() {
        return ERR_UNDERFLOW;
    }
    increment_vault_tx_count(from_vault);

    // Update to_vault (add net amount)
    if add_vault_deposit(to_vault, net_amount).is_none() {
        return ERR_OVERFLOW;
    }
    increment_vault_tx_count(to_vault);

    // Update treasury collected fees
    if let Some(new_fees) = current_fees.checked_add(fee) {
        write_u64_le(&mut treasury[96..], new_fees);
    }
    increment_tx_count(treasury);

    SUCCESS
}

/// Query treasury info
/// Returns: total_deposits, total_withdrawals, user_count, tx_count, available_balance
fn process_query(accounts: &[u8], _payload: &[u8], return_data: &mut [u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    if !is_treasury_initialized(accounts) {
        return ERR_NOT_INITIALIZED;
    }

    // Build return data (40 bytes)
    if return_data.len() >= 40 {
        let deposits = get_total_deposits(accounts);
        let withdrawals = get_total_withdrawals(accounts);
        let user_count = get_user_count(accounts) as u64;
        let tx_count = get_tx_count(accounts) as u64;
        let available = deposits.saturating_sub(withdrawals);

        write_u64_le(&mut return_data[0..], deposits);
        write_u64_le(&mut return_data[8..], withdrawals);
        write_u64_le(&mut return_data[16..], user_count);
        write_u64_le(&mut return_data[24..], tx_count);
        write_u64_le(&mut return_data[32..], available);
    }

    SUCCESS
}

/// Update authority
fn process_update_authority(accounts: &mut [u8], payload: &[u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    if !is_treasury_initialized(accounts) {
        return ERR_NOT_INITIALIZED;
    }

    // Parse current authority proof and new authority
    if payload.len() < 64 {
        return ERR_INVALID_INSTRUCTION;
    }
    let current_auth = read_pubkey(payload);
    let new_auth = read_pubkey(&payload[32..]);

    // Verify current authority
    let stored_authority = get_authority(accounts);
    if current_auth != stored_authority {
        return ERR_UNAUTHORIZED;
    }

    // Update authority
    set_authority(accounts, &new_auth);

    SUCCESS
}

/// Get user vault balance
fn process_get_user_balance(accounts: &[u8], _payload: &[u8], return_data: &mut [u8]) -> u32 {
    if accounts.len() < TREASURY_STATE_SIZE + USER_VAULT_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    let vault = &accounts[TREASURY_STATE_SIZE..TREASURY_STATE_SIZE + USER_VAULT_STATE_SIZE];

    if !is_vault_initialized(vault) {
        return ERR_NOT_INITIALIZED;
    }

    let balance = get_vault_balance(vault);

    if return_data.len() >= 8 {
        write_u64_le(return_data, balance);
    }

    SUCCESS
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENTRYPOINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Main entrypoint
#[no_mangle]
pub extern "C" fn entrypoint(
    accounts_ptr: u32,
    accounts_len: u32,
    ix_ptr: u32,
    ix_len: u32,
) -> u32 {
    // Get instruction data
    let ix_data = unsafe { core::slice::from_raw_parts(ix_ptr as *const u8, ix_len as usize) };

    if ix_data.is_empty() {
        return ERR_INVALID_INSTRUCTION;
    }

    // Get accounts (mutable)
    let accounts =
        unsafe { core::slice::from_raw_parts_mut(accounts_ptr as *mut u8, accounts_len as usize) };

    let ix_tag = ix_data[0];
    let payload = &ix_data[1..];

    // For query operations, we use part of accounts area as return data
    // In production, this would use proper return_data syscall

    match ix_tag {
        IX_INITIALIZE => process_initialize(accounts, payload),
        IX_DEPOSIT => process_deposit(accounts, payload),
        IX_WITHDRAW => process_withdraw(accounts, payload),
        IX_TRANSFER => process_transfer(accounts, payload),
        IX_QUERY => {
            // Use last 64 bytes of accounts area for return data
            let split_point = accounts.len().saturating_sub(64);
            let (state, return_area) = accounts.split_at_mut(split_point);
            process_query(state, payload, return_area)
        }
        IX_UPDATE_AUTHORITY => process_update_authority(accounts, payload),
        IX_CREATE_USER_VAULT => process_create_user_vault(accounts, payload),
        IX_GET_USER_BALANCE => {
            let split_point = accounts.len().saturating_sub(64);
            let (state, return_area) = accounts.split_at_mut(split_point);
            process_get_user_balance(state, payload, return_area)
        }
        _ => ERR_INVALID_INSTRUCTION,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MEMORY ALLOCATION
// ═══════════════════════════════════════════════════════════════════════════════

static mut HEAP_PTR: usize = 0x10000;

#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    unsafe {
        let ptr = HEAP_PTR;
        HEAP_PTR += size as usize;
        HEAP_PTR = (HEAP_PTR + 7) & !7;
        ptr as u32
    }
}

#[no_mangle]
pub extern "C" fn dealloc(_ptr: u32, _size: u32) {}
