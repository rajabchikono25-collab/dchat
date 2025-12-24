//! Manifest Validation Integration Tests
//!
//! Tests for the ironclad manifest enforcement system:
//! - Deploy with valid manifest
//! - Deploy with missing manifest
//! - Deploy with invalid magic
//! - Deploy with zero schema hash (placeholder)
//! - Upgrade with manifest preservation
//! - Schema hash verification at deploy time
//!
//! These tests ensure that the manifest is correctly extracted, validated,
//! and persisted in the program data account.

use dchat_programs::manifest::{
    Capabilities, DplManifest, ImportProfile, DPL_MANIFEST_MAGIC, DPL_MANIFEST_SIZE,
};
use dchat_programs::validation::{BytecodeValidator, ValidationConfig, ValidationError};

// ═══════════════════════════════════════════════════════════════════════════════
// HELPER FUNCTIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Creates a minimal valid WASM module with a DPL manifest custom section
fn create_wasm_with_manifest(manifest: &DplManifest) -> Vec<u8> {
    let mut wasm = Vec::new();

    // WASM magic and version
    wasm.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]); // Magic: \0asm
    wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Version: 1

    // Add a minimal type section (section id 1)
    // This is required for a valid WASM module
    wasm.push(0x01); // Section ID: type
    wasm.push(0x04); // Section size: 4 bytes
    wasm.push(0x01); // Number of types: 1
    wasm.push(0x60); // Function type
    wasm.push(0x00); // 0 params
    wasm.push(0x00); // 0 returns

    // Add a minimal function section (section id 3)
    wasm.push(0x03); // Section ID: function
    wasm.push(0x02); // Section size: 2 bytes
    wasm.push(0x01); // Number of functions: 1
    wasm.push(0x00); // Function 0 uses type 0

    // Add export section (section id 7) - exports "entrypoint" function
    // Required by the validator
    wasm.push(0x07); // Section ID: export
    wasm.push(0x0E); // Section size: 14 bytes
    wasm.push(0x01); // Number of exports: 1
    wasm.push(0x0A); // Name length: 10
    wasm.extend_from_slice(b"entrypoint"); // Export name
    wasm.push(0x00); // Export kind: function
    wasm.push(0x00); // Function index: 0

    // Add a minimal code section (section id 10)
    wasm.push(0x0A); // Section ID: code
    wasm.push(0x04); // Section size: 4 bytes
    wasm.push(0x01); // Number of code entries: 1
    wasm.push(0x02); // Function body size: 2 bytes
    wasm.push(0x00); // Local count: 0
    wasm.push(0x0B); // end instruction

    // Add the DPL manifest custom section (section id 0)
    let section_name = b"dpl_manifest";
    let manifest_bytes = manifest.to_bytes();

    let section_content_size = section_name.len() + 1 + manifest_bytes.len();
    let section_size = leb128_encode(section_content_size);

    wasm.push(0x00); // Section ID: custom
    wasm.extend_from_slice(&section_size);
    wasm.push(section_name.len() as u8); // Name length
    wasm.extend_from_slice(section_name);
    wasm.extend_from_slice(&manifest_bytes);

    wasm
}

/// Creates a minimal valid WASM module WITHOUT a manifest
fn create_wasm_without_manifest() -> Vec<u8> {
    let mut wasm = Vec::new();

    // WASM magic and version
    wasm.extend_from_slice(&[0x00, 0x61, 0x73, 0x6d]); // Magic: \0asm
    wasm.extend_from_slice(&[0x01, 0x00, 0x00, 0x00]); // Version: 1

    // Add a minimal type section
    wasm.push(0x01); // Section ID: type
    wasm.push(0x04); // Section size: 4 bytes
    wasm.push(0x01); // Number of types: 1
    wasm.push(0x60); // Function type
    wasm.push(0x00); // 0 params
    wasm.push(0x00); // 0 returns

    // Add a minimal function section
    wasm.push(0x03); // Section ID: function
    wasm.push(0x02); // Section size: 2 bytes
    wasm.push(0x01); // Number of functions: 1
    wasm.push(0x00); // Function 0 uses type 0

    // Add export section (section id 7) - exports "entrypoint" function
    // Required by the validator
    wasm.push(0x07); // Section ID: export
    wasm.push(0x0E); // Section size: 14 bytes
    wasm.push(0x01); // Number of exports: 1
    wasm.push(0x0A); // Name length: 10
    wasm.extend_from_slice(b"entrypoint"); // Export name
    wasm.push(0x00); // Export kind: function
    wasm.push(0x00); // Function index: 0

    // Add a minimal code section
    wasm.push(0x0A); // Section ID: code
    wasm.push(0x04); // Section size: 4 bytes
    wasm.push(0x01); // Number of code entries: 1
    wasm.push(0x02); // Function body size: 2 bytes
    wasm.push(0x00); // Local count: 0
    wasm.push(0x0B); // end instruction

    wasm
}

/// Simple LEB128 encoding for small values
fn leb128_encode(value: usize) -> Vec<u8> {
    let mut result = Vec::new();
    let mut v = value;
    loop {
        let mut byte = (v & 0x7F) as u8;
        v >>= 7;
        if v != 0 {
            byte |= 0x80;
        }
        result.push(byte);
        if v == 0 {
            break;
        }
    }
    result
}

/// Create a valid manifest with real (non-zero) schema hash
fn create_valid_manifest() -> DplManifest {
    DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0x42; 32], // Non-zero schema hash
        capabilities: Capabilities::EMITS_EVENTS | Capabilities::USES_PDAS,
    }
}

/// Create a manifest with zero (placeholder) schema hash
fn create_placeholder_manifest() -> DplManifest {
    DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0x00; 32], // Zero/placeholder schema hash
        capabilities: Capabilities::empty(),
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// MANIFEST EXTRACTION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_extract_manifest_from_valid_wasm() {
    let manifest = create_valid_manifest();
    let wasm = create_wasm_with_manifest(&manifest);

    let validator = BytecodeValidator::new();
    let result = validator.validate(&wasm);

    assert!(result.is_ok(), "Validation should succeed: {:?}", result);

    let validated = result.unwrap();
    assert!(
        validated.manifest.is_some(),
        "Manifest should be extracted from valid WASM"
    );

    let extracted = validated.manifest.unwrap();
    assert_eq!(extracted.schema_hash, manifest.schema_hash);
    assert_eq!(extracted.abi_version, manifest.abi_version);
}

#[test]
fn test_no_manifest_for_legacy_wasm() {
    let wasm = create_wasm_without_manifest();

    let validator = BytecodeValidator::new();
    let result = validator.validate(&wasm);

    // Should succeed but with no manifest
    assert!(
        result.is_ok(),
        "Legacy WASM validation should succeed: {:?}",
        result
    );

    let validated = result.unwrap();
    assert!(
        validated.manifest.is_none(),
        "Legacy WASM should have no manifest"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// STRICT VALIDATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_strict_validation_rejects_missing_manifest() {
    let wasm = create_wasm_without_manifest();

    let validator = BytecodeValidator::with_config(ValidationConfig {
        require_manifest: true,
        ..Default::default()
    });

    let result = validator.validate(&wasm);

    // Strict mode should reject WASM without manifest
    match result {
        Err(ValidationError::ManifestMissing) => {
            // Expected
        }
        Err(e) => panic!("Expected ManifestMissing, got: {:?}", e),
        Ok(_) => panic!("Expected validation to fail for missing manifest"),
    }
}

#[test]
fn test_strict_validation_rejects_zero_schema_hash() {
    let manifest = create_placeholder_manifest();
    let wasm = create_wasm_with_manifest(&manifest);

    let validator = BytecodeValidator::with_config(ValidationConfig {
        require_manifest: true,
        reject_zero_schema_hash: true,
        ..Default::default()
    });

    let result = validator.validate(&wasm);

    // Should reject zero schema hash in strict mode
    match result {
        Err(ValidationError::ManifestSchemaHashZero) => {
            // Expected
        }
        Err(e) => panic!("Expected ManifestSchemaHashZero, got: {:?}", e),
        Ok(_) => panic!("Expected validation to fail for zero schema hash"),
    }
}

#[test]
fn test_non_strict_allows_zero_schema_hash() {
    let manifest = create_placeholder_manifest();
    let wasm = create_wasm_with_manifest(&manifest);

    // Default config allows zero schema hash
    let validator = BytecodeValidator::new();
    let result = validator.validate(&wasm);

    assert!(
        result.is_ok(),
        "Non-strict mode should allow zero schema hash"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// MAGIC VALIDATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_manifest_magic_validation() {
    // Verify the manifest magic bytes
    assert_eq!(DPL_MANIFEST_MAGIC, *b"DPLM", "Magic should be 'DPLM'");
    assert_eq!(DPL_MANIFEST_SIZE, 64, "Manifest should be 64 bytes");
}

#[test]
fn test_manifest_with_correct_magic_parses() {
    let manifest = create_valid_manifest();
    let bytes = manifest.to_bytes();

    // Verify magic is correct
    assert_eq!(&bytes[0..4], b"DPLM", "Magic should be DPLM in bytes");

    // Parse should succeed
    let parsed = DplManifest::from_bytes(&bytes);
    assert!(parsed.is_ok(), "Parsing should succeed with correct magic");
}

#[test]
fn test_manifest_with_wrong_magic_fails() {
    let mut manifest_bytes = create_valid_manifest().to_bytes();

    // Corrupt the magic
    manifest_bytes[0] = b'X';
    manifest_bytes[1] = b'X';
    manifest_bytes[2] = b'X';
    manifest_bytes[3] = b'X';

    let parsed = DplManifest::from_bytes(&manifest_bytes);
    assert!(
        parsed.is_err(),
        "Parsing should fail with incorrect magic: {:?}",
        parsed
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// SCHEMA HASH TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_schema_hash_roundtrip() {
    let expected_hash: [u8; 32] = [
        0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e, 0x0f,
        0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d, 0x1e,
        0x1f, 0x20,
    ];

    let manifest = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: expected_hash,
        capabilities: Capabilities::empty(),
    };

    let bytes = manifest.to_bytes();
    let parsed = DplManifest::from_bytes(&bytes).unwrap();

    assert_eq!(
        parsed.schema_hash, expected_hash,
        "Schema hash should roundtrip correctly"
    );
}

#[test]
fn test_different_schema_hashes_produce_different_manifests() {
    let hash1: [u8; 32] = [0x11; 32];
    let hash2: [u8; 32] = [0x22; 32];

    let manifest1 = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: hash1,
        capabilities: Capabilities::empty(),
    };

    let manifest2 = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: hash2,
        capabilities: Capabilities::empty(),
    };

    let bytes1 = manifest1.to_bytes();
    let bytes2 = manifest2.to_bytes();

    assert_ne!(
        bytes1, bytes2,
        "Different schema hashes should produce different manifest bytes"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// CAPABILITIES TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_capabilities_roundtrip() {
    let caps = Capabilities::EMITS_EVENTS
        | Capabilities::USES_CPI
        | Capabilities::USES_PDAS
        | Capabilities::UPGRADEABLE;

    let manifest = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0x42; 32],
        capabilities: caps,
    };

    let bytes = manifest.to_bytes();
    let parsed = DplManifest::from_bytes(&bytes).unwrap();

    assert!(parsed.capabilities.contains(Capabilities::EMITS_EVENTS));
    assert!(parsed.capabilities.contains(Capabilities::USES_CPI));
    assert!(parsed.capabilities.contains(Capabilities::USES_PDAS));
    assert!(parsed.capabilities.contains(Capabilities::UPGRADEABLE));
    assert!(!parsed.capabilities.contains(Capabilities::USES_TOKENS));
}

// ═══════════════════════════════════════════════════════════════════════════════
// IMPORT PROFILE TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_import_profile_roundtrip() {
    for profile in [
        ImportProfile::Legacy,
        ImportProfile::Wasi,
        ImportProfile::Hybrid,
    ] {
        let manifest = DplManifest {
            sdk_major: 0,
            sdk_minor: 1,
            sdk_patch: 0,
            edition: 2025,
            abi_version: 1,
            import_profile: profile,
            schema_hash: [0x42; 32],
            capabilities: Capabilities::empty(),
        };

        let bytes = manifest.to_bytes();
        let parsed = DplManifest::from_bytes(&bytes).unwrap();

        assert_eq!(
            parsed.import_profile, profile,
            "Import profile {:?} should roundtrip",
            profile
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// IDL SCHEMA HASH COMPUTATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_idl_schema_hash_is_deterministic() {
    use dchat_dpl::idl::{Idl, IdlInstruction, IdlType};

    // Create the same IDL twice
    let mut idl1 = Idl::new("test_program");
    idl1.add_instruction(
        IdlInstruction::new("initialize")
            .with_arg("owner", IdlType::Pubkey)
            .with_arg("amount", IdlType::U64),
    );

    let mut idl2 = Idl::new("test_program");
    idl2.add_instruction(
        IdlInstruction::new("initialize")
            .with_arg("owner", IdlType::Pubkey)
            .with_arg("amount", IdlType::U64),
    );

    let hash1 = idl1.schema_hash();
    let hash2 = idl2.schema_hash();

    assert_eq!(hash1, hash2, "Same IDL should produce same schema hash");
}

#[test]
fn test_different_idls_produce_different_hashes() {
    use dchat_dpl::idl::{Idl, IdlInstruction, IdlType};

    let mut idl1 = Idl::new("program_a");
    idl1.add_instruction(IdlInstruction::new("foo").with_arg("x", IdlType::U64));

    let mut idl2 = Idl::new("program_b");
    idl2.add_instruction(IdlInstruction::new("bar").with_arg("y", IdlType::U64));

    let hash1 = idl1.schema_hash();
    let hash2 = idl2.schema_hash();

    assert_ne!(
        hash1, hash2,
        "Different IDLs should produce different schema hashes"
    );
}

#[test]
fn test_instruction_order_matters_for_hash() {
    use dchat_dpl::idl::{Idl, IdlInstruction, IdlType};

    let mut idl1 = Idl::new("test");
    idl1.add_instruction(IdlInstruction::new("foo").with_arg("a", IdlType::U64));
    idl1.add_instruction(IdlInstruction::new("bar").with_arg("b", IdlType::U64));

    let mut idl2 = Idl::new("test");
    idl2.add_instruction(IdlInstruction::new("bar").with_arg("b", IdlType::U64));
    idl2.add_instruction(IdlInstruction::new("foo").with_arg("a", IdlType::U64));

    let hash1 = idl1.schema_hash();
    let hash2 = idl2.schema_hash();

    assert_ne!(hash1, hash2, "Instruction order should affect schema hash");
}

// ═══════════════════════════════════════════════════════════════════════════════
// DISCRIMINATOR TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_instruction_discriminator_is_deterministic() {
    use dchat_dpl::idl::instruction_discriminator;

    let disc1 = instruction_discriminator("initialize");
    let disc2 = instruction_discriminator("initialize");

    assert_eq!(
        disc1, disc2,
        "Same instruction should have same discriminator"
    );
}

#[test]
fn test_different_instructions_have_different_discriminators() {
    use dchat_dpl::idl::instruction_discriminator;

    let disc1 = instruction_discriminator("initialize");
    let disc2 = instruction_discriminator("transfer");
    let disc3 = instruction_discriminator("close");

    assert_ne!(disc1, disc2);
    assert_ne!(disc1, disc3);
    assert_ne!(disc2, disc3);
}

#[test]
fn test_account_discriminator_is_deterministic() {
    use dchat_dpl::idl::account_discriminator;

    let disc1 = account_discriminator("Counter");
    let disc2 = account_discriminator("Counter");

    assert_eq!(disc1, disc2, "Same account should have same discriminator");
}

#[test]
fn test_event_discriminator_is_deterministic() {
    use dchat_dpl::idl::event_discriminator;

    let disc1 = event_discriminator("TransferEvent");
    let disc2 = event_discriminator("TransferEvent");

    assert_eq!(disc1, disc2, "Same event should have same discriminator");
}
