//! Fuzz target for WASM bytecode validation
//!
//! This fuzz target tests the BytecodeValidator with arbitrary inputs to ensure
//! it correctly rejects malformed WASM while never panicking or exhibiting
//! undefined behavior.
//!
//! Security-critical: The validator is the first line of defense against
//! malicious programs. Any bypass could allow code execution attacks.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Test bytecode validation with default config
    let validator = dchat_programs::validation::BytecodeValidator::new();

    // validate() must never panic - only return Ok/Err
    let _ = validator.validate(data);

    // Test quick_check (fast rejection path)
    let _ = validator.quick_check(data);

    // Test with strict validation config (require manifest)
    let mut strict_config = dchat_programs::validation::ValidationConfig::default();
    strict_config.require_manifest = true;
    strict_config.reject_zero_schema_hash = true;
    let strict_validator =
        dchat_programs::validation::BytecodeValidator::with_config(strict_config);
    let _ = strict_validator.validate(data);

    // Test with various size limits
    for max_size in [0, 1, 64, 1024, 1024 * 1024] {
        let mut config = dchat_programs::validation::ValidationConfig::default();
        config.max_size = max_size;
        let limited_validator = dchat_programs::validation::BytecodeValidator::with_config(config);
        let _ = limited_validator.validate(data);
    }

    // Test direct manifest extraction from raw bytes
    let _ = dchat_programs::manifest::extract_manifest(data);

    // Test with WASM magic prefix but corrupted body
    if data.len() >= 8 {
        // Valid WASM magic: 0x00 0x61 0x73 0x6d (0asm)
        // Valid WASM version: 0x01 0x00 0x00 0x00
        let mut wasm_like = vec![0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00];
        wasm_like.extend_from_slice(&data[8..]);
        let _ = validator.validate(&wasm_like);
    }

    // Test with manifest custom section structure
    if data.len() >= 64 {
        // Construct a WASM-like structure with custom section
        let mut wasm = vec![
            0x00, 0x61, 0x73, 0x6d, // WASM magic
            0x01, 0x00, 0x00, 0x00, // WASM version 1
            0x00, // Custom section ID
        ];

        // Add section length (LEB128 encoded)
        let section_name = b"dpl_manifest";
        let content_len = section_name.len() + 1 + 64; // name len + name + manifest
        wasm.push(content_len as u8);
        wasm.push(section_name.len() as u8);
        wasm.extend_from_slice(section_name);
        wasm.extend_from_slice(&data[..64]); // Use fuzz data as manifest

        let _ = validator.validate(&wasm);
    }
});
