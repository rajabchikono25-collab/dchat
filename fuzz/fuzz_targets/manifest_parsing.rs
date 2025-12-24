//! Fuzz target for DPL manifest parsing
//!
//! This fuzz target tests the manifest parsing code with arbitrary byte inputs
//! to ensure it handles malformed data gracefully without panicking or
//! exhibiting undefined behavior.
//!
//! Security-critical: Manifest parsing is in the hot path for program deployment.
//! Any vulnerability here could allow malicious programs to be deployed.

#![no_main]

use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    // Test DplManifest::from_bytes with arbitrary input
    // This must never panic, only return errors for invalid data
    let _ = dchat_programs::manifest::DplManifest::from_bytes(data);

    // Test extract_manifest with arbitrary WASM-like input
    // This parses custom sections and must be robust against malformed data
    let _ = dchat_programs::manifest::extract_manifest(data);

    // Test parsing with exact manifest size (64 bytes)
    // This tests edge cases around the expected size
    if data.len() >= 64 {
        let manifest_sized = &data[..64];
        let _ = dchat_programs::manifest::DplManifest::from_bytes(manifest_sized);
    }

    // Test with various magic byte combinations
    if data.len() >= 4 {
        // Try with correct magic but corrupted rest
        let mut with_magic = vec![0x44, 0x50, 0x4C, 0x4D]; // "DPLM"
        with_magic.extend_from_slice(&data[4..data.len().min(64)]);
        while with_magic.len() < 64 {
            with_magic.push(0);
        }
        let _ = dchat_programs::manifest::DplManifest::from_bytes(&with_magic);
    }

    // Test Capabilities parsing with arbitrary bits
    if data.len() >= 4 {
        let bits = u32::from_le_bytes([
            data[0],
            data.get(1).copied().unwrap_or(0),
            data.get(2).copied().unwrap_or(0),
            data.get(3).copied().unwrap_or(0),
        ]);
        // Capabilities::from_bits_retain handles any u32 safely
        let _ = dchat_programs::manifest::Capabilities::from_bits_retain(bits);
    }

    // Test ImportProfile parsing with arbitrary u8 values
    for &byte in data.iter().take(256) {
        // Must not panic on any u8 value
        let _ = dchat_programs::manifest::ImportProfile::try_from(byte);
    }
});
