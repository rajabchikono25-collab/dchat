use crate::{
    BackupTier, DisasterRecoveryConfig, DistributedStorageConfig, HealthMonitorConfig,
    MultiRegionConfig, RelayNetworkConfig, StorageBackendType, StorageTier,
};
use dchat_core::{Error, Result};

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;
use tokio::fs;
use tracing::info;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentArtifacts {
    pub validators_dir: String,
    pub relays_dir: String,
    pub storage_config: String,
    pub backup_config: String,
    pub health_config: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeploymentPlanSummary {
    pub network_name: String,
    pub base_domain: String,
    pub validator_count: usize,
    pub validator_regions: usize,
    pub bft_threshold: f64,
    pub relay_count: usize,
    pub min_relays_per_region: usize,
    pub relay_regions: usize,
    pub storage_total_nodes: usize,
    pub storage_tiers: HashMap<StorageTier, Vec<StorageBackendType>>,
    pub backup_daily_storage_tb: f64,
    pub backup_estimated_monthly_cost_usd: f64,
    pub backup_tiers: HashMap<BackupTier, Vec<String>>,
    pub health_check_interval_seconds: u64,
    pub generated_at: DateTime<Utc>,
    pub artifacts: DeploymentArtifacts,
}

const VALIDATOR_SUMMARY_FILE: &str = "deployment-summary.json";
const RELAY_SUMMARY_FILE: &str = "deployment-summary.json";
const STORAGE_PLAN_FILE: &str = "storage-plan.json";
const BACKUP_PLAN_FILE: &str = "backup-plan.json";
const HEALTH_PLAN_FILE: &str = "health-plan.json";
const PLAN_SUMMARY_FILE: &str = "deployment-plan.json";

pub async fn generate_full_plan(
    network: &str,
    domain: &str,
    relay_count: usize,
    output: &Path,
) -> Result<DeploymentPlanSummary> {
    if relay_count < 20 || relay_count > 50 {
        return Err(Error::Config(format!(
            "Relay count must be between 20 and 50 (received {relay_count})"
        )));
    }

    fs::create_dir_all(output)
        .await
        .map_err(|e| Error::Config(format!("Failed to create {}: {e}", output.display())))?;

    let validators_dir = output.join("validators");
    fs::create_dir_all(&validators_dir).await.map_err(|e| {
        Error::Config(format!(
            "Failed to create validator directory {}: {e}",
            validators_dir.display()
        ))
    })?;

    let relays_dir = output.join("relays");
    fs::create_dir_all(&relays_dir).await.map_err(|e| {
        Error::Config(format!(
            "Failed to create relay directory {}: {e}",
            relays_dir.display()
        ))
    })?;

    let storage_dir = output.join("storage");
    fs::create_dir_all(&storage_dir).await.map_err(|e| {
        Error::Config(format!(
            "Failed to create storage directory {}: {e}",
            storage_dir.display()
        ))
    })?;

    let backup_dir = output.join("backup");
    fs::create_dir_all(&backup_dir).await.map_err(|e| {
        Error::Config(format!(
            "Failed to create backup directory {}: {e}",
            backup_dir.display()
        ))
    })?;

    let monitoring_dir = output.join("monitoring");
    fs::create_dir_all(&monitoring_dir).await.map_err(|e| {
        Error::Config(format!(
            "Failed to create monitoring directory {}: {e}",
            monitoring_dir.display()
        ))
    })?;

    // Validators
    let validator_config =
        MultiRegionConfig::new_recommended(network.to_string(), domain.to_string());
    validator_config.verify_decentralization().map_err(|e| {
        Error::Config(format!(
            "Validator configuration failed decentralization checks: {e}"
        ))
    })?;

    for validator in &validator_config.validators {
        let toml = validator_config
            .generate_toml(&validator.validator_id)
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to generate TOML for {}: {e}",
                    validator.validator_id
                ))
            })?;
        let file_path = validators_dir.join(format!("{}.toml", validator.validator_id));
        fs::write(&file_path, toml)
            .await
            .map_err(|e| Error::Config(format!("Failed to write {}: {e}", file_path.display())))?;
    }

    let validator_summary_path = validators_dir.join(VALIDATOR_SUMMARY_FILE);
    let validator_summary = serde_json::to_string_pretty(&validator_config)
        .map_err(|e| Error::Config(format!("Failed to serialize validator config: {e}")))?;
    fs::write(&validator_summary_path, validator_summary)
        .await
        .map_err(|e| {
            Error::Config(format!(
                "Failed to write {}: {e}",
                validator_summary_path.display()
            ))
        })?;

    // Relays
    let relay_config =
        RelayNetworkConfig::new_recommended(network.to_string(), domain.to_string(), relay_count)
            .map_err(|e| {
            Error::Config(format!(
                "Relay network configuration error for {network}: {e}"
            ))
        })?;
    relay_config
        .verify_diversity()
        .map_err(|e| Error::Config(format!("Relay configuration failed diversity checks: {e}")))?;

    for relay in &relay_config.relays {
        let toml = relay_config
            .generate_relay_toml(&relay.relay_id)
            .map_err(|e| {
                Error::Config(format!(
                    "Failed to generate TOML for {}: {e}",
                    relay.relay_id
                ))
            })?;
        let file_path = relays_dir.join(format!("{}.toml", relay.relay_id));
        fs::write(&file_path, toml)
            .await
            .map_err(|e| Error::Config(format!("Failed to write {}: {e}", file_path.display())))?;
    }

    let relay_summary_path = relays_dir.join(RELAY_SUMMARY_FILE);
    let relay_summary = serde_json::to_string_pretty(&relay_config)
        .map_err(|e| Error::Config(format!("Failed to serialize relay config: {e}")))?;
    fs::write(&relay_summary_path, relay_summary)
        .await
        .map_err(|e| {
            Error::Config(format!(
                "Failed to write {}: {e}",
                relay_summary_path.display()
            ))
        })?;

    // Storage
    let storage_config = DistributedStorageConfig::new_recommended(network.to_string());
    storage_config
        .verify_all()
        .map_err(|e| Error::Config(format!("Storage configuration invalid: {e}")))?;
    let storage_path = storage_dir.join(STORAGE_PLAN_FILE);
    let storage_json = serde_json::to_string_pretty(&storage_config)
        .map_err(|e| Error::Config(format!("Failed to serialize storage config: {e}")))?;
    fs::write(&storage_path, storage_json)
        .await
        .map_err(|e| Error::Config(format!("Failed to write {}: {e}", storage_path.display())))?;

    // Backup / disaster recovery
    let backup_config = DisasterRecoveryConfig::new_production();
    backup_config
        .verify_configuration()
        .map_err(|e| Error::Config(format!("Backup configuration invalid: {e}")))?;
    let backup_path = backup_dir.join(BACKUP_PLAN_FILE);
    let backup_json = backup_config
        .generate_json()
        .map_err(|e| Error::Config(format!("Failed to serialize backup config: {e}")))?;
    fs::write(&backup_path, backup_json)
        .await
        .map_err(|e| Error::Config(format!("Failed to write {}: {e}", backup_path.display())))?;

    // Monitoring / health
    let health_config = HealthMonitorConfig::new_production();
    health_config
        .verify()
        .map_err(|e| Error::Config(format!("Health monitoring configuration invalid: {e}")))?;
    let health_path = monitoring_dir.join(HEALTH_PLAN_FILE);
    let health_json = health_config
        .generate_json()
        .map_err(|e| Error::Config(format!("Failed to serialize health config: {e}")))?;
    fs::write(&health_path, health_json)
        .await
        .map_err(|e| Error::Config(format!("Failed to write {}: {e}", health_path.display())))?;

    let validator_regions: HashSet<_> = validator_config
        .validators
        .iter()
        .map(|v| v.region)
        .collect();
    let relay_regions: HashSet<_> = relay_config.relays.iter().map(|r| r.region).collect();

    let summary = DeploymentPlanSummary {
        network_name: validator_config.network_name.clone(),
        base_domain: validator_config.base_domain.clone(),
        validator_count: validator_config.validators.len(),
        validator_regions: validator_regions.len(),
        bft_threshold: validator_config.bft_threshold,
        relay_count: relay_config.relays.len(),
        min_relays_per_region: relay_config.min_relays_per_region,
        relay_regions: relay_regions.len(),
        storage_total_nodes: storage_config.total_node_count(),
        storage_tiers: storage_config.tier_mapping(),
        backup_daily_storage_tb: backup_config.calculate_daily_storage(),
        backup_estimated_monthly_cost_usd: backup_config.estimate_monthly_cost(),
        backup_tiers: backup_config.get_tier_mapping(),
        health_check_interval_seconds: health_config.health_check.interval_seconds,
        generated_at: Utc::now(),
        artifacts: DeploymentArtifacts {
            validators_dir: validators_dir.to_string_lossy().to_string(),
            relays_dir: relays_dir.to_string_lossy().to_string(),
            storage_config: storage_path.to_string_lossy().to_string(),
            backup_config: backup_path.to_string_lossy().to_string(),
            health_config: health_path.to_string_lossy().to_string(),
        },
    };

    let plan_summary_path = output.join(PLAN_SUMMARY_FILE);
    let summary_json = serde_json::to_string_pretty(&summary)
        .map_err(|e| Error::Config(format!("Failed to serialize deployment summary: {e}")))?;
    fs::write(&plan_summary_path, summary_json)
        .await
        .map_err(|e| {
            Error::Config(format!(
                "Failed to write {}: {e}",
                plan_summary_path.display()
            ))
        })?;

    Ok(summary)
}

pub async fn validate_plan(path: &Path) -> Result<DeploymentPlanSummary> {
    let validators_summary_path = path.join("validators").join(VALIDATOR_SUMMARY_FILE);
    let validator_json = fs::read_to_string(&validators_summary_path)
        .await
        .map_err(|e| {
            Error::Config(format!(
                "Failed to read {}: {e}",
                validators_summary_path.display()
            ))
        })?;
    let validator_config: MultiRegionConfig =
        serde_json::from_str(&validator_json).map_err(|e| {
            Error::Config(format!(
                "Failed to parse validator summary {}: {e}",
                validators_summary_path.display()
            ))
        })?;
    validator_config
        .verify_decentralization()
        .map_err(|e| Error::Config(format!("Validator summary failed checks: {e}")))?;

    let relay_summary_path = path.join("relays").join(RELAY_SUMMARY_FILE);
    let relay_json = fs::read_to_string(&relay_summary_path).await.map_err(|e| {
        Error::Config(format!(
            "Failed to read {}: {e}",
            relay_summary_path.display()
        ))
    })?;
    let relay_config: RelayNetworkConfig = serde_json::from_str(&relay_json).map_err(|e| {
        Error::Config(format!(
            "Failed to parse relay summary {}: {e}",
            relay_summary_path.display()
        ))
    })?;
    relay_config
        .verify_diversity()
        .map_err(|e| Error::Config(format!("Relay summary failed checks: {e}")))?;

    let storage_path = path.join("storage").join(STORAGE_PLAN_FILE);
    let storage_json = fs::read_to_string(&storage_path)
        .await
        .map_err(|e| Error::Config(format!("Failed to read {}: {e}", storage_path.display())))?;
    let storage_config: DistributedStorageConfig =
        serde_json::from_str(&storage_json).map_err(|e| {
            Error::Config(format!(
                "Failed to parse storage plan {}: {e}",
                storage_path.display()
            ))
        })?;
    storage_config
        .verify_all()
        .map_err(|e| Error::Config(format!("Stored storage plan invalid: {e}")))?;

    let backup_path = path.join("backup").join(BACKUP_PLAN_FILE);
    let backup_json = fs::read_to_string(&backup_path)
        .await
        .map_err(|e| Error::Config(format!("Failed to read {}: {e}", backup_path.display())))?;
    let backup_config: DisasterRecoveryConfig =
        serde_json::from_str(&backup_json).map_err(|e| {
            Error::Config(format!(
                "Failed to parse backup plan {}: {e}",
                backup_path.display()
            ))
        })?;
    backup_config
        .verify_configuration()
        .map_err(|e| Error::Config(format!("Stored backup plan invalid: {e}")))?;

    let health_path = path.join("monitoring").join(HEALTH_PLAN_FILE);
    let health_json = fs::read_to_string(&health_path)
        .await
        .map_err(|e| Error::Config(format!("Failed to read {}: {e}", health_path.display())))?;
    let health_config: HealthMonitorConfig = serde_json::from_str(&health_json).map_err(|e| {
        Error::Config(format!(
            "Failed to parse health plan {}: {e}",
            health_path.display()
        ))
    })?;
    health_config
        .verify()
        .map_err(|e| Error::Config(format!("Stored health plan invalid: {e}")))?;

    let summary = read_plan_summary(path).await?;

    info!(
        "Validated deployment plan: {} (validators: {}, relays: {}, storage nodes: {})",
        summary.network_name,
        summary.validator_count,
        summary.relay_count,
        summary.storage_total_nodes
    );

    Ok(summary)
}

pub async fn read_plan_summary(path: &Path) -> Result<DeploymentPlanSummary> {
    let plan_path = path.join(PLAN_SUMMARY_FILE);
    let json = fs::read_to_string(&plan_path)
        .await
        .map_err(|e| Error::Config(format!("Failed to read {}: {e}", plan_path.display())))?;
    let summary = serde_json::from_str(&json).map_err(|e| {
        Error::Config(format!(
            "Failed to parse deployment summary {}: {e}",
            plan_path.display()
        ))
    })?;
    Ok(summary)
}

pub fn log_plan_summary(summary: &DeploymentPlanSummary) {
    info!(
        "Deployment plan for {}.{}: {} validators across {} regions (BFT {:.1}%), {} relays across {} regions",
        summary.network_name,
        summary.base_domain,
        summary.validator_count,
        summary.validator_regions,
        summary.bft_threshold * 100.0,
        summary.relay_count,
        summary.relay_regions
    );
    info!(
        "Storage nodes: {} (tiers: {:?}); Backup daily {:.2} TB (~${:.2}/month)",
        summary.storage_total_nodes,
        summary.storage_tiers,
        summary.backup_daily_storage_tb,
        summary.backup_estimated_monthly_cost_usd
    );
    info!(
        "Monitoring interval: {}s | Artifacts: validators={}, relays={}, storage={}, backup={}, monitoring={}",
        summary.health_check_interval_seconds,
        summary.artifacts.validators_dir,
        summary.artifacts.relays_dir,
        summary.artifacts.storage_config,
        summary.artifacts.backup_config,
        summary.artifacts.health_config
    );
}
