//! dchat-identity: Identity management and verification
//!
//! This crate provides identity management including:
//! - Identity registration and verification
//! - Multi-device synchronization
//! - Hierarchical key derivation
//! - Burner identities
//! - Guardian-based account recovery
//! - User profiles and status
//! - CAPTCHA verification for bot protection
//! 
//! # Production Security
//! 
//! This crate is production-ready with the following security features:
//! - FROST threshold signatures using audited frost-ed25519 library (NCC Group audited)
//! - Platform-specific device attestation (iOS App Attest, Android Play Integrity)
//! - CAPTCHA verification support (hCaptcha, Cloudflare Turnstile)
//! - Simulated/testing code is compile-time excluded from release builds

pub mod biometric; // Phase 7 Sprint 6: Keyless UX - Biometric authentication
pub mod burner;
pub mod captcha; // CAPTCHA verification for bot protection
pub mod derivation;
pub mod device;
pub mod enclave; // Phase 7 Sprint 6: Keyless UX - Secure enclave integration
pub mod guardian;
pub mod guardian_recovery; // Phase 2: Guardian-based account recovery
pub mod identity; // Core identity management
pub mod mpc; // Phase 7 Sprint 6: Keyless UX - MPC threshold signing (debug builds only)
pub mod mpc_frost; // Phase 2 Sprint 2: FROST threshold signatures (production-ready)
pub mod peer_registry; // Peer registry and discovery
pub mod profile; // User profiles, status, and privacy settings
pub mod storage;
pub mod sync;
pub mod verification; // Profile database storage
pub mod attestation; // Device attestation verification (platform-specific)

pub use biometric::{BiometricAuthResult, BiometricAuthenticator, BiometricConfig, BiometricType};
pub use burner::BurnerIdentity;
pub use captcha::{
    CaptchaConfig, CaptchaProvider, CaptchaRequirementChecker, CaptchaVerificationResult,
    verify_captcha,
};
pub use derivation::{IdentityDerivation, KeyPath};
// Re-export mnemonic types for wallet-style recovery
pub use dchat_crypto::{Mnemonic, MnemonicLength, Seed};
pub use device::{Device, DeviceManager};
pub use enclave::{EnclaveConfig, SecureEnclave};
pub use guardian::{Guardian, GuardianManager, RecoveryRequest};
pub use guardian_recovery::{GuardianId, GuardianRecoveryManager, RecoveryStatus};
pub use identity::{Identity, IdentityManager};
pub use mpc::{MpcConfig, MpcSigner, ThresholdSignature};
pub use mpc_frost::{
    FrostCoordinator, FrostKeyShare, FrostSignature, FrostConfig, FrostError,
    SigningRound1Package, SigningRound2Package,
};
pub use peer_registry::{PeerRegistry, PeerRole}; // Add peer_registry exports
pub use profile::{
    MusicApiTrack, MusicProvider, OnlineStatus, PrivacySettings, ProfileManager, ProfilePicture,
    StatusType, UserProfile, UserStatus, VisibilityLevel,
};
pub use storage::ProfileStorage;
pub use verification::{VerificationProof, VerifiedBadge};
pub use attestation::{verify_device_attestation, AttestationResult};
