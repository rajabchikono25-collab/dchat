//! Build script for DPL Counter contract
//!
//! This script generates the DPL manifest metadata that gets embedded
//! into the WASM binary as a custom section. The manifest contains:
//!
//! - Program name and version
//! - ABI version for runtime compatibility
//! - Schema hash for IDL verification
//! - Instruction discriminators

fn main() {
    // Configure DPL build with default settings
    // The BuildConfig reads program info from Cargo.toml automatically
    let config = dchat_dpl::build::BuildConfig {
        idl_path: Some(std::path::PathBuf::from("idl/dpl_counter.json")),
        verbose: true,
        ..Default::default()
    };

    // Generate manifest metadata
    if let Err(e) = dchat_dpl::build::generate_manifest_metadata(&config) {
        // Log warning but don't fail the build
        println!("cargo:warning=DPL manifest generation: {}", e);
    }

    // Rebuild if source changes
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=idl/dpl_counter.json");
    println!("cargo:rerun-if-env-changed=DPL_SCHEMA_HASH");
}
