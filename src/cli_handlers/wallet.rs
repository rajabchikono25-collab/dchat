// Wallet CLI Command Handlers
//
// Dedicated handler functions for wallet subcommands.
// Extracted from main.rs for better maintainability.

use dchat_blockchain::currency_chain::{CurrencyChainClient, CurrencyChainConfig};
use dchat_blockchain::wallet::{Wallet, WalletConfig};
use dchat_core::error::{Error, Result};
use dchat_core::UserId;
use dchat_crypto::MnemonicLength;
use std::io::{self, Write};
use std::path::PathBuf;
use uuid::Uuid;

/// Format token amounts for display (assumes 6 decimal places)
fn format_tokens(amount: u64) -> String {
    let whole = amount / 1_000_000;
    let frac = amount % 1_000_000;
    if frac == 0 {
        format!("{}", whole)
    } else {
        format!("{}.{:06}", whole, frac)
            .trim_end_matches('0')
            .to_string()
    }
}

/// Handle `dchat wallet create` command
pub async fn handle_wallet_create(name: String, output: PathBuf) -> Result<()> {
    println!("💰 Creating new wallet: {}", name);

    // Create proper production wallet with BIP-39 mnemonic
    let config = WalletConfig {
        name: name.clone(),
        ..Default::default()
    };
    let (wallet, mnemonic_phrase) = Wallet::create(config, MnemonicLength::Words24, None)?;

    let user_id = UserId(wallet.id());

    // Get addresses
    let dchat_address = wallet
        .address()
        .map(|a| a.to_hex())
        .unwrap_or_else(|| "N/A".to_string());
    let solana_address = wallet
        .solana_address()
        .map(|a| a.to_string())
        .unwrap_or_else(|| "N/A".to_string());
    let public_key_hex = wallet
        .public_key()
        .map(|pk| hex::encode(pk.as_bytes()))
        .unwrap_or_else(|| "N/A".to_string());

    // Create wallet backup data
    let wallet_data = serde_json::json!({
        "name": name,
        "user_id": user_id.0.to_string(),
        "public_key": public_key_hex,
        "address_dchat": dchat_address,
        "address_solana": solana_address,
        "created_at": chrono::Utc::now().to_rfc3339(),
        "version": "2.0",
        "wallet_type": "normal"
    });

    std::fs::write(&output, serde_json::to_string_pretty(&wallet_data)?)?;

    println!("\n✅ Wallet created successfully!");
    println!("Name: {}", name);
    println!("Wallet ID: {}", user_id.0);
    println!("DCHAT Address: {}", dchat_address);
    println!("Solana Address: {}", solana_address);
    println!("Public Key: {}", public_key_hex);
    println!("Saved to: {:?}", output);
    println!();
    println!("⚠️  CRITICAL: Write down your recovery phrase and store it safely!");
    println!("╔══════════════════════════════════════════════════════════════════╗");
    println!("║                     RECOVERY PHRASE (24 words)                    ║");
    println!("╠══════════════════════════════════════════════════════════════════╣");
    for (i, word) in mnemonic_phrase.split_whitespace().enumerate() {
        print!("  {:2}. {:<12}", i + 1, word);
        if (i + 1) % 4 == 0 {
            println!();
        }
    }
    println!("╚══════════════════════════════════════════════════════════════════╝");
    println!();
    println!("⚠️  Never share this phrase. Anyone with it can access your funds!");

    Ok(())
}

/// Handle `dchat wallet balance` command
pub async fn handle_wallet_balance(user_id: String) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n💰 Wallet Balance:");
    println!("══════════════════════════════════════════════════════════");
    println!("User ID: {}", user_id);
    println!();

    // Query blockchain for wallet balance
    let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
        .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

    let chain_config = CurrencyChainConfig {
        rpc_url,
        ..Default::default()
    };

    match CurrencyChainClient::new(chain_config) {
        Ok(currency_chain) => {
            // Query wallet from blockchain
            match currency_chain.get_wallet(&uid) {
                Ok(Some(wallet)) => {
                    println!("Available Balance: {} DCHAT", format_tokens(wallet.balance));
                    println!("Staked Balance:    {} DCHAT", format_tokens(wallet.staked));
                    println!(
                        "Pending Rewards:   {} DCHAT",
                        format_tokens(wallet.rewards_pending)
                    );
                    println!("──────────────────────────");
                    let total = wallet.balance + wallet.staked + wallet.rewards_pending;
                    println!("Total Assets:      {} DCHAT", format_tokens(total));
                }
                Ok(None) => {
                    println!("Available Balance: 0 DCHAT");
                    println!("Staked Balance:    0 DCHAT");
                    println!("Pending Rewards:   0 DCHAT");
                    println!("──────────────────────────");
                    println!("Total Assets:      0 DCHAT");
                    println!();
                    println!("💡 Wallet not found. Create one with: dchat wallet create");
                }
                Err(e) => {
                    tracing::warn!("Failed to query wallet from chain: {}", e);
                    println!("⚠️  Unable to query blockchain: {}", e);
                    println!();
                    println!("Check that DCHAT_CURRENCY_RPC_URL is set correctly.");
                }
            }
        }
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
            println!();
            println!("Set DCHAT_CURRENCY_RPC_URL environment variable to configure.");
        }
    }

    println!();
    println!("💡 Earn tokens by running a relay node or staking!");

    Ok(())
}

/// Handle `dchat wallet export` command
pub async fn handle_wallet_export(
    user_id: String,
    output: PathBuf,
    password: Option<String>,
) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    // Get password if not provided
    let pass = match password {
        Some(p) => p,
        None => {
            print!("Enter encryption password: ");
            io::stdout().flush()?;
            let mut pass = String::new();
            io::stdin().read_line(&mut pass)?;
            pass.trim().to_string()
        }
    };

    if pass.len() < 8 {
        return Err(Error::validation("Password must be at least 8 characters"));
    }

    println!("\n📤 Exporting wallet: {}", user_id);

    // Query wallet data from blockchain
    let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
        .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

    let chain_config = CurrencyChainConfig {
        rpc_url,
        ..Default::default()
    };

    let wallet_info = match CurrencyChainClient::new(chain_config) {
        Ok(currency_chain) => match currency_chain.get_wallet(&uid) {
            Ok(Some(w)) => Some((w.balance, w.staked, w.rewards_pending)),
            _ => None,
        },
        _ => None,
    };

    // Create export data with encryption
    let export_payload = serde_json::json!({
        "user_id": user_id,
        "balance": wallet_info.as_ref().map(|(b, _, _)| b).unwrap_or(&0),
        "staked": wallet_info.as_ref().map(|(_, s, _)| s).unwrap_or(&0),
        "rewards": wallet_info.as_ref().map(|(_, _, r)| r).unwrap_or(&0),
        "exported_at": chrono::Utc::now().to_rfc3339(),
    });

    // Encrypt with password using dchat_crypto
    let plaintext = serde_json::to_vec(&export_payload)?;
    let encrypted = dchat_crypto::encrypt_with_password(&pass, &plaintext)?;

    let export_data = serde_json::json!({
        "version": "2.0",
        "encrypted": true,
        "algorithm": "aes-256-gcm",
        "kdf": "argon2id",
        "ciphertext": hex::encode(&encrypted.ciphertext),
        "nonce": hex::encode(&encrypted.nonce),
        "salt": hex::encode(&encrypted.salt),
        "argon2_hash": encrypted.argon2_hash,
    });

    std::fs::write(&output, serde_json::to_string_pretty(&export_data)?)?;

    println!("✅ Wallet exported to: {:?}", output);
    println!();
    println!("⚠️  Store this backup securely and remember your password!");

    Ok(())
}

/// Handle `dchat wallet import` command
pub async fn handle_wallet_import(file: PathBuf, password: Option<String>) -> Result<()> {
    if !file.exists() {
        return Err(Error::NotFound(format!("File not found: {:?}", file)));
    }

    // Get password if not provided
    let pass = match password {
        Some(p) => p,
        None => {
            print!("Enter decryption password: ");
            io::stdout().flush()?;
            let mut pass = String::new();
            io::stdin().read_line(&mut pass)?;
            pass.trim().to_string()
        }
    };

    println!("\n📥 Importing wallet from: {:?}", file);

    let contents = std::fs::read_to_string(&file)?;
    let data: serde_json::Value = serde_json::from_str(&contents)?;

    // Check for v2.0 encrypted format
    if data.get("version").and_then(|v| v.as_str()) == Some("2.0") {
        let ciphertext = hex::decode(
            data.get("ciphertext")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::validation("Missing ciphertext"))?,
        )
        .map_err(|_| Error::validation("Invalid ciphertext hex"))?;

        let nonce_bytes = hex::decode(
            data.get("nonce")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::validation("Missing nonce"))?,
        )
        .map_err(|_| Error::validation("Invalid nonce hex"))?;

        let salt_bytes = hex::decode(
            data.get("salt")
                .and_then(|v| v.as_str())
                .ok_or_else(|| Error::validation("Missing salt"))?,
        )
        .map_err(|_| Error::validation("Invalid salt hex"))?;

        let argon2_hash = data
            .get("argon2_hash")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();

        let nonce: [u8; 12] = nonce_bytes
            .try_into()
            .map_err(|_| Error::validation("Invalid nonce length (expected 12 bytes)"))?;

        let salt: [u8; 16] = salt_bytes
            .try_into()
            .map_err(|_| Error::validation("Invalid salt length (expected 16 bytes)"))?;

        let encrypted = dchat_crypto::EncryptedData {
            version: 1,
            ciphertext,
            nonce,
            salt,
            argon2_hash,
        };

        let plaintext = dchat_crypto::decrypt_with_password(&pass, &encrypted)?;
        let wallet_data: serde_json::Value = serde_json::from_slice(&plaintext)?;

        let user_id = wallet_data
            .get("user_id")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::validation("Missing user_id in decrypted data"))?;

        println!("✅ Wallet imported successfully!");
        println!("User ID: {}", user_id);

        if let Some(balance) = wallet_data.get("balance").and_then(|v| v.as_u64()) {
            println!("Last known balance: {} DCHAT", format_tokens(balance));
        }
    } else {
        // Legacy v1.0 format (unencrypted)
        if let Some(user_id) = data.get("user_id") {
            println!("✅ Wallet imported successfully!");
            println!("User ID: {}", user_id);
            println!(
                "⚠️  This is a legacy v1.0 wallet file. Consider re-exporting with encryption."
            );
        } else {
            return Err(Error::validation("Invalid wallet file format"));
        }
    }

    Ok(())
}

/// Handle `dchat wallet history` command
pub async fn handle_wallet_history(user_id: String, limit: usize) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    println!("\n📜 Transaction History (last {}):", limit);
    println!("══════════════════════════════════════════════════════════");
    println!(
        "{:<24} {:<12} {:>15} {:<20}",
        "Date", "Type", "Amount", "Status"
    );
    println!("{}", "-".repeat(75));

    // Query blockchain for transaction history
    let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
        .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

    let chain_config = CurrencyChainConfig {
        rpc_url,
        ..Default::default()
    };

    match CurrencyChainClient::new(chain_config) {
        Ok(currency_chain) => match currency_chain.get_transaction_history(&uid, limit) {
            Ok(transactions) if !transactions.is_empty() => {
                for tx in transactions {
                    let date = tx.timestamp.format("%Y-%m-%d %H:%M:%S").to_string();
                    let tx_type = format!("{:?}", tx.tx_type);
                    let amount = format_tokens(tx.amount);
                    let status = format!("{:?}", tx.status);
                    println!("{:<24} {:<12} {:>15} {:<20}", date, tx_type, amount, status);
                }
            }
            Ok(_) => {
                println!("No transactions found.");
                println!();
                println!("💡 Transactions will appear here after your first activity.");
            }
            Err(e) => {
                tracing::warn!("Failed to fetch transaction history: {}", e);
                println!("⚠️  Unable to fetch transaction history: {}", e);
            }
        },
        Err(e) => {
            tracing::warn!("Failed to connect to currency chain: {}", e);
            println!("⚠️  Cannot connect to currency chain: {}", e);
            println!();
            println!("Set DCHAT_CURRENCY_RPC_URL environment variable to configure.");
        }
    }

    Ok(())
}

/// Handle `dchat wallet new-address` command
pub async fn handle_wallet_new_address(user_id: String) -> Result<()> {
    let uid = UserId(Uuid::parse_str(&user_id).map_err(|_| Error::validation("Invalid user ID"))?);

    // Query blockchain to get current address index for this user
    let rpc_url = std::env::var("DCHAT_CURRENCY_RPC_URL")
        .unwrap_or_else(|_| "https://currency.dchat.network/rpc".to_string());

    let chain_config = CurrencyChainConfig {
        rpc_url,
        ..Default::default()
    };

    let next_index = match CurrencyChainClient::new(chain_config) {
        Ok(currency_chain) => match currency_chain.get_address_count(&uid) {
            Ok(count) => count,
            Err(_) => 0,
        },
        Err(_) => 0,
    };

    // Generate deterministic address using user's ID as seed
    let mut hasher = blake3::Hasher::new();
    hasher.update(uid.0.as_bytes());
    hasher.update(&next_index.to_le_bytes());
    let hash = hasher.finalize();
    let address_bytes = &hash.as_bytes()[..20];
    let address = format!("dchat1{}", hex::encode(address_bytes));

    println!("\n📫 New Receiving Address:");
    println!("══════════════════════════════════════════════════════════");
    println!("{}", address);
    println!("Address Index: {}", next_index);
    println!();
    println!("Share this address to receive DCHAT tokens.");
    println!("💡 Each new address is derived from your wallet for privacy.");

    Ok(())
}
