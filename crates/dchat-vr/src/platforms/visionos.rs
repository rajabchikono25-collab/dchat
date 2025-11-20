//! visionOS Platform Implementation
//!
//! Supports Apple Vision Pro with native RealityKit and ARKit integration

use super::{VrPlatform, PlatformCapabilities, PlatformError};
use crate::{DeviceType, Transform, Vector3, gesture::{Hand, FingerJoints, FingerPositions}};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// visionOS platform backend for Apple Vision Pro
pub struct VisionOsPlatform {
    session_active: bool,
    capabilities: PlatformCapabilities,
    head_transform: Arc<Mutex<Option<Transform>>>,
    left_hand_transform: Arc<Mutex<Option<Transform>>>,
    right_hand_transform: Arc<Mutex<Option<Transform>>>,
    left_hand_tracking: Arc<Mutex<Option<FingerPositions>>>,
    right_hand_tracking: Arc<Mutex<Option<FingerPositions>>>,
    spatial_audio_enabled: bool,
    passthrough_opacity: f32,
}

impl VisionOsPlatform {
    /// Create new visionOS platform instance
    pub fn new() -> Self {
        Self {
            session_active: false,
            capabilities: PlatformCapabilities {
                supports_hand_tracking: true,
                supports_eye_tracking: true,
                supports_haptics: true, // Via TapticEngine in controllers/bands
                supports_passthrough: true,
                max_refresh_rate: 90, // Can go up to 96 Hz
                field_of_view: 100.0, // ~100° horizontal
                has_6dof: true,
            },
            head_transform: Arc::new(Mutex::new(None)),
            left_hand_transform: Arc::new(Mutex::new(None)),
            right_hand_transform: Arc::new(Mutex::new(None)),
            left_hand_tracking: Arc::new(Mutex::new(None)),
            right_hand_tracking: Arc::new(Mutex::new(None)),
            spatial_audio_enabled: true,
            passthrough_opacity: 1.0,
        }
    }

    /// Set passthrough opacity (0.0 = fully immersive, 1.0 = full passthrough)
    pub fn set_passthrough_opacity(&mut self, opacity: f32) {
        self.passthrough_opacity = opacity.clamp(0.0, 1.0);
    }

    /// Enable/disable spatial audio with RealityKit
    pub fn set_spatial_audio(&mut self, enabled: bool) {
        self.spatial_audio_enabled = enabled;
    }

    /// Convert ARKit hand anchor to FingerPositions
    /// In production: Use ARHandAnchor.handSkeleton joint transforms
    fn convert_arkit_hand(&self, hand: Hand) -> Option<FingerPositions> {
        let tracking = match hand {
            Hand::Left => self.left_hand_tracking.lock().unwrap().clone(),
            Hand::Right => self.right_hand_tracking.lock().unwrap().clone(),
        };
        tracking
    }
}

impl Default for VisionOsPlatform {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl VrPlatform for VisionOsPlatform {
    async fn initialize(&mut self) -> Result<(), PlatformError> {
        // In production:
        // 1. Initialize ARKitSession
        // 2. Request hand tracking authorization
        // 3. Create ImmersiveSpace
        // 4. Set up RealityKit scene with spatial audio
        // 5. Configure passthrough rendering
        
        println!("visionOS Platform initialized for Apple Vision Pro");
        Ok(())
    }

    fn get_capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn get_device_type(&self) -> DeviceType {
        DeviceType::VisionPro
    }

    async fn start_session(&mut self) -> Result<(), PlatformError> {
        // In production:
        // - Open ImmersiveSpace
        // - Start ARKit session with hand tracking
        // - Enable Spatial Personas if multiplayer
        
        self.session_active = true;
        println!("visionOS session started");
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), PlatformError> {
        // In production:
        // - Dismiss ImmersiveSpace
        // - Stop ARKit session
        // - Clean up RealityKit resources
        
        self.session_active = false;
        println!("visionOS session ended");
        Ok(())
    }

    fn get_head_transform(&self) -> Option<Transform> {
        // In production: Query ARKit camera transform
        self.head_transform.lock().unwrap().clone()
    }

    fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>) {
        let left = self.left_hand_transform.lock().unwrap().clone();
        let right = self.right_hand_transform.lock().unwrap().clone();
        (left, right)
    }

    fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)> {
        // In production:
        // 1. Query ARHandTrackingProvider
        // 2. Get ARHandAnchor for left/right hand
        // 3. Extract handSkeleton.joint positions for all 27 joints
        // 4. Map to FingerJoints (thumb metacarpal = joint 4, index proximal = joint 9, etc.)
        
        if let Some(left) = self.convert_arkit_hand(Hand::Left) {
            return Some((Hand::Left, left));
        }
        if let Some(right) = self.convert_arkit_hand(Hand::Right) {
            return Some((Hand::Right, right));
        }
        None
    }

    fn trigger_haptic(&mut self, hand: Hand, intensity: f32, duration_ms: u32) -> Result<(), PlatformError> {
        // In production:
        // - Use CHHapticEngine for controller haptics
        // - Or TapticEngine for wristbands
        // - Create CHHapticPattern with intensity and duration
        
        println!("visionOS haptic: {:?} hand, intensity {}, duration {}ms", hand, intensity, duration_ms);
        Ok(())
    }

    fn get_frame_time(&self) -> f32 {
        // Target 90 Hz (11.1 ms per frame)
        11.1
    }

    fn is_session_active(&self) -> bool {
        self.session_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_visionos_initialization() {
        let mut platform = VisionOsPlatform::new();
        assert!(platform.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let mut platform = VisionOsPlatform::new();
        platform.initialize().await.unwrap();
        
        platform.start_session().await.unwrap();
        assert!(platform.is_session_active());
        platform.end_session().await.unwrap();
        assert!(!platform.is_session_active());
    }

    #[test]
    fn test_capabilities() {
        let platform = VisionOsPlatform::new();
        let caps = platform.get_capabilities();
        
        assert!(caps.supports_hand_tracking);
        assert!(caps.supports_eye_tracking);
        assert!(caps.supports_passthrough);
        assert!(caps.has_6dof);
    }

    #[test]
    fn test_passthrough_opacity() {
        let mut platform = VisionOsPlatform::new();
        platform.set_passthrough_opacity(0.5);
        assert_eq!(platform.passthrough_opacity, 0.5);
        
        // Test clamping
        platform.set_passthrough_opacity(1.5);
        assert_eq!(platform.passthrough_opacity, 1.0);
    }
}
