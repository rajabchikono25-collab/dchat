//! Spatial Audio System
//!
//! Provides positional 3D audio for immersive voice chat

use crate::{Transform, Vector3};
use dchat_core::{types::UserId, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Audio source in 3D space
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioSource {
    pub user_id: UserId,
    pub position: Vector3,
    pub volume: f32,
    pub is_speaking: bool,
    pub attenuation: AttenuationModel,
}

/// Audio attenuation model
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttenuationModel {
    /// Linear distance attenuation
    Linear { max_distance: f32 },
    /// Inverse distance (realistic)
    Inverse { reference_distance: f32, max_distance: f32 },
    /// Exponential falloff
    Exponential { reference_distance: f32, rolloff_factor: f32 },
}

/// Audio listener (user's position)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioListener {
    pub position: Vector3,
    pub forward: Vector3,
    pub up: Vector3,
}

/// Spatial audio parameters calculated for a source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialAudioParams {
    pub volume: f32,
    pub pan: f32, // -1.0 (left) to 1.0 (right)
    pub distance: f32,
    pub should_play: bool,
}

/// Spatial audio manager
pub struct SpatialAudioManager {
    sources: HashMap<UserId, AudioSource>,
    listener: AudioListener,
}

impl SpatialAudioManager {
    pub fn new() -> Self {
        Self {
            sources: HashMap::new(),
            listener: AudioListener {
                position: Vector3::zero(),
                forward: Vector3::new(0.0, 0.0, -1.0),
                up: Vector3::new(0.0, 1.0, 0.0),
            },
        }
    }

    /// Update listener position and orientation
    pub fn update_listener(&mut self, position: Vector3, forward: Vector3, up: Vector3) {
        self.listener.position = position;
        self.listener.forward = forward;
        self.listener.up = up;
    }

    /// Add or update audio source
    pub fn update_source(
        &mut self,
        user_id: UserId,
        position: Vector3,
        volume: f32,
        is_speaking: bool,
    ) {
        let attenuation = AttenuationModel::Inverse {
            reference_distance: 1.0,
            max_distance: 50.0,
        };

        self.sources.insert(
            user_id,
            AudioSource {
                user_id,
                position,
                volume,
                is_speaking,
                attenuation,
            },
        );
    }

    /// Remove audio source
    pub fn remove_source(&mut self, user_id: &UserId) {
        self.sources.remove(user_id);
    }

    /// Calculate spatial audio parameters for a source
    pub fn calculate_spatial_params(&self, user_id: &UserId) -> Option<SpatialAudioParams> {
        let source = self.sources.get(user_id)?;
        let distance = self.listener.position.distance(&source.position);

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

        // Calculate stereo pan based on angle
        let to_source = Vector3 {
            x: source.position.x - self.listener.position.x,
            y: source.position.y - self.listener.position.y,
            z: source.position.z - self.listener.position.z,
        };

        // Simple pan calculation (could be more sophisticated)
        let pan = (to_source.x / (distance + 0.001)).clamp(-1.0, 1.0);

        let final_volume = source.volume * attenuation_factor;

        Some(SpatialAudioParams {
            volume: final_volume,
            pan,
            distance,
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
            1.0,
            true,
        );

        assert!(manager.sources.contains_key(&user_id));
    }

    #[test]
    fn test_spatial_params_distance() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_listener(Vector3::zero(), Vector3::new(0.0, 0.0, -1.0), Vector3::new(0.0, 1.0, 0.0));
        manager.update_source(user_id.clone(), Vector3::new(10.0, 0.0, 0.0), 1.0, true);

        let params = manager.calculate_spatial_params(&user_id).unwrap();
        assert_eq!(params.distance, 10.0);
        assert!(params.volume < 1.0); // Should be attenuated
    }

    #[test]
    fn test_remove_source() {
        let mut manager = SpatialAudioManager::new();
        let user_id = create_test_user();

        manager.update_source(user_id.clone(), Vector3::zero(), 1.0, true);
        manager.remove_source(&user_id);

        assert!(!manager.sources.contains_key(&user_id));
    }

    #[test]
    fn test_get_speaking_sources() {
        let mut manager = SpatialAudioManager::new();
        let user1 = create_test_user();
        let user2 = create_test_user();

        manager.update_source(user1.clone(), Vector3::new(1.0, 0.0, 0.0), 1.0, true);
        manager.update_source(user2.clone(), Vector3::new(2.0, 0.0, 0.0), 1.0, false);

        let speaking = manager.get_speaking_sources();
        assert_eq!(speaking.len(), 1);
    }
}
