//! Build script for DPL Token Minter contract
//!
//! This script generates the DPL manifest metadata that gets embedded
//! into the WASM binary as a custom section.

fn main() {
    // Tell Cargo to rerun if the source changes
    println!("cargo:rerun-if-changed=src/lib.rs");
}
