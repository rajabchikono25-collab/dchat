//! Syscall wrappers for DPL programs
//!
//! These functions wrap the VM syscalls for use by DPL programs.

/// Log a message
pub fn log(msg: &str) {
    // In WASI mode, this uses println! which routes to fd_write
    // which the VM shim captures for logging
    #[cfg(feature = "std")]
    println!("{}", msg);

    #[cfg(not(feature = "std"))]
    {
        let _ = msg;
        // In no_std mode, we would call the raw syscall
    }
}

/// Log compute units remaining
pub fn log_compute_units() {
    log("[compute units remaining]");
}

/// Compute SHA-256 hash
pub fn sha256(data: &[u8]) -> [u8; 32] {
    use sha2::{Digest, Sha256};
    let mut hasher = Sha256::new();
    hasher.update(data);
    let result = hasher.finalize();
    let mut output = [0u8; 32];
    output.copy_from_slice(&result);
    output
}

/// Compute BLAKE3 hash
pub fn blake3(data: &[u8]) -> [u8; 32] {
    *blake3::hash(data).as_bytes()
}

/// Get current slot (from clock sysvar)
pub fn get_slot() -> u64 {
    // In production, this reads from the clock sysvar
    // For now, return 0
    0
}

/// Get current epoch (from clock sysvar)
pub fn get_epoch() -> u64 {
    // In production, this reads from the clock sysvar
    0
}

/// Get unix timestamp (from clock sysvar)
pub fn get_unix_timestamp() -> i64 {
    // In production, this reads from the clock sysvar
    0
}
