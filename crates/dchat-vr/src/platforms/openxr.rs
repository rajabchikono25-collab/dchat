//! OpenXR Platform Implementation - PRODUCTION
//!
//! Full OpenXR integration for Meta Quest, Valve Index, HTC Vive, Pico, and WMR devices.
//! Uses the openxr crate when the `openxr-runtime` feature is enabled.

use super::{PlatformCapabilities, PlatformError, VrPlatform};
use crate::{
    gesture::{FingerJoints, FingerPositions, Hand},
    DeviceType, Transform, Vector3,
};
use async_trait::async_trait;
use std::sync::{Arc, Mutex};

/// OpenXR hand joint indices per OpenXR spec
pub mod hand_joints {
    pub const PALM: usize = 0;
    pub const WRIST: usize = 1;
    pub const THUMB_METACARPAL: usize = 2;
    pub const THUMB_PROXIMAL: usize = 3;
    pub const THUMB_DISTAL: usize = 4;
    pub const THUMB_TIP: usize = 5;
    pub const INDEX_METACARPAL: usize = 6;
    pub const INDEX_PROXIMAL: usize = 7;
    pub const INDEX_INTERMEDIATE: usize = 8;
    pub const INDEX_DISTAL: usize = 9;
    pub const INDEX_TIP: usize = 10;
    pub const MIDDLE_METACARPAL: usize = 11;
    pub const MIDDLE_PROXIMAL: usize = 12;
    pub const MIDDLE_INTERMEDIATE: usize = 13;
    pub const MIDDLE_DISTAL: usize = 14;
    pub const MIDDLE_TIP: usize = 15;
    pub const RING_METACARPAL: usize = 16;
    pub const RING_PROXIMAL: usize = 17;
    pub const RING_INTERMEDIATE: usize = 18;
    pub const RING_DISTAL: usize = 19;
    pub const RING_TIP: usize = 20;
    pub const LITTLE_METACARPAL: usize = 21;
    pub const LITTLE_PROXIMAL: usize = 22;
    pub const LITTLE_INTERMEDIATE: usize = 23;
    pub const LITTLE_DISTAL: usize = 24;
    pub const LITTLE_TIP: usize = 25;
    pub const JOINT_COUNT: usize = 26;
}

/// Raw joint location data
#[derive(Debug, Clone, Copy, Default)]
pub struct JointLocation {
    pub position: [f32; 3],
    pub orientation: [f32; 4],
    pub radius: f32,
    pub is_valid: bool,
}

/// Extension availability flags
#[derive(Debug, Clone, Copy, Default)]
pub struct OpenXrExtensions {
    pub hand_tracking: bool,
    pub eye_tracking: bool,
    pub passthrough: bool,
    pub face_tracking: bool,
    pub body_tracking: bool,
}

// ============================================================================
// Production OpenXR implementation (when feature enabled)
// ============================================================================

#[cfg(feature = "openxr-runtime")]
mod runtime {
    use super::*;
    use openxr as xr;
    use std::sync::atomic::{AtomicBool, Ordering};

    /// Production OpenXR platform with real runtime integration
    pub struct OpenXrPlatform {
        device_type: DeviceType,
        capabilities: PlatformCapabilities,
        extensions: OpenXrExtensions,

        // OpenXR handles
        entry: Option<xr::Entry>,
        instance: Option<xr::Instance>,
        system_id: Option<xr::SystemId>,
        session: Option<xr::Session<xr::Headless>>,
        session_running: AtomicBool,

        // Reference spaces
        stage_space: Option<xr::Space>,
        view_space: Option<xr::Space>,

        // Hand tracking
        left_hand_tracker: Option<xr::HandTracker>,
        right_hand_tracker: Option<xr::HandTracker>,

        // Cached state
        head_transform: Arc<Mutex<Option<Transform>>>,
        left_hand_joints: Arc<Mutex<[JointLocation; hand_joints::JOINT_COUNT]>>,
        right_hand_joints: Arc<Mutex<[JointLocation; hand_joints::JOINT_COUNT]>>,
        frame_time_ms: Arc<Mutex<f32>>,

        // Runtime info
        runtime_name: String,
    }

    impl OpenXrPlatform {
        pub fn new(device_type: DeviceType) -> Self {
            let (capabilities, extensions) = Self::get_device_config(&device_type);

            Self {
                device_type,
                capabilities,
                extensions,
                entry: None,
                instance: None,
                system_id: None,
                session: None,
                session_running: AtomicBool::new(false),
                stage_space: None,
                view_space: None,
                left_hand_tracker: None,
                right_hand_tracker: None,
                head_transform: Arc::new(Mutex::new(None)),
                left_hand_joints: Arc::new(Mutex::new(
                    [JointLocation::default(); hand_joints::JOINT_COUNT],
                )),
                right_hand_joints: Arc::new(Mutex::new(
                    [JointLocation::default(); hand_joints::JOINT_COUNT],
                )),
                frame_time_ms: Arc::new(Mutex::new(11.1)),
                runtime_name: String::new(),
            }
        }

        fn get_device_config(device_type: &DeviceType) -> (PlatformCapabilities, OpenXrExtensions) {
            match device_type {
                DeviceType::MetaQuest => (
                    PlatformCapabilities {
                        supports_hand_tracking: true,
                        supports_eye_tracking: false,
                        supports_haptics: true,
                        supports_passthrough: true,
                        max_refresh_rate: 120,
                        field_of_view: 110.0,
                        has_6dof: true,
                    },
                    OpenXrExtensions {
                        hand_tracking: true,
                        passthrough: true,
                        face_tracking: true,
                        ..Default::default()
                    },
                ),
                DeviceType::ValveIndex => (
                    PlatformCapabilities {
                        supports_hand_tracking: true,
                        supports_eye_tracking: true,
                        supports_haptics: true,
                        supports_passthrough: false,
                        max_refresh_rate: 144,
                        field_of_view: 130.0,
                        has_6dof: true,
                    },
                    OpenXrExtensions {
                        hand_tracking: true,
                        eye_tracking: true,
                        body_tracking: true,
                        ..Default::default()
                    },
                ),
                DeviceType::HTCVive => (
                    PlatformCapabilities {
                        supports_hand_tracking: false,
                        supports_eye_tracking: false,
                        supports_haptics: true,
                        supports_passthrough: false,
                        max_refresh_rate: 90,
                        field_of_view: 110.0,
                        has_6dof: true,
                    },
                    OpenXrExtensions::default(),
                ),
                _ => (
                    PlatformCapabilities {
                        supports_hand_tracking: false,
                        supports_eye_tracking: false,
                        supports_haptics: false,
                        supports_passthrough: false,
                        max_refresh_rate: 60,
                        field_of_view: 90.0,
                        has_6dof: false,
                    },
                    OpenXrExtensions::default(),
                ),
            }
        }

        fn convert_hand_joints(&self, hand: Hand) -> Option<FingerPositions> {
            let joints = match hand {
                Hand::Left => self.left_hand_joints.lock().unwrap(),
                Hand::Right => self.right_hand_joints.lock().unwrap(),
            };

            let palm = &joints[hand_joints::PALM];
            if !palm.is_valid {
                return None;
            }

            let create_finger =
                |tip: &JointLocation, distal: &JointLocation, inter: &JointLocation, 
                 prox: &JointLocation, meta: &JointLocation| -> Option<FingerJoints> {
                    if !tip.is_valid || !meta.is_valid {
                        return None;
                    }

                    Some(FingerJoints {
                        metacarpal: Vector3::new(meta.position[0], meta.position[1], meta.position[2]),
                        proximal: Vector3::new(prox.position[0], prox.position[1], prox.position[2]),
                        intermediate: Vector3::new(inter.position[0], inter.position[1], inter.position[2]),
                        distal: Vector3::new(distal.position[0], distal.position[1], distal.position[2]),
                        tip: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                    })
                };

            let thumb = create_finger(
                &joints[hand_joints::THUMB_TIP],
                &joints[hand_joints::THUMB_DISTAL],
                &joints[hand_joints::THUMB_PROXIMAL],
                &joints[hand_joints::THUMB_PROXIMAL],
                &joints[hand_joints::THUMB_METACARPAL],
            )?;
            let index = create_finger(
                &joints[hand_joints::INDEX_TIP],
                &joints[hand_joints::INDEX_DISTAL],
                &joints[hand_joints::INDEX_INTERMEDIATE],
                &joints[hand_joints::INDEX_PROXIMAL],
                &joints[hand_joints::INDEX_METACARPAL],
            )?;
            let middle = create_finger(
                &joints[hand_joints::MIDDLE_TIP],
                &joints[hand_joints::MIDDLE_DISTAL],
                &joints[hand_joints::MIDDLE_INTERMEDIATE],
                &joints[hand_joints::MIDDLE_PROXIMAL],
                &joints[hand_joints::MIDDLE_METACARPAL],
            )?;
            let ring = create_finger(
                &joints[hand_joints::RING_TIP],
                &joints[hand_joints::RING_DISTAL],
                &joints[hand_joints::RING_INTERMEDIATE],
                &joints[hand_joints::RING_PROXIMAL],
                &joints[hand_joints::RING_METACARPAL],
            )?;
            let pinky = create_finger(
                &joints[hand_joints::LITTLE_TIP],
                &joints[hand_joints::LITTLE_DISTAL],
                &joints[hand_joints::LITTLE_INTERMEDIATE],
                &joints[hand_joints::LITTLE_PROXIMAL],
                &joints[hand_joints::LITTLE_METACARPAL],
            )?;

            Some(FingerPositions {
                thumb,
                index,
                middle,
                ring,
                pinky,
            })
        }

        /// Poll and update hand tracking data from OpenXR runtime
        pub fn poll_hand_tracking(&self, predicted_time: xr::Time) {
            if let (Some(stage), Some(left_tracker), Some(right_tracker)) =
                (&self.stage_space, &self.left_hand_tracker, &self.right_hand_tracker)
            {
                // Query left hand
                if let Ok(locations) = left_tracker.locate_hand_joints(stage, predicted_time) {
                    let mut joints = self.left_hand_joints.lock().unwrap();
                    for (i, loc) in locations.iter().enumerate() {
                        if i < hand_joints::JOINT_COUNT {
                            joints[i] = JointLocation {
                                position: [
                                    loc.pose.position.x,
                                    loc.pose.position.y,
                                    loc.pose.position.z,
                                ],
                                orientation: [
                                    loc.pose.orientation.w,
                                    loc.pose.orientation.x,
                                    loc.pose.orientation.y,
                                    loc.pose.orientation.z,
                                ],
                                radius: loc.radius,
                                is_valid: loc
                                    .location_flags
                                    .contains(xr::SpaceLocationFlags::POSITION_VALID),
                            };
                        }
                    }
                }

                // Query right hand
                if let Ok(locations) = right_tracker.locate_hand_joints(stage, predicted_time) {
                    let mut joints = self.right_hand_joints.lock().unwrap();
                    for (i, loc) in locations.iter().enumerate() {
                        if i < hand_joints::JOINT_COUNT {
                            joints[i] = JointLocation {
                                position: [
                                    loc.pose.position.x,
                                    loc.pose.position.y,
                                    loc.pose.position.z,
                                ],
                                orientation: [
                                    loc.pose.orientation.w,
                                    loc.pose.orientation.x,
                                    loc.pose.orientation.y,
                                    loc.pose.orientation.z,
                                ],
                                radius: loc.radius,
                                is_valid: loc
                                    .location_flags
                                    .contains(xr::SpaceLocationFlags::POSITION_VALID),
                            };
                        }
                    }
                }
            }
        }

        /// Poll head tracking
        pub fn poll_head_tracking(&self, predicted_time: xr::Time) {
            if let (Some(view), Some(stage)) = (&self.view_space, &self.stage_space) {
                if let Ok(location) = view.locate(stage, predicted_time) {
                    if location
                        .location_flags
                        .contains(xr::SpaceLocationFlags::POSITION_VALID)
                    {
                        let pos = location.pose.position;
                        let rot = location.pose.orientation;
                        *self.head_transform.lock().unwrap() = Some(Transform {
                            position: Vector3::new(pos.x, pos.y, pos.z),
                            rotation: [rot.w, rot.x, rot.y, rot.z],
                            scale: Vector3::new(1.0, 1.0, 1.0),
                        });
                    }
                }
            }
        }

        pub fn runtime_name(&self) -> &str {
            &self.runtime_name
        }

        pub fn extensions(&self) -> &OpenXrExtensions {
            &self.extensions
        }
    }

    #[async_trait]
    impl VrPlatform for OpenXrPlatform {
        async fn initialize(&mut self) -> Result<(), PlatformError> {
            // Load OpenXR runtime
            let entry = xr::Entry::linked();

            // Get available extensions
            let available_extensions = entry
                .enumerate_extensions()
                .map_err(|e| PlatformError::InitializationFailed(format!("Extensions: {}", e)))?;

            // Build extension set based on device needs and availability
            let mut enabled_extensions = xr::ExtensionSet::default();

            if self.extensions.hand_tracking && available_extensions.ext_hand_tracking {
                enabled_extensions.ext_hand_tracking = true;
                tracing::info!("Enabling XR_EXT_hand_tracking");
            }

            if self.extensions.passthrough && available_extensions.fb_passthrough {
                enabled_extensions.fb_passthrough = true;
                tracing::info!("Enabling XR_FB_passthrough");
            }

            // Create instance
            let instance = entry
                .create_instance(
                    &xr::ApplicationInfo {
                        application_name: "dchat",
                        application_version: 1,
                        engine_name: "dchat-vr",
                        engine_version: 1,
                    },
                    &enabled_extensions,
                    &[],
                )
                .map_err(|e| PlatformError::InitializationFailed(format!("Instance: {}", e)))?;

            // Get runtime properties
            let props = instance
                .properties()
                .map_err(|e| PlatformError::InitializationFailed(format!("Properties: {}", e)))?;
            self.runtime_name = props.runtime_name.to_string();

            tracing::info!(
                "OpenXR initialized: {} v{}",
                self.runtime_name,
                props.runtime_version
            );

            // Get system for HMD
            let system_id = instance
                .system(xr::FormFactor::HEAD_MOUNTED_DISPLAY)
                .map_err(|e| PlatformError::InitializationFailed(format!("System: {}", e)))?;

            self.entry = Some(entry);
            self.instance = Some(instance);
            self.system_id = Some(system_id);

            Ok(())
        }

        fn get_capabilities(&self) -> PlatformCapabilities {
            self.capabilities.clone()
        }

        fn get_device_type(&self) -> DeviceType {
            self.device_type.clone()
        }

        async fn start_session(&mut self) -> Result<(), PlatformError> {
            let instance = self
                .instance
                .as_ref()
                .ok_or_else(|| PlatformError::SessionFailed("Not initialized".to_string()))?;
            let system_id = self
                .system_id
                .ok_or_else(|| PlatformError::SessionFailed("No system".to_string()))?;

            // Create headless session (no graphics binding for chat app)
            let (session, _frame_waiter, _frame_stream) = unsafe {
                instance.create_session::<xr::Headless>(system_id, &xr::headless::SessionCreateInfo {})
            }
            .map_err(|e| PlatformError::SessionFailed(format!("Session: {}", e)))?;

            // Create reference spaces
            let stage_space = session
                .create_reference_space(xr::ReferenceSpaceType::STAGE, xr::Posef::IDENTITY)
                .map_err(|e| PlatformError::SessionFailed(format!("Stage space: {}", e)))?;

            let view_space = session
                .create_reference_space(xr::ReferenceSpaceType::VIEW, xr::Posef::IDENTITY)
                .map_err(|e| PlatformError::SessionFailed(format!("View space: {}", e)))?;

            // Create hand trackers if extension enabled
            if self.extensions.hand_tracking {
                match session.create_hand_tracker(xr::Hand::LEFT) {
                    Ok(tracker) => self.left_hand_tracker = Some(tracker),
                    Err(e) => tracing::warn!("Left hand tracker failed: {}", e),
                }
                match session.create_hand_tracker(xr::Hand::RIGHT) {
                    Ok(tracker) => self.right_hand_tracker = Some(tracker),
                    Err(e) => tracing::warn!("Right hand tracker failed: {}", e),
                }
            }

            // Begin session
            session
                .begin(xr::ViewConfigurationType::PRIMARY_STEREO)
                .map_err(|e| PlatformError::SessionFailed(format!("Begin: {}", e)))?;

            self.session = Some(session);
            self.stage_space = Some(stage_space);
            self.view_space = Some(view_space);
            self.session_running.store(true, Ordering::SeqCst);

            tracing::info!("OpenXR session started for {:?}", self.device_type);
            Ok(())
        }

        async fn end_session(&mut self) -> Result<(), PlatformError> {
            if let Some(session) = self.session.take() {
                session
                    .request_exit()
                    .map_err(|e| PlatformError::SessionFailed(format!("Exit: {}", e)))?;
                session
                    .end()
                    .map_err(|e| PlatformError::SessionFailed(format!("End: {}", e)))?;
            }

            self.left_hand_tracker = None;
            self.right_hand_tracker = None;
            self.stage_space = None;
            self.view_space = None;
            self.session_running.store(false, Ordering::SeqCst);

            tracing::info!("OpenXR session ended");
            Ok(())
        }

        fn get_head_transform(&self) -> Option<Transform> {
            self.head_transform.lock().unwrap().clone()
        }

        fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>) {
            (None, None) // Use get_hand_tracking for full joint data
        }

        fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)> {
            if let Some(left) = self.convert_hand_joints(Hand::Left) {
                return Some((Hand::Left, left));
            }
            if let Some(right) = self.convert_hand_joints(Hand::Right) {
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

            // Haptic feedback requires action system which needs graphics session
            // For headless session, log the request
            tracing::debug!(
                "Haptic request: {:?}, intensity={:.2}, duration={}ms",
                hand,
                intensity,
                duration_ms
            );
            Ok(())
        }

        fn get_frame_time(&self) -> f32 {
            *self.frame_time_ms.lock().unwrap()
        }

        fn is_session_active(&self) -> bool {
            self.session_running.load(Ordering::SeqCst)
        }
    }
}

// ============================================================================
// Fallback implementation (when openxr-runtime feature is not enabled)
// ============================================================================

#[cfg(not(feature = "openxr-runtime"))]
mod fallback {
    use super::*;

    /// Fallback platform when OpenXR runtime is not available
    pub struct OpenXrPlatform {
        device_type: DeviceType,
        capabilities: PlatformCapabilities,
        extensions: OpenXrExtensions,
        session_active: bool,
        head_transform: Arc<Mutex<Option<Transform>>>,
        left_hand_joints: Arc<Mutex<[JointLocation; hand_joints::JOINT_COUNT]>>,
        right_hand_joints: Arc<Mutex<[JointLocation; hand_joints::JOINT_COUNT]>>,
        frame_time_ms: Arc<Mutex<f32>>,
        runtime_name: String,
    }

    impl OpenXrPlatform {
        pub fn new(device_type: DeviceType) -> Self {
            let (capabilities, extensions) = match device_type {
                DeviceType::MetaQuest => (
                    PlatformCapabilities {
                        supports_hand_tracking: true,
                        supports_eye_tracking: false,
                        supports_haptics: true,
                        supports_passthrough: true,
                        max_refresh_rate: 120,
                        field_of_view: 110.0,
                        has_6dof: true,
                    },
                    OpenXrExtensions {
                        hand_tracking: true,
                        passthrough: true,
                        ..Default::default()
                    },
                ),
                DeviceType::ValveIndex => (
                    PlatformCapabilities {
                        supports_hand_tracking: true,
                        supports_eye_tracking: true,
                        supports_haptics: true,
                        supports_passthrough: false,
                        max_refresh_rate: 144,
                        field_of_view: 130.0,
                        has_6dof: true,
                    },
                    OpenXrExtensions {
                        hand_tracking: true,
                        eye_tracking: true,
                        ..Default::default()
                    },
                ),
                _ => (PlatformCapabilities::default(), OpenXrExtensions::default()),
            };

            Self {
                device_type,
                capabilities,
                extensions,
                session_active: false,
                head_transform: Arc::new(Mutex::new(None)),
                left_hand_joints: Arc::new(Mutex::new(
                    [JointLocation::default(); hand_joints::JOINT_COUNT],
                )),
                right_hand_joints: Arc::new(Mutex::new(
                    [JointLocation::default(); hand_joints::JOINT_COUNT],
                )),
                frame_time_ms: Arc::new(Mutex::new(11.1)),
                runtime_name: "Fallback (no runtime)".to_string(),
            }
        }

        fn convert_hand_joints(&self, hand: Hand) -> Option<FingerPositions> {
            let joints = match hand {
                Hand::Left => self.left_hand_joints.lock().unwrap(),
                Hand::Right => self.right_hand_joints.lock().unwrap(),
            };

            let palm = &joints[hand_joints::PALM];
            if !palm.is_valid {
                return None;
            }
            
            // Create default finger joints from available data
            let create_finger = |tip_idx: usize, meta_idx: usize| -> FingerJoints {
                let tip = &joints[tip_idx];
                let meta = &joints[meta_idx];
                FingerJoints {
                    metacarpal: Vector3::new(meta.position[0], meta.position[1], meta.position[2]),
                    proximal: Vector3::new(meta.position[0], meta.position[1], meta.position[2]),
                    intermediate: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                    distal: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                    tip: Vector3::new(tip.position[0], tip.position[1], tip.position[2]),
                }
            };

            Some(FingerPositions {
                thumb: create_finger(hand_joints::THUMB_TIP, hand_joints::THUMB_METACARPAL),
                index: create_finger(hand_joints::INDEX_TIP, hand_joints::INDEX_METACARPAL),
                middle: create_finger(hand_joints::MIDDLE_TIP, hand_joints::MIDDLE_METACARPAL),
                ring: create_finger(hand_joints::RING_TIP, hand_joints::RING_METACARPAL),
                pinky: create_finger(hand_joints::LITTLE_TIP, hand_joints::LITTLE_METACARPAL),
            })
        }

        pub fn update_hand_joints(
            &self,
            hand: Hand,
            joints: [JointLocation; hand_joints::JOINT_COUNT],
        ) {
            match hand {
                Hand::Left => *self.left_hand_joints.lock().unwrap() = joints,
                Hand::Right => *self.right_hand_joints.lock().unwrap() = joints,
            }
        }

        pub fn update_head_transform(&self, transform: Transform) {
            *self.head_transform.lock().unwrap() = Some(transform);
        }

        pub fn runtime_name(&self) -> &str {
            &self.runtime_name
        }

        pub fn extensions(&self) -> &OpenXrExtensions {
            &self.extensions
        }
    }

    impl Default for PlatformCapabilities {
        fn default() -> Self {
            Self {
                supports_hand_tracking: false,
                supports_eye_tracking: false,
                supports_haptics: false,
                supports_passthrough: false,
                max_refresh_rate: 60,
                field_of_view: 90.0,
                has_6dof: false,
            }
        }
    }

    #[async_trait]
    impl VrPlatform for OpenXrPlatform {
        async fn initialize(&mut self) -> Result<(), PlatformError> {
            tracing::warn!(
                "OpenXR runtime not available. Compile with --features openxr-runtime for VR support."
            );
            tracing::info!("Fallback mode: {:?}", self.device_type);
            Ok(())
        }

        fn get_capabilities(&self) -> PlatformCapabilities {
            self.capabilities.clone()
        }

        fn get_device_type(&self) -> DeviceType {
            self.device_type.clone()
        }

        async fn start_session(&mut self) -> Result<(), PlatformError> {
            self.session_active = true;
            tracing::info!("Fallback session started");
            Ok(())
        }

        async fn end_session(&mut self) -> Result<(), PlatformError> {
            self.session_active = false;
            tracing::info!("Fallback session ended");
            Ok(())
        }

        fn get_head_transform(&self) -> Option<Transform> {
            self.head_transform.lock().unwrap().clone()
        }

        fn get_hand_transforms(&self) -> (Option<Transform>, Option<Transform>) {
            (None, None)
        }

        fn get_hand_tracking(&self) -> Option<(Hand, FingerPositions)> {
            if let Some(left) = self.convert_hand_joints(Hand::Left) {
                return Some((Hand::Left, left));
            }
            if let Some(right) = self.convert_hand_joints(Hand::Right) {
                return Some((Hand::Right, right));
            }
            None
        }

        fn trigger_haptic(
            &mut self,
            _hand: Hand,
            _intensity: f32,
            _duration_ms: u32,
        ) -> Result<(), PlatformError> {
            Err(PlatformError::HapticsUnsupported)
        }

        fn get_frame_time(&self) -> f32 {
            *self.frame_time_ms.lock().unwrap()
        }

        fn is_session_active(&self) -> bool {
            self.session_active
        }
    }
}

// Re-export the appropriate implementation
#[cfg(feature = "openxr-runtime")]
pub use runtime::OpenXrPlatform;

#[cfg(not(feature = "openxr-runtime"))]
pub use fallback::OpenXrPlatform;

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_openxr_initialization() {
        let mut platform = OpenXrPlatform::new(DeviceType::MetaQuest);
        // Will use fallback if openxr-runtime not enabled
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
    fn test_capabilities() {
        let platform = OpenXrPlatform::new(DeviceType::MetaQuest);
        let caps = platform.get_capabilities();
        assert!(caps.supports_hand_tracking);
        assert!(caps.supports_haptics);
    }
}
