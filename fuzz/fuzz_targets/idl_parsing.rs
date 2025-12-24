//! Fuzz target for IDL parsing (both JSON and Borsh formats)
//!
//! This fuzz target tests IDL deserialization with arbitrary inputs to ensure
//! the parser handles malformed data gracefully. IDL parsing is used by:
//! - CLI tools for manifest verification
//! - Build systems for schema hash computation
//!
//! Security-critical: A parsing vulnerability could allow schema hash
//! collisions or bypass verification.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Test Borsh deserialization of IDL
    // This is the canonical format used for schema hash computation
    use dchat_dpl::borsh::BorshDeserialize;
    let _ = dchat_dpl::idl::Idl::try_from_slice(data);

    // Test JSON deserialization of IDL (used by CLI tools)
    if let Ok(text) = std::str::from_utf8(data) {
        let _ = serde_json::from_str::<dchat_dpl::idl::Idl>(text);
    }

    // Test individual IDL type parsing
    let _ = dchat_dpl::idl::IdlInstruction::try_from_slice(data);
    let _ = dchat_dpl::idl::IdlAccountDef::try_from_slice(data);
    let _ = dchat_dpl::idl::IdlEvent::try_from_slice(data);
    let _ = dchat_dpl::idl::IdlType::try_from_slice(data);
    let _ = dchat_dpl::idl::IdlField::try_from_slice(data);

    // Test discriminator computation with arbitrary strings
    if let Ok(text) = std::str::from_utf8(data) {
        // These should never panic regardless of input
        let _ = dchat_dpl::idl::instruction_discriminator(text);
        let _ = dchat_dpl::idl::account_discriminator(text);
        let _ = dchat_dpl::idl::event_discriminator(text);
    }

    // Test schema hash computation on partially valid IDLs
    // First, create a minimal valid IDL and add fuzzed content
    let mut idl = dchat_dpl::idl::Idl::new("fuzz_test");

    // Add instructions with fuzzed names (if valid UTF-8)
    if let Ok(name) = std::str::from_utf8(data) {
        if !name.is_empty() && name.len() < 256 {
            let instruction = dchat_dpl::idl::IdlInstruction::new(name);
            idl.add_instruction(instruction);
            // Schema hash must never panic
            let _ = idl.schema_hash();
        }
    }
});
