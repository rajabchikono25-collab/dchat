//! Storage Provider Registry and Marketplace
//!
//! This module extends the storage provider system to support:
//! - Provider capabilities (ObjectS3, IpfsPinning, ArchiveObject)
//! - Endpoints, regions, pricing, limits, authentication
//! - Persistent registry (database-backed, not in-memory)
//! - Chain synchronization for provider state
//!
//! Implements Section 23 (Data Lifecycle & Storage Economics) from ARCHITECTURE-2.0.md

pub mod blob_ref;
pub mod capabilities;
pub mod challenges;
pub mod registry;
pub mod router;
pub mod selection;

pub use blob_ref::{BlobCodec, BlobLocation, BlobRef, LocationStatus};
pub use capabilities::{
    ArchiveObjectCapability, IpfsPinningCapability, ObjectS3Capability, ProviderCapabilities,
    ProviderCapability, StorageLimits,
};
pub use challenges::{
    ChallengeProof, ChallengeResult, ChallengeStatus, StorageChallenge, StorageChallengeManager,
};
pub use registry::{ProviderRegistry, ProviderRegistryConfig, RegisteredProvider};
pub use router::{MessageMetadata, MessageType, StorageRouter, StorageRouterConfig};
pub use selection::{ProviderSelection, ReplicationConfig, SelectionCriteria};
