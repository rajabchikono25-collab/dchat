//! Bytecode validation for program deployment

use std::collections::HashSet;

use serde::{Deserialize, Serialize};

use crate::error::{ProgramError, ProgramResult};
use crate::{MAX_PROGRAM_SIZE, PROTOCOL_VERSION};

/// Validate a bytecode blob and return detailed results
pub fn validate_bytecode_strict(bytecode: &[u8]) -> Result<ValidationReport, ValidationError> {
    let validator = BytecodeValidator::new();
    let validated = validator.validate(bytecode)?;
    Ok(ValidationReport {
        passed: true,
        function_count: validated.function_count,
        global_count: validated.global_count,
        bytecode_size: validated.size,
        warnings: Vec::new(),
        errors: Vec::new(),
    })
}

/// Quick validation check that returns error on first failure
pub fn validate_bytecode_quick(bytecode: &[u8]) -> ProgramResult<()> {
    if bytecode.is_empty() {
        return Err(ProgramError::InvalidBytecode("Empty bytecode".into()));
    }
    if bytecode.len() > MAX_PROGRAM_SIZE {
        return Err(ProgramError::InvalidBytecode(format!(
            "Bytecode too large: {} > {}",
            bytecode.len(),
            MAX_PROGRAM_SIZE
        )));
    }
    Ok(())
}

/// Validation report with detailed findings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationReport {
    /// Whether validation passed
    pub passed: bool,
    /// Number of functions found
    pub function_count: usize,
    /// Number of globals found
    pub global_count: usize,
    /// Total bytecode size
    pub bytecode_size: usize,
    /// Warnings (non-fatal issues)
    pub warnings: Vec<String>,
    /// Errors (fatal issues)
    pub errors: Vec<String>,
}

/// Maximum number of functions in a program
pub const MAX_FUNCTIONS: usize = 10_000;

/// Maximum number of globals
pub const MAX_GLOBALS: usize = 1_000;

/// Maximum number of tables
pub const MAX_TABLES: usize = 1;

/// Maximum number of memories
pub const MAX_MEMORIES: usize = 1;

/// Maximum function parameters
pub const MAX_FUNCTION_PARAMS: usize = 128;

/// Maximum function locals
pub const MAX_FUNCTION_LOCALS: usize = 16_384;

/// Validation configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidationConfig {
    /// Maximum program size in bytes
    pub max_size: usize,
    /// Maximum number of functions
    pub max_functions: usize,
    /// Maximum number of globals
    pub max_globals: usize,
    /// Maximum number of tables
    pub max_tables: usize,
    /// Maximum number of memories
    pub max_memories: usize,
    /// Maximum function parameters
    pub max_function_params: usize,
    /// Maximum function locals
    pub max_function_locals: usize,
    /// Required protocol version
    pub protocol_version: u32,
    /// Allowed imports (host functions)
    pub allowed_imports: HashSet<String>,
    /// Forbidden opcodes
    pub forbidden_opcodes: HashSet<String>,
}

impl Default for ValidationConfig {
    fn default() -> Self {
        let mut allowed_imports = HashSet::new();
        // System syscalls
        allowed_imports.insert("sol_log_".to_string());
        allowed_imports.insert("sol_log_64_".to_string());
        allowed_imports.insert("sol_log_compute_units_".to_string());
        allowed_imports.insert("sol_log_pubkey".to_string());
        allowed_imports.insert("sol_invoke_signed_c".to_string());
        allowed_imports.insert("sol_invoke_signed_rust".to_string());
        allowed_imports.insert("sol_alloc_free_".to_string());
        allowed_imports.insert("sol_set_return_data".to_string());
        allowed_imports.insert("sol_get_return_data".to_string());
        // Crypto syscalls
        allowed_imports.insert("sol_sha256".to_string());
        allowed_imports.insert("sol_blake3".to_string());
        allowed_imports.insert("sol_keccak256".to_string());
        allowed_imports.insert("sol_secp256k1_recover".to_string());
        allowed_imports.insert("sol_curve25519_validate_point".to_string());
        allowed_imports.insert("sol_curve25519_point_multiply".to_string());
        allowed_imports.insert("sol_poseidon".to_string());
        // Account access
        allowed_imports.insert("sol_get_account_key".to_string());
        allowed_imports.insert("sol_get_account_owner".to_string());
        allowed_imports.insert("sol_get_account_is_signer".to_string());
        allowed_imports.insert("sol_get_account_is_writable".to_string());
        allowed_imports.insert("sol_get_account_motes".to_string());
        allowed_imports.insert("sol_get_account_data".to_string());
        // Memory operations
        allowed_imports.insert("sol_memcpy".to_string());
        allowed_imports.insert("sol_memset".to_string());
        allowed_imports.insert("sol_memmove".to_string());
        allowed_imports.insert("sol_memcmp".to_string());
        // Capability verification
        allowed_imports.insert("sol_verify_capability".to_string());
        // Privacy operations
        allowed_imports.insert("sol_verify_commitment".to_string());

        // WASI imports (deterministic subset for DPL programs targeting wasm32-wasi)
        allowed_imports.insert("wasi:fd_write".to_string());
        allowed_imports.insert("wasi:proc_exit".to_string());
        allowed_imports.insert("wasi:environ_sizes_get".to_string());
        allowed_imports.insert("wasi:environ_get".to_string());
        allowed_imports.insert("wasi:args_sizes_get".to_string());
        allowed_imports.insert("wasi:args_get".to_string());
        allowed_imports.insert("wasi:fd_prestat_get".to_string());
        allowed_imports.insert("wasi:fd_prestat_dir_name".to_string());
        allowed_imports.insert("wasi:fd_close".to_string());

        // Forbidden opcodes that break determinism
        let mut forbidden_opcodes = HashSet::new();
        forbidden_opcodes.insert("f32.const".to_string());
        forbidden_opcodes.insert("f64.const".to_string());
        forbidden_opcodes.insert("f32.add".to_string());
        forbidden_opcodes.insert("f32.sub".to_string());
        forbidden_opcodes.insert("f32.mul".to_string());
        forbidden_opcodes.insert("f32.div".to_string());
        forbidden_opcodes.insert("f64.add".to_string());
        forbidden_opcodes.insert("f64.sub".to_string());
        forbidden_opcodes.insert("f64.mul".to_string());
        forbidden_opcodes.insert("f64.div".to_string());
        // All floating point operations
        for op in &[
            "f32.abs",
            "f32.neg",
            "f32.ceil",
            "f32.floor",
            "f32.trunc",
            "f32.nearest",
            "f32.sqrt",
            "f32.min",
            "f32.max",
            "f32.copysign",
            "f64.abs",
            "f64.neg",
            "f64.ceil",
            "f64.floor",
            "f64.trunc",
            "f64.nearest",
            "f64.sqrt",
            "f64.min",
            "f64.max",
            "f64.copysign",
            "f32.eq",
            "f32.ne",
            "f32.lt",
            "f32.gt",
            "f32.le",
            "f32.ge",
            "f64.eq",
            "f64.ne",
            "f64.lt",
            "f64.gt",
            "f64.le",
            "f64.ge",
        ] {
            forbidden_opcodes.insert(op.to_string());
        }

        Self {
            max_size: MAX_PROGRAM_SIZE,
            max_functions: MAX_FUNCTIONS,
            max_globals: MAX_GLOBALS,
            max_tables: MAX_TABLES,
            max_memories: MAX_MEMORIES,
            max_function_params: MAX_FUNCTION_PARAMS,
            max_function_locals: MAX_FUNCTION_LOCALS,
            protocol_version: PROTOCOL_VERSION,
            allowed_imports,
            forbidden_opcodes,
        }
    }
}

/// Validation error details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ValidationError {
    /// Invalid magic bytes
    InvalidMagic,
    /// Invalid version
    InvalidVersion {
        /// Version found in bytecode
        found: u32,
        /// Version expected
        expected: u32,
    },
    /// Program too large
    TooLarge {
        /// Actual size in bytes
        size: usize,
        /// Maximum allowed size
        max: usize,
    },
    /// Too many functions
    TooManyFunctions {
        /// Number of functions found
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Too many globals
    TooManyGlobals {
        /// Number of globals found
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Too many tables
    TooManyTables {
        /// Number of tables found
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Too many memories
    TooManyMemories {
        /// Number of memories found
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Forbidden import
    ForbiddenImport {
        /// Name of forbidden import
        name: String,
    },
    /// Unknown import
    UnknownImport {
        /// Name of unknown import
        name: String,
    },
    /// Forbidden opcode
    ForbiddenOpcode {
        /// Opcode that is forbidden
        opcode: String,
    },
    /// Too many function parameters
    TooManyParams {
        /// Function index
        func: usize,
        /// Number of parameters
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Too many function locals
    TooManyLocals {
        /// Function index
        func: usize,
        /// Number of locals
        count: usize,
        /// Maximum allowed
        max: usize,
    },
    /// Invalid section
    InvalidSection {
        /// Name of invalid section
        section: String,
    },
    /// Missing entrypoint
    MissingEntrypoint,
    /// Invalid entrypoint signature
    InvalidEntrypointSignature,
    /// Parse error
    ParseError {
        /// Error message from parser
        message: String,
    },
}

impl std::fmt::Display for ValidationError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ValidationError::InvalidMagic => write!(f, "Invalid WASM magic bytes"),
            ValidationError::InvalidVersion { found, expected } => {
                write!(f, "Invalid version: found {}, expected {}", found, expected)
            }
            ValidationError::TooLarge { size, max } => {
                write!(f, "Program too large: {} bytes (max {})", size, max)
            }
            ValidationError::TooManyFunctions { count, max } => {
                write!(f, "Too many functions: {} (max {})", count, max)
            }
            ValidationError::TooManyGlobals { count, max } => {
                write!(f, "Too many globals: {} (max {})", count, max)
            }
            ValidationError::TooManyTables { count, max } => {
                write!(f, "Too many tables: {} (max {})", count, max)
            }
            ValidationError::TooManyMemories { count, max } => {
                write!(f, "Too many memories: {} (max {})", count, max)
            }
            ValidationError::ForbiddenImport { name } => {
                write!(f, "Forbidden import: {}", name)
            }
            ValidationError::UnknownImport { name } => {
                write!(f, "Unknown import: {}", name)
            }
            ValidationError::ForbiddenOpcode { opcode } => {
                write!(f, "Forbidden opcode: {}", opcode)
            }
            ValidationError::TooManyParams { func, count, max } => {
                write!(
                    f,
                    "Function {} has too many params: {} (max {})",
                    func, count, max
                )
            }
            ValidationError::TooManyLocals { func, count, max } => {
                write!(
                    f,
                    "Function {} has too many locals: {} (max {})",
                    func, count, max
                )
            }
            ValidationError::InvalidSection { section } => {
                write!(f, "Invalid section: {}", section)
            }
            ValidationError::MissingEntrypoint => {
                write!(f, "Missing entrypoint function")
            }
            ValidationError::InvalidEntrypointSignature => {
                write!(f, "Invalid entrypoint signature")
            }
            ValidationError::ParseError { message } => {
                write!(f, "Parse error: {}", message)
            }
        }
    }
}

/// Validation result
pub type ValidationResult<T> = Result<T, ValidationError>;

/// Validated bytecode information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValidatedBytecode {
    /// Code hash (BLAKE3)
    pub code_hash: [u8; 32],
    /// Program size
    pub size: usize,
    /// Number of functions
    pub function_count: usize,
    /// Number of globals
    pub global_count: usize,
    /// Has memory section
    pub has_memory: bool,
    /// Memory pages (initial)
    pub memory_pages: u32,
    /// Imported functions
    pub imports: Vec<String>,
    /// Exported functions
    pub exports: Vec<String>,
    /// Protocol version
    pub protocol_version: u32,
}

/// Bytecode validator
pub struct BytecodeValidator {
    config: ValidationConfig,
}

impl BytecodeValidator {
    /// Create a new validator with default config
    pub fn new() -> Self {
        Self {
            config: ValidationConfig::default(),
        }
    }

    /// Create a validator with custom config
    pub fn with_config(config: ValidationConfig) -> Self {
        Self { config }
    }

    /// Get reference to allowed imports
    pub fn allowed_imports(&self) -> &std::collections::HashSet<String> {
        &self.config.allowed_imports
    }

    /// Get reference to configuration
    pub fn config(&self) -> &ValidationConfig {
        &self.config
    }

    /// Validate bytecode and return information
    pub fn validate(&self, bytecode: &[u8]) -> ValidationResult<ValidatedBytecode> {
        // Check size
        if bytecode.len() > self.config.max_size {
            return Err(ValidationError::TooLarge {
                size: bytecode.len(),
                max: self.config.max_size,
            });
        }

        // Check WASM magic
        if bytecode.len() < 8 {
            return Err(ValidationError::InvalidMagic);
        }

        let magic = &bytecode[0..4];
        if magic != [0x00, 0x61, 0x73, 0x6d] {
            return Err(ValidationError::InvalidMagic);
        }

        // Check WASM version (must be 1)
        let version = u32::from_le_bytes([bytecode[4], bytecode[5], bytecode[6], bytecode[7]]);
        if version != 1 {
            return Err(ValidationError::InvalidVersion {
                found: version,
                expected: 1,
            });
        }

        // Parse and validate using wasmi's validation
        let engine = wasmi::Engine::default();
        let module =
            wasmi::Module::new(&engine, bytecode).map_err(|e| ValidationError::ParseError {
                message: e.to_string(),
            })?;

        // Collect module information
        let mut imports = Vec::new();
        let mut exports = Vec::new();

        // Validate imports
        for import in module.imports() {
            let import_name = format!("{}:{}", import.module(), import.name());

            // Check if import is allowed
            let base_name = import.name().to_string();
            if !self.config.allowed_imports.contains(&base_name) {
                // Check if it starts with any allowed prefix
                let is_allowed = self
                    .config
                    .allowed_imports
                    .iter()
                    .any(|allowed| base_name.starts_with(allowed));
                if !is_allowed {
                    return Err(ValidationError::UnknownImport { name: import_name });
                }
            }

            imports.push(import_name);
        }

        // Collect exports
        for export in module.exports() {
            exports.push(export.name().to_string());
        }

        // Check for entrypoint
        let has_entrypoint = exports
            .iter()
            .any(|e| e == "entrypoint" || e == "process_instruction");
        if !has_entrypoint {
            return Err(ValidationError::MissingEntrypoint);
        }

        // Compute code hash
        let code_hash = *blake3::hash(bytecode).as_bytes();

        Ok(ValidatedBytecode {
            code_hash,
            size: bytecode.len(),
            function_count: exports.len(), // Approximation
            global_count: 0,               // Would need deeper inspection
            has_memory: true,              // Assume true for valid WASM
            memory_pages: 0,               // Would need deeper inspection
            imports,
            exports,
            protocol_version: self.config.protocol_version,
        })
    }

    /// Quick check if bytecode looks valid (fast path)
    pub fn quick_check(&self, bytecode: &[u8]) -> bool {
        if bytecode.len() < 8 || bytecode.len() > self.config.max_size {
            return false;
        }

        // Check WASM magic
        bytecode[0..4] == [0x00, 0x61, 0x73, 0x6d]
    }

    /// Compute code hash without full validation
    pub fn compute_hash(bytecode: &[u8]) -> [u8; 32] {
        *blake3::hash(bytecode).as_bytes()
    }
}

impl Default for BytecodeValidator {
    fn default() -> Self {
        Self::new()
    }
}

/// Code hash allowlist for approved programs
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct CodeHashAllowlist {
    /// Allowed code hashes
    hashes: HashSet<[u8; 32]>,
    /// Whether allowlist is enforced
    enforced: bool,
}

impl CodeHashAllowlist {
    /// Create a new allowlist
    pub fn new() -> Self {
        Self::default()
    }

    /// Create an enforced allowlist
    pub fn enforced() -> Self {
        Self {
            hashes: HashSet::new(),
            enforced: true,
        }
    }

    /// Add a hash to the allowlist
    pub fn allow(&mut self, hash: [u8; 32]) {
        self.hashes.insert(hash);
    }

    /// Remove a hash from the allowlist
    pub fn revoke(&mut self, hash: &[u8; 32]) {
        self.hashes.remove(hash);
    }

    /// Check if a hash is allowed
    pub fn is_allowed(&self, hash: &[u8; 32]) -> bool {
        if !self.enforced {
            return true;
        }
        self.hashes.contains(hash)
    }

    /// Set enforcement status
    pub fn set_enforced(&mut self, enforced: bool) {
        self.enforced = enforced;
    }

    /// Get number of allowed hashes
    pub fn len(&self) -> usize {
        self.hashes.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.hashes.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Minimal valid WASM module
    const MINIMAL_WASM: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, // magic
        0x01, 0x00, 0x00, 0x00, // version 1
        // Type section
        0x01, 0x04, 0x01, 0x60, 0x00, 0x00, // Function section
        0x03, 0x02, 0x01, 0x00, // Export section - exports "entrypoint"
        0x07, 0x0e, 0x01, 0x0a, 0x65, 0x6e, 0x74, 0x72, 0x79, 0x70, 0x6f, 0x69, 0x6e, 0x74, 0x00,
        0x00, // Code section
        0x0a, 0x04, 0x01, 0x02, 0x00, 0x0b,
    ];

    #[test]
    fn test_validator_quick_check() {
        let validator = BytecodeValidator::new();

        assert!(validator.quick_check(MINIMAL_WASM));
        assert!(!validator.quick_check(&[0x00, 0x00, 0x00, 0x00])); // Wrong magic
        assert!(!validator.quick_check(&[0x00, 0x61])); // Too short
    }

    #[test]
    fn test_validator_size_limit() {
        let mut config = ValidationConfig::default();
        config.max_size = 10;
        let validator = BytecodeValidator::with_config(config);

        let result = validator.validate(MINIMAL_WASM);
        assert!(matches!(result, Err(ValidationError::TooLarge { .. })));
    }

    #[test]
    fn test_compute_hash_determinism() {
        let hash1 = BytecodeValidator::compute_hash(MINIMAL_WASM);
        let hash2 = BytecodeValidator::compute_hash(MINIMAL_WASM);
        assert_eq!(hash1, hash2);

        let hash3 = BytecodeValidator::compute_hash(&[1, 2, 3]);
        assert_ne!(hash1, hash3);
    }

    #[test]
    fn test_allowlist() {
        let mut allowlist = CodeHashAllowlist::enforced();

        let hash1 = [1u8; 32];
        let hash2 = [2u8; 32];

        assert!(!allowlist.is_allowed(&hash1));

        allowlist.allow(hash1);
        assert!(allowlist.is_allowed(&hash1));
        assert!(!allowlist.is_allowed(&hash2));

        allowlist.revoke(&hash1);
        assert!(!allowlist.is_allowed(&hash1));
    }

    #[test]
    fn test_allowlist_unenforced() {
        let allowlist = CodeHashAllowlist::new();

        let hash = [1u8; 32];
        // Unenforced allowlist allows everything
        assert!(allowlist.is_allowed(&hash));
    }
}
