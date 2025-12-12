// Binary to verify ceremony artifacts
//
// Run this to verify ceremony artifacts match expected hash:
//   cargo run --release -p dchat-privacy --bin verify_ceremony
//

use dchat_privacy::ceremony_gen::{verify_ceremony_reproducibility, CeremonySeedConfig};
use std::fs;
use std::path::PathBuf;

fn main() {
    println!("═══════════════════════════════════════════════════════════════");
    println!("            dchat ZK Ceremony Artifact Verifier");
    println!("═══════════════════════════════════════════════════════════════");
    println!();

    let ceremony_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("ceremony");
    let hash_file = ceremony_dir.join("final_hash.txt");

    if !hash_file.exists() {
        eprintln!("ERROR: Ceremony artifacts not found at {:?}", ceremony_dir);
        eprintln!("Run generate_ceremony first to create artifacts.");
        std::process::exit(1);
    }

    // Read expected hash from file
    let hash_content = fs::read_to_string(&hash_file).expect("Failed to read hash file");
    let expected_hash = hash_content
        .lines()
        .filter(|line| !line.starts_with('#') && !line.trim().is_empty())
        .next()
        .expect("No hash found in file")
        .trim();

    println!("Expected hash from file:");
    println!("  {}", expected_hash);
    println!();

    if expected_hash.starts_with("PLACEHOLDER") {
        eprintln!("ERROR: Ceremony artifacts contain placeholder hash.");
        eprintln!("Run generate_ceremony to create actual artifacts.");
        std::process::exit(1);
    }

    println!("Regenerating artifacts with mainnet seed configuration...");
    println!("(This verifies reproducibility)");
    println!();

    let config = CeremonySeedConfig::mainnet_config();
    let start = std::time::Instant::now();

    match verify_ceremony_reproducibility(&config, expected_hash) {
        Ok(true) => {
            let elapsed = start.elapsed();
            println!("═══════════════════════════════════════════════════════════════");
            println!("                    VERIFICATION PASSED ✓");
            println!("═══════════════════════════════════════════════════════════════");
            println!();
            println!("Ceremony artifacts are valid and reproducible.");
            println!("Verification completed in {:.2?}", elapsed);
            println!();
        }
        Ok(false) => {
            eprintln!("═══════════════════════════════════════════════════════════════");
            eprintln!("                    VERIFICATION FAILED ✗");
            eprintln!("═══════════════════════════════════════════════════════════════");
            eprintln!();
            eprintln!("Hash mismatch! Artifacts may be tampered or corrupted.");
            eprintln!("DO NOT USE THESE ARTIFACTS IN PRODUCTION.");
            eprintln!();
            std::process::exit(1);
        }
        Err(e) => {
            eprintln!("ERROR: Verification failed with error: {}", e);
            std::process::exit(1);
        }
    }

    // Also verify the files on disk match
    println!("Verifying on-disk artifacts...");

    let files = [
        "pot_final.bin",
        "contact_circuit_final.bin",
        "contact_vk_final.bin",
        "reputation_circuit_final.bin",
        "reputation_vk_final.bin",
    ];

    let mut hasher = blake3::Hasher::new();
    for filename in &files {
        let path = ceremony_dir.join(filename);
        if !path.exists() {
            eprintln!("Missing file: {}", filename);
            std::process::exit(1);
        }
        let data = fs::read(&path).expect(&format!("Failed to read {}", filename));
        hasher.update(&data);
        println!("  ✓ {}", filename);
    }

    let computed_hash = hasher.finalize().to_hex().to_string();

    if computed_hash == expected_hash {
        println!();
        println!("On-disk artifacts verified successfully.");
        println!();
        println!("These artifacts are ready for production use.");
    } else {
        eprintln!();
        eprintln!("ERROR: On-disk artifact hash mismatch!");
        eprintln!("Expected: {}", expected_hash);
        eprintln!("Got:      {}", computed_hash);
        eprintln!();
        eprintln!("Regenerate artifacts with: cargo run --release -p dchat-privacy --bin generate_ceremony");
        std::process::exit(1);
    }
}
