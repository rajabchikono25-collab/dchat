//! Immersive VR/AR Environments
//!
//! Provides virtual spaces for channels and meetings

use crate::{Transform, Vector3};
use chrono::{DateTime, Utc};
use dchat_core::Result;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Virtual environment/room
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VirtualEnvironment {
    pub id: Uuid,
    pub name: String,
    pub environment_type: EnvironmentType,
    pub skybox: String,
    pub lighting: LightingConfig,
    pub spawn_points: Vec<Transform>,
    pub interactive_objects: Vec<InteractiveObject>,
    pub spatial_zones: Vec<SpatialZone>,
    pub created_at: DateTime<Utc>,
}

/// Types of VR environments
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum EnvironmentType {
    /// Open space (conference hall)
    OpenSpace,
    /// Meeting room
    MeetingRoom { capacity: u32 },
    /// Theater/auditorium
    Theater { rows: u32, seats_per_row: u32 },
    /// Outdoor scene
    Outdoor { terrain: String },
    /// Custom environment
    Custom { scene_data: String },
}

/// Lighting configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LightingConfig {
    pub ambient_color: Color,
    pub directional_lights: Vec<DirectionalLight>,
    pub point_lights: Vec<PointLight>,
}

/// RGB color
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
}

/// Directional light (like sun)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DirectionalLight {
    pub direction: Vector3,
    pub color: Color,
    pub intensity: f32,
}

/// Point light source
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PointLight {
    pub position: Vector3,
    pub color: Color,
    pub intensity: f32,
    pub range: f32,
}

/// Interactive object in environment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InteractiveObject {
    pub id: Uuid,
    pub object_type: ObjectType,
    pub transform: Transform,
    pub interaction_type: InteractionType,
}

/// Object types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ObjectType {
    /// Whiteboard for drawing
    Whiteboard,
    /// Screen for sharing
    Screen,
    /// 3D model
    Model { model_id: String },
    /// Portal to another environment
    Portal { target_env_id: Uuid },
    /// Teleport pad
    TeleportPad { destination: Vector3 },
}

/// Interaction type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum InteractionType {
    Click,
    Grab,
    Touch,
    Gaze,
}

/// Spatial zone with special properties
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SpatialZone {
    pub id: Uuid,
    pub name: String,
    pub center: Vector3,
    pub radius: f32,
    pub zone_type: ZoneType,
}

/// Zone types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ZoneType {
    /// Only certain users can enter
    Restricted { allowed_users: Vec<String> },
    /// Audio is amplified
    Stage,
    /// Quiet zone (muted audio)
    QuietZone,
    /// Trigger custom events
    EventTrigger { event_name: String },
}

/// Environment manager
pub struct EnvironmentManager {
    environments: std::collections::HashMap<Uuid, VirtualEnvironment>,
}

impl EnvironmentManager {
    pub fn new() -> Self {
        Self {
            environments: std::collections::HashMap::new(),
        }
    }

    /// Create a new environment
    pub fn create_environment(
        &mut self,
        name: String,
        environment_type: EnvironmentType,
        skybox: String,
    ) -> Result<Uuid> {
        let env = VirtualEnvironment {
            id: Uuid::new_v4(),
            name,
            environment_type,
            skybox,
            lighting: LightingConfig {
                ambient_color: Color {
                    r: 0.2,
                    g: 0.2,
                    b: 0.2,
                },
                directional_lights: vec![DirectionalLight {
                    direction: Vector3::new(0.5, -1.0, 0.5),
                    color: Color {
                        r: 1.0,
                        g: 1.0,
                        b: 0.9,
                    },
                    intensity: 1.0,
                }],
                point_lights: Vec::new(),
            },
            spawn_points: vec![Transform::identity()],
            interactive_objects: Vec::new(),
            spatial_zones: Vec::new(),
            created_at: Utc::now(),
        };

        let env_id = env.id;
        self.environments.insert(env_id, env);

        Ok(env_id)
    }

    /// Add interactive object
    pub fn add_object(
        &mut self,
        env_id: Uuid,
        object_type: ObjectType,
        transform: Transform,
        interaction_type: InteractionType,
    ) -> Result<Uuid> {
        let env = self
            .environments
            .get_mut(&env_id)
            .ok_or_else(|| dchat_core::Error::validation("Environment not found"))?;

        let obj = InteractiveObject {
            id: Uuid::new_v4(),
            object_type,
            transform,
            interaction_type,
        };

        let obj_id = obj.id;
        env.interactive_objects.push(obj);

        Ok(obj_id)
    }

    /// Add spatial zone
    pub fn add_zone(
        &mut self,
        env_id: Uuid,
        name: String,
        center: Vector3,
        radius: f32,
        zone_type: ZoneType,
    ) -> Result<Uuid> {
        let env = self
            .environments
            .get_mut(&env_id)
            .ok_or_else(|| dchat_core::Error::validation("Environment not found"))?;

        let zone = SpatialZone {
            id: Uuid::new_v4(),
            name,
            center,
            radius,
            zone_type,
        };

        let zone_id = zone.id;
        env.spatial_zones.push(zone);

        Ok(zone_id)
    }

    /// Get environment
    pub fn get_environment(&self, env_id: Uuid) -> Option<&VirtualEnvironment> {
        self.environments.get(&env_id)
    }

    /// Check if position is in a zone
    pub fn get_zone_at_position(&self, env_id: Uuid, position: Vector3) -> Option<&SpatialZone> {
        let env = self.environments.get(&env_id)?;

        env.spatial_zones
            .iter()
            .find(|zone| zone.center.distance(&position) <= zone.radius)
    }
}

impl Default for EnvironmentManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_environment() {
        let mut manager = EnvironmentManager::new();

        let env_id = manager
            .create_environment(
                "Test Room".to_string(),
                EnvironmentType::MeetingRoom { capacity: 10 },
                "sky_001".to_string(),
            )
            .unwrap();

        assert!(manager.get_environment(env_id).is_some());
    }

    #[test]
    fn test_add_object() {
        let mut manager = EnvironmentManager::new();
        let env_id = manager
            .create_environment(
                "Test".to_string(),
                EnvironmentType::OpenSpace,
                "sky".to_string(),
            )
            .unwrap();

        let obj_id = manager
            .add_object(
                env_id,
                ObjectType::Whiteboard,
                Transform::identity(),
                InteractionType::Touch,
            )
            .unwrap();

        let env = manager.get_environment(env_id).unwrap();
        assert_eq!(env.interactive_objects.len(), 1);
    }

    #[test]
    fn test_add_zone() {
        let mut manager = EnvironmentManager::new();
        let env_id = manager
            .create_environment(
                "Test".to_string(),
                EnvironmentType::OpenSpace,
                "sky".to_string(),
            )
            .unwrap();

        manager
            .add_zone(
                env_id,
                "Stage".to_string(),
                Vector3::zero(),
                5.0,
                ZoneType::Stage,
            )
            .unwrap();

        let env = manager.get_environment(env_id).unwrap();
        assert_eq!(env.spatial_zones.len(), 1);
    }

    #[test]
    fn test_get_zone_at_position() {
        let mut manager = EnvironmentManager::new();
        let env_id = manager
            .create_environment(
                "Test".to_string(),
                EnvironmentType::OpenSpace,
                "sky".to_string(),
            )
            .unwrap();

        manager
            .add_zone(
                env_id,
                "Zone1".to_string(),
                Vector3::new(10.0, 0.0, 0.0),
                5.0,
                ZoneType::Stage,
            )
            .unwrap();

        let zone = manager.get_zone_at_position(env_id, Vector3::new(12.0, 0.0, 0.0));
        assert!(zone.is_some());

        let no_zone = manager.get_zone_at_position(env_id, Vector3::new(100.0, 0.0, 0.0));
        assert!(no_zone.is_none());
    }
}
