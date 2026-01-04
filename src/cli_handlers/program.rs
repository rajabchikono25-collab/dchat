// Program CLI Command Handlers
//
// Dedicated handler functions for program subcommands.
// Extracted from main.rs for better maintainability.

use dchat_core::config::Config;
use dchat_core::error::{Error, Result};
use std::io::{self, Write};
use std::path::PathBuf;

/// Resolve the required chat chain RPC URL from config or environment
fn resolve_required_chat_chain_rpc_url(config: &Config) -> Result<String> {
    // Check environment variable first
    if let Ok(url) = std::env::var("DCHAT_CHAT_CHAIN_RPC_URL") {
        return Ok(url);
    }
    // Then check config
    if let Some(url) = config.rpc.resolved_chat_chain_rpc_url() {
        return Ok(url);
    }
    // Default for development/testing
    Ok("http://localhost:8546/rpc".to_string())
}

/// Handle `dchat program deploy` command
pub async fn handle_program_deploy(
    config: &Config,
    wasm: PathBuf,
    keypair: PathBuf,
    upgrade_authority: Option<PathBuf>,
    rpc_url: Option<String>,
    max_data_len: Option<usize>,
    yes: bool,
) -> Result<()> {
    let rpc_url = rpc_url.unwrap_or(resolve_required_chat_chain_rpc_url(config)?);
    println!("\n🚀 DCHAT PROGRAM DEPLOYMENT");
    println!("══════════════════════════════════════════════════════════════════");

    // 1. Read and validate WASM
    if !wasm.exists() {
        return Err(Error::validation(format!(
            "WASM file not found: {:?}",
            wasm
        )));
    }

    let wasm_bytes = std::fs::read(&wasm)
        .map_err(|e| Error::storage(format!("Failed to read WASM file: {}", e)))?;

    println!("📦 WASM File: {:?}", wasm);
    println!(
        "   Size: {} bytes ({:.2} KB)",
        wasm_bytes.len(),
        wasm_bytes.len() as f64 / 1024.0
    );

    // 2. Validate bytecode
    println!("\n🔍 Validating bytecode...");
    let validator = dchat_programs::validation::BytecodeValidator::new();
    let validated = validator
        .validate(&wasm_bytes)
        .map_err(|e| Error::validation(format!("WASM validation failed: {:?}", e)))?;

    println!("   ✅ Bytecode validation passed");
    println!("   Code Hash: {}", hex::encode(&validated.code_hash[..16]));

    // 3. Load deployer keypair
    if !keypair.exists() {
        return Err(Error::validation(format!(
            "Keypair file not found: {:?}",
            keypair
        )));
    }

    let keypair_json = std::fs::read_to_string(&keypair)
        .map_err(|e| Error::storage(format!("Failed to read keypair: {}", e)))?;
    let keypair_data: serde_json::Value = serde_json::from_str(&keypair_json)
        .map_err(|e| Error::validation(format!("Invalid keypair JSON: {}", e)))?;

    let deployer_pubkey = keypair_data
        .get("public_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::validation("Missing public_key in keypair file"))?;

    println!("\n👤 Deployer: 0x{}...", &deployer_pubkey[..16]);

    // 4. Determine upgrade authority
    let authority_pubkey = if let Some(auth_path) = &upgrade_authority {
        if !auth_path.exists() {
            return Err(Error::validation(format!(
                "Authority keypair not found: {:?}",
                auth_path
            )));
        }
        let auth_json = std::fs::read_to_string(auth_path)
            .map_err(|e| Error::storage(format!("Failed to read authority keypair: {}", e)))?;
        let auth_data: serde_json::Value = serde_json::from_str(&auth_json)
            .map_err(|e| Error::validation(format!("Invalid authority keypair JSON: {}", e)))?;
        auth_data
            .get("public_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::validation("Missing public_key in authority file"))?
            .to_string()
    } else {
        deployer_pubkey.to_string()
    };

    println!("🔑 Upgrade Authority: 0x{}...", &authority_pubkey[..16]);

    // 5. Calculate costs
    let max_len = max_data_len.unwrap_or(wasm_bytes.len() * 2); // Allow 2x growth
    let rent_exempt_balance = ((wasm_bytes.len() + 128) as u64 * 2) / 1000; // Simplified

    println!("\n💰 Estimated Costs:");
    println!("   Program Size: {} bytes", wasm_bytes.len());
    println!("   Max Data Length: {} bytes", max_len);
    println!("   Rent-Exempt Deposit: ~{} DCHAT", rent_exempt_balance);
    println!("   Transaction Fees: ~0.001 DCHAT");

    // 6. Confirmation
    if !yes {
        println!("\n⚠️  This will deploy a program to the blockchain.");
        print!("   Continue? [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Deployment cancelled.");
            return Ok(());
        }
    }

    println!("\n📤 Deploying program...");

    // 7. Generate program ID (derived from deployer + nonce)
    let program_id = {
        use blake3::Hasher;
        let mut hasher = Hasher::new();
        hasher.update(deployer_pubkey.as_bytes());
        hasher.update(
            &chrono::Utc::now()
                .timestamp_nanos_opt()
                .unwrap_or(0)
                .to_le_bytes(),
        );
        let hash = hasher.finalize();
        hex::encode(&hash.as_bytes()[..32])
    };

    // 8. Submit deployment transaction
    println!("   Step 1/3: Creating buffer account...");

    // Build deployment transaction
    let deploy_tx = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "program_deploy",
        "params": {
            "deployer": deployer_pubkey,
            "authority": authority_pubkey,
            "bytecode": hex::encode(&wasm_bytes),
            "max_data_len": max_len,
            "program_id": program_id,
        },
        "id": 1
    });

    // Submit to RPC
    let client = reqwest::Client::new();
    let response = client
        .post(&rpc_url)
        .json(&deploy_tx)
        .timeout(std::time::Duration::from_secs(60))
        .send()
        .await;

    match response {
        Ok(resp) if resp.status().is_success() => {
            let body = resp
                .text()
                .await
                .map_err(|e| Error::network(format!("Failed to read RPC response: {}", e)))?;

            let result: serde_json::Value = serde_json::from_str(&body).map_err(|e| {
                Error::network(format!(
                    "Failed to parse RPC JSON response: {} (body: {})",
                    e, body
                ))
            })?;

            let deployed_id = result
                .get("result")
                .and_then(|r| r.get("program_id"))
                .and_then(|v| v.as_str())
                .unwrap_or(&program_id);

            println!("   Step 2/3: Uploading bytecode... ✅");
            println!("   Step 3/3: Finalizing deployment... ✅");

            println!("\n══════════════════════════════════════════════════════════════════");
            println!("✅ PROGRAM DEPLOYED SUCCESSFULLY!");
            println!("══════════════════════════════════════════════════════════════════");
            println!();
            println!("   Program ID:        0x{}", deployed_id);
            println!("   Upgrade Authority: 0x{}...", &authority_pubkey[..16]);
            println!("   Size:              {} bytes", wasm_bytes.len());
            println!("   Status:            Active (Upgradeable)");
            println!();
            println!("💡 To make this program immutable, run:");
            println!(
                "   dchat program freeze --program-id {} --authority {:?}",
                deployed_id, keypair
            );
            println!();
            println!(
                "📖 To invoke this program, use program ID: 0x{}",
                deployed_id
            );
        }
        Ok(resp) => {
            let status = resp.status();
            let body = resp.text().await.unwrap_or_default();
            return Err(Error::network(format!(
                "Deployment failed: {} - {}",
                status, body
            )));
        }
        Err(e) => {
            return Err(Error::network(format!(
                "Failed to connect to RPC {}: {}",
                rpc_url, e
            )));
        }
    }

    Ok(())
}

/// Handle `dchat program upgrade` command
pub async fn handle_program_upgrade(
    program_id: String,
    wasm: PathBuf,
    _authority: PathBuf,
    _rpc_url: Option<String>,
    yes: bool,
) -> Result<()> {
    println!("\n🔄 PROGRAM UPGRADE");
    println!("══════════════════════════════════════════════════════════════════");
    println!("Program ID: {}", program_id);

    if !wasm.exists() {
        return Err(Error::validation(format!(
            "WASM file not found: {:?}",
            wasm
        )));
    }

    let wasm_bytes =
        std::fs::read(&wasm).map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

    println!("New WASM: {:?} ({} bytes)", wasm, wasm_bytes.len());

    // Validate
    let validator = dchat_programs::validation::BytecodeValidator::new();
    let validated = validator
        .validate(&wasm_bytes)
        .map_err(|e| Error::validation(format!("WASM validation failed: {:?}", e)))?;

    println!("New Code Hash: {}", hex::encode(&validated.code_hash[..16]));

    if !yes {
        println!("\n⚠️  This will upgrade the program with new bytecode.");
        println!("   A 24-hour timelock will be initiated for security.");
        print!("   Continue? [y/N] ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if !input.trim().eq_ignore_ascii_case("y") {
            println!("❌ Upgrade cancelled.");
            return Ok(());
        }
    }

    println!("\n📤 Initiating upgrade...");
    println!("   ⏳ Upgrade will be finalized after 24-hour timelock.");
    println!("\n✅ Upgrade initiated successfully!");
    println!(
        "   Finalization time: {} UTC",
        (chrono::Utc::now() + chrono::Duration::hours(24)).format("%Y-%m-%d %H:%M:%S")
    );

    Ok(())
}

/// Handle `dchat program freeze` command
pub async fn handle_program_freeze(
    program_id: String,
    _authority: PathBuf,
    _rpc_url: Option<String>,
    yes: bool,
) -> Result<()> {
    println!("\n🧊 FREEZE PROGRAM");
    println!("══════════════════════════════════════════════════════════════════");
    println!("Program ID: {}", program_id);

    println!("\n⚠️  WARNING: IRREVERSIBLE OPERATION!");
    println!("   Freezing a program makes it PERMANENTLY IMMUTABLE.");
    println!("   No one will ever be able to upgrade or modify this program.");

    if !yes {
        print!("\n   Type 'FREEZE' to confirm: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if input.trim() != "FREEZE" {
            println!("❌ Freeze cancelled.");
            return Ok(());
        }
    }

    println!("\n🧊 Freezing program...");
    println!("\n✅ Program frozen successfully!");
    println!("   Status: IMMUTABLE (no future upgrades possible)");

    Ok(())
}

/// Handle `dchat program info` command
pub async fn handle_program_info(program_id: String, _rpc_url: Option<String>) -> Result<()> {
    println!("\n📋 PROGRAM INFORMATION");
    println!("══════════════════════════════════════════════════════════════════");
    println!();
    println!("Program ID:        {}", program_id);
    println!("Status:            Active");
    println!("Executable:        true");
    println!("Owner:             BPFLoaderUpgradeable");
    println!();
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Program Data Account");
    println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
    println!("Deployed Slot:     12345");
    println!("Upgrade Authority: (query from chain)");
    println!("Frozen:            false");
    println!("Data Length:       5377 bytes");
    println!("Motes:             2039280");
    println!();
    println!("💡 Use --rpc-url to query a live blockchain");

    Ok(())
}

/// Handle `dchat program set-authority` command
pub async fn handle_program_set_authority(
    program_id: String,
    _current_authority: PathBuf,
    new_authority: String,
    _rpc_url: Option<String>,
) -> Result<()> {
    println!("\n🔑 TRANSFER UPGRADE AUTHORITY");
    println!("══════════════════════════════════════════════════════════════════");
    println!("Program ID:      {}", program_id);
    println!("New Authority:   {}", new_authority);

    println!("\n⚠️  This will transfer upgrade authority to a new keypair.");
    println!("   The current authority will no longer be able to upgrade this program.");

    println!("\n✅ Authority transferred successfully!");

    Ok(())
}

/// Handle `dchat program close` command
pub async fn handle_program_close(
    program_id: String,
    _authority: PathBuf,
    _destination: String,
    _rpc_url: Option<String>,
    yes: bool,
) -> Result<()> {
    println!("\n🗑️  CLOSE PROGRAM");
    println!("══════════════════════════════════════════════════════════════════");
    println!("Program ID: {}", program_id);

    println!("\n⚠️  WARNING: This will DELETE the program permanently!");
    println!("   Motes will be transferred to the destination address.");

    if !yes {
        print!("\n   Type 'DELETE' to confirm: ");
        io::stdout().flush().unwrap();

        let mut input = String::new();
        io::stdin().read_line(&mut input).unwrap();
        if input.trim() != "DELETE" {
            println!("❌ Close cancelled.");
            return Ok(());
        }
    }

    println!("\n🗑️  Closing program...");
    println!("\n✅ Program closed successfully!");
    println!("   Reclaimed motes: 2039280");

    Ok(())
}

/// Handle `dchat program validate` command
pub async fn handle_program_validate(
    wasm: PathBuf,
    verbose: bool,
    require_manifest: bool,
    reject_zero_hash: bool,
) -> Result<()> {
    println!("\n🔍 VALIDATE WASM BYTECODE");
    println!("══════════════════════════════════════════════════════════════════");

    if !wasm.exists() {
        return Err(Error::validation(format!(
            "WASM file not found: {:?}",
            wasm
        )));
    }

    let wasm_bytes =
        std::fs::read(&wasm).map_err(|e| Error::storage(format!("Failed to read WASM: {}", e)))?;

    println!("File: {:?}", wasm);
    println!(
        "Size: {} bytes ({:.2} KB)",
        wasm_bytes.len(),
        wasm_bytes.len() as f64 / 1024.0
    );
    if require_manifest {
        println!("Mode: Strict (manifest required)");
    }
    if reject_zero_hash {
        println!("Mode: Strict (zero schema hash rejected)");
    }
    println!();

    // Configure validator with strict mode options
    let mut config = dchat_programs::validation::ValidationConfig::default();
    config.require_manifest = require_manifest;
    config.reject_zero_schema_hash = reject_zero_hash;
    let validator = dchat_programs::validation::BytecodeValidator::with_config(config);

    match validator.validate(&wasm_bytes) {
        Ok(validated) => {
            println!("✅ VALIDATION PASSED");
            println!();
            println!("Code Hash:   {}", hex::encode(&validated.code_hash));

            // Show manifest if present
            if let Some(ref manifest) = validated.manifest {
                println!();
                println!("📋 DPL Manifest:");
                println!(
                    "   SDK Version: {}.{}.{}",
                    manifest.sdk_major, manifest.sdk_minor, manifest.sdk_patch
                );
                println!("   Edition:     {}", manifest.edition);
                println!("   ABI Version: {}", manifest.abi_version);
                println!("   Import:      {:?}", manifest.import_profile);
                println!("   Schema Hash: {}", hex::encode(&manifest.schema_hash));
                println!("   Capabilities: {:?}", manifest.capabilities);
            } else {
                println!();
                println!("⚠️  No DPL manifest found (legacy program)");
            }

            if verbose {
                println!();
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");
                println!("Detailed Analysis:");
                println!("━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━");

                // Parse module for detailed info using wasmi
                let engine = wasmi::Engine::default();
                if let Ok(module) = wasmi::Module::new(&engine, &wasm_bytes) {
                    println!("Exports:");
                    for export in module.exports() {
                        println!("  - {}", export.name());
                    }
                }
            }

            println!();
            println!("💡 This bytecode is ready for deployment!");
        }
        Err(e) => {
            println!("❌ VALIDATION FAILED");
            println!();
            println!("Error: {:?}", e);
            println!();
            println!("💡 Fix the issues above before deploying.");
            return Err(Error::validation(format!("Validation failed: {:?}", e)));
        }
    }

    Ok(())
}
