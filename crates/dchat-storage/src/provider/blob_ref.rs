//! Blob Reference (BlobRef)
//!
//! Content-addressable reference to stored blobs with:
//! - Cryptographic hash for integrity verification
//! - Size and codec information
//! - Multiple storage locations (provider IDs + S3 keys/IPFS CIDs)
//! - Location status tracking
//!
//! BlobRefs are stored with messages instead of embedding large bytes in SQL rows.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;

/// A reference to a blob stored in external storage
///
/// BlobRefs are stable identifiers that can be stored in message metadata.
/// The actual blob content is stored in S3-compatible storage and/or IPFS.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobRef {
    /// Content hash (SHA-256) - primary identifier for content addressing
    pub hash: [u8; 32],
    /// Size in bytes
    pub size: u64,
    /// Content codec/type
    pub codec: BlobCodec,
    /// MIME type of the original content
    pub mime_type: String,
    /// Whether the blob is encrypted
    pub encrypted: bool,
    /// Encryption key ID (if encrypted) - references key in user's keychain
    pub encryption_key_id: Option<String>,
    /// Storage locations (at least one required)
    pub locations: Vec<BlobLocation>,
    /// Creation timestamp
    pub created_at: DateTime<Utc>,
    /// Last verification timestamp
    pub last_verified: Option<DateTime<Utc>>,
    /// Metadata (optional, for search/filtering)
    pub metadata: HashMap<String, String>,
}

impl BlobRef {
    /// Create a new BlobRef from content
    pub fn from_content(
        content: &[u8],
        codec: BlobCodec,
        mime_type: &str,
        encrypted: bool,
    ) -> Self {
        let hash = Self::compute_hash(content);
        Self {
            hash,
            size: content.len() as u64,
            codec,
            mime_type: mime_type.to_string(),
            encrypted,
            encryption_key_id: None,
            locations: vec![],
            created_at: Utc::now(),
            last_verified: None,
            metadata: HashMap::new(),
        }
    }

    /// Compute SHA-256 hash of content
    pub fn compute_hash(content: &[u8]) -> [u8; 32] {
        let mut hasher = Sha256::new();
        hasher.update(content);
        hasher.finalize().into()
    }

    /// Get hash as hex string
    pub fn hash_hex(&self) -> String {
        hex::encode(self.hash)
    }

    /// Add a storage location
    pub fn add_location(&mut self, location: BlobLocation) {
        // Avoid duplicate locations for same provider
        if !self
            .locations
            .iter()
            .any(|l| l.provider_id == location.provider_id)
        {
            self.locations.push(location);
        }
    }

    /// Remove a storage location by provider ID
    pub fn remove_location(&mut self, provider_id: &[u8; 32]) {
        self.locations.retain(|l| &l.provider_id != provider_id);
    }

    /// Get all healthy locations
    pub fn healthy_locations(&self) -> Vec<&BlobLocation> {
        self.locations
            .iter()
            .filter(|l| l.status == LocationStatus::Healthy)
            .collect()
    }

    /// Check if blob has minimum required replicas
    pub fn has_minimum_replicas(&self, min_replicas: usize) -> bool {
        self.healthy_locations().len() >= min_replicas
    }

    /// Get locations in a specific region
    pub fn locations_in_region(&self, region: &str) -> Vec<&BlobLocation> {
        self.locations
            .iter()
            .filter(|l| l.region.as_deref() == Some(region))
            .collect()
    }

    /// Check if blob needs replication to more providers
    pub fn needs_replication(&self, target_replicas: usize) -> bool {
        self.healthy_locations().len() < target_replicas
    }

    /// Verify content matches hash
    pub fn verify_content(&self, content: &[u8]) -> bool {
        let computed = Self::compute_hash(content);
        computed == self.hash
    }

    /// Mark verification timestamp
    pub fn mark_verified(&mut self) {
        self.last_verified = Some(Utc::now());
    }

    /// Get primary location (first healthy location)
    pub fn primary_location(&self) -> Option<&BlobLocation> {
        self.healthy_locations().first().copied()
    }

    /// Check if any location has IPFS CID
    pub fn has_ipfs(&self) -> bool {
        self.locations.iter().any(|l| l.ipfs_cid.is_some())
    }

    /// Get all IPFS CIDs
    pub fn ipfs_cids(&self) -> Vec<&str> {
        self.locations
            .iter()
            .filter_map(|l| l.ipfs_cid.as_deref())
            .collect()
    }
}

/// Blob content codec/encoding
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BlobCodec {
    /// Raw bytes (no encoding)
    Raw,
    /// CBOR encoded
    Cbor,
    /// JSON encoded
    Json,
    /// MessagePack encoded
    MessagePack,
    /// Protobuf encoded
    Protobuf,
    /// Compressed with zstd
    Zstd,
    /// Compressed with brotli
    Brotli,
    /// Compressed with lz4
    Lz4,
}

impl Default for BlobCodec {
    fn default() -> Self {
        Self::Raw
    }
}

/// A storage location for a blob
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobLocation {
    /// Provider ID (public key hash)
    pub provider_id: [u8; 32],
    /// S3 object key (for S3-compatible storage)
    pub s3_key: Option<String>,
    /// S3 bucket (if different from provider default)
    pub s3_bucket: Option<String>,
    /// IPFS CID (if pinned to IPFS)
    pub ipfs_cid: Option<String>,
    /// Archive object key (if in archive tier)
    pub archive_key: Option<String>,
    /// Storage class/tier
    pub storage_class: StorageClass,
    /// Region where stored
    pub region: Option<String>,
    /// Location status
    pub status: LocationStatus,
    /// When this location was created
    pub created_at: DateTime<Utc>,
    /// Last successful access
    pub last_accessed: Option<DateTime<Utc>>,
    /// Last challenge verification
    pub last_challenged: Option<DateTime<Utc>>,
    /// Number of failed challenges
    pub failed_challenges: u32,
}

impl BlobLocation {
    /// Create a new S3 location
    pub fn s3(provider_id: [u8; 32], s3_key: &str, region: &str) -> Self {
        Self {
            provider_id,
            s3_key: Some(s3_key.to_string()),
            s3_bucket: None,
            ipfs_cid: None,
            archive_key: None,
            storage_class: StorageClass::Standard,
            region: Some(region.to_string()),
            status: LocationStatus::Pending,
            created_at: Utc::now(),
            last_accessed: None,
            last_challenged: None,
            failed_challenges: 0,
        }
    }

    /// Create a new IPFS location
    pub fn ipfs(provider_id: [u8; 32], cid: &str) -> Self {
        Self {
            provider_id,
            s3_key: None,
            s3_bucket: None,
            ipfs_cid: Some(cid.to_string()),
            archive_key: None,
            storage_class: StorageClass::Standard,
            region: None, // IPFS is location-agnostic
            status: LocationStatus::Pending,
            created_at: Utc::now(),
            last_accessed: None,
            last_challenged: None,
            failed_challenges: 0,
        }
    }

    /// Create a hybrid S3+IPFS location
    pub fn hybrid(provider_id: [u8; 32], s3_key: &str, cid: &str, region: &str) -> Self {
        Self {
            provider_id,
            s3_key: Some(s3_key.to_string()),
            s3_bucket: None,
            ipfs_cid: Some(cid.to_string()),
            archive_key: None,
            storage_class: StorageClass::Standard,
            region: Some(region.to_string()),
            status: LocationStatus::Pending,
            created_at: Utc::now(),
            last_accessed: None,
            last_challenged: None,
            failed_challenges: 0,
        }
    }

    /// Mark location as healthy
    pub fn mark_healthy(&mut self) {
        self.status = LocationStatus::Healthy;
        self.last_accessed = Some(Utc::now());
    }

    /// Mark location as failed
    pub fn mark_failed(&mut self) {
        self.failed_challenges += 1;
        if self.failed_challenges >= 3 {
            self.status = LocationStatus::Failed;
        } else {
            self.status = LocationStatus::Degraded;
        }
    }

    /// Mark challenge completed
    pub fn mark_challenged(&mut self, success: bool) {
        self.last_challenged = Some(Utc::now());
        if success {
            self.mark_healthy();
        } else {
            self.mark_failed();
        }
    }

    /// Get provider ID as hex
    pub fn provider_id_hex(&self) -> String {
        hex::encode(self.provider_id)
    }

    /// Check if location is usable for retrieval
    pub fn is_usable(&self) -> bool {
        matches!(
            self.status,
            LocationStatus::Healthy | LocationStatus::Pending | LocationStatus::Degraded
        )
    }
}

/// Storage class for tiering
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum StorageClass {
    /// Hot storage - fast access, higher cost
    Standard,
    /// Infrequent access - slower, cheaper
    InfrequentAccess,
    /// Archive - very slow retrieval, cheapest
    Archive,
    /// Deep archive - hours to retrieve, cheapest
    DeepArchive,
}

impl Default for StorageClass {
    fn default() -> Self {
        Self::Standard
    }
}

/// Location status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LocationStatus {
    /// Upload pending/in progress
    Pending,
    /// Location is healthy and accessible
    Healthy,
    /// Location has some issues but is still accessible
    Degraded,
    /// Location has failed challenges or is inaccessible
    Failed,
    /// Location is being migrated to another provider
    Migrating,
    /// Location has been deleted
    Deleted,
}

impl Default for LocationStatus {
    fn default() -> Self {
        Self::Pending
    }
}

/// Result of a blob retrieval operation
#[derive(Debug, Clone)]
pub struct BlobRetrievalResult {
    /// The retrieved content
    pub content: Vec<u8>,
    /// Which location was used
    pub location: BlobLocation,
    /// Retrieval duration in milliseconds
    pub duration_ms: u64,
    /// Whether content hash was verified
    pub verified: bool,
}

/// Summary of blob storage status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BlobStorageStatus {
    /// Total number of blobs
    pub total_blobs: u64,
    /// Total size in bytes
    pub total_size: u64,
    /// Blobs needing replication
    pub needs_replication: u64,
    /// Blobs with failed locations
    pub degraded_blobs: u64,
    /// Average replication factor
    pub avg_replication: f64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blob_ref_creation() {
        let content = b"Hello, World!";
        let blob = BlobRef::from_content(content, BlobCodec::Raw, "text/plain", false);

        assert_eq!(blob.size, 13);
        assert!(!blob.encrypted);
        assert!(blob.locations.is_empty());
    }

    #[test]
    fn test_content_verification() {
        let content = b"Test content";
        let blob = BlobRef::from_content(content, BlobCodec::Raw, "text/plain", false);

        assert!(blob.verify_content(content));
        assert!(!blob.verify_content(b"Different content"));
    }

    #[test]
    fn test_location_management() {
        let content = b"Test";
        let mut blob = BlobRef::from_content(content, BlobCodec::Raw, "text/plain", false);

        let provider_id = [1u8; 32];
        let location = BlobLocation::s3(provider_id, "objects/test.bin", "us-east-1");
        blob.add_location(location);

        assert_eq!(blob.locations.len(), 1);

        // Adding duplicate provider should not add another location
        let location2 = BlobLocation::s3(provider_id, "objects/test2.bin", "us-west-2");
        blob.add_location(location2);
        assert_eq!(blob.locations.len(), 1);

        // Different provider should add
        let location3 = BlobLocation::s3([2u8; 32], "objects/test.bin", "eu-west-1");
        blob.add_location(location3);
        assert_eq!(blob.locations.len(), 2);
    }

    #[test]
    fn test_healthy_locations() {
        let content = b"Test";
        let mut blob = BlobRef::from_content(content, BlobCodec::Raw, "text/plain", false);

        let mut loc1 = BlobLocation::s3([1u8; 32], "key1", "us-east-1");
        loc1.status = LocationStatus::Healthy;
        blob.add_location(loc1);

        let mut loc2 = BlobLocation::s3([2u8; 32], "key2", "us-west-2");
        loc2.status = LocationStatus::Failed;
        blob.add_location(loc2);

        let mut loc3 = BlobLocation::s3([3u8; 32], "key3", "eu-west-1");
        loc3.status = LocationStatus::Healthy;
        blob.add_location(loc3);

        assert_eq!(blob.healthy_locations().len(), 2);
        assert!(blob.has_minimum_replicas(2));
        assert!(!blob.has_minimum_replicas(3));
    }

    #[test]
    fn test_ipfs_locations() {
        let content = b"Test";
        let mut blob = BlobRef::from_content(content, BlobCodec::Raw, "text/plain", false);

        let loc1 = BlobLocation::s3([1u8; 32], "key1", "us-east-1");
        blob.add_location(loc1);
        assert!(!blob.has_ipfs());

        let loc2 = BlobLocation::ipfs([2u8; 32], "QmTestCid123");
        blob.add_location(loc2);
        assert!(blob.has_ipfs());
        assert_eq!(blob.ipfs_cids().len(), 1);
    }
}
