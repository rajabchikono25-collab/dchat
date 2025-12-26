//! Storage Provider Registry and Marketplace
//!
//! This module extends the storage provider system to support:
//! - Provider capabilities (ObjectS3, IpfsPinning, ArchiveObject)
//! - Endpoints, regions, pricing, limits, authentication
//! - Persistent registry (database-backed, not in-memory)
//! - Chain synchronization for provider state
//! - Unified storage facade for SDK and user management
//!
//! Implements Section 23 (Data Lifecycle & Storage Economics) from ARCHITECTURE-2.0.md

pub mod blob_ref;
pub mod capabilities;
pub mod challenges;
pub mod facade;
pub mod registry;
pub mod router;
pub mod selection;

pub use blob_ref::{BlobCodec, BlobLocation, BlobRef, LocationStatus, StorageClass};
pub use capabilities::{
    ArchiveObjectCapability, IpfsPinningCapability, ObjectS3Capability, ProviderCapabilities,
    ProviderCapability, StorageLimits,
};
pub use challenges::{
    ChallengeConfig, ChallengeProof, ChallengeResult, ChallengeStatus, StorageChallenge,
    StorageChallengeManager,
};
pub use facade::{
    BlobHealthStatus, HealthLevel, StorageFacade, StorageFacadeConfig, StorageFacadeError,
    StorageOperation, StorageReceipt, StoreBlobResult, StoreMessageResult, TierMigrationRequest,
    UserStorageQuota,
};
pub use registry::{ProviderRegistry, ProviderRegistryConfig, RegisteredProvider};
pub use router::{MessageMetadata, MessageType, StorageRouter, StorageRouterConfig};
pub use selection::{ProviderSelection, ProviderSelector, ReplicationConfig, SelectionCriteria};
