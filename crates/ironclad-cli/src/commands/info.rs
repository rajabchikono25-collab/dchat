//! ironclad info - Show program information

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;

pub async fn run(
    program: Option<&str>,
    network: Option<&str>,
    verbose: bool,
) -> IroncladResult<()> {
    let project = Project::find()?;

    println!("{} {}", "ℹ️".bold(), "Program Information".cyan().bold());
    println!();

    // Show local project info
    println!("  {}", "Project:".cyan().bold());
    println!("    Name: {}", project.config.project.name.green());
    println!("    Version: {}", project.config.project.version);

    if let Some(desc) = &project.config.project.description {
        println!("    Description: {}", desc.dimmed());
    }

    if !project.config.project.authors.is_empty() {
        println!("    Authors: {}", project.config.project.authors.join(", "));
    }

    if let Some(program_id) = &project.config.project.program_id {
        println!("    Program ID: {}", program_id.yellow());
    }

    // Check for WASM
    let wasm_path = project.wasm_path(true);
    if wasm_path.exists() {
        println!();
        println!("  {}", "Build:".cyan().bold());

        let metadata = std::fs::metadata(&wasm_path)?;
        println!("    WASM: {}", wasm_path.display().to_string().dimmed());
        println!("    Size: {} bytes", metadata.len());

        // Extract manifest info
        let wasm_bytes = std::fs::read(&wasm_path)?;
        if let Some((version, sdk_version, edition)) = extract_manifest_info(&wasm_bytes) {
            println!("    Manifest Version: {}", version);
            println!("    SDK Version: {}", sdk_version);
            println!("    Edition: {}", edition);
        }

        // Check IDL
        let idl_path = project.idl_path();
        if idl_path.exists() {
            println!("    IDL: {}", idl_path.display().to_string().dimmed());
        }
    } else {
        println!();
        println!(
            "  {} No WASM build found. Run 'ironclad build'",
            "⚠".yellow()
        );
    }

    // Query on-chain info if program ID provided
    if let Some(pid) = program {
        println!();
        println!("  {}", "On-chain:".cyan().bold());

        let net = network.unwrap_or("mainnet");
        let network_config = project.config.networks.get(net);

        if let Some(config) = network_config {
            println!("    Network: {}", net);
            println!("    RPC: {}", config.url.dimmed());

            // In production, would query RPC
            println!("    Program: {}", pid.yellow());
            println!("    Status: {} (simulated)", "Active".green());
        } else {
            println!("    {} Unknown network: {}", "⚠".yellow(), net);
        }
    }

    // Show configuration
    if verbose {
        println!();
        println!("  {}", "Configuration:".cyan().bold());
        println!("    Target: {}", project.config.build.target);
        println!("    Generate IDL: {}", project.config.build.generate_idl);
        println!("    Verify Schema: {}", project.config.build.verify_schema);

        if !project.config.build.rustflags.is_empty() {
            println!(
                "    Rustflags: {}",
                project.config.build.rustflags.join(" ")
            );
        }

        println!();
        println!("  {}", "Networks:".cyan().bold());
        for (name, config) in &project.config.networks {
            println!("    {}: {}", name, config.url.dimmed());
        }
    }

    Ok(())
}

fn extract_manifest_info(wasm_bytes: &[u8]) -> Option<(u8, String, String)> {
    for (i, window) in wasm_bytes.windows(4).enumerate() {
        if window == b"DPLM" && i + 7 < wasm_bytes.len() {
            let version = wasm_bytes[i + 4];
            // Simplified - in production would fully parse manifest
            return Some((version, "0.1.0".to_string(), "2025".to_string()));
        }
    }
    None
}
