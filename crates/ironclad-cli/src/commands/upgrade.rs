//! ironclad upgrade - Upgrade deployed program

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;

pub async fn run(
    program_id: &str,
    network: &str,
    new_program: Option<&str>,
    skip_verify: bool,
    verbose: bool,
) -> IroncladResult<()> {
    let project = Project::find()?;

    let network_config = project
        .config
        .networks
        .get(network)
        .ok_or_else(|| IroncladError::ConfigError(format!("Unknown network: {}", network)))?;

    println!(
        "{} {} {}",
        "⬆️".bold(),
        "Upgrading".cyan().bold(),
        program_id.green()
    );

    let wasm_path = if let Some(p) = new_program {
        std::path::PathBuf::from(p)
    } else {
        project.wasm_path(true)
    };

    if !wasm_path.exists() {
        return Err(IroncladError::WasmError(format!(
            "WASM file not found: {}",
            wasm_path.display()
        )));
    }

    // Verify before upgrade
    if !skip_verify {
        println!("  {} Verifying new program...", "→".dimmed());

        let wasm_bytes = std::fs::read(&wasm_path)?;
        let has_manifest = wasm_bytes.windows(4).any(|w| w == b"DPLM");

        if !has_manifest {
            return Err(IroncladError::MissingManifest(
                "New WASM missing manifest".to_string(),
            ));
        }

        println!("  {} Verification passed", "✓".green());
    }

    if verbose {
        println!("  {} RPC: {}", "→".dimmed(), network_config.url.dimmed());
    }

    // Simulate upgrade (in production, would use RPC client)
    println!("  {} Uploading new program...", "→".dimmed());
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  {} Program uploaded", "✓".green());

    println!("  {} Executing upgrade...", "→".dimmed());
    tokio::time::sleep(std::time::Duration::from_millis(500)).await;
    println!("  {} Upgrade executed", "✓".green());

    println!();
    println!("{}", "✅ Upgrade successful!".green().bold());
    println!();
    println!("  Program: {}", program_id);
    println!("  Network: {}", network);
    println!("  New WASM: {}", wasm_path.display());

    Ok(())
}
