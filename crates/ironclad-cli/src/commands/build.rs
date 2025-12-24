//! ironclad build - Build the smart contract

use crate::config::IroncladConfig;
use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;
use std::time::Instant;

pub async fn run(release: bool, verify: bool, verbose: bool) -> IroncladResult<()> {
    let start = Instant::now();
    let project = Project::find()?;

    println!(
        "{} {} {}",
        "🔨".bold(),
        "Building".cyan().bold(),
        project.config.project.name.green()
    );

    // Create progress spinner
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    // Step 1: Compile to WASM
    pb.set_message("Compiling to WASM...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--target")
        .arg(&project.config.build.target)
        .current_dir(&project.root);

    if release {
        cmd.arg("--release");
    }

    // Add rustflags if configured
    if !project.config.build.rustflags.is_empty() {
        cmd.env("RUSTFLAGS", project.config.build.rustflags.join(" "));
    }

    if verbose {
        cmd.arg("-v");
    }

    let output = cmd
        .output()
        .map_err(|e| IroncladError::BuildFailed(format!("Failed to run cargo: {}", e)))?;

    if !output.status.success() {
        pb.finish_and_clear();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        return Err(IroncladError::BuildFailed("Cargo build failed".to_string()));
    }

    pb.set_message("Locating WASM artifact...");

    // Find WASM file
    let wasm_path = project.wasm_path(release);
    if !wasm_path.exists() {
        pb.finish_and_clear();
        return Err(IroncladError::BuildFailed(format!(
            "WASM file not found at {}",
            wasm_path.display()
        )));
    }

    let wasm_size = std::fs::metadata(&wasm_path)?.len();

    pb.finish_and_clear();
    println!(
        "  {} WASM compiled: {} ({})",
        "✓".green(),
        wasm_path.display().to_string().dimmed(),
        format_size(wasm_size)
    );

    // Step 2: Extract IDL if configured
    if project.config.build.generate_idl {
        println!("  {} Extracting IDL...", "→".dimmed());

        match extract_idl(&wasm_path) {
            Ok(idl) => {
                let idl_path = project.idl_path();
                std::fs::create_dir_all(idl_path.parent().unwrap())?;
                std::fs::write(&idl_path, idl)?;
                println!(
                    "  {} IDL generated: {}",
                    "✓".green(),
                    idl_path.display().to_string().dimmed()
                );
            }
            Err(e) => {
                println!(
                    "  {} IDL extraction failed: {}",
                    "⚠".yellow(),
                    e.to_string().dimmed()
                );
            }
        }
    }

    // Step 3: Verify manifest if requested
    if verify {
        println!("  {} Verifying manifest...", "→".dimmed());

        let verify_output = Command::new("dchat")
            .arg("program")
            .arg("validate")
            .arg("--wasm")
            .arg(&wasm_path)
            .output();

        match verify_output {
            Ok(output) if output.status.success() => {
                println!("  {} Manifest verified", "✓".green());
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "  {} Manifest verification failed: {}",
                    "✗".red(),
                    stderr.trim()
                );
            }
            Err(e) => {
                println!(
                    "  {} Could not verify manifest: {}",
                    "⚠".yellow(),
                    e.to_string().dimmed()
                );
            }
        }
    }

    // Print summary
    let elapsed = start.elapsed();
    println!();
    println!(
        "{}",
        format!("✅ Built in {:.2}s", elapsed.as_secs_f64())
            .green()
            .bold()
    );

    if verbose {
        println!();
        println!("  {}", "Build artifacts:".cyan());
        println!("    WASM: {}", wasm_path.display());
        if project.config.build.generate_idl {
            println!("    IDL:  {}", project.idl_path().display());
        }
    }

    Ok(())
}

fn extract_idl(wasm_path: &std::path::Path) -> IroncladResult<String> {
    // Try to extract manifest from WASM
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Look for DPLM section
    for (i, window) in wasm_bytes.windows(4).enumerate() {
        if window == b"DPLM" {
            // Found manifest, extract it
            if i + 8 > wasm_bytes.len() {
                return Err(IroncladError::IdlError("Truncated manifest".to_string()));
            }

            let version = wasm_bytes[i + 4];
            let len_bytes: [u8; 2] = [wasm_bytes[i + 5], wasm_bytes[i + 6]];
            let len = u16::from_le_bytes(len_bytes) as usize;

            if i + 7 + len > wasm_bytes.len() {
                return Err(IroncladError::IdlError(
                    "Invalid manifest length".to_string(),
                ));
            }

            let manifest_data = &wasm_bytes[i + 7..i + 7 + len];

            // Convert manifest to IDL JSON
            let idl = manifest_to_idl(manifest_data, version)?;
            return Ok(idl);
        }
    }

    Err(IroncladError::IdlError(
        "No DPLM manifest found in WASM".to_string(),
    ))
}

fn manifest_to_idl(data: &[u8], version: u8) -> IroncladResult<String> {
    // Parse manifest and generate IDL JSON
    // This is a simplified version - in production would fully parse the manifest

    let idl = serde_json::json!({
        "version": "0.1.0",
        "name": "program",
        "metadata": {
            "manifest_version": version,
            "manifest_size": data.len()
        },
        "instructions": [],
        "accounts": [],
        "types": [],
        "events": [],
        "errors": []
    });

    serde_json::to_string_pretty(&idl)
        .map_err(|e| IroncladError::IdlError(format!("Failed to serialize IDL: {}", e)))
}

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
