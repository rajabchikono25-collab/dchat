//! DPL Integration Tests
//!
//! Tests for the DPL (Dchat Program Language) framework including:
//! - WASI shim functionality
//! - Manifest parsing
//! - Validation of WASI imports
//! - Determinism verification
//! - Legacy contract compatibility

use std::collections::HashSet;

use dchat_programs::manifest::{
    Capabilities, DplManifest, ImportProfile, DPL_MANIFEST_MAGIC, DPL_MANIFEST_SIZE,
};
use dchat_programs::validation::{BytecodeValidator, ValidationConfig, ValidationError};
use dchat_programs::wasi_shim::{
    is_allowed_wasi_import, is_forbidden_wasi_import, ALLOWED_WASI_IMPORTS, FORBIDDEN_WASI_IMPORTS,
};

// ═══════════════════════════════════════════════════════════════════════════════
// WASI SHIM TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_allowed_wasi_imports() {
    // These WASI imports should be allowed (deterministic subset)
    let allowed = vec![
        "fd_write",
        "proc_exit",
        "environ_sizes_get",
        "environ_get",
        "args_sizes_get",
        "args_get",
        "fd_prestat_get",
        "fd_prestat_dir_name",
        "fd_close",
    ];

    for import in allowed {
        assert!(
            is_allowed_wasi_import(import),
            "Expected {} to be allowed",
            import
        );
    }
}

#[test]
fn test_forbidden_wasi_imports() {
    // These WASI imports should be forbidden (non-deterministic)
    let forbidden = vec![
        "clock_time_get",
        "clock_res_get",
        "random_get",
        "fd_read",
        "path_open",
        "sock_recv",
        "poll_oneoff",
        "sched_yield",
    ];

    for import in forbidden {
        assert!(
            is_forbidden_wasi_import(import),
            "Expected {} to be forbidden",
            import
        );
    }
}

#[test]
fn test_wasi_import_lists_are_disjoint() {
    // Ensure no import is in both allowed and forbidden lists
    let allowed_set: HashSet<&str> = ALLOWED_WASI_IMPORTS.iter().copied().collect();
    let forbidden_set: HashSet<&str> = FORBIDDEN_WASI_IMPORTS.iter().copied().collect();

    let intersection: Vec<_> = allowed_set.intersection(&forbidden_set).collect();
    assert!(
        intersection.is_empty(),
        "WASI import lists should be disjoint, found overlap: {:?}",
        intersection
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// MANIFEST TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_manifest_roundtrip() {
    let manifest = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0x42; 32],
        capabilities: Capabilities::EMITS_EVENTS | Capabilities::USES_PDAS,
    };

    let bytes = manifest.to_bytes();
    assert_eq!(bytes.len(), DPL_MANIFEST_SIZE);

    let parsed = DplManifest::from_bytes(&bytes).expect("Should parse");

    assert_eq!(parsed.sdk_major, 0);
    assert_eq!(parsed.sdk_minor, 1);
    assert_eq!(parsed.sdk_patch, 0);
    assert_eq!(parsed.edition, 2025);
    assert_eq!(parsed.abi_version, 1);
    assert_eq!(parsed.import_profile, ImportProfile::Wasi);
    assert_eq!(parsed.schema_hash, [0x42; 32]);
    assert!(parsed.capabilities.contains(Capabilities::EMITS_EVENTS));
    assert!(parsed.capabilities.contains(Capabilities::USES_PDAS));
}

#[test]
fn test_manifest_magic() {
    // Valid magic
    let mut valid = [0u8; DPL_MANIFEST_SIZE];
    valid[0..4].copy_from_slice(&DPL_MANIFEST_MAGIC);
    assert!(DplManifest::from_bytes(&valid).is_ok());

    // Invalid magic
    let mut invalid = [0u8; DPL_MANIFEST_SIZE];
    invalid[0..4].copy_from_slice(b"XXXX");
    assert!(DplManifest::from_bytes(&invalid).is_err());
}

#[test]
fn test_manifest_size_validation() {
    // Too short
    let short = [0u8; 32];
    assert!(DplManifest::from_bytes(&short).is_err());

    // Too long (should fail - exact size required)
    let long = [0u8; 128];
    assert!(DplManifest::from_bytes(&long).is_err());
}

#[test]
fn test_manifest_import_profiles() {
    for (profile, expected_byte) in [
        (ImportProfile::Legacy, 0),
        (ImportProfile::Wasi, 1),
        (ImportProfile::Hybrid, 2),
    ] {
        let manifest = DplManifest {
            sdk_major: 0,
            sdk_minor: 1,
            sdk_patch: 0,
            edition: 2025,
            abi_version: 1,
            import_profile: profile,
            schema_hash: [0; 32],
            capabilities: Capabilities::empty(),
        };

        let bytes = manifest.to_bytes();
        assert_eq!(bytes[13], expected_byte);
    }
}

#[test]
fn test_manifest_capabilities() {
    let all_caps = Capabilities::EMITS_EVENTS
        | Capabilities::USES_CPI
        | Capabilities::USES_PDAS
        | Capabilities::REQUIRES_SIGNERS
        | Capabilities::USES_TOKENS
        | Capabilities::USES_PRIVACY
        | Capabilities::USES_CAPABILITIES
        | Capabilities::UPGRADEABLE
        | Capabilities::USES_GOVERNANCE
        | Capabilities::USES_STAKING;

    let manifest = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0; 32],
        capabilities: all_caps,
    };

    let bytes = manifest.to_bytes();
    let parsed = DplManifest::from_bytes(&bytes).unwrap();

    assert!(parsed.capabilities.contains(Capabilities::EMITS_EVENTS));
    assert!(parsed.capabilities.contains(Capabilities::USES_CPI));
    assert!(parsed.capabilities.contains(Capabilities::USES_PDAS));
    assert!(parsed.capabilities.contains(Capabilities::REQUIRES_SIGNERS));
    assert!(parsed.capabilities.contains(Capabilities::USES_TOKENS));
    assert!(parsed.capabilities.contains(Capabilities::USES_PRIVACY));
    assert!(parsed
        .capabilities
        .contains(Capabilities::USES_CAPABILITIES));
    assert!(parsed.capabilities.contains(Capabilities::UPGRADEABLE));
    assert!(parsed.capabilities.contains(Capabilities::USES_GOVERNANCE));
    assert!(parsed.capabilities.contains(Capabilities::USES_STAKING));
}

#[test]
fn test_manifest_version_string() {
    let manifest = DplManifest {
        sdk_major: 1,
        sdk_minor: 2,
        sdk_patch: 3,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Legacy,
        schema_hash: [0; 32],
        capabilities: Capabilities::empty(),
    };

    assert_eq!(manifest.sdk_version_string(), "1.2.3");
}

#[test]
fn test_manifest_compatibility() {
    let manifest = DplManifest {
        sdk_major: 0,
        sdk_minor: 1,
        sdk_patch: 0,
        edition: 2025,
        abi_version: 1,
        import_profile: ImportProfile::Wasi,
        schema_hash: [0; 32],
        capabilities: Capabilities::empty(),
    };

    // Compatible with same or newer ABI
    assert!(manifest.is_compatible(1));
    assert!(manifest.is_compatible(2));
    assert!(manifest.is_compatible(255));

    // Incompatible with older ABI
    assert!(!manifest.is_compatible(0));
}

// ═══════════════════════════════════════════════════════════════════════════════
// VALIDATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_validation_config_has_wasi_imports() {
    let config = ValidationConfig::default();

    // Check that WASI imports are in the allowed list
    assert!(config.allowed_imports.contains("wasi:fd_write"));
    assert!(config.allowed_imports.contains("wasi:proc_exit"));
    assert!(config.allowed_imports.contains("wasi:environ_sizes_get"));
    assert!(config.allowed_imports.contains("wasi:environ_get"));
    assert!(config.allowed_imports.contains("wasi:args_sizes_get"));
    assert!(config.allowed_imports.contains("wasi:args_get"));
}

#[test]
fn test_validation_config_has_env_imports() {
    let config = ValidationConfig::default();

    // Legacy env imports should still be allowed
    assert!(config.allowed_imports.contains("sol_log_"));
    assert!(config.allowed_imports.contains("sol_sha256"));
    assert!(config.allowed_imports.contains("sol_blake3"));
    assert!(config.allowed_imports.contains("sol_alloc_free_"));
}

#[test]
fn test_validator_quick_check() {
    let validator = BytecodeValidator::new();

    // Valid WASM magic
    let valid_wasm = vec![
        0x00, 0x61, 0x73, 0x6d, // Magic: \0asm
        0x01, 0x00, 0x00, 0x00, // Version: 1
    ];
    assert!(validator.quick_check(&valid_wasm));

    // Invalid magic
    let invalid_magic = vec![0x00, 0x00, 0x00, 0x00, 0x01, 0x00, 0x00, 0x00];
    assert!(!validator.quick_check(&invalid_magic));

    // Too short
    let too_short = vec![0x00, 0x61, 0x73, 0x6d];
    assert!(!validator.quick_check(&too_short));

    // Empty
    assert!(!validator.quick_check(&[]));
}

#[test]
fn test_compute_hash_deterministic() {
    let bytecode = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];

    let hash1 = BytecodeValidator::compute_hash(&bytecode);
    let hash2 = BytecodeValidator::compute_hash(&bytecode);

    assert_eq!(hash1, hash2, "Hash should be deterministic");

    // Different input should produce different hash
    let different = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x01];
    let hash3 = BytecodeValidator::compute_hash(&different);
    assert_ne!(
        hash1, hash3,
        "Different input should produce different hash"
    );
}

// ═══════════════════════════════════════════════════════════════════════════════
// DETERMINISM TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_forbidden_opcodes_are_floating_point() {
    let config = ValidationConfig::default();

    // All floating point operations should be forbidden
    let float_ops = vec![
        "f32.const",
        "f64.const",
        "f32.add",
        "f32.sub",
        "f32.mul",
        "f32.div",
        "f64.add",
        "f64.sub",
        "f64.mul",
        "f64.div",
        "f32.abs",
        "f32.neg",
        "f32.sqrt",
        "f64.abs",
        "f64.neg",
        "f64.sqrt",
    ];

    for op in float_ops {
        assert!(
            config.forbidden_opcodes.contains(op),
            "Expected {} to be forbidden for determinism",
            op
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// LEGACY COMPATIBILITY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_legacy_imports_still_allowed() {
    let config = ValidationConfig::default();

    // All legacy sol_* imports should still be allowed for backward compatibility
    let legacy_imports = vec![
        "sol_log_",
        "sol_log_64_",
        "sol_log_compute_units_",
        "sol_invoke_signed_c",
        "sol_invoke_signed_rust",
        "sol_alloc_free_",
        "sol_set_return_data",
        "sol_get_return_data",
        "sol_sha256",
        "sol_blake3",
        "sol_keccak256",
        "sol_memcpy",
        "sol_memset",
        "sol_memmove",
        "sol_memcmp",
        "sol_verify_capability",
        "sol_verify_commitment",
    ];

    for import in legacy_imports {
        assert!(
            config.allowed_imports.contains(import),
            "Legacy import {} should be allowed",
            import
        );
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// PROTOCOL VERSION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_protocol_version() {
    let config = ValidationConfig::default();

    // Protocol version should be 1
    assert_eq!(config.protocol_version, 1);
}

// ═══════════════════════════════════════════════════════════════════════════════
// INTEGRATION TESTS
// ═══════════════════════════════════════════════════════════════════════════════

/// Test that manifest extraction returns None for non-DPL WASM
#[test]
fn test_manifest_extraction_returns_none_for_legacy() {
    use dchat_programs::manifest::extract_manifest;

    // Minimal valid WASM without custom section
    let minimal_wasm = vec![
        0x00, 0x61, 0x73, 0x6d, // Magic
        0x01, 0x00, 0x00, 0x00, // Version
              // No sections
    ];

    let result = extract_manifest(&minimal_wasm);
    assert!(result.is_ok());
    assert!(
        result.unwrap().is_none(),
        "Legacy WASM should have no manifest"
    );
}

#[test]
fn test_dpl_constants() {
    // DPL SDK version should be available
    use dchat_dpl::{DPL_VERSION, DPL_VERSION_MAJOR, DPL_VERSION_MINOR, DPL_VERSION_PATCH};

    assert_eq!(DPL_VERSION, "0.1.0");
    assert_eq!(DPL_VERSION_MAJOR, 0);
    assert_eq!(DPL_VERSION_MINOR, 1);
    assert_eq!(DPL_VERSION_PATCH, 0);
}

#[test]
fn test_dpl_abi_constants() {
    use dchat_dpl::abi::{ABI_VERSION, ACCOUNTS_MAGIC, IX_MAGIC};

    // ABI version should be 1
    assert_eq!(ABI_VERSION, 1);

    // Magic bytes should be correct
    assert_eq!(IX_MAGIC, *b"DCHX");
    assert_eq!(ACCOUNTS_MAGIC, *b"DCHA");
}
