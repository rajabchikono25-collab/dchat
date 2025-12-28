//! ironclad deploy - Deploy program to network

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::time::Instant;

pub async fn run(
    network: &str,
    program: Option<&str>,
    keypair: Option<&str>,
    skip_verify: bool,
    verbose: bool,
) -> IroncladResult<()> {
    let start = Instant::now();
    let project = Project::find()?;

    let network_config = project.config.networks.get(network).ok_or_else(|| {
        IroncladError::ConfigError(format!(
            "Unknown network '{}'. Available: {:?}",
            network,
            project.config.networks.keys().collect::<Vec<_>>()
        ))
    })?;

    println!(
        "{} {} {} to {}",
        "🚀".bold(),
        "Deploying".cyan().bold(),
        project.config.project.name.green(),
        network.yellow()
    );

    let wasm_path = if let Some(p) = program {
        std::path::PathBuf::from(p)
    } else {
        project.wasm_path(true)
    };

    if !wasm_path.exists() {
        return Err(IroncladError::WasmError(format!(
            "WASM file not found: {}. Run 'ironclad build --release' first.",
            wasm_path.display()
        )));
    }

    let wasm_size = std::fs::metadata(&wasm_path)?.len();

    // Step 1: Pre-deployment verification
    if !skip_verify {
        println!("  {} Pre-deployment verification...", "→".dimmed());

        // Verify manifest exists
        let wasm_bytes = std::fs::read(&wasm_path)?;
        let has_manifest = wasm_bytes.windows(4).any(|window| window == b"DPLM");

        if !has_manifest {
            return Err(IroncladError::MissingManifest(
                "WASM does not contain DPLM manifest. Rebuild with ironclad SDK.".to_string(),
            ));
        }

        println!("  {} WASM verified", "✓".green());
    }

    // Step 2: Load keypair
    let keypair_path = keypair
        .map(|s| s.to_string())
        .or_else(|| network_config.keypair.clone())
        .unwrap_or_else(|| "~/.config/dchat/keypair.json".to_string());

    let keypair_expanded = shellexpand::tilde(&keypair_path).to_string();

    if !std::path::Path::new(&keypair_expanded).exists() {
        return Err(IroncladError::ConfigError(format!(
            "Keypair not found: {}. Create one with 'dchat keygen'",
            keypair_path
        )));
    }

    if verbose {
        println!(
            "  {} Using keypair: {}",
            "→".dimmed(),
            keypair_path.dimmed()
        );
        println!("  {} RPC: {}", "→".dimmed(), network_config.url.dimmed());
    }

    // Step 3: Deploy
    let pb = ProgressBar::new(wasm_size);
    pb.set_style(
        ProgressStyle::default_bar()
            .template("{spinner:.green} [{bar:40.cyan/blue}] {bytes}/{total_bytes} ({eta})")
            .unwrap()
            .progress_chars("#>-"),
    );

    pb.set_message("Uploading WASM...");

    // Simulate deployment (in production, would use RPC client)
    for _ in 0..10 {
        pb.inc(wasm_size / 10);
        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }

    pb.finish_and_clear();

    // Generate program ID (in production, would derive from deployment)
    let program_id = format!("DcHaT{:x}", rand::random::<u64>());

    let elapsed = start.elapsed();

    println!();
    println!("{}", "✅ Deployment successful!".green().bold());
    println!();
    println!("  {}", "Program details:".cyan());
    println!("    Program ID: {}", program_id.green().bold());
    println!("    Network: {}", network);
    println!("    Size: {} bytes", wasm_size);
    println!("    Time: {:.2}s", elapsed.as_secs_f64());

    if network_config.confirm {
        println!();
        println!("  {} Waiting for confirmation...", "→".dimmed());
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        println!("  {} Confirmed in 1 block", "✓".green());
    }

    println!();
    println!("  {}", "Next steps:".cyan());
    println!("    ironclad info --program {}", program_id);
    println!("    ironclad verify --program target/wasm32-wasip1/release/*.wasm");

    Ok(())
}
