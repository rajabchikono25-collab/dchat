//! Platform-Specific VR/AR Integrations
//!
//! Provides platform-specific implementations for OpenXR (Quest, Vive, Index)
//! and visionOS (Apple Vision Pro)

pub mod openxr;
pub mod platform_trait;
pub mod visionos;

pub use platform_trait::{PlatformCapabilities, PlatformError, VrPlatform};
