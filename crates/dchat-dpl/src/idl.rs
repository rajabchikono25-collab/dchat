//! IDL (Interface Definition Language) Module for DPL Programs
//!
//! This module defines the canonical IDL format for DPL programs, which is used
//! to compute the `schema_hash` embedded in the DPL manifest. The IDL describes
//! the program's instructions, accounts, types, and events in a deterministic,
//! serializable format.
//!
//! # Schema Hash Computation
//!
//! The `schema_hash` is the BLAKE3 hash of the canonically-serialized IDL.
//! This provides cryptographic binding between the deployed bytecode and its
//! expected interface, enabling "ironclad" verification.
//!
//! # Canonicalization Rules
//!
//! 1. All fields are serialized in declaration order
//! 2. Strings use UTF-8 encoding with length prefix
//! 3. Arrays use length prefix followed by elements
//! 4. Integers use little-endian encoding
//! 5. Optional fields use a boolean tag followed by value if present
//! 6. No padding or alignment bytes
//!
//! # What's Included in the Hash
//!
//! - Instruction names and discriminators
//! - Instruction argument types and order
//! - Account definitions with constraints
//! - Custom type definitions
//! - Event definitions
//!
//! # What's Excluded from the Hash
//!
//! - Documentation comments
//! - Source file locations
//! - Internal implementation details

use borsh::{BorshDeserialize, BorshSerialize};

#[cfg(not(feature = "std"))]
use alloc::{string::String, vec::Vec};

// ═══════════════════════════════════════════════════════════════════════════════
// IDL ROOT STRUCTURE
// ═══════════════════════════════════════════════════════════════════════════════

/// The root IDL structure describing a DPL program's interface.
///
/// This structure is serialized canonically to compute the schema hash.
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct Idl {
    /// IDL format version (for forward compatibility)
    pub version: IdlVersion,
    /// Program name (from #[program] module name)
    pub name: String,
    /// Program instructions
    pub instructions: Vec<IdlInstruction>,
    /// Account type definitions
    pub accounts: Vec<IdlAccountDef>,
    /// Custom type definitions (structs/enums used in instructions)
    pub types: Vec<IdlTypeDef>,
    /// Event definitions
    pub events: Vec<IdlEvent>,
    /// Error codes
    pub errors: Vec<IdlError>,
    /// Program metadata (not included in hash computation)
    #[borsh(skip)]
    pub metadata: Option<IdlMetadata>,
}

/// IDL format version
#[derive(Debug, Clone, Copy, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlVersion {
    pub major: u8,
    pub minor: u8,
}

impl IdlVersion {
    /// Current IDL version
    pub const CURRENT: Self = Self { major: 1, minor: 0 };
}

impl Default for IdlVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INSTRUCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// An instruction defined in the program
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlInstruction {
    /// Instruction name (function name)
    pub name: String,
    /// 8-byte discriminator (first 8 bytes of BLAKE3("global:<name>"))
    pub discriminator: [u8; 8],
    /// Accounts required by this instruction
    pub accounts: Vec<IdlAccountMeta>,
    /// Arguments passed to this instruction
    pub args: Vec<IdlField>,
    /// Return type (if any)
    pub returns: Option<IdlType>,
}

/// Account metadata for an instruction
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlAccountMeta {
    /// Account name
    pub name: String,
    /// Whether this account must be writable
    pub is_mut: bool,
    /// Whether this account must be a signer
    pub is_signer: bool,
    /// Whether this account is optional
    pub is_optional: bool,
    /// PDA seeds (if this is a PDA)
    pub pda: Option<IdlPda>,
    /// Associated account type (references IdlAccountDef.name)
    pub account_type: Option<String>,
}

/// PDA derivation information
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlPda {
    /// Seeds used to derive the PDA
    pub seeds: Vec<IdlSeed>,
    /// Program that owns the PDA (None = current program)
    pub program: Option<String>,
}

/// A seed component for PDA derivation
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum IdlSeed {
    /// Literal bytes
    Const(Vec<u8>),
    /// Reference to an account's pubkey
    Account(String),
    /// Reference to an instruction argument
    Arg(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// TYPES
// ═══════════════════════════════════════════════════════════════════════════════

/// A field in a struct or instruction args
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlField {
    /// Field name
    pub name: String,
    /// Field type
    pub ty: IdlType,
}

/// Type definitions for custom structs/enums
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlTypeDef {
    /// Type name
    pub name: String,
    /// Type kind (struct or enum)
    pub kind: IdlTypeKind,
}

/// Kind of type definition
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum IdlTypeKind {
    /// Struct with named fields
    Struct { fields: Vec<IdlField> },
    /// Enum with variants
    Enum { variants: Vec<IdlEnumVariant> },
}

/// An enum variant
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlEnumVariant {
    /// Variant name
    pub name: String,
    /// Variant fields (empty for unit variants)
    pub fields: Vec<IdlField>,
}

/// Account type definition (for #[account] structs)
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlAccountDef {
    /// Account type name
    pub name: String,
    /// Fields in the account data
    pub fields: Vec<IdlField>,
    /// Account discriminator (8 bytes)
    pub discriminator: [u8; 8],
}

/// Primitive and composite types
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub enum IdlType {
    // Primitives
    Bool,
    U8,
    I8,
    U16,
    I16,
    U32,
    I32,
    U64,
    I64,
    U128,
    I128,

    // Fixed-size byte arrays
    Bytes,           // Vec<u8>
    FixedBytes(u32), // [u8; N]

    // Strings
    String,

    // Pubkey (32-byte address)
    Pubkey,

    // Collections
    Vec(Box<IdlType>),
    Array(Box<IdlType>, u32), // [T; N]
    Option(Box<IdlType>),

    // Custom types (reference by name)
    Defined(String),
}

// ═══════════════════════════════════════════════════════════════════════════════
// EVENTS
// ═══════════════════════════════════════════════════════════════════════════════

/// An event emitted by the program
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlEvent {
    /// Event name
    pub name: String,
    /// Event discriminator (8 bytes)
    pub discriminator: [u8; 8],
    /// Event fields
    pub fields: Vec<IdlField>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// ERRORS
// ═══════════════════════════════════════════════════════════════════════════════

/// A custom error code
#[derive(Debug, Clone, PartialEq, Eq, BorshSerialize, BorshDeserialize)]
pub struct IdlError {
    /// Error code (numeric)
    pub code: u32,
    /// Error name
    pub name: String,
    /// Error message (not included in hash)
    #[borsh(skip)]
    pub msg: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// METADATA (NOT HASHED)
// ═══════════════════════════════════════════════════════════════════════════════

/// Program metadata (excluded from schema hash)
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct IdlMetadata {
    /// Program description
    pub description: Option<String>,
    /// Repository URL
    pub repository: Option<String>,
    /// License
    pub license: Option<String>,
    /// Build information
    pub build_info: Option<IdlBuildInfo>,
}

/// Build information for reproducibility
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdlBuildInfo {
    /// Rust compiler version
    pub rustc_version: String,
    /// DPL SDK version
    pub sdk_version: String,
    /// Build timestamp (ISO 8601)
    pub timestamp: String,
    /// Git commit hash
    pub git_hash: Option<String>,
}

// ═══════════════════════════════════════════════════════════════════════════════
// SCHEMA HASH COMPUTATION
// ═══════════════════════════════════════════════════════════════════════════════

impl Idl {
    /// Create a new empty IDL
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            version: IdlVersion::CURRENT,
            name: name.into(),
            instructions: Vec::new(),
            accounts: Vec::new(),
            types: Vec::new(),
            events: Vec::new(),
            errors: Vec::new(),
            metadata: None,
        }
    }

    /// Serialize the IDL to canonical bytes for hashing.
    ///
    /// This excludes metadata and documentation, including only the
    /// structural elements that define the program's interface.
    pub fn to_canonical_bytes(&self) -> Vec<u8> {
        // Use Borsh for deterministic serialization
        borsh::to_vec(self).expect("IDL serialization should never fail")
    }

    /// Compute the schema hash (BLAKE3) of this IDL.
    ///
    /// This is the value that gets embedded in the DPL manifest's `schema_hash` field.
    pub fn schema_hash(&self) -> [u8; 32] {
        let bytes = self.to_canonical_bytes();
        *blake3::hash(&bytes).as_bytes()
    }

    /// Verify that a schema hash matches this IDL
    pub fn verify_hash(&self, expected: &[u8; 32]) -> bool {
        &self.schema_hash() == expected
    }

    /// Add an instruction to the IDL
    pub fn add_instruction(&mut self, instruction: IdlInstruction) {
        self.instructions.push(instruction);
    }

    /// Add an account definition
    pub fn add_account(&mut self, account: IdlAccountDef) {
        self.accounts.push(account);
    }

    /// Add a type definition
    pub fn add_type(&mut self, typedef: IdlTypeDef) {
        self.types.push(typedef);
    }

    /// Add an event
    pub fn add_event(&mut self, event: IdlEvent) {
        self.events.push(event);
    }

    /// Add an error code
    pub fn add_error(&mut self, error: IdlError) {
        self.errors.push(error);
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// DISCRIMINATOR COMPUTATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Compute a discriminator for a given namespace and name.
///
/// The discriminator is the first 8 bytes of BLAKE3(namespace:name).
/// This matches the scheme used by the #[program] macro.
pub fn compute_discriminator(namespace: &str, name: &str) -> [u8; 8] {
    let input = format!("{}:{}", namespace, name);
    let hash = blake3::hash(input.as_bytes());
    let mut discriminator = [0u8; 8];
    discriminator.copy_from_slice(&hash.as_bytes()[..8]);
    discriminator
}

/// Compute instruction discriminator (uses "global" namespace)
pub fn instruction_discriminator(name: &str) -> [u8; 8] {
    compute_discriminator("global", name)
}

/// Compute account discriminator (uses "account" namespace)
pub fn account_discriminator(name: &str) -> [u8; 8] {
    compute_discriminator("account", name)
}

/// Compute event discriminator (uses "event" namespace)
pub fn event_discriminator(name: &str) -> [u8; 8] {
    compute_discriminator("event", name)
}

// ═══════════════════════════════════════════════════════════════════════════════
// JSON SERIALIZATION (for external tooling)
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(feature = "std")]
impl Idl {
    /// Convert the IDL to a pretty-printed JSON string.
    ///
    /// Note: JSON serialization is for human consumption and tooling.
    /// The schema hash is always computed from Borsh serialization.
    #[cfg(feature = "idl")]
    pub fn to_json(&self) -> Result<String, serde_json::Error> {
        serde_json::to_string_pretty(self)
    }

    /// Parse an IDL from JSON.
    #[cfg(feature = "idl")]
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// BUILDER HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

impl IdlInstruction {
    /// Create a new instruction with computed discriminator
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let discriminator = instruction_discriminator(&name);
        Self {
            name,
            discriminator,
            accounts: Vec::new(),
            args: Vec::new(),
            returns: None,
        }
    }

    /// Add an account to this instruction
    pub fn with_account(mut self, account: IdlAccountMeta) -> Self {
        self.accounts.push(account);
        self
    }

    /// Add an argument to this instruction
    pub fn with_arg(mut self, name: impl Into<String>, ty: IdlType) -> Self {
        self.args.push(IdlField {
            name: name.into(),
            ty,
        });
        self
    }

    /// Set return type
    pub fn with_returns(mut self, ty: IdlType) -> Self {
        self.returns = Some(ty);
        self
    }
}

impl IdlAccountMeta {
    /// Create a new read-only account
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            is_mut: false,
            is_signer: false,
            is_optional: false,
            pda: None,
            account_type: None,
        }
    }

    /// Mark as mutable
    pub fn mutable(mut self) -> Self {
        self.is_mut = true;
        self
    }

    /// Mark as signer
    pub fn signer(mut self) -> Self {
        self.is_signer = true;
        self
    }

    /// Mark as optional
    pub fn optional(mut self) -> Self {
        self.is_optional = true;
        self
    }

    /// Set account type
    pub fn with_type(mut self, ty: impl Into<String>) -> Self {
        self.account_type = Some(ty.into());
        self
    }
}

impl IdlAccountDef {
    /// Create a new account definition with computed discriminator
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let discriminator = account_discriminator(&name);
        Self {
            name,
            fields: Vec::new(),
            discriminator,
        }
    }

    /// Add a field
    pub fn with_field(mut self, name: impl Into<String>, ty: IdlType) -> Self {
        self.fields.push(IdlField {
            name: name.into(),
            ty,
        });
        self
    }
}

impl IdlEvent {
    /// Create a new event with computed discriminator
    pub fn new(name: impl Into<String>) -> Self {
        let name = name.into();
        let discriminator = event_discriminator(&name);
        Self {
            name,
            discriminator,
            fields: Vec::new(),
        }
    }

    /// Add a field
    pub fn with_field(mut self, name: impl Into<String>, ty: IdlType) -> Self {
        self.fields.push(IdlField {
            name: name.into(),
            ty,
        });
        self
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_idl_hash_is_stable() {
        let idl = Idl::new("test_program");
        let hash = idl.schema_hash();

        // Hash should be non-zero
        assert_ne!(hash, [0u8; 32]);

        // Hash should be deterministic
        let hash2 = Idl::new("test_program").schema_hash();
        assert_eq!(hash, hash2);
    }

    #[test]
    fn test_different_programs_have_different_hashes() {
        let idl1 = Idl::new("program_a");
        let idl2 = Idl::new("program_b");

        assert_ne!(idl1.schema_hash(), idl2.schema_hash());
    }

    #[test]
    fn test_instruction_changes_hash() {
        let mut idl1 = Idl::new("test");
        let mut idl2 = Idl::new("test");

        idl1.add_instruction(IdlInstruction::new("foo"));
        idl2.add_instruction(IdlInstruction::new("bar"));

        assert_ne!(idl1.schema_hash(), idl2.schema_hash());
    }

    #[test]
    fn test_instruction_discriminator() {
        let disc = instruction_discriminator("initialize");

        // Should be 8 bytes
        assert_eq!(disc.len(), 8);

        // Should be deterministic
        assert_eq!(disc, instruction_discriminator("initialize"));

        // Different names should produce different discriminators
        assert_ne!(disc, instruction_discriminator("transfer"));
    }

    #[test]
    fn test_account_discriminator() {
        let disc = account_discriminator("MyAccount");
        assert_eq!(disc.len(), 8);
        assert_ne!(disc, instruction_discriminator("MyAccount")); // Different namespace
    }

    #[test]
    fn test_idl_roundtrip() {
        let mut idl = Idl::new("test_program");

        idl.add_instruction(
            IdlInstruction::new("transfer")
                .with_account(IdlAccountMeta::new("from").mutable().signer())
                .with_account(IdlAccountMeta::new("to").mutable())
                .with_arg("amount", IdlType::U64),
        );

        idl.add_account(
            IdlAccountDef::new("TokenAccount")
                .with_field("owner", IdlType::Pubkey)
                .with_field("balance", IdlType::U64),
        );

        // Serialize and deserialize
        let bytes = idl.to_canonical_bytes();
        let restored: Idl = borsh::from_slice(&bytes).unwrap();

        // Should be identical
        assert_eq!(idl, restored);
        assert_eq!(idl.schema_hash(), restored.schema_hash());
    }

    #[test]
    fn test_verify_hash() {
        let idl = Idl::new("test");
        let hash = idl.schema_hash();

        assert!(idl.verify_hash(&hash));

        let wrong_hash = [0x42u8; 32];
        assert!(!idl.verify_hash(&wrong_hash));
    }

    #[test]
    fn test_builder_pattern() {
        let instruction = IdlInstruction::new("create_account")
            .with_account(IdlAccountMeta::new("payer").mutable().signer())
            .with_account(IdlAccountMeta::new("new_account").mutable())
            .with_account(IdlAccountMeta::new("system_program"))
            .with_arg("space", IdlType::U64)
            .with_arg("owner", IdlType::Pubkey);

        assert_eq!(instruction.name, "create_account");
        assert_eq!(instruction.accounts.len(), 3);
        assert_eq!(instruction.args.len(), 2);
        assert!(instruction.accounts[0].is_signer);
        assert!(instruction.accounts[0].is_mut);
    }
}
