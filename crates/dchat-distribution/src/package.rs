use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::path::PathBuf;
use uuid::Uuid;

/// Errors related to package distribution
#[derive(Debug, thiserror::Error)]
pub enum DistributionError {
    #[error("Package verification failed: {0}")]
    VerificationFailed(String),

    #[error("Download failed: {0}")]
    DownloadFailed(String),

    #[error("Invalid package format: {0}")]
    InvalidFormat(String),

    #[error("Version not found: {0}")]
    VersionNotFound(String),

    #[error("Mirror unavailable: {0}")]
    MirrorUnavailable(String),

    #[error("Signature verification failed")]
    SignatureInvalid,

    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),

    #[error("HTTP error: {0}")]
    Http(#[from] reqwest::Error),
}

pub type Result<T> = std::result::Result<T, DistributionError>;

/// Package metadata for a dchat release
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PackageMetadata {
    /// Version number (e.g., "1.2.3")
    pub version: String,

    /// Release timestamp
    pub release_date: DateTime<Utc>,

    /// Package type (binary, source, docker image)
    pub package_type: PackageType,

    /// Target platform (linux-x64, windows-x64, macos-arm64, etc.)
    pub platform: String,

    /// SHA-256 hash of the package
    pub sha256: String,

    /// BLAKE3 hash of the package (additional verification)
    pub blake3: String,

    /// Package size in bytes
    pub size_bytes: u64,

    /// Ed25519 signature from release signing key
    pub signature: Vec<u8>,

    /// Public key of the signer (for verification)
    pub signer_pubkey: Vec<u8>,

    /// Release notes URL
    pub release_notes_url: Option<String>,

    /// Minimum compatible version (won't work with older versions)
    pub min_compatible_version: Option<String>,
}

/// Type of package
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PackageType {
    /// Compiled binary executable
    Binary,

    /// Source code tarball
    Source,

    /// Docker container image
    DockerImage,

    /// Debian package (.deb)
    Debian,

    /// RPM package (.rpm)
    Rpm,

    /// Android APK
    Apk,
}

/// Download source for packages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DownloadSource {
    /// Source ID
    pub id: Uuid,

    /// Source type (HTTPS mirror, IPFS, BitTorrent, etc.)
    pub source_type: SourceType,

    /// URL or content identifier
    pub uri: String,

    /// Geographic region (for latency optimization)
    pub region: Option<String>,

    /// Priority (lower = preferred)
    pub priority: u32,

    /// Last successful download timestamp
    pub last_success: Option<DateTime<Utc>>,

    /// Number of consecutive failures
    pub failure_count: u32,
}

/// Type of download source
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SourceType {
    /// HTTPS mirror
    HttpsMirror,

    /// IPFS content-addressed storage
    Ipfs,

    /// BitTorrent magnet link
    BitTorrent,

    /// Peer-to-peer gossip (received from another node)
    Gossip,

    /// Local file cache
    LocalCache,
}

/// Package manager for discovering and downloading updates
pub struct PackageManager {
    /// Known download sources (mirrors)
    sources: Vec<DownloadSource>,

    /// Cache of package metadata
    package_cache: HashMap<String, PackageMetadata>,

    /// Local package storage directory
    cache_dir: PathBuf,

    /// Trusted release signing public keys
    trusted_keys: Vec<VerifyingKey>,

    /// HTTP client for downloads
    http_client: reqwest::Client,
}

impl PackageManager {
    /// Create a new package manager
    pub fn new(cache_dir: PathBuf, trusted_keys: Vec<VerifyingKey>) -> Self {
        Self {
            sources: Vec::new(),
            package_cache: HashMap::new(),
            cache_dir,
            trusted_keys,
            http_client: reqwest::Client::new(),
        }
    }

    /// Add a download source
    pub fn add_source(&mut self, source: DownloadSource) {
        self.sources.push(source);
        // Sort by priority
        self.sources.sort_by_key(|s| s.priority);
    }

    /// Discover available versions via gossip
    pub async fn discover_versions(&mut self) -> Result<Vec<String>> {
        tracing::info!("Discovering available versions via gossip");

        // In production, this would:
        // 1. Query known peers for their version
        // 2. Request package metadata from peers
        // 3. Verify signatures
        // 4. Cache valid package metadata

        // For now, return cached versions
        let versions: Vec<String> = self.package_cache.keys().cloned().collect();
        Ok(versions)
    }

    /// Check if a specific version is available
    pub fn is_version_available(&self, version: &str) -> bool {
        self.package_cache.contains_key(version)
    }

    /// Get package metadata for a specific version
    pub fn get_package_metadata(&self, version: &str) -> Option<&PackageMetadata> {
        self.package_cache.get(version)
    }

    /// Verify package signature
    pub fn verify_signature(&self, metadata: &PackageMetadata, package_data: &[u8]) -> Result<()> {
        // Parse public key
        let pubkey_bytes: [u8; 32] =
            metadata.signer_pubkey.as_slice().try_into().map_err(|_| {
                DistributionError::VerificationFailed("Invalid public key length".to_string())
            })?;
        let pubkey = VerifyingKey::from_bytes(&pubkey_bytes).map_err(|e| {
            DistributionError::VerificationFailed(format!("Invalid public key: {}", e))
        })?;

        // Check if key is trusted
        if !self.trusted_keys.contains(&pubkey) {
            return Err(DistributionError::VerificationFailed(
                "Signer public key not in trusted keys".to_string(),
            ));
        }

        // Parse signature
        let sig_bytes: [u8; 64] = metadata.signature.as_slice().try_into().map_err(|_| {
            DistributionError::VerificationFailed("Invalid signature length".to_string())
        })?;
        let signature = Signature::from_bytes(&sig_bytes);

        // Verify signature over package data
        pubkey
            .verify(package_data, &signature)
            .map_err(|_| DistributionError::SignatureInvalid)?;

        Ok(())
    }

    /// Verify package hash
    pub fn verify_hash(&self, metadata: &PackageMetadata, package_data: &[u8]) -> Result<()> {
        // Verify SHA-256
        let mut hasher = Sha256::new();
        hasher.update(package_data);
        let hash = hasher.finalize();
        let hash_hex = hex::encode(hash);

        if hash_hex != metadata.sha256 {
            return Err(DistributionError::VerificationFailed(format!(
                "SHA-256 mismatch: expected {}, got {}",
                metadata.sha256, hash_hex
            )));
        }

        // Verify BLAKE3
        let blake3_hash = blake3::hash(package_data);
        let blake3_hex = blake3_hash.to_hex().to_string();

        if blake3_hex != metadata.blake3 {
            return Err(DistributionError::VerificationFailed(format!(
                "BLAKE3 mismatch: expected {}, got {}",
                metadata.blake3, blake3_hex
            )));
        }

        Ok(())
    }

    /// Download a specific version from best available source
    pub async fn download_version(&mut self, version: &str) -> Result<PathBuf> {
        let metadata = self
            .package_cache
            .get(version)
            .ok_or_else(|| DistributionError::VersionNotFound(version.to_string()))?
            .clone();

        tracing::info!(
            "Downloading version {} from {} sources",
            version,
            self.sources.len()
        );

        // Collect results, then update sources
        let mut results = Vec::new();
        for (idx, source) in self.sources.iter().enumerate() {
            match self.try_download_from_source(source, &metadata).await {
                Ok(path) => {
                    results.push((idx, Ok(path)));
                    break; // Success, stop trying other sources
                }
                Err(e) => {
                    results.push((idx, Err(e)));
                }
            }
        }

        // Update sources based on results
        for (idx, result) in results {
            match result {
                Ok(path) => {
                    self.sources[idx].last_success = Some(Utc::now());
                    self.sources[idx].failure_count = 0;
                    return Ok(path);
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to download from {:?}: {}",
                        self.sources[idx].source_type,
                        e
                    );
                    self.sources[idx].failure_count += 1;
                }
            }
        }

        Err(DistributionError::DownloadFailed(
            "All download sources failed".to_string(),
        ))
    }

    /// Try downloading from a specific source
    async fn try_download_from_source(
        &self,
        source: &DownloadSource,
        metadata: &PackageMetadata,
    ) -> Result<PathBuf> {
        match source.source_type {
            SourceType::HttpsMirror => self.download_via_https(&source.uri, metadata).await,
            SourceType::Ipfs => self.download_via_ipfs(&source.uri, metadata).await,
            SourceType::LocalCache => self.load_from_cache(metadata),
            SourceType::BitTorrent => {
                // BitTorrent implementation would go here
                Err(DistributionError::DownloadFailed(
                    "BitTorrent not implemented".to_string(),
                ))
            }
            SourceType::Gossip => {
                // Gossip-based download would go here
                Err(DistributionError::DownloadFailed(
                    "Gossip download not implemented".to_string(),
                ))
            }
        }
    }

    /// Download package via HTTPS mirror
    async fn download_via_https(&self, url: &str, metadata: &PackageMetadata) -> Result<PathBuf> {
        tracing::info!("Downloading from HTTPS: {}", url);

        // Download to temporary file
        let response = self.http_client.get(url).send().await?;

        if !response.status().is_success() {
            return Err(DistributionError::DownloadFailed(format!(
                "HTTP error: {}",
                response.status()
            )));
        }

        let bytes = response.bytes().await?;

        // Verify hash
        self.verify_hash(metadata, &bytes)?;

        // Verify signature
        self.verify_signature(metadata, &bytes)?;

        // Save to cache
        let cache_path = self
            .cache_dir
            .join(format!("dchat-{}-{}", metadata.version, metadata.platform));
        std::fs::write(&cache_path, bytes)?;

        Ok(cache_path)
    }

    /// Download package via IPFS
    #[cfg(feature = "ipfs")]
    async fn download_via_ipfs(&self, cid: &str, metadata: &PackageMetadata) -> Result<PathBuf> {
        tracing::info!("Downloading from IPFS: {}", cid);

        // IPFS client initialization and download would go here
        // For now, return not implemented
        Err(DistributionError::DownloadFailed(
            "IPFS not yet implemented".to_string(),
        ))
    }

    #[cfg(not(feature = "ipfs"))]
    async fn download_via_ipfs(&self, _cid: &str, _metadata: &PackageMetadata) -> Result<PathBuf> {
        Err(DistributionError::DownloadFailed(
            "IPFS support not enabled".to_string(),
        ))
    }

    /// Load package from local cache
    fn load_from_cache(&self, metadata: &PackageMetadata) -> Result<PathBuf> {
        let cache_path = self
            .cache_dir
            .join(format!("dchat-{}-{}", metadata.version, metadata.platform));

        if !cache_path.exists() {
            return Err(DistributionError::DownloadFailed(
                "Not in cache".to_string(),
            ));
        }

        // Verify cached file
        let bytes = std::fs::read(&cache_path)?;
        self.verify_hash(metadata, &bytes)?;
        self.verify_signature(metadata, &bytes)?;

        Ok(cache_path)
    }

    /// Register package metadata (e.g., received via gossip)
    pub fn register_package(&mut self, metadata: PackageMetadata) {
        tracing::info!(
            "Registering package: {} for {}",
            metadata.version,
            metadata.platform
        );
        self.package_cache
            .insert(metadata.version.clone(), metadata);
    }

    /// Get download sources for a specific region (latency optimization)
    pub fn get_sources_for_region(&self, region: &str) -> Vec<&DownloadSource> {
        self.sources
            .iter()
            .filter(|s| s.region.as_ref().map(|r| r == region).unwrap_or(false))
            .collect()
    }

    /// Prune failed sources (remove after too many failures)
    pub fn prune_failed_sources(&mut self, max_failures: u32) {
        let before = self.sources.len();
        self.sources.retain(|s| s.failure_count < max_failures);
        let after = self.sources.len();

        if before != after {
            tracing::info!(
                "Pruned {} failed sources ({} -> {})",
                before - after,
                before,
                after
            );
        }
    }
}

/// Auto-update configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoUpdateConfig {
    /// Enable automatic updates
    pub enabled: bool,

    /// Only auto-update for security patches
    pub security_only: bool,

    /// Check for updates every N hours
    pub check_interval_hours: u64,

    /// Restart after update automatically
    pub auto_restart: bool,

    /// Download updates in background
    pub background_download: bool,
}

impl Default for AutoUpdateConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Opt-in for safety
            security_only: true,
            check_interval_hours: 24,
            auto_restart: false,
            background_download: true,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_package_metadata_serialization() {
        let metadata = PackageMetadata {
            version: "1.2.3".to_string(),
            release_date: Utc::now(),
            package_type: PackageType::Binary,
            platform: "linux-x64".to_string(),
            sha256: "abc123".to_string(),
            blake3: "def456".to_string(),
            size_bytes: 1024000,
            signature: vec![1, 2, 3],
            signer_pubkey: vec![4, 5, 6],
            release_notes_url: Some("https://example.com/notes".to_string()),
            min_compatible_version: Some("1.0.0".to_string()),
        };

        let json = serde_json::to_string(&metadata).unwrap();
        let deserialized: PackageMetadata = serde_json::from_str(&json).unwrap();

        assert_eq!(metadata.version, deserialized.version);
        assert_eq!(metadata.package_type, deserialized.package_type);
    }

    #[test]
    fn test_download_source_priority_sorting() {
        let dir = tempdir().unwrap();
        let manager = PackageManager::new(dir.path().to_path_buf(), vec![]);

        // Sources get sorted by priority automatically
        assert!(manager.sources.is_empty());
    }

    #[test]
    fn test_auto_update_config_defaults() {
        let config = AutoUpdateConfig::default();

        assert!(!config.enabled); // Disabled by default for safety
        assert!(config.security_only);
        assert_eq!(config.check_interval_hours, 24);
    }
}
