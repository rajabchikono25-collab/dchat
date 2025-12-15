//! Storage Provider Capabilities
//!
//! Defines the capabilities that storage providers can advertise.
//! These are marketplace types for actual storage operations:
//! - ObjectS3: S3-compatible object storage (required)
//! - IpfsPinning: IPFS pinning service (optional)
//! - ArchiveObject: Long-term archive storage (optional)
//!
//! Note: Redis/TiKV/CockroachDB are NOT marketplace provider types -
//! they are internal infrastructure components.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

/// Provider capabilities - what services a storage provider offers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProviderCapabilities {
    /// S3-compatible object storage (required for all providers)
    pub object_s3: ObjectS3Capability,
    /// Optional IPFS pinning capability
    pub ipfs_pinning: Option<IpfsPinningCapability>,
    /// Optional archive/cold storage capability
    pub archive_object: Option<ArchiveObjectCapability>,
}

impl ProviderCapabilities {
    /// Create minimal capabilities (S3 only)
    pub fn s3_only(s3: ObjectS3Capability) -> Self {
        Self {
            object_s3: s3,
            ipfs_pinning: None,
            archive_object: None,
        }
    }

    /// Create full capabilities
    pub fn full(
        s3: ObjectS3Capability,
        ipfs: Option<IpfsPinningCapability>,
        archive: Option<ArchiveObjectCapability>,
    ) -> Self {
        Self {
            object_s3: s3,
            ipfs_pinning: ipfs,
            archive_object: archive,
        }
    }

    /// Check if provider supports IPFS pinning
    pub fn supports_ipfs(&self) -> bool {
        self.ipfs_pinning.is_some()
    }

    /// Check if provider supports archive storage
    pub fn supports_archive(&self) -> bool {
        self.archive_object.is_some()
    }
}

/// Individual capability types for matching
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum ProviderCapability {
    /// S3-compatible object storage
    ObjectS3,
    /// IPFS pinning service
    IpfsPinning,
    /// Archive/cold storage
    ArchiveObject,
}

/// S3-compatible object storage capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectS3Capability {
    /// S3 endpoint URL (e.g., "https://s3.us-east-1.amazonaws.com" or "https://minio.provider.com")
    pub endpoint: String,
    /// Bucket name for this provider's storage
    pub bucket: String,
    /// Geographic region (e.g., "us-east-1", "eu-west-1")
    pub region: String,
    /// Secondary regions for replication (if multi-region)
    pub secondary_regions: Vec<String>,
    /// Authentication configuration
    pub auth: S3AuthConfig,
    /// Pricing per GB per month (in smallest token unit, 8 decimals)
    pub price_per_gb_month: u64,
    /// Storage limits
    pub limits: StorageLimits,
    /// Supported storage classes
    pub storage_classes: Vec<S3StorageClass>,
    /// Whether provider supports presigned URLs
    pub supports_presigned_urls: bool,
    /// Whether provider supports multipart uploads
    pub supports_multipart: bool,
    /// CDN URL for public content (optional)
    pub cdn_url: Option<String>,
    /// Last verified timestamp
    pub last_verified: DateTime<Utc>,
}

impl ObjectS3Capability {
    /// Create a MinIO-compatible endpoint
    pub fn minio(endpoint: &str, bucket: &str, region: &str, auth: S3AuthConfig) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            bucket: bucket.to_string(),
            region: region.to_string(),
            secondary_regions: vec![],
            auth,
            price_per_gb_month: 1_0000_0000, // 1 DCHAT per GB/month default
            limits: StorageLimits::default(),
            storage_classes: vec![S3StorageClass::Standard],
            supports_presigned_urls: true,
            supports_multipart: true,
            cdn_url: None,
            last_verified: Utc::now(),
        }
    }
}

/// S3 storage classes
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum S3StorageClass {
    /// Standard storage - frequent access
    Standard,
    /// Standard-IA - infrequent access
    StandardInfrequentAccess,
    /// One Zone-IA - single zone infrequent access
    OneZoneInfrequentAccess,
    /// Glacier Instant Retrieval
    GlacierInstantRetrieval,
    /// Glacier Flexible Retrieval
    GlacierFlexibleRetrieval,
    /// Glacier Deep Archive
    GlacierDeepArchive,
    /// Intelligent-Tiering
    IntelligentTiering,
}

/// Authentication configuration for S3
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct S3AuthConfig {
    /// Access key ID (encrypted in database)
    #[serde(skip_serializing)]
    pub access_key_id: String,
    /// Secret access key (encrypted in database)
    #[serde(skip_serializing)]
    pub secret_access_key: String,
    /// Optional session token for temporary credentials
    #[serde(skip_serializing)]
    pub session_token: Option<String>,
    /// Whether to use IAM role authentication (EC2/ECS)
    pub use_iam_role: bool,
    /// Custom signing region (if different from endpoint region)
    pub signing_region: Option<String>,
}

impl S3AuthConfig {
    /// Create new static credentials
    pub fn static_credentials(access_key: &str, secret_key: &str) -> Self {
        Self {
            access_key_id: access_key.to_string(),
            secret_access_key: secret_key.to_string(),
            session_token: None,
            use_iam_role: false,
            signing_region: None,
        }
    }

    /// Create IAM role-based authentication
    pub fn iam_role() -> Self {
        Self {
            access_key_id: String::new(),
            secret_access_key: String::new(),
            session_token: None,
            use_iam_role: true,
            signing_region: None,
        }
    }
}

/// IPFS pinning service capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsPinningCapability {
    /// IPFS API endpoint (e.g., "https://ipfs.provider.com")
    pub api_endpoint: String,
    /// IPFS gateway URL for retrieval
    pub gateway_url: String,
    /// Pinning service API type
    pub pinning_api: PinningApiType,
    /// Authentication token (encrypted in database)
    #[serde(skip_serializing)]
    pub auth_token: String,
    /// Pricing per GB per month
    pub price_per_gb_month: u64,
    /// Maximum pin size in bytes
    pub max_pin_size: u64,
    /// Replication count (how many nodes pin the content)
    pub replication_count: u32,
    /// Supported CID versions
    pub cid_versions: Vec<u8>,
    /// Whether provider supports pinning by CID only (no upload)
    pub supports_remote_pin: bool,
    /// Last verified timestamp
    pub last_verified: DateTime<Utc>,
}

impl IpfsPinningCapability {
    /// Create standard IPFS pinning capability
    pub fn new(api_endpoint: &str, gateway_url: &str, auth_token: &str) -> Self {
        Self {
            api_endpoint: api_endpoint.to_string(),
            gateway_url: gateway_url.to_string(),
            pinning_api: PinningApiType::Standard,
            auth_token: auth_token.to_string(),
            price_per_gb_month: 2_0000_0000, // 2 DCHAT per GB/month default
            max_pin_size: 1024 * 1024 * 1024, // 1 GB default
            replication_count: 3,
            cid_versions: vec![0, 1],
            supports_remote_pin: true,
            last_verified: Utc::now(),
        }
    }
}

/// IPFS pinning API types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum PinningApiType {
    /// Standard IPFS HTTP API (/api/v0/pin/add)
    Standard,
    /// IPFS Pinning Services API (PSA)
    PinningServicesApi,
    /// Pinata-style API
    Pinata,
    /// Infura-style API
    Infura,
    /// Web3.Storage API
    Web3Storage,
}

/// Archive/cold storage capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArchiveObjectCapability {
    /// Archive endpoint (may be same as S3 endpoint)
    pub endpoint: String,
    /// Archive bucket/container
    pub bucket: String,
    /// Region
    pub region: String,
    /// Authentication
    pub auth: S3AuthConfig,
    /// Archive tier
    pub archive_tier: ArchiveTier,
    /// Pricing per GB per month (typically cheaper than standard)
    pub price_per_gb_month: u64,
    /// Minimum storage duration (days) - early deletion incurs penalty
    pub min_storage_days: u32,
    /// Retrieval time (typical hours for restore)
    pub retrieval_hours: u32,
    /// Retrieval pricing per GB
    pub retrieval_price_per_gb: u64,
    /// Last verified timestamp
    pub last_verified: DateTime<Utc>,
}

impl ArchiveObjectCapability {
    /// Create standard archive capability
    pub fn new(endpoint: &str, bucket: &str, region: &str, auth: S3AuthConfig) -> Self {
        Self {
            endpoint: endpoint.to_string(),
            bucket: bucket.to_string(),
            region: region.to_string(),
            auth,
            archive_tier: ArchiveTier::Standard,
            price_per_gb_month: 5000_0000, // 0.5 DCHAT per GB/month default
            min_storage_days: 90,
            retrieval_hours: 12,
            retrieval_price_per_gb: 1_0000_0000, // 1 DCHAT per GB retrieval
            last_verified: Utc::now(),
        }
    }
}

/// Archive storage tiers
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ArchiveTier {
    /// Standard archive (12-48 hour retrieval)
    Standard,
    /// Deep archive (48+ hour retrieval, cheapest)
    Deep,
    /// Instant retrieval archive (millisecond retrieval, more expensive)
    Instant,
}

/// Storage limits for a provider capability
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StorageLimits {
    /// Maximum object size in bytes
    pub max_object_size: u64,
    /// Maximum total storage per user in bytes
    pub max_storage_per_user: u64,
    /// Maximum objects per user
    pub max_objects_per_user: u64,
    /// Maximum requests per minute
    pub rate_limit_rpm: u32,
    /// Maximum bandwidth per day in bytes
    pub max_bandwidth_per_day: u64,
    /// Minimum object size (for efficient storage)
    pub min_object_size: u64,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            max_object_size: 5 * 1024 * 1024 * 1024,        // 5 GB
            max_storage_per_user: 100 * 1024 * 1024 * 1024, // 100 GB
            max_objects_per_user: 1_000_000,
            rate_limit_rpm: 1000,
            max_bandwidth_per_day: 1024 * 1024 * 1024 * 1024, // 1 TB
            min_object_size: 0,
        }
    }
}

impl StorageLimits {
    /// Create permissive limits for testing
    pub fn permissive() -> Self {
        Self {
            max_object_size: u64::MAX,
            max_storage_per_user: u64::MAX,
            max_objects_per_user: u64::MAX,
            rate_limit_rpm: u32::MAX,
            max_bandwidth_per_day: u64::MAX,
            min_object_size: 0,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_provider_capabilities_s3_only() {
        let s3 = ObjectS3Capability::minio(
            "https://minio.example.com",
            "dchat",
            "us-east-1",
            S3AuthConfig::static_credentials("access", "secret"),
        );
        let caps = ProviderCapabilities::s3_only(s3);

        assert!(!caps.supports_ipfs());
        assert!(!caps.supports_archive());
    }

    #[test]
    fn test_provider_capabilities_full() {
        let s3 = ObjectS3Capability::minio(
            "https://minio.example.com",
            "dchat",
            "us-east-1",
            S3AuthConfig::static_credentials("access", "secret"),
        );
        let ipfs = IpfsPinningCapability::new(
            "https://ipfs.example.com",
            "https://gateway.example.com",
            "token",
        );
        let caps = ProviderCapabilities::full(s3, Some(ipfs), None);

        assert!(caps.supports_ipfs());
        assert!(!caps.supports_archive());
    }

    #[test]
    fn test_storage_limits_default() {
        let limits = StorageLimits::default();
        assert_eq!(limits.max_object_size, 5 * 1024 * 1024 * 1024);
        assert!(limits.rate_limit_rpm > 0);
    }
}
