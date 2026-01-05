//! Motion Sickness Mitigation and Comfort Features
//!
//! Implements comfort features to reduce VR sickness:
//! - Comfort vignette (peripheral vision darkening)
//! - Snap turning (instant rotation instead of smooth)
//! - Teleportation system
//! - Field of view reduction during fast movement

use crate::Vector3;
use serde::{Deserialize, Serialize};

/// Comfort settings for reducing motion sickness
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ComfortSettings {
    /// Enable comfort vignette during movement
    pub vignette_enabled: bool,
    /// Vignette strength (0.0 = none, 1.0 = maximum darkening)
    pub vignette_strength: f32,
    /// Enable snap turning
    pub snap_turn_enabled: bool,
    /// Snap turn angle in degrees (15, 30, 45, or 90)
    pub snap_turn_degrees: f32,
    /// Enable teleportation
    pub teleport_enabled: bool,
    /// Maximum teleport distance in meters
    pub teleport_max_distance: f32,
    /// Enable FOV reduction during fast movement
    pub fov_reduction_enabled: bool,
    /// Movement speed threshold for triggering comfort features (m/s)
    pub movement_threshold: f32,
}

impl Default for ComfortSettings {
    fn default() -> Self {
        Self {
            vignette_enabled: true,
            vignette_strength: 0.7,
            snap_turn_enabled: true,
            snap_turn_degrees: 30.0,
            teleport_enabled: true,
            teleport_max_distance: 10.0,
            fov_reduction_enabled: true,
            movement_threshold: 2.0, // 2 m/s
        }
    }
}

/// Comfort mode presets
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub enum ComfortMode {
    /// Full comfort features (recommended for beginners)
    Maximum,
    /// Moderate comfort features
    Moderate,
    /// Minimal comfort features (for experienced VR users)
    Minimal,
    /// No comfort features (VR veterans only)
    None,
}

impl ComfortMode {
    pub fn to_settings(&self) -> ComfortSettings {
        match self {
            Self::Maximum => ComfortSettings {
                vignette_enabled: true,
                vignette_strength: 0.9,
                snap_turn_enabled: true,
                snap_turn_degrees: 45.0,
                teleport_enabled: true,
                teleport_max_distance: 10.0,
                fov_reduction_enabled: true,
                movement_threshold: 1.5,
            },
            Self::Moderate => ComfortSettings::default(),
            Self::Minimal => ComfortSettings {
                vignette_enabled: true,
                vignette_strength: 0.3,
                snap_turn_enabled: true,
                snap_turn_degrees: 15.0,
                teleport_enabled: false,
                teleport_max_distance: 5.0,
                fov_reduction_enabled: false,
                movement_threshold: 3.0,
            },
            Self::None => ComfortSettings {
                vignette_enabled: false,
                vignette_strength: 0.0,
                snap_turn_enabled: false,
                snap_turn_degrees: 0.0,
                teleport_enabled: false,
                teleport_max_distance: 0.0,
                fov_reduction_enabled: false,
                movement_threshold: 999.0,
            },
        }
    }
}

/// Comfort system manager
pub struct ComfortSystem {
    settings: ComfortSettings,
    current_vignette: f32,
    current_fov_scale: f32,
    last_position: Vector3,
    last_update_time: f32,
}

impl ComfortSystem {
    pub fn new(settings: ComfortSettings) -> Self {
        Self {
            settings,
            current_vignette: 0.0,
            current_fov_scale: 1.0,
            last_position: Vector3::zero(),
            last_update_time: 0.0,
        }
    }

    /// Update comfort features based on movement
    pub fn update(&mut self, position: Vector3, delta_time: f32) {
        // Calculate movement speed
        let distance = position.distance(&self.last_position);
        let speed = if delta_time > 0.0 {
            distance / delta_time
        } else {
            0.0
        };

        // Update vignette based on speed
        if self.settings.vignette_enabled && speed > self.settings.movement_threshold {
            let excess_speed = speed - self.settings.movement_threshold;
            let target_vignette = (excess_speed / 5.0).min(1.0) * self.settings.vignette_strength;

            // Smooth transition
            self.current_vignette =
                self.current_vignette + (target_vignette - self.current_vignette) * 0.1;
        } else {
            // Fade out vignette
            self.current_vignette *= 0.9;
        }

        // Update FOV reduction
        if self.settings.fov_reduction_enabled && speed > self.settings.movement_threshold * 1.5 {
            // Reduce FOV by up to 30%
            let excess_speed = speed - (self.settings.movement_threshold * 1.5);
            let target_fov_scale = 1.0 - (excess_speed / 10.0).min(0.3);
            self.current_fov_scale =
                self.current_fov_scale + (target_fov_scale - self.current_fov_scale) * 0.1;
        } else {
            // Restore FOV
            self.current_fov_scale = self.current_fov_scale + (1.0 - self.current_fov_scale) * 0.1;
        }

        self.last_position = position;
        self.last_update_time += delta_time;
    }

    /// Get current vignette amount (0.0 = no vignette, 1.0 = maximum darkening)
    pub fn get_vignette(&self) -> f32 {
        self.current_vignette
    }

    /// Get current FOV scale (1.0 = normal, 0.7 = 30% reduction)
    pub fn get_fov_scale(&self) -> f32 {
        self.current_fov_scale
    }

    /// Perform snap turn
    pub fn snap_turn(&self, current_rotation: f32, direction: i32) -> f32 {
        if !self.settings.snap_turn_enabled {
            return current_rotation;
        }

        let angle_radians = self.settings.snap_turn_degrees.to_radians();
        current_rotation + (angle_radians * direction as f32)
    }

    /// Validate teleport destination
    pub fn can_teleport(&self, distance: f32) -> bool {
        self.settings.teleport_enabled && distance <= self.settings.teleport_max_distance
    }

    /// Update settings
    pub fn set_settings(&mut self, settings: ComfortSettings) {
        self.settings = settings;
    }

    /// Get current settings
    pub fn get_settings(&self) -> &ComfortSettings {
        &self.settings
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_comfort_modes() {
        let maximum = ComfortMode::Maximum.to_settings();
        assert!(maximum.vignette_enabled);
        assert_eq!(maximum.snap_turn_degrees, 45.0);

        let none = ComfortMode::None.to_settings();
        assert!(!none.vignette_enabled);
        assert!(!none.snap_turn_enabled);
    }

    #[test]
    fn test_vignette_activation() {
        let mut system = ComfortSystem::new(ComfortSettings::default());

        // Slow movement - no vignette
        system.update(Vector3::new(0.1, 0.0, 0.0), 0.1);
        assert!(system.get_vignette() < 0.1);

        // Fast movement - activate vignette (need to move far enough to exceed threshold)
        // Default threshold is 2.0 m/s, so move 0.3m per 0.1s = 3.0 m/s
        let mut pos = Vector3::new(0.0, 0.0, 0.0);
        for i in 0..20 {
            pos.x += 0.3; // Move 3 m/s
            system.update(pos, 0.1);
        }
        assert!(system.get_vignette() > 0.1);
    }

    #[test]
    fn test_snap_turn() {
        let system = ComfortSystem::new(ComfortSettings::default());

        let current = 0.0;
        let turned_right = system.snap_turn(current, 1);
        assert!((turned_right - 30.0f32.to_radians()).abs() < 0.01);

        let turned_left = system.snap_turn(current, -1);
        assert!((turned_left + 30.0f32.to_radians()).abs() < 0.01);
    }

    #[test]
    fn test_teleport_validation() {
        let system = ComfortSystem::new(ComfortSettings::default());

        assert!(system.can_teleport(5.0)); // Within max distance
        assert!(!system.can_teleport(15.0)); // Exceeds max distance
    }

    #[test]
    fn test_fov_reduction() {
        let mut system = ComfortSystem::new(ComfortSettings::default());

        // Very fast movement
        for _ in 0..20 {
            system.update(Vector3::new(10.0, 0.0, 0.0), 0.1);
        }

        assert!(system.get_fov_scale() < 1.0);
    }
}
