//! Simple Counter Contract for dchat
//!
//! This is a minimal example contract that demonstrates:
//! - WASM entrypoint signature compatible with dchat VM
//! - State management through accounts
//! - Instruction parsing and routing
//!
//! Compile with: cargo build --target wasm32-unknown-unknown --release

#![no_std]
#![allow(unused)]

// Core panic handler required for no_std
#[panic_handler]
fn panic(_info: &core::panic::PanicInfo) -> ! {
    loop {}
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION TAGS
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize counter instruction tag
const IX_INITIALIZE: u8 = 0;
/// Increment counter instruction tag
const IX_INCREMENT: u8 = 1;
/// Decrement counter instruction tag
const IX_DECREMENT: u8 = 2;
/// Set counter to specific value instruction tag
const IX_SET: u8 = 3;

// ═══════════════════════════════════════════════════════════════════════════════
// ERROR CODES
// ═══════════════════════════════════════════════════════════════════════════════

/// Success
const SUCCESS: u32 = 0;
/// Invalid instruction data
const ERR_INVALID_INSTRUCTION: u32 = 1;
/// Account not initialized
const ERR_NOT_INITIALIZED: u32 = 2;
/// Account already initialized
const ERR_ALREADY_INITIALIZED: u32 = 3;
/// Overflow during operation
const ERR_OVERFLOW: u32 = 4;
/// Underflow during operation
const ERR_UNDERFLOW: u32 = 5;
/// Invalid account count
const ERR_INVALID_ACCOUNTS: u32 = 6;
/// Insufficient data
const ERR_INSUFFICIENT_DATA: u32 = 7;

// ═══════════════════════════════════════════════════════════════════════════════
// ACCOUNT STATE
// ═══════════════════════════════════════════════════════════════════════════════

/// Magic bytes for initialized counter state
const COUNTER_MAGIC: [u8; 4] = [0xC0, 0x55, 0x4E, 0x54]; // "CUNT" (Counter)

/// Counter state layout in account data:
/// [0..4]   - Magic bytes (4 bytes)
/// [4..5]   - Version (1 byte)
/// [5..13]  - Counter value (8 bytes, little-endian u64)
/// [13..45] - Authority pubkey (32 bytes)
const COUNTER_STATE_SIZE: usize = 45;

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Read a u64 from bytes (little-endian)
#[inline]
fn read_u64_le(bytes: &[u8]) -> u64 {
    if bytes.len() < 8 {
        return 0;
    }
    let mut arr = [0u8; 8];
    arr.copy_from_slice(&bytes[..8]);
    u64::from_le_bytes(arr)
}

/// Write a u64 to bytes (little-endian)
#[inline]
fn write_u64_le(bytes: &mut [u8], value: u64) {
    if bytes.len() >= 8 {
        let arr = value.to_le_bytes();
        bytes[..8].copy_from_slice(&arr);
    }
}

/// Check if account data is initialized
#[inline]
fn is_initialized(data: &[u8]) -> bool {
    data.len() >= 4 && data[0..4] == COUNTER_MAGIC
}

/// Get counter value from state
#[inline]
fn get_counter(data: &[u8]) -> u64 {
    if data.len() >= 13 {
        read_u64_le(&data[5..13])
    } else {
        0
    }
}

/// Set counter value in state
#[inline]
fn set_counter(data: &mut [u8], value: u64) {
    if data.len() >= 13 {
        write_u64_le(&mut data[5..13], value);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTION HANDLERS
// ═══════════════════════════════════════════════════════════════════════════════

/// Initialize the counter account
fn process_initialize(account_data: &mut [u8]) -> u32 {
    // Check if already initialized
    if is_initialized(account_data) {
        return ERR_ALREADY_INITIALIZED;
    }

    // Check account size
    if account_data.len() < COUNTER_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    // Write magic bytes
    account_data[0..4].copy_from_slice(&COUNTER_MAGIC);
    // Version 1
    account_data[4] = 1;
    // Initialize counter to 0
    set_counter(account_data, 0);

    SUCCESS
}

/// Increment the counter
fn process_increment(account_data: &mut [u8], amount: u64) -> u32 {
    // Check initialization
    if !is_initialized(account_data) {
        return ERR_NOT_INITIALIZED;
    }

    // Get current value
    let current = get_counter(account_data);

    // Check overflow
    match current.checked_add(amount) {
        Some(new_value) => {
            set_counter(account_data, new_value);
            SUCCESS
        }
        None => ERR_OVERFLOW,
    }
}

/// Decrement the counter
fn process_decrement(account_data: &mut [u8], amount: u64) -> u32 {
    // Check initialization
    if !is_initialized(account_data) {
        return ERR_NOT_INITIALIZED;
    }

    // Get current value
    let current = get_counter(account_data);

    // Check underflow
    match current.checked_sub(amount) {
        Some(new_value) => {
            set_counter(account_data, new_value);
            SUCCESS
        }
        None => ERR_UNDERFLOW,
    }
}

/// Set the counter to a specific value
fn process_set(account_data: &mut [u8], value: u64) -> u32 {
    // Check initialization
    if !is_initialized(account_data) {
        return ERR_NOT_INITIALIZED;
    }

    set_counter(account_data, value);
    SUCCESS
}

// ═══════════════════════════════════════════════════════════════════════════════
// ENTRYPOINT
// ═══════════════════════════════════════════════════════════════════════════════

/// Contract entrypoint
///
/// Called by the dchat VM with:
/// - accounts_ptr: Pointer to serialized accounts blob
/// - accounts_len: Length of accounts blob
/// - ix_ptr: Pointer to instruction data
/// - ix_len: Length of instruction data
///
/// Returns 0 on success, non-zero error code on failure.
#[no_mangle]
pub extern "C" fn entrypoint(
    accounts_ptr: u32,
    accounts_len: u32,
    ix_ptr: u32,
    ix_len: u32,
) -> u32 {
    // Safety: The VM guarantees these pointers are valid within WASM linear memory
    let ix_data = unsafe { core::slice::from_raw_parts(ix_ptr as *const u8, ix_len as usize) };

    // Need at least 1 byte for instruction tag
    if ix_data.is_empty() {
        return ERR_INVALID_INSTRUCTION;
    }

    let ix_tag = ix_data[0];
    let ix_payload = &ix_data[1..];

    // For this simple example, we simulate account access
    // In production, we'd parse the accounts blob properly
    let account_data =
        unsafe { core::slice::from_raw_parts_mut(accounts_ptr as *mut u8, accounts_len as usize) };

    // Need at least COUNTER_STATE_SIZE bytes for account data
    if account_data.len() < COUNTER_STATE_SIZE {
        return ERR_INSUFFICIENT_DATA;
    }

    // Route to appropriate handler
    match ix_tag {
        IX_INITIALIZE => process_initialize(account_data),

        IX_INCREMENT => {
            // Parse amount from payload (default to 1 if not provided)
            let amount = if ix_payload.len() >= 8 {
                read_u64_le(ix_payload)
            } else {
                1
            };
            process_increment(account_data, amount)
        }

        IX_DECREMENT => {
            // Parse amount from payload (default to 1 if not provided)
            let amount = if ix_payload.len() >= 8 {
                read_u64_le(ix_payload)
            } else {
                1
            };
            process_decrement(account_data, amount)
        }

        IX_SET => {
            // Require value in payload
            if ix_payload.len() < 8 {
                return ERR_INVALID_INSTRUCTION;
            }
            let value = read_u64_le(ix_payload);
            process_set(account_data, value)
        }

        _ => ERR_INVALID_INSTRUCTION,
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MEMORY ALLOCATION (for guest->host communication)
// ═══════════════════════════════════════════════════════════════════════════════

/// Simple bump allocator for WASM
static mut HEAP_PTR: usize = 0x10000; // Start at 64KB offset

/// Allocate memory (for host to place data)
#[no_mangle]
pub extern "C" fn alloc(size: u32) -> u32 {
    unsafe {
        let ptr = HEAP_PTR;
        HEAP_PTR += size as usize;
        // Align to 8 bytes
        HEAP_PTR = (HEAP_PTR + 7) & !7;
        ptr as u32
    }
}

/// Deallocate memory (no-op for bump allocator)
#[no_mangle]
pub extern "C" fn dealloc(_ptr: u32, _size: u32) {
    // No-op for simple bump allocator
}
