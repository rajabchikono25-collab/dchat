//! dchat-identity: Identity management and verification
//!
//! This crate provides identity management including:
//! - Identity registration and verification
//! - Multi-device synchronization
//! - Hierarchical key derivation
//! - Burner identities
//! - Guardian-based account recovery
//! - User profiles and status

pub mod biometric; // Phase 7 Sprint 6: Keyless UX - Biometric authentication
pub mod burner;
pub mod derivation;
pub mod device;
pub mod enclave; // Phase 7 Sprint 6: Keyless UX - Secure enclave integration
pub mod guardian;
pub mod guardian_recovery; // Phase 2: Guardian-based account recovery
pub mod identity; // Core identity management
pub mod mpc; // Phase 7 Sprint 6: Keyless UX - MPC threshold signing
pub mod peer_registry; // Peer registry and discovery
pub mod profile; // User profiles, status, and privacy settings
pub mod storage;
pub mod sync;
pub mod verification; // Profile database storage

pub use biometric::{BiometricAuthResult, BiometricAuthenticator, BiometricConfig, BiometricType};
pub use burner::BurnerIdentity;
pub use derivation::{IdentityDerivation, KeyPath};
pub use device::{Device, DeviceManager};
pub use enclave::{EnclaveConfig, SecureEnclave};
pub use guardian::{Guardian, GuardianManager, RecoveryRequest};
pub use guardian_recovery::{GuardianId, GuardianRecoveryManager, RecoveryStatus};
pub use identity::{Identity, IdentityManager};
pub use mpc::{MpcConfig, MpcSigner, ThresholdSignature};
pub use peer_registry::{PeerRegistry, PeerRole}; // Add peer_registry exports
pub use profile::{
    MusicApiTrack, MusicProvider, OnlineStatus, PrivacySettings, ProfileManager, ProfilePicture,
    StatusType, UserProfile, UserStatus, VisibilityLevel,
};
pub use storage::ProfileStorage;
pub use verification::{VerificationProof, VerifiedBadge};
