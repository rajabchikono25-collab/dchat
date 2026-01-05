// Governance CLI Command Handlers
//
// Dedicated handler functions for governance subcommands.
// Handles protocol upgrade proposals, voting, and fork management.
// Extracted from main.rs for better maintainability.

use crate::governance::{
    UpgradeManager, UpgradeProposal, UpgradeStatus, UpgradeType, ValidatorSignature, Version,
};
use dchat_core::error::{Error, Result};
use dchat_core::UserId;
use dchat_crypto::{KeyPair, PrivateKey};
use std::path::PathBuf;
use tracing::{debug, info, warn};

/// Load governance database path
fn governance_db_path() -> PathBuf {
    PathBuf::from("./data/governance.db")
}

/// Ensure data directory exists
fn ensure_data_dir() {
    if let Err(e) = std::fs::create_dir_all("./data") {
        warn!("⚠️  Failed to create ./data directory: {}", e);
    }
}

/// Load validator key from JSON file
async fn load_validator_key(path: &PathBuf) -> Result<KeyPair> {
    let contents = tokio::fs::read_to_string(path).await.map_err(Error::Io)?;
    let key_json: serde_json::Value = serde_json::from_str(&contents)
        .map_err(|e| Error::Crypto(format!("Invalid key file: {}", e)))?;

    let private_key_str = key_json["private_key"]
        .as_str()
        .ok_or_else(|| Error::Crypto("Missing private_key field".to_string()))?;

    // Parse the debug format string like "[1, 2, 3, ...]"
    let bytes_str = private_key_str.trim_matches(&['[', ']'][..]);
    let mut bytes = [0u8; 32];
    for (i, byte_str) in bytes_str.split(',').enumerate().take(32) {
        bytes[i] = byte_str
            .trim()
            .parse()
            .map_err(|e| Error::Crypto(format!("Invalid byte: {}", e)))?;
    }

    let private_key = PrivateKey::from_bytes(bytes);
    Ok(KeyPair::from_private_key(private_key))
}

/// Load upgrade manager from database
async fn load_manager() -> UpgradeManager {
    let db_path = governance_db_path();
    ensure_data_dir();

    if db_path.exists() {
        info!("Loading upgrade manager state from {}", db_path.display());
        match tokio::fs::read_to_string(&db_path).await {
            Ok(json_data) => match serde_json::from_str::<UpgradeManager>(&json_data) {
                Ok(mgr) => {
                    info!("✓ Loaded upgrade manager from database");
                    info!("  Current version: {}", mgr.current_version());
                    info!("  Active proposals: {}", mgr.get_active_proposals().len());
                    return mgr;
                }
                Err(e) => {
                    warn!("Failed to deserialize upgrade manager: {}", e);
                }
            },
            Err(e) => {
                warn!("Failed to read upgrade manager from database: {}", e);
            }
        }
    }

    info!("Creating new upgrade manager instance");
    UpgradeManager::new()
}

/// Persist manager state to database
fn persist_manager(manager: &UpgradeManager) -> Result<()> {
    let db_path = governance_db_path();
    let json_data = serde_json::to_string_pretty(manager)
        .map_err(|e| Error::internal(format!("Failed to serialize manager: {}", e)))?;
    std::fs::write(&db_path, json_data)
        .map_err(|e| Error::internal(format!("Failed to save manager: {}", e)))?;
    debug!("✓ Upgrade manager state persisted to {}", db_path.display());
    Ok(())
}

/// Handle `dchat governance propose-upgrade` command
pub async fn handle_propose_upgrade(
    proposer: String,
    upgrade_type: String,
    target_version: String,
    title: String,
    description: String,
    spec_url: Option<String>,
    voting_days: u32,
    quorum: u32,
) -> Result<()> {
    println!("\n📜 Submitting Protocol Upgrade Proposal");

    let proposer_id = UserId(
        uuid::Uuid::parse_str(&proposer).map_err(|_| Error::validation("Invalid proposer ID"))?,
    );

    let upgrade_type = match upgrade_type.to_lowercase().as_str() {
        "soft-fork" => UpgradeType::SoftFork,
        "hard-fork" => UpgradeType::HardFork,
        "security-patch" => UpgradeType::SecurityPatch,
        name if name.starts_with("feature-toggle:") => {
            let feature = match name.strip_prefix("feature-toggle:") {
                Some(feature) if !feature.is_empty() => feature.to_string(),
                _ => return Err(Error::validation("Invalid feature-toggle format")),
            };
            UpgradeType::FeatureToggle { feature }
        }
        _ => {
            return Err(Error::validation(
                "Invalid upgrade type. Use: soft-fork, hard-fork, security-patch, or feature-toggle:<name>"
            ));
        }
    };

    let target = Version::parse(&target_version)?;

    let mut manager = load_manager().await;
    let current = manager.current_version().clone();

    let mut proposal = UpgradeProposal::new(
        proposer_id,
        upgrade_type,
        current,
        target,
        title.clone(),
        description.clone(),
        voting_days.into(),
        quorum as u16 * 100, // Convert percentage to bps (60% = 6000 bps)
    )?;

    if let Some(url) = spec_url {
        proposal.spec_url = Some(url);
    }

    let proposal_id = manager.submit_proposal(proposal)?;
    persist_manager(&manager)?;

    println!("✅ Proposal submitted successfully!");
    println!("Proposal ID: {}", proposal_id);
    println!("Title: {}", title);
    println!("Target Version: {}", target_version);
    println!("Voting Deadline: {} days from now", voting_days);
    println!("Required Quorum: {}%", quorum);

    Ok(())
}

/// Handle `dchat governance list-proposals` command
pub async fn handle_list_proposals(status: Option<String>) -> Result<()> {
    let manager = load_manager().await;
    let proposals = manager.get_active_proposals();

    println!("\n📊 Upgrade Proposals ({}):", proposals.len());

    if proposals.is_empty() {
        println!("No active proposals found.");
        return Ok(());
    }

    for proposal in proposals {
        // Filter by status if specified
        if let Some(ref status_filter) = status {
            let matches = match status_filter.to_lowercase().as_str() {
                "proposed" => matches!(proposal.status, UpgradeStatus::Proposed),
                "approved" => matches!(proposal.status, UpgradeStatus::Approved),
                "scheduled" => matches!(proposal.status, UpgradeStatus::Scheduled { .. }),
                "active" => matches!(proposal.status, UpgradeStatus::Active),
                "rejected" => matches!(proposal.status, UpgradeStatus::Rejected),
                "cancelled" => matches!(proposal.status, UpgradeStatus::Cancelled),
                _ => continue,
            };

            if !matches {
                continue;
            }
        }

        println!("\n{}", "-".repeat(80));
        println!("ID: {}", proposal.id);
        println!("Title: {}", proposal.title);
        println!(
            "Version: {} → {}",
            proposal.current_version, proposal.target_version
        );
        println!("Type: {:?}", proposal.upgrade_type);
        println!("Status: {:?}", proposal.status);
        println!(
            "Votes: {} for, {} against",
            proposal.votes_for, proposal.votes_against
        );
        println!("Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
        println!(
            "Deadline: {}",
            proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
        );

        if let Some(ref url) = proposal.spec_url {
            println!("Spec: {}", url);
        }
    }

    Ok(())
}

/// Handle `dchat governance get-proposal` command
pub async fn handle_get_proposal(proposal_id: String) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;

    let manager = load_manager().await;

    match manager.get_proposal(&id) {
        Some(proposal) => {
            println!("\n📋 Proposal Details");
            println!("{}", "=".repeat(80));
            println!("ID: {}", proposal.id);
            println!("Proposer: {}", proposal.proposer);
            println!("Title: {}", proposal.title);
            println!("Description:\n{}", proposal.description);
            println!(
                "\nVersion: {} → {}",
                proposal.current_version, proposal.target_version
            );
            println!("Type: {:?}", proposal.upgrade_type);
            println!("Status: {:?}", proposal.status);
            println!("\nVoting:");
            println!("  For: {}", proposal.votes_for);
            println!("  Against: {}", proposal.votes_against);
            println!("  Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
            println!(
                "  Deadline: {}",
                proposal.voting_deadline.format("%Y-%m-%d %H:%M:%S UTC")
            );

            if let Some(ref url) = proposal.spec_url {
                println!("\nSpecification: {}", url);
            }

            if !proposal.validator_signatures.is_empty() {
                println!(
                    "\nValidator Signatures: {}",
                    proposal.validator_signatures.len()
                );
                for (i, sig) in proposal.validator_signatures.iter().enumerate() {
                    println!(
                        "  {}. {} (stake: {})",
                        i + 1,
                        sig.validator_id,
                        sig.stake_amount
                    );
                }
            }

            if let Some(height) = proposal.activation_height {
                println!("\nActivation Height: {}", height);
            }
            if let Some(time) = proposal.activation_time {
                println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));
            }

            Ok(())
        }
        None => {
            println!("❌ Proposal not found: {}", proposal_id);
            Ok(())
        }
    }
}

/// Handle `dchat governance vote` command
pub async fn handle_vote(
    proposal_id: String,
    voter: String,
    vote_for: bool,
    voting_power: u64,
) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;
    let voter_id =
        UserId(uuid::Uuid::parse_str(&voter).map_err(|_| Error::validation("Invalid voter ID"))?);

    let mut manager = load_manager().await;
    manager.cast_upgrade_vote(id, voter_id, vote_for, voting_power)?;
    persist_manager(&manager)?;

    println!("\n✅ Vote cast successfully!");
    println!("Proposal: {}", proposal_id);
    println!("Voter: {}", voter);
    println!("Vote: {}", if vote_for { "FOR" } else { "AGAINST" });
    println!("Voting Power: {}", voting_power);

    Ok(())
}

/// Handle `dchat governance sign-upgrade` command
pub async fn handle_sign_upgrade(
    proposal_id: String,
    validator_id: String,
    stake: u64,
    key_file: PathBuf,
) -> Result<()> {
    use dchat_crypto::signatures::SigningKey;
    use sha2::{Digest, Sha256};

    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;
    let val_id = UserId(
        uuid::Uuid::parse_str(&validator_id)
            .map_err(|_| Error::validation("Invalid validator ID"))?,
    );

    println!("\n🔑 Validator Signing Upgrade Approval");
    println!("Proposal: {}", proposal_id);
    println!("Validator: {}", validator_id);
    println!("Stake: {}", stake);
    println!("Key File: {}", key_file.display());

    // Load validator key
    let validator_keypair = load_validator_key(&key_file).await?;

    let mut manager = load_manager().await;
    let proposal = manager
        .get_proposal(&id)
        .ok_or_else(|| Error::NotFound("Proposal not found".to_string()))?;

    // Create proposal commitment hash for signing
    let mut hasher = Sha256::new();
    hasher.update(proposal.id.as_bytes());
    hasher.update(proposal.id.as_bytes());
    hasher.update(proposal.description.as_bytes());
    let commitment = hasher.finalize().to_vec();

    info!(
        "✓ Proposal commitment created (hash: {})",
        hex::encode(&commitment)
    );

    // Sign with validator key
    let signing_key = SigningKey::from_private_key(validator_keypair.private_key());
    let signature = signing_key.sign(&commitment);
    info!("✓ Proposal signed with validator key (Ed25519)");

    let sig = ValidatorSignature {
        validator_id: val_id,
        stake_amount: stake,
        signature: signature.to_bytes().to_vec(),
        signed_at: chrono::Utc::now(),
    };

    manager
        .add_validator_signature(&id, sig)
        .map_err(|e| Error::validation(format!("Failed to add validator signature: {}", e)))?;
    persist_manager(&manager)?;

    info!(
        "✓ Validator signature recorded for proposal {} (sig: {})",
        id,
        hex::encode(&signature.to_bytes()[..8])
    );

    println!("✅ Validator signature added!");

    Ok(())
}

/// Handle `dchat governance finalize-proposal` command
pub async fn handle_finalize_proposal(proposal_id: String) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;

    let mut manager = load_manager().await;
    let passed = manager.finalize_proposal(id)?;
    persist_manager(&manager)?;

    println!("\n📊 Proposal Finalized");
    println!("Proposal ID: {}", proposal_id);
    println!(
        "Result: {}",
        if passed {
            "✅ APPROVED"
        } else {
            "❌ REJECTED"
        }
    );

    if let Some(proposal) = manager.get_proposal(&id) {
        println!("Votes For: {}", proposal.votes_for);
        println!("Votes Against: {}", proposal.votes_against);
        println!("Quorum: {:.2}%", proposal.quorum_bps as f64 / 100.0);
    }

    Ok(())
}

/// Handle `dchat governance schedule-upgrade` command
pub async fn handle_schedule_upgrade(
    proposal_id: String,
    activation_height: u64,
    activation_time: String,
) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;
    let time = chrono::DateTime::parse_from_rfc3339(&activation_time)
        .map_err(|e| Error::validation(format!("Invalid timestamp: {}", e)))?
        .with_timezone(&chrono::Utc);

    let mut manager = load_manager().await;
    manager.schedule_upgrade(id, activation_height, time)?;
    persist_manager(&manager)?;

    println!("\n⏰ Upgrade Scheduled");
    println!("Proposal ID: {}", proposal_id);
    println!("Activation Height: {}", activation_height);
    println!("Activation Time: {}", time.format("%Y-%m-%d %H:%M:%S UTC"));

    Ok(())
}

/// Handle `dchat governance activate-upgrade` command
pub async fn handle_activate_upgrade(proposal_id: String, current_height: u64) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;

    let mut manager = load_manager().await;
    manager.activate_upgrade(id, current_height)?;
    persist_manager(&manager)?;

    println!("\n🚀 Upgrade Activated!");
    println!("Proposal ID: {}", proposal_id);
    println!("Block Height: {}", current_height);
    println!("New Version: {}", manager.current_version());

    Ok(())
}

/// Handle `dchat governance cancel-upgrade` command
pub async fn handle_cancel_upgrade(proposal_id: String) -> Result<()> {
    let id = uuid::Uuid::parse_str(&proposal_id)
        .map_err(|_| Error::validation("Invalid proposal ID"))?;

    let mut manager = load_manager().await;
    manager.cancel_upgrade(id)?;
    persist_manager(&manager)?;

    println!("\n❌ Upgrade Cancelled");
    println!("Proposal ID: {}", proposal_id);

    Ok(())
}

/// Handle `dchat governance version` command
pub async fn handle_version() -> Result<()> {
    let manager = load_manager().await;
    println!(
        "\n🔖 Current Protocol Version: {}",
        manager.current_version()
    );
    Ok(())
}

/// Handle `dchat governance fork-history` command
pub async fn handle_fork_history() -> Result<()> {
    let manager = load_manager().await;
    let forks = manager.get_fork_history();

    println!("\n🌿 Fork History ({} forks):", forks.len());

    if forks.is_empty() {
        println!("No forks recorded yet.");
        return Ok(());
    }

    for fork in forks {
        println!("\n{}", "-".repeat(80));
        println!("Fork ID: {}", fork.fork_id);
        println!("Parent Version: {}", fork.parent_version);
        println!("Fork Version: {}", fork.fork_version);
        println!("Fork Height: {}", fork.fork_height);
        println!(
            "Fork Time: {}",
            fork.fork_time.format("%Y-%m-%d %H:%M:%S UTC")
        );
        println!("Supporting Nodes: {}", fork.supporting_nodes.len());
        println!("Total Stake: {}", fork.total_stake);
        println!(
            "Canonical: {}",
            if fork.is_canonical { "Yes" } else { "No" }
        );
    }

    Ok(())
}

/// Handle `dchat governance check-compatibility` command
pub async fn handle_check_compatibility(peer_version: String) -> Result<()> {
    let peer_ver = Version::parse(&peer_version)?;
    let manager = load_manager().await;

    let compatible = manager.is_compatible_version(&peer_ver);

    println!("\n🔍 Version Compatibility Check");
    println!("Current Version: {}", manager.current_version());
    println!("Peer Version: {}", peer_ver);
    println!(
        "Compatible: {}",
        if compatible { "✅ Yes" } else { "❌ No" }
    );

    if !compatible {
        println!("\n⚠️  Warning: Incompatible versions may not be able to communicate!");
    }

    Ok(())
}

/// Handle `dchat governance configure` command
pub async fn handle_configure(
    hard_fork_threshold: Option<u32>,
    total_stake: Option<u64>,
) -> Result<()> {
    let mut manager = load_manager().await;

    if let Some(threshold) = hard_fork_threshold {
        manager.set_hard_fork_threshold(threshold)?;
        println!("✅ Hard fork threshold set to {}%", threshold);
    }

    if let Some(stake) = total_stake {
        manager.update_total_stake(stake);
        println!("✅ Total stake updated to {}", stake);
    }

    persist_manager(&manager)?;
    println!("\n⚙️  Governance Configuration Updated");

    Ok(())
}
