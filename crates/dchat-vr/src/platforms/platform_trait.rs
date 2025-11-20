//! Platform abstraction trait for VR/AR systems

use crate::{DeviceType, Transform, Vector3, gesture::{Hand, FingerJoints, FingerPositions}};
use async_trait::async_trait;
use std::error::Error;
use std::fmt;

/// Platform-specific error types
#[derive(Debug)]
pub enum PlatformError {
    /// Platform initialization failed
    InitializationFailed(String),
    /// Session management error
    SessionError(String),
    /// Hand tracking not available or failed
    HandTrackingUnavailable,
    /// Haptic feedback not supported
    HapticsUnsupported,
    /// Platform-specific error
    PlatformSpecific(String),
}

impl fmt::Display for PlatformError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InitializationFailed(msg) => write!(f, "Platform initialization failed: {}", msg),
            Self::SessionError(msg) => write!(f, "Session error: {}", msg),
            Self::HandTrackingUnavailable => write!(f, "Hand tracking not available"),
            Self::HapticsUnsupported => write!(f, "Haptic feedback not supported"),
            Self::PlatformSpecific(msg) => write!(f, "Platform error: {}", msg),
        }
    }
}

impl Error for PlatformError {}

/// Platform capabilities
#[derive(Debug, Clone)]
pub struct PlatformCapabilities {
    pub supports_hand_tracking: bool,
    pub supports_eye_tracking: bool,
    pub supports_haptics: bool,
    pub supports_passthrough: bool,
    pub max_refresh_rate: u32,
    pub field_of_view: f32,
    pub has_6dof: bool,
}

/// Platform abstraction for VR/AR systems
#[async_trait]
pub trait VrPlatform: Send + Sync {
    /// Initialize the platform
    async fn initialize(&mut self) -> Result<(), PlatformError>;

    /// Get platform capabilities
    fn get_capabilities(&self) -> PlatformCapabilities;

    /// Get device type
    fn get_device_type(&self) -> DeviceType;

    /// Start VR session
    async fn start_session(&mut self) -> Result<(), PlatformError>;

    /// End VR session
    async fn end_session(&mut self) -> Result<(), PlatformError>;

    /// Update head transform
    fn get_head_transform(&self) -> Option<Transform>;

    /// Get hand transforms
    fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>);

    /// Get hand tracking data (skeletal)
    fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)>;

    /// Trigger haptic feedback
    fn trigger_haptic(&mut self, hand: Hand, intensity: f32, duration_ms: u32) -> Result<(), PlatformError>;

    /// Get frame timing info for performance optimization
    fn get_frame_time(&self) -> f32;

    /// Check if session is active
    fn is_session_active(&self) -> bool;
}
