//! Build script for the DPL Escrow Ironclad example
//!
//! This demonstrates how to use the DPL build helpers for:
//! 1. Generating manifest metadata at build time
//! 2. Verifying schema hash against committed IDL (for releases)
//! 3. Ensuring reproducible builds

use std::path::PathBuf;

fn main() {
    // Use DPL build helpers for manifest metadata generation
    let config = dchat_dpl::build::BuildConfig {
        // For development, we don't require IDL verification
        #[cfg(not(feature = "release-verify"))]
        idl_path: None,

        // For release builds with verification enabled, check against committed IDL
        #[cfg(feature = "release-verify")]
        idl_path: Some(PathBuf::from("idl.json")),

        // Expected schema hash (set via environment for CI)
        expected_schema_hash: std::env::var("DPL_SCHEMA_HASH").ok(),

        // Output directory from Cargo
        out_dir: std::env::var("OUT_DIR").ok().map(PathBuf::from),

        // Enable verbose output for debugging
        verbose: std::env::var("DPL_BUILD_VERBOSE").is_ok(),
    };

    // Generate metadata - this will panic if schema verification fails
    if let Err(e) = dchat_dpl::build::generate_manifest_metadata(&config) {
        println!("cargo:warning=DPL build helper warning: {}", e);
    }

    // Re-run build if source files change
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-changed=idl.json");
    println!("cargo:rerun-if-env-changed=DPL_SCHEMA_HASH");
    println!("cargo:rerun-if-env-changed=DPL_BUILD_VERBOSE");
}
