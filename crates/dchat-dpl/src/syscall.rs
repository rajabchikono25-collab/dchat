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

/// Program manifest info returned by `get_program_manifest`
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ProgramManifest {
    /// BLAKE3 hash of the canonical IDL
    pub schema_hash: [u8; 32],
    /// ABI version (1 = current)
    pub abi_version: u8,
    /// Import profile (0=Legacy, 1=WASI, 2=Hybrid)
    pub import_profile: u8,
    /// SDK version major
    pub sdk_major: u16,
    /// SDK version minor
    pub sdk_minor: u16,
    /// SDK version patch
    pub sdk_patch: u16,
    /// Edition year (e.g., 2025)
    pub edition: u16,
    /// Capability bitflags
    pub capabilities: u64,
}

impl ProgramManifest {
    /// Deserialize from raw bytes (50 bytes)
    pub fn from_bytes(data: &[u8; 50]) -> Self {
        let mut schema_hash = [0u8; 32];
        schema_hash.copy_from_slice(&data[0..32]);

        Self {
            schema_hash,
            abi_version: data[32],
            import_profile: data[33],
            sdk_major: u16::from_le_bytes([data[34], data[35]]),
            sdk_minor: u16::from_le_bytes([data[36], data[37]]),
            sdk_patch: u16::from_le_bytes([data[38], data[39]]),
            edition: u16::from_le_bytes([data[40], data[41]]),
            capabilities: u64::from_le_bytes([
                data[42], data[43], data[44], data[45], data[46], data[47], data[48], data[49],
            ]),
        }
    }

    /// Check if this manifest has a valid (non-zero) schema hash
    pub fn has_schema_hash(&self) -> bool {
        self.schema_hash != [0u8; 32]
    }

    /// Verify that this manifest's schema hash matches the expected hash
    pub fn verify_schema_hash(&self, expected: &[u8; 32]) -> bool {
        &self.schema_hash == expected
    }
}

/// Query the manifest of a deployed program
///
/// Returns `Some(manifest)` if the program has a valid manifest,
/// or `None` if the program doesn't have a manifest or doesn't exist.
///
/// # Arguments
/// * `program_id` - The 32-byte public key of the program to query
///
/// # Example
/// ```ignore
/// use dchat_dpl::syscall::get_program_manifest;
/// use dchat_dpl::Pubkey;
///
/// let target_program = Pubkey::new([1u8; 32]);
/// if let Some(manifest) = get_program_manifest(&target_program.0) {
///     // Verify the program's schema matches what we expect
///     let expected_schema: [u8; 32] = /* ... */;
///     assert!(manifest.verify_schema_hash(&expected_schema));
/// }
/// ```
pub fn get_program_manifest(_program_id: &[u8; 32]) -> Option<ProgramManifest> {
    // In production, this calls the SOL_GET_PROGRAM_MANIFEST syscall
    // which reads the manifest from the program's ProgramData account.
    //
    // Syscall arguments:
    // - arg0: pointer to 32-byte program ID
    // - arg1: pointer to 50-byte output buffer
    //
    // Returns 0 if manifest found, 1 if no manifest, error otherwise.

    #[cfg(feature = "std")]
    {
        // In std mode (testing), return None
        None
    }

    #[cfg(not(feature = "std"))]
    {
        // In no_std mode (actual WASM execution), invoke the syscall
        extern "C" {
            /// VM-provided syscall to query program manifests
            /// Returns: 0 = success, 1 = no manifest, other = error
            fn sol_get_program_manifest(program_id_ptr: *const u8, output_ptr: *mut u8) -> u64;
        }

        let mut output = [0u8; 50];

        // SAFETY: We're passing valid pointers to properly sized buffers
        // and the VM guarantees memory safety for syscall execution
        let result = unsafe { sol_get_program_manifest(_program_id.as_ptr(), output.as_mut_ptr()) };

        if result == 0 {
            // Success - parse the manifest from output buffer
            Some(ProgramManifest::from_bytes(&output))
        } else {
            // No manifest or error
            None
        }
    }
}

/// Verify that a program has a specific schema hash
///
/// This is a convenience wrapper around `get_program_manifest` that
/// only checks the schema hash matches.
///
/// # Arguments
/// * `program_id` - The program to verify
/// * `expected_schema_hash` - The expected BLAKE3 hash of the program's IDL
///
/// # Returns
/// * `Ok(())` if the schema hash matches
/// * `Err(reason)` if verification fails
pub fn verify_program_schema(
    program_id: &[u8; 32],
    expected_schema_hash: &[u8; 32],
) -> Result<(), &'static str> {
    match get_program_manifest(program_id) {
        Some(manifest) => {
            if manifest.verify_schema_hash(expected_schema_hash) {
                Ok(())
            } else {
                Err("schema hash mismatch")
            }
        }
        None => Err("program has no manifest"),
    }
}
