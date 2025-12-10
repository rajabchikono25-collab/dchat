//! Spatial Audio System (Production-Grade)
//!
//! Provides positional 3D audio for immersive voice chat with HRTF,
//! reverb, Doppler effect, and room acoustics simulation.

use crate::Vector3;
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::f32::consts::PI;

/// Audio source in 3D space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSource {
    pub user_id: UserId,
    pub position: Vector3,
    pub velocity: Vector3, // For Doppler effect
    pub volume: f32,
    pub is_speaking: bool,
    pub attenuation: AttenuationModel,
    pub reverb_enabled: bool,
    pub occlusion: f32, // 0.0 (no occlusion) to 1.0 (fully occluded)
}

/// Audio attenuation model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttenuationModel {
    /// Linear distance attenuation
    Linear { max_distance: f32 },
    /// Inverse distance (realistic, default for voice)
    Inverse { reference_distance: f32, max_distance: f32 },
    /// Exponential falloff
    Exponential { reference_distance: f32, rolloff_factor: f32 },
}

/// Audio listener (user's position)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioListener {
    pub position: Vector3,
    pub velocity: Vector3, // For Doppler effect
    pub forward: Vector3,
    pub up: Vector3,
}

/// Spatial audio parameters calculated for a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialAudioParams {
    pub volume: f32,
    pub pan: f32, // -1.0 (left) to 1.0 (right)
    pub distance: f32,
    pub doppler_shift: f32, // Frequency multiplier for Doppler effect
    pub reverb_mix: f32, // 0.0 to 1.0
    pub hrtf_enabled: bool,
    pub azimuth: f32, // Horizontal angle in radians (-PI to PI)
    pub elevation: f32, // Vertical angle in radians (-PI/2 to PI/2)
    pub should_play: bool,
}

/// HRTF (Head-Related Transfer Function) processor for realistic 3D audio
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HrtfProcessor {
    pub enabled: bool,
    pub head_radius: f32, // Average human head radius ~0.0875m
    pub speed_of_sound: f32, // m/s
}

impl HrtfProcessor {
    pub fn new() -> Self {
        Self {
            enabled: true,
            head_radius: 0.0875,
            speed_of_sound: 343.0,
        }
    }

    /// Calculate interaural time difference (ITD) in milliseconds
    pub fn calculate_itd(&self, azimuth: f32) -> f32 {
        // Woodworth ITD formula
        let itd_seconds = (self.head_radius / self.speed_of_sound) 
            * (azimuth.sin() + azimuth);
        itd_seconds * 1000.0 // Convert to milliseconds
    }

    /// Calculate interaural level difference (ILD) in decibels
    pub fn calculate_ild(&self, azimuth: f32, frequency: f32) -> f32 {
        // Simplified ILD based on head shadow effect
        // Higher frequencies have more pronounced ILD
        let frequency_factor = (frequency / 1000.0).min(10.0);
        let shadow_db = azimuth.abs() * 20.0 * frequency_factor;
        shadow_db.min(30.0) // Cap at 30 dB
    }
}

impl Default for HrtfProcessor {
    fn default() -> Self {
        Self::new()
    }
}

/// Room acoustics simulation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoomAcoustics {
    pub enabled: bool,
    pub room_size: Vector3, // Room dimensions in meters
    pub wall_absorption: f32, // 0.0 (reflective) to 1.0 (absorptive)
    pub reverb_time: f32, // RT60 in seconds
}

impl RoomAcoustics {
    pub fn new(room_size: Vector3, wall_absorption: f32) -> Self {
        // Calculate RT60 (reverberation time) using Sabine's formula
        let volume = room_size.x * room_size.y * room_size.z;
        let surface_area = 2.0 * (room_size.x * room_size.y + room_size.y * room_size.z + room_size.z * room_size.x);
        let absorption_area = surface_area * wall_absorption;
        let reverb_time = if absorption_area > 0.0 {
            0.161 * volume / absorption_area
        } else {
            2.0 // Default for highly reflective room
        };

        Self {
            enabled: true,
            room_size,
            wall_absorption,
            reverb_time: reverb_time.clamp(0.1, 5.0),
        }
    }

    /// Calculate early reflections based on source and listener positions
    pub fn calculate_early_reflections(&self, source: &Vector3, listener: &Vector3) -> Vec<EarlyReflection> {
        if !self.enabled {
            return Vec::new();
        }

        let mut reflections = Vec::new();

        // Calculate first-order reflections from 6 walls
        // Floor
        let floor_point = Vector3::new(source.x, 0.0, source.z);
        let path_length = source.distance(&floor_point) + floor_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0; // Convert to milliseconds
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        // Ceiling
        let ceiling_point = Vector3::new(source.x, self.room_size.y, source.z);
        let path_length = source.distance(&ceiling_point) + ceiling_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0;
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        // Left/Right walls
        let left_point = Vector3::new(0.0, source.y, source.z);
        let path_length = source.distance(&left_point) + left_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0;
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        let right_point = Vector3::new(self.room_size.x, source.y, source.z);
        let path_length = source.distance(&right_point) + right_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0;
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        // Front/Back walls
        let front_point = Vector3::new(source.x, source.y, 0.0);
        let path_length = source.distance(&front_point) + front_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0;
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        let back_point = Vector3::new(source.x, source.y, self.room_size.z);
        let path_length = source.distance(&back_point) + back_point.distance(listener);
        let delay = (path_length / 343.0) * 1000.0;
        let gain = (1.0 - self.wall_absorption) / path_length.powi(2);
        reflections.push(EarlyReflection { delay_ms: delay, gain });

        reflections
    }

    /// Calculate reverb mix based on distance
    pub fn calculate_reverb_mix(&self, distance: f32) -> f32 {
        if !self.enabled {
            return 0.0;
        }

        // Direct-to-reverberant ratio decreases with distance
        let critical_distance = 0.057 * (self.room_size.x * self.room_size.y * self.room_size.z).sqrt() 
            / self.wall_absorption.max(0.1);
        
        if distance < critical_distance {
            // More direct sound
            (distance / critical_distance) * 0.3
        } else {
            // More reverb
            0.3 + (distance - critical_distance) / (critical_distance * 2.0) * 0.4
        }.clamp(0.0, 0.7)
    }
}

impl Default for RoomAcoustics {
    fn default() -> Self {
        // Default: Medium-sized room (10m x 8m x 3m) with moderate absorption
        Self::new(
            Vector3::new(10.0, 3.0, 8.0),
            0.3,
        )
    }
}

/// Early reflection data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EarlyReflection {
    pub delay_ms: f32,
    pub gain: f32,
}

/// Spatial audio manager
pub struct SpatialAudioManager {
    sources: HashMap<UserId, AudioSource>,
    listener: AudioListener,
    hrtf_processor: HrtfProcessor,
    room_acoustics: RoomAcoustics,
}

impl SpatialAudioManager {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            listener: AudioListener {
                position: Vector3::zero(),
                velocity: Vector3::zero(),
                forward: Vector3::new(0.0, 0.0, -1.0),
                up: Vector3::new(0.0, 1.0, 0.0),
            },
            hrtf_processor: HrtfProcessor::new(),
            room_acoustics: RoomAcoustics::default(),
        }
    }

    pub fn new_with_room(room_size: Vector3, wall_absorption: f32) -> Self {
        Self {
            sources: HashMap::new(),
            listener: AudioListener {
                position: Vector3::zero(),
                velocity: Vector3::zero(),
                forward: Vector3::new(0.0, 0.0, -1.0),
                up: Vector3::new(0.0, 1.0, 0.0),
            },
            hrtf_processor: HrtfProcessor::new(),
            room_acoustics: RoomAcoustics::new(room_size, wall_absorption),
        }
    }

    /// Update listener position, orientation, and velocity
    pub fn update_listener(&mut self, position: Vector3, velocity: Vector3, forward: Vector3, up: Vector3) {
        self.listener.position = position;
        self.listener.velocity = velocity;
        self.listener.forward = forward;
        self.listener.up = up;
    }

    /// Add or update audio source
    pub fn update_source(
        &mut self,
        user_id: UserId,
        position: Vector3,
        velocity: Vector3,
        volume: f32,
        is_speaking: bool,
    ) {
        let attenuation = AttenuationModel::Inverse {
            reference_distance: 1.0,
            max_distance: 50.0,
        };

        self.sources.insert(
            user_id.clone(),
            AudioSource {
                user_id,
                position,
                velocity,
                volume,
                is_speaking,
                attenuation,
                reverb_enabled: true,
                occlusion: 0.0,
            },
        );
    }

    /// Update source occlusion (e.g., behind walls)
    pub fn set_source_occlusion(&mut self, user_id: &UserId, occlusion: f32) {
        if let Some(source) = self.sources.get_mut(user_id) {
            source.occlusion = occlusion.clamp(0.0, 1.0);
        }
    }

    /// Enable/disable HRTF processing
    pub fn set_hrtf_enabled(&mut self, enabled: bool) {
        self.hrtf_processor.enabled = enabled;
    }

    /// Update room acoustics
    pub fn set_room_acoustics(&mut self, room_size: Vector3, wall_absorption: f32) {
        self.room_acoustics = RoomAcoustics::new(room_size, wall_absorption);
    }

    /// Remove audio source
    pub fn remove_source(&mut self, user_id: &UserId) {
        self.sources.remove(user_id);
    }

    /// Calculate Doppler shift
    fn calculate_doppler_shift(&self, source_velocity: &Vector3, to_listener: &Vector3, distance: f32) -> f32 {
        const SPEED_OF_SOUND: f32 = 343.0; // m/s at 20°C

        // Calculate velocity components along the line between source and listener
        let direction = Vector3 {
            x: to_listener.x / distance,
            y: to_listener.y / distance,
            z: to_listener.z / distance,
        };

        let source_radial_velocity = 
            source_velocity.x * direction.x +
            source_velocity.y * direction.y +
            source_velocity.z * direction.z;

        let listener_radial_velocity = 
            self.listener.velocity.x * direction.x +
            self.listener.velocity.y * direction.y +
            self.listener.velocity.z * direction.z;

        // Doppler formula: f' = f * (v + v_listener) / (v + v_source)
        let doppler_shift = (SPEED_OF_SOUND + listener_radial_velocity) / 
                           (SPEED_OF_SOUND + source_radial_velocity);

        doppler_shift.clamp(0.8, 1.2) // Limit to ±20% shift
    }

    /// Calculate spatial audio parameters for a source
    pub fn calculate_spatial_params(&self, user_id: &UserId) -> Option<SpatialAudioParams> {
        let source = self.sources.get(user_id)?;
        let distance = self.listener.position.distance(&source.position);

        // Calculate direction vector from listener to source
        let to_source = Vector3 {
            x: source.position.x - self.listener.position.x,
            y: source.position.y - self.listener.position.y,
            z: source.position.z - self.listener.position.z,
        };

        // Calculate attenuation
        let attenuation_factor = match &source.attenuation {
            AttenuationModel::Linear { max_distance } => {
                if distance >= *max_distance {
                    0.0
                } else {
                    1.0 - (distance / max_distance)
                }
            }
            AttenuationModel::Inverse { reference_distance, max_distance } => {
                if distance >= *max_distance {
                    0.0
                } else {
                    reference_distance / (reference_distance + distance)
                }
            }
            AttenuationModel::Exponential { reference_distance, rolloff_factor } => {
                (reference_distance / distance).powf(*rolloff_factor)
            }
        };

        // Apply occlusion
        let occlusion_attenuation = 1.0 - (source.occlusion * 0.8);

        // Calculate azimuth (horizontal angle)
        // Transform to listener's coordinate system
        let right = Vector3 {
            x: self.listener.forward.y * self.listener.up.z - self.listener.forward.z * self.listener.up.y,
            y: self.listener.forward.z * self.listener.up.x - self.listener.forward.x * self.listener.up.z,
            z: self.listener.forward.x * self.listener.up.y - self.listener.forward.y * self.listener.up.x,
        };

        let forward_dot = to_source.x * self.listener.forward.x + 
                         to_source.y * self.listener.forward.y + 
                         to_source.z * self.listener.forward.z;
        let right_dot = to_source.x * right.x + to_source.y * right.y + to_source.z * right.z;
        let azimuth = right_dot.atan2(forward_dot);

        // Calculate elevation (vertical angle)
        let up_dot = to_source.x * self.listener.up.x + 
                    to_source.y * self.listener.up.y + 
                    to_source.z * self.listener.up.z;
        let horizontal_distance = (right_dot.powi(2) + forward_dot.powi(2)).sqrt();
        let elevation = up_dot.atan2(horizontal_distance);

        // Simple stereo pan calculation (improved with HRTF)
        let pan = if self.hrtf_processor.enabled {
            // HRTF-based pan uses azimuth
            (azimuth / PI).clamp(-1.0, 1.0)
        } else {
            // Simple pan
            (to_source.x / (distance + 0.001)).clamp(-1.0, 1.0)
        };

        // Calculate Doppler shift
        let doppler_shift = self.calculate_doppler_shift(&source.velocity, &to_source, distance);

        // Calculate reverb mix
        let reverb_mix = if source.reverb_enabled {
            self.room_acoustics.calculate_reverb_mix(distance)
        } else {
            0.0
        };

        let final_volume = source.volume * attenuation_factor * occlusion_attenuation;

        Some(SpatialAudioParams {
            volume: final_volume,
            pan,
            distance,
            doppler_shift,
            reverb_mix,
            hrtf_enabled: self.hrtf_processor.enabled,
            azimuth,
            elevation,
            should_play: source.is_speaking && final_volume > 0.01,
        })
    }

    /// Get all speaking users with their spatial parameters
    pub fn get_speaking_sources(&self) -> Vec<(UserId, SpatialAudioParams)> {
        self.sources
            .keys()
            .filter_map(|user_id| {
                self.calculate_spatial_params(user_id)
                    .map(|params| (user_id.clone(), params))
            })
            .filter(|(_, params)| params.should_play)
            .collect()
    }

    /// Get early reflections for a source
    pub fn get_early_reflections(&self, user_id: &UserId) -> Option<Vec<EarlyReflection>> {
        let source = self.sources.get(user_id)?;
        Some(self.room_acoustics.calculate_early_reflections(
            &source.position,
            &self.listener.position,
        ))
    }

    /// Get HRTF parameters for a source
    pub fn get_hrtf_params(&self, user_id: &UserId) -> Option<HrtfParams> {
        if !self.hrtf_processor.enabled {
            return None;
        }

        let params = self.calculate_spatial_params(user_id)?;
        
        Some(HrtfParams {
            itd_ms: self.hrtf_processor.calculate_itd(params.azimuth),
            ild_db: self.hrtf_processor.calculate_ild(params.azimuth, 1000.0),
            azimuth: params.azimuth,
            elevation: params.elevation,
        })
    }
}

/// HRTF parameters for audio processing
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HrtfParams {
    pub itd_ms: f32, // Interaural time difference in milliseconds
    pub ild_db: f32, // Interaural level difference in decibels
    pub azimuth: f32,
    pub elevation: f32,
}

impl Default for SpatialAudioManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    #[test]
    fn test_update_source() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_source(
            user_id.clone(),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::zero(),
            1.0,
            true,
        );

        assert!(manager.sources.contains_key(&user_id));
    }

    #[test]
    fn test_spatial_params_distance() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_listener(
            Vector3::zero(),
            Vector3::zero(),
            Vector3::new(0.0, 0.0, -1.0),
            Vector3::new(0.0, 1.0, 0.0)
        );
        manager.update_source(
            user_id.clone(),
            Vector3::new(10.0, 0.0, 0.0),
            Vector3::zero(),
            1.0,
            true
        );

        let params = manager.calculate_spatial_params(&user_id).unwrap();
        assert_eq!(params.distance, 10.0);
        assert!(params.volume < 1.0); // Should be attenuated
    }

    #[test]
    fn test_remove_source() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_source(user_id.clone(), Vector3::zero(), Vector3::zero(), 1.0, true);
        manager.remove_source(&user_id);

        assert!(!manager.sources.contains_key(&user_id));
    }

    #[test]
    fn test_get_speaking_sources() {
        let mut manager = SpatialAudioManager::new();
        let user1 = create_test_user();
        let user2 = create_test_user();

        manager.update_source(user1.clone(), Vector3::new(1.0, 0.0, 0.0), Vector3::zero(), 1.0, true);
        manager.update_source(user2.clone(), Vector3::new(2.0, 0.0, 0.0), Vector3::zero(), 1.0, false);

        let speaking = manager.get_speaking_sources();
        assert_eq!(speaking.len(), 1);
    }

    #[test]
    fn test_hrtf_itd_calculation() {
        let hrtf = HrtfProcessor::new();
        
        // Test at 90 degrees (directly to the right)
        let itd_right = hrtf.calculate_itd(PI / 2.0);
        assert!(itd_right > 0.0); // Sound reaches right ear first
        
        // Test at -90 degrees (directly to the left)
        let itd_left = hrtf.calculate_itd(-PI / 2.0);
        assert!(itd_left < 0.0); // Sound reaches left ear first
        
        // Test at 0 degrees (directly in front)
        let itd_front = hrtf.calculate_itd(0.0);
        assert!(itd_front.abs() < 0.1); // Should be near zero
    }

    #[test]
    fn test_hrtf_ild_calculation() {
        let hrtf = HrtfProcessor::new();
        
        // Test at 90 degrees with 1kHz
        let ild = hrtf.calculate_ild(PI / 2.0, 1000.0);
        assert!(ild > 0.0); // Should have positive ILD
        assert!(ild <= 30.0); // Should be capped at 30 dB (use <= to account for exactly 30.0)
    }

    #[test]
    fn test_doppler_shift() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        // Source moving towards listener at 10 m/s
        let source_velocity = Vector3::new(-10.0, 0.0, 0.0);
        manager.update_source(
            user_id.clone(),
            Vector3::new(10.0, 0.0, 0.0),
            source_velocity,
            1.0,
            true,
        );

        let params = manager.calculate_spatial_params(&user_id).unwrap();
        // Moving towards should increase frequency (doppler_shift > 1.0)
        assert!(params.doppler_shift > 1.0);
    }

    #[test]
    fn test_occlusion() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_source(
            user_id.clone(),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::zero(),
            1.0,
            true,
        );

        // No occlusion
        let params_clear = manager.calculate_spatial_params(&user_id).unwrap();
        let volume_clear = params_clear.volume;

        // Full occlusion
        manager.set_source_occlusion(&user_id, 1.0);
        let params_occluded = manager.calculate_spatial_params(&user_id).unwrap();
        let volume_occluded = params_occluded.volume;

        assert!(volume_occluded < volume_clear);
    }

    #[test]
    fn test_room_acoustics_reverb_time() {
        // Small absorptive room (bedroom)
        let small_room = RoomAcoustics::new(Vector3::new(4.0, 2.5, 5.0), 0.6);
        assert!(small_room.reverb_time < 0.5);

        // Large reflective room (concert hall)
        let large_room = RoomAcoustics::new(Vector3::new(30.0, 15.0, 40.0), 0.1);
        assert!(large_room.reverb_time > 1.0);
    }

    #[test]
    fn test_early_reflections() {
        let room = RoomAcoustics::new(Vector3::new(10.0, 3.0, 8.0), 0.3);
        let source = Vector3::new(5.0, 1.5, 4.0);
        let listener = Vector3::new(3.0, 1.5, 4.0);

        let reflections = room.calculate_early_reflections(&source, &listener);
        
        // Should have 6 reflections (one per wall)
        assert_eq!(reflections.len(), 6);
        
        // All reflections should have positive delay and gain
        for reflection in &reflections {
            assert!(reflection.delay_ms > 0.0);
            assert!(reflection.gain > 0.0);
        }
    }

    #[test]
    fn test_reverb_mix_distance() {
        let manager = SpatialAudioManager::new_with_room(
            Vector3::new(10.0, 3.0, 8.0),
            0.3,
        );

        // Close source should have less reverb
        let reverb_close = manager.room_acoustics.calculate_reverb_mix(2.0);
        
        // Far source should have more reverb
        let reverb_far = manager.room_acoustics.calculate_reverb_mix(15.0);
        
        assert!(reverb_far > reverb_close);
    }

    #[test]
    fn test_azimuth_elevation_calculation() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_listener(
            Vector3::zero(),
            Vector3::zero(),
            Vector3::new(0.0, 0.0, -1.0), // Forward: -Z
            Vector3::new(0.0, 1.0, 0.0),  // Up: +Y
        );

        // Source to the right
        manager.update_source(
            user_id.clone(),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::zero(),
            1.0,
            true,
        );

        let params = manager.calculate_spatial_params(&user_id).unwrap();
        
        // Should be roughly 90 degrees to the right
        assert!(params.azimuth > 1.4 && params.azimuth < 1.7); // ~PI/2
        
        // Should be at ear level
        assert!(params.elevation.abs() < 0.2);
    }

    #[test]
    fn test_hrtf_params() {
        let mut manager = SpatialAudioManager::new();
        manager.set_hrtf_enabled(true);
        let user_id = create_test_user();

        manager.update_source(
            user_id.clone(),
            Vector3::new(5.0, 0.0, 0.0),
            Vector3::zero(),
            1.0,
            true,
        );

        let hrtf_params = manager.get_hrtf_params(&user_id);
        assert!(hrtf_params.is_some());

        let params = hrtf_params.unwrap();
        assert!(params.itd_ms.abs() > 0.0); // Should have ITD for off-center source
        assert!(params.ild_db > 0.0); // Should have ILD
    }
}
