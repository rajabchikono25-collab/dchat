//! OpenXR Platform Implementation
//!
//! Supports Meta Quest (2/3/Pro), HTC Vive, Valve Index, and other OpenXR devices

use super::{VrPlatform, PlatformCapabilities, PlatformError};
use crate::{DeviceType, Transform, Vector3, gesture::{Hand, FingerJoints, FingerPositions}};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// OpenXR platform backend
pub struct OpenXrPlatform {
    device_type: DeviceType,
    session_active: bool,
    capabilities: PlatformCapabilities,
    head_transform: Arc<Mutex<Option<Transform>>>,
    left_hand_transform: Arc<Mutex<Option<Transform>>>,
    right_hand_transform: Arc<Mutex<Option<Transform>>>,
    left_hand_tracking: Arc<Mutex<Option<FingerPositions>>>,
    right_hand_tracking: Arc<Mutex<Option<FingerPositions>>>,
    frame_time: Arc<Mutex<f32>>,
}

impl OpenXrPlatform {
    /// Create new OpenXR platform instance
    pub fn new(device_type: DeviceType) -> Self {
        let capabilities = match device_type {
            DeviceType::MetaQuest => PlatformCapabilities {
                supports_hand_tracking: true,
                supports_eye_tracking: false,
                supports_haptics: true,
                supports_passthrough: true,
                max_refresh_rate: 120,
                field_of_view: 110.0,
                has_6dof: true,
            },
            DeviceType::ValveIndex => PlatformCapabilities {
                supports_hand_tracking: true,
                supports_eye_tracking: true,
                supports_haptics: true,
                supports_passthrough: false,
                max_refresh_rate: 144,
                field_of_view: 130.0,
                has_6dof: true,
            },
            DeviceType::HTCVive => PlatformCapabilities {
                supports_hand_tracking: false,
                supports_eye_tracking: false,
                supports_haptics: true,
                supports_passthrough: false,
                max_refresh_rate: 90,
                field_of_view: 110.0,
                has_6dof: true,
            },
            _ => PlatformCapabilities {
                supports_hand_tracking: false,
                supports_eye_tracking: false,
                supports_haptics: false,
                supports_passthrough: false,
                max_refresh_rate: 60,
                field_of_view: 90.0,
                has_6dof: false,
            },
        };

        Self {
            device_type,
            session_active: false,
            capabilities,
            head_transform: Arc::new(Mutex::new(None)),
            left_hand_transform: Arc::new(Mutex::new(None)),
            right_hand_transform: Arc::new(Mutex::new(None)),
            left_hand_tracking: Arc::new(Mutex::new(None)),
            right_hand_tracking: Arc::new(Mutex::new(None)),
            frame_time: Arc::new(Mutex::new(11.1)), // Default 90fps
        }
    }

    /// Simulate hand tracking data conversion from OpenXR joints
    /// In production: Convert XrHandJointEXT to FingerJoints
    fn convert_hand_joints(&self, hand: Hand) -> Option<FingerPositions> {
        let tracking = match hand {
            Hand::Left => self.left_hand_tracking.lock().unwrap().clone(),
            Hand::Right => self.right_hand_tracking.lock().unwrap().clone(),
        };
        tracking
    }
}

#[async_trait]
impl VrPlatform for OpenXrPlatform {
    async fn initialize(&mut self) -> Result<(), PlatformError> {
        // In production: Initialize OpenXR instance with extensions
        // - XR_EXT_hand_tracking
        // - XR_FB_passthrough (Meta Quest)
        // - XR_HTC_vive_focus3_controller_interaction
        
        println!("OpenXR Platform initialized for {:?}", self.device_type);
        Ok(())
    }

    fn get_capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn get_device_type(&self) -> DeviceType {
        self.device_type.clone()
    }

    async fn start_session(&mut self) -> Result<(), PlatformError> {
        // In production: xrBeginSession with composition layers
        self.session_active = true;
        println!("OpenXR session started");
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), PlatformError> {
        // In production: xrEndSession and cleanup
        self.session_active = false;
        println!("OpenXR session ended");
        Ok(())
    }

    fn get_head_transform(&self) -> Option<Transform> {
        self.head_transform.lock().unwrap().clone()
    }

    fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>) {
        let left = self.left_hand_transform.lock().unwrap().clone();
        let right = self.right_hand_transform.lock().unwrap().clone();
        (left, right)
    }

    fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)> {
        // In production: Query XrHandTrackerEXT for joint locations
        // Convert XR_HAND_JOINT_THUMB_TIP_EXT, etc. to FingerJoints
        
        if let Some(left) = self.convert_hand_joints(Hand::Left) {
            return Some((Hand::Left, left));
        }
        if let Some(right) = self.convert_hand_joints(Hand::Right) {
            return Some((Hand::Right, right));
        }
        None
    }

    fn trigger_haptic(&mut self, hand: Hand, intensity: f32, duration_ms: u32) -> Result<(), PlatformError> {
        if !self.capabilities.supports_haptics {
            return Err(PlatformError::HapticsUnsupported);
        }

        // In production: xrApplyHapticFeedback with XrHapticVibration
        // - frequency: Match device (e.g., 160 Hz for Quest controllers)
        // - amplitude: intensity clamped 0.0-1.0
        // - duration: duration_ms in nanoseconds
        
        println!("Haptic feedback: {:?} hand, intensity {}, duration {}ms", hand, intensity, duration_ms);
        Ok(())
    }

    fn get_frame_time(&self) -> f32 {
        *self.frame_time.lock().unwrap()
    }

    fn is_session_active(&self) -> bool {
        self.session_active
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_openxr_initialization() {
        let mut platform = OpenXrPlatform::new(DeviceType::MetaQuest);
        assert!(platform.initialize().await.is_ok());
    }

    #[tokio::test]
    async fn test_session_lifecycle() {
        let mut platform = OpenXrPlatform::new(DeviceType::ValveIndex);
        platform.initialize().await.unwrap();
        
        assert!(!platform.is_session_active());
        platform.start_session().await.unwrap();
        assert!(platform.is_session_active());
        platform.end_session().await.unwrap();
        assert!(!platform.is_session_active());
    }

    #[test]
    fn test_capabilities_meta_quest() {
        let platform = OpenXrPlatform::new(DeviceType::MetaQuest);
        let caps = platform.get_capabilities();
        
        assert!(caps.supports_hand_tracking);
        assert!(caps.supports_haptics);
        assert!(caps.supports_passthrough);
        assert_eq!(caps.max_refresh_rate, 120);
    }

    #[test]
    fn test_haptic_feedback() {
        let mut platform = OpenXrPlatform::new(DeviceType::MetaQuest);
        let result = platform.trigger_haptic(Hand::Right, 0.5, 100);
        assert!(result.is_ok());
    }
}
