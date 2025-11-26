//! visionOS Platform Implementation - PRODUCTION
//!
//! Apple Vision Pro integration via Swift/Obj-C FFI bridge.
//! Requires the dchat-visionos-bridge framework when running on visionOS.

use super::{PlatformCapabilities, PlatformError, VrPlatform};
use crate::{
    gesture::{FingerJoints, FingerPositions, Hand},
    DeviceType, Transform, Vector3,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// ARKit hand skeleton joint indices
/// Reference: https://developer.apple.com/documentation/arkit/arskeletonjointname
pub mod arkit_joints {
    pub const WRIST: usize = 0;
    pub const THUMB_KNUCKLE: usize = 1;
    pub const THUMB_INTERMEDIATE_BASE: usize = 2;
    pub const THUMB_INTERMEDIATE_TIP: usize = 3;
    pub const THUMB_TIP: usize = 4;
    pub const INDEX_METACARPAL: usize = 5;
    pub const INDEX_KNUCKLE: usize = 6;
    pub const INDEX_INTERMEDIATE_BASE: usize = 7;
    pub const INDEX_INTERMEDIATE_TIP: usize = 8;
    pub const INDEX_TIP: usize = 9;
    pub const MIDDLE_METACARPAL: usize = 10;
    pub const MIDDLE_KNUCKLE: usize = 11;
    pub const MIDDLE_INTERMEDIATE_BASE: usize = 12;
    pub const MIDDLE_INTERMEDIATE_TIP: usize = 13;
    pub const MIDDLE_TIP: usize = 14;
    pub const RING_METACARPAL: usize = 15;
    pub const RING_KNUCKLE: usize = 16;
    pub const RING_INTERMEDIATE_BASE: usize = 17;
    pub const RING_INTERMEDIATE_TIP: usize = 18;
    pub const RING_TIP: usize = 19;
    pub const LITTLE_METACARPAL: usize = 20;
    pub const LITTLE_KNUCKLE: usize = 21;
    pub const LITTLE_INTERMEDIATE_BASE: usize = 22;
    pub const LITTLE_INTERMEDIATE_TIP: usize = 23;
    pub const LITTLE_TIP: usize = 24;
    pub const FOREARM_WRIST: usize = 25;
    pub const FOREARM_ARM: usize = 26;
    pub const JOINT_COUNT: usize = 27;
}

/// Joint position from ARKit
#[derive(Debug, Clone, Copy, Default)]
pub struct ArkitJoint {
    pub position: [f32; 3],
    pub is_tracked: bool,
}

// ============================================================================
// FFI Bridge to Swift/Objective-C (visionOS only)
// ============================================================================

#[cfg(target_os = "visionos")]
mod ffi {
    use super::*;

    #[link(name = "dchat_visionos_bridge", kind = "framework")]
    extern "C" {
        /// Initialize ARKit session with hand tracking
        pub fn dchat_arkit_init() -> i32;

        /// Start immersive space
        pub fn dchat_start_immersive_space() -> i32;

        /// Stop immersive space
        pub fn dchat_stop_immersive_space() -> i32;

        /// Get hand tracking authorization status
        pub fn dchat_hand_tracking_authorized() -> bool;

        /// Request hand tracking authorization
        pub fn dchat_request_hand_tracking_auth() -> i32;

        /// Get left hand joints (fills buffer with 27 joints * 4 floats each)
        pub fn dchat_get_left_hand_joints(out_joints: *mut f32, out_tracked: *mut bool) -> i32;

        /// Get right hand joints
        pub fn dchat_get_right_hand_joints(out_joints: *mut f32, out_tracked: *mut bool) -> i32;

        /// Get head transform (position + quaternion = 7 floats)
        pub fn dchat_get_head_transform(out_transform: *mut f32) -> i32;

        /// Trigger haptic feedback
        pub fn dchat_trigger_haptic(hand: i32, intensity: f32, duration_ms: u32) -> i32;

        /// Set passthrough opacity
        pub fn dchat_set_passthrough_opacity(opacity: f32) -> i32;

        /// Enable/disable spatial audio
        pub fn dchat_set_spatial_audio(enabled: bool) -> i32;

        /// Get current frame rate
        pub fn dchat_get_frame_rate() -> f32;

        /// Cleanup ARKit resources
        pub fn dchat_arkit_cleanup();
    }
}

/// visionOS platform backend for Apple Vision Pro
pub struct VisionOsPlatform {
    session_active: bool,
    initialized: bool,
    capabilities: PlatformCapabilities,
    head_transform: Arc<Mutex<Option<Transform>>>,
    left_hand_joints: Arc<Mutex<[ArkitJoint; arkit_joints::JOINT_COUNT]>>,
    right_hand_joints: Arc<Mutex<[ArkitJoint; arkit_joints::JOINT_COUNT]>>,
    spatial_audio_enabled: bool,
    passthrough_opacity: f32,
    frame_time_ms: f32,
}

impl VisionOsPlatform {
    /// Create new visionOS platform instance
    pub fn new() -> Self {
        Self {
            session_active: false,
            initialized: false,
            capabilities: PlatformCapabilities {
                supports_hand_tracking: true,
                supports_eye_tracking: true,
                supports_haptics: true,
                supports_passthrough: true,
                max_refresh_rate: 96,
                field_of_view: 100.0,
                has_6dof: true,
            },
            head_transform: Arc::new(Mutex::new(None)),
            left_hand_joints: Arc::new(Mutex::new(
                [ArkitJoint::default(); arkit_joints::JOINT_COUNT],
            )),
            right_hand_joints: Arc::new(Mutex::new(
                [ArkitJoint::default(); arkit_joints::JOINT_COUNT],
            )),
            spatial_audio_enabled: true,
            passthrough_opacity: 1.0,
            frame_time_ms: 11.1, // ~90Hz default
        }
    }

    /// Set passthrough opacity (0.0 = fully immersive, 1.0 = full passthrough)
    pub fn set_passthrough_opacity(&mut self, opacity: f32) -> Result<(), PlatformError> {
        self.passthrough_opacity = opacity.clamp(0.0, 1.0);

        #[cfg(target_os = "visionos")]
        {
            let result = unsafe { ffi::dchat_set_passthrough_opacity(self.passthrough_opacity) };
            if result != 0 {
                return Err(PlatformError::SessionError(format!(
                    "Failed to set passthrough: {}",
                    result
                )));
            }
        }

        Ok(())
    }

    /// Enable/disable spatial audio
    pub fn set_spatial_audio(&mut self, enabled: bool) -> Result<(), PlatformError> {
        self.spatial_audio_enabled = enabled;

        #[cfg(target_os = "visionos")]
        {
            let result = unsafe { ffi::dchat_set_spatial_audio(enabled) };
            if result != 0 {
                return Err(PlatformError::SessionError(format!(
                    "Failed to set spatial audio: {}",
                    result
                )));
            }
        }

        Ok(())
    }

    /// Poll hand tracking data from ARKit
    #[cfg(target_os = "visionos")]
    pub fn poll_hand_tracking(&self) {
        // Left hand
        {
            let mut positions = [0.0f32; arkit_joints::JOINT_COUNT * 3];
            let mut tracked = [false; arkit_joints::JOINT_COUNT];

            let result = unsafe {
                ffi::dchat_get_left_hand_joints(positions.as_mut_ptr(), tracked.as_mut_ptr())
            };

            if result == 0 {
                let mut joints = self.left_hand_joints.lock().unwrap();
                for i in 0..arkit_joints::JOINT_COUNT {
                    joints[i] = ArkitJoint {
                        position: [
                            positions[i * 3],
                            positions[i * 3 + 1],
                            positions[i * 3 + 2],
                        ],
                        is_tracked: tracked[i],
                    };
                }
            }
        }

        // Right hand
        {
            let mut positions = [0.0f32; arkit_joints::JOINT_COUNT * 3];
            let mut tracked = [false; arkit_joints::JOINT_COUNT];

            let result = unsafe {
                ffi::dchat_get_right_hand_joints(positions.as_mut_ptr(), tracked.as_mut_ptr())
            };

            if result == 0 {
                let mut joints = self.right_hand_joints.lock().unwrap();
                for i in 0..arkit_joints::JOINT_COUNT {
                    joints[i] = ArkitJoint {
                        position: [
                            positions[i * 3],
                            positions[i * 3 + 1],
                            positions[i * 3 + 2],
                        ],
                        is_tracked: tracked[i],
                    };
                }
            }
        }
    }

    /// Poll head tracking from ARKit
    #[cfg(target_os = "visionos")]
    pub fn poll_head_tracking(&self) {
        let mut transform = [0.0f32; 7]; // position (3) + quaternion (4)
        let result = unsafe { ffi::dchat_get_head_transform(transform.as_mut_ptr()) };

        if result == 0 {
            *self.head_transform.lock().unwrap() = Some(Transform {
                position: Vector3::new(transform[0], transform[1], transform[2]),
                rotation: [transform[3], transform[4], transform[5], transform[6]],
                scale: Vector3::new(1.0, 1.0, 1.0),
            });
        }
    }

    /// Convert ARKit joints to FingerPositions
    fn convert_arkit_hand(&self, hand: Hand) -> Option<FingerPositions> {
        let joints = match hand {
            Hand::Left => self.left_hand_joints.lock().unwrap(),
            Hand::Right => self.right_hand_joints.lock().unwrap(),
        };

        let wrist = &joints[arkit_joints::WRIST];
        if !wrist.is_tracked {
            return None;
        }

        let create_finger =
            |tip_idx: usize, base_idx: usize, int_idx: usize| -> Option<FingerJoints> {
                let tip = &joints[tip_idx];
                let base = &joints[base_idx];
                let intermediate = &joints[int_idx];
                if !tip.is_tracked || !base.is_tracked {
                    return None;
                }

                Some(FingerJoints {
                    metacarpal: Vector3::new(base.position[0], base.position[1], base.position[2]),
                    proximal: Vector3::new(base.position[0], base.position[1], base.position[2]),
                    intermediate: Vector3::new(intermediate.position[0], intermediate.position[1], intermediate.position[2]),
                    distal: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                    tip: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                })
            };

        let thumb = create_finger(arkit_joints::THUMB_TIP, arkit_joints::THUMB_KNUCKLE, arkit_joints::THUMB_INTERMEDIATE_TIP)?;
        let index = create_finger(arkit_joints::INDEX_TIP, arkit_joints::INDEX_METACARPAL, arkit_joints::INDEX_INTERMEDIATE_TIP)?;
        let middle = create_finger(arkit_joints::MIDDLE_TIP, arkit_joints::MIDDLE_METACARPAL, arkit_joints::MIDDLE_INTERMEDIATE_TIP)?;
        let ring = create_finger(arkit_joints::RING_TIP, arkit_joints::RING_METACARPAL, arkit_joints::RING_INTERMEDIATE_TIP)?;
        let pinky = create_finger(arkit_joints::LITTLE_TIP, arkit_joints::LITTLE_METACARPAL, arkit_joints::LITTLE_INTERMEDIATE_TIP)?;
        
        Some(FingerPositions {
            thumb,
            index,
            middle,
            ring,
            pinky,
        })
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
        #[cfg(target_os = "visionos")]
        {
            // Initialize ARKit
            let result = unsafe { ffi::dchat_arkit_init() };
            if result != 0 {
                return Err(PlatformError::InitializationFailed(format!(
                    "ARKit init failed: {}",
                    result
                )));
            }

            // Check/request hand tracking authorization
            if !unsafe { ffi::dchat_hand_tracking_authorized() } {
                let auth_result = unsafe { ffi::dchat_request_hand_tracking_auth() };
                if auth_result != 0 {
                    tracing::warn!("Hand tracking authorization not granted");
                }
            }

            self.initialized = true;
            tracing::info!("visionOS platform initialized for Apple Vision Pro");
        }

        #[cfg(not(target_os = "visionos"))]
        {
            tracing::warn!(
                "visionOS platform not available on this OS. Vision Pro features disabled."
            );
            self.initialized = true;
        }

        Ok(())
    }

    fn get_capabilities(&self) -> PlatformCapabilities {
        self.capabilities.clone()
    }

    fn get_device_type(&self) -> DeviceType {
        DeviceType::VisionPro
    }

    async fn start_session(&mut self) -> Result<(), PlatformError> {
        if !self.initialized {
            return Err(PlatformError::SessionError("Not initialized".to_string()));
        }

        #[cfg(target_os = "visionos")]
        {
            let result = unsafe { ffi::dchat_start_immersive_space() };
            if result != 0 {
                return Err(PlatformError::SessionError(format!(
                    "Failed to start immersive space: {}",
                    result
                )));
            }
        }

        self.session_active = true;
        tracing::info!("visionOS session started");
        Ok(())
    }

    async fn end_session(&mut self) -> Result<(), PlatformError> {
        #[cfg(target_os = "visionos")]
        {
            let result = unsafe { ffi::dchat_stop_immersive_space() };
            if result != 0 {
                tracing::warn!("Failed to stop immersive space cleanly: {}", result);
            }
        }

        self.session_active = false;
        tracing::info!("visionOS session ended");
        Ok(())
    }

    fn get_head_transform(&self) -> Option<Transform> {
        self.head_transform.lock().unwrap().clone()
    }

    fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>) {
        (None, None) // Use get_hand_tracking for full joint data
    }

    fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)> {
        if let Some(left) = self.convert_arkit_hand(Hand::Left) {
            return Some((Hand::Left, left));
        }
        if let Some(right) = self.convert_arkit_hand(Hand::Right) {
            return Some((Hand::Right, right));
        }
        None
    }

    fn trigger_haptic(
        &mut self,
        hand: Hand,
        intensity: f32,
        duration_ms: u32,
    ) -> Result<(), PlatformError> {
        if !self.capabilities.supports_haptics {
            return Err(PlatformError::HapticsUnsupported);
        }

        #[cfg(target_os = "visionos")]
        {
            let hand_id = match hand {
                Hand::Left => 0,
                Hand::Right => 1,
            };
            let result =
                unsafe { ffi::dchat_trigger_haptic(hand_id, intensity.clamp(0.0, 1.0), duration_ms) };
            if result != 0 {
                return Err(PlatformError::HapticsUnsupported);
            }
        }

        #[cfg(not(target_os = "visionos"))]
        {
            tracing::debug!(
                "Haptic request (non-visionOS): {:?}, intensity={:.2}, duration={}ms",
                hand,
                intensity,
                duration_ms
            );
        }

        Ok(())
    }

    fn get_frame_time(&self) -> f32 {
        #[cfg(target_os = "visionos")]
        {
            let rate = unsafe { ffi::dchat_get_frame_rate() };
            if rate > 0.0 {
                return 1000.0 / rate;
            }
        }

        self.frame_time_ms
    }

    fn is_session_active(&self) -> bool {
        self.session_active
    }
}

impl Drop for VisionOsPlatform {
    fn drop(&mut self) {
        #[cfg(target_os = "visionos")]
        {
            if self.initialized {
                unsafe { ffi::dchat_arkit_cleanup() };
            }
        }
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
        assert!(platform.set_passthrough_opacity(0.5).is_ok());
        assert_eq!(platform.passthrough_opacity, 0.5);

        // Test clamping
        assert!(platform.set_passthrough_opacity(1.5).is_ok());
        assert_eq!(platform.passthrough_opacity, 1.0);
    }
}

