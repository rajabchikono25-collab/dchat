//! 3D Avatar System
//!
//! Provides avatar customization, animation, and rendering

use crate::{Transform, Vector3};
use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// 3D Avatar representation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Avatar {
    pub id: Uuid,
    pub user_id: UserId,
    pub display_name: String,
    pub model_id: String,
    pub customization: AvatarCustomization,
    pub current_animation: Option<AvatarAnimation>,
    pub transform: Transform,
    pub visible: bool,
    pub created_at: DateTime<Utc>,
}

/// Avatar customization options
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarCustomization {
    pub skin_tone: String,
    pub hair_style: String,
    pub hair_color: String,
    pub eye_color: String,
    pub clothing: Vec<ClothingItem>,
    pub accessories: Vec<Accessory>,
    pub height: f32,
    pub proportions: BodyProportions,
}

/// Body proportions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BodyProportions {
    pub head_scale: f32,
    pub torso_scale: f32,
    pub arm_length: f32,
    pub leg_length: f32,
}

/// Clothing item
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClothingItem {
    pub slot: ClothingSlot,
    pub item_id: String,
    pub color: String,
}

/// Clothing slots
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ClothingSlot {
    Hat,
    Shirt,
    Pants,
    Shoes,
    Gloves,
    Jacket,
}

/// Avatar accessory
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Accessory {
    pub id: String,
    pub name: String,
    pub attachment_point: AttachmentPoint,
}

/// Attachment points for accessories
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentPoint {
    Head,
    LeftHand,
    RightHand,
    Back,
    Waist,
}

/// Avatar animation
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AvatarAnimation {
    pub animation_type: AnimationType,
    pub start_time: DateTime<Utc>,
    pub duration_ms: u32,
    pub loop_animation: bool,
}

/// Animation types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnimationType {
    Idle,
    Walking,
    Running,
    Sitting,
    Standing,
    Waving,
    Pointing,
    Clapping,
    Dancing,
    Emote(String),
}

/// Avatar manager
pub struct AvatarManager {
    avatars: std::collections::HashMap<UserId, Avatar>,
}

impl AvatarManager {
    pub fn new() -> Self {
        Self {
            avatars: std::collections::HashMap::new(),
        }
    }

    /// Create a new avatar
    pub fn create_avatar(
        &mut self,
        user_id: UserId,
        display_name: String,
        model_id: String,
        customization: AvatarCustomization,
    ) -> Result<Uuid> {
        let avatar = Avatar {
            id: Uuid::new_v4(),
            user_id: user_id.clone(),
            display_name,
            model_id,
            customization,
            current_animation: Some(AvatarAnimation {
                animation_type: AnimationType::Idle,
                start_time: Utc::now(),
                duration_ms: 0,
                loop_animation: true,
            }),
            transform: Transform::identity(),
            visible: true,
            created_at: Utc::now(),
        };

        let avatar_id = avatar.id;
        self.avatars.insert(user_id, avatar);

        Ok(avatar_id)
    }

    /// Update avatar position
    pub fn update_position(&mut self, user_id: &UserId, transform: Transform) -> Result<()> {
        let avatar = self.avatars.get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Avatar not found"))?;

        avatar.transform = transform;
        Ok(())
    }

    /// Play animation
    pub fn play_animation(
        &mut self,
        user_id: &UserId,
        animation_type: AnimationType,
        duration_ms: u32,
        loop_animation: bool,
    ) -> Result<()> {
        let avatar = self.avatars.get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Avatar not found"))?;

        avatar.current_animation = Some(AvatarAnimation {
            animation_type,
            start_time: Utc::now(),
            duration_ms,
            loop_animation,
        });

        Ok(())
    }

    /// Get avatar
    pub fn get_avatar(&self, user_id: &UserId) -> Option<&Avatar> {
        self.avatars.get(user_id)
    }

    /// Get all visible avatars in range
    pub fn get_avatars_in_range(
        &self,
        center: Vector3,
        radius: f32,
    ) -> Vec<&Avatar> {
        self.avatars.values()
            .filter(|a| a.visible)
            .filter(|a| a.transform.position.distance(&center) <= radius)
            .collect()
    }
}

impl Default for AvatarManager {
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

    fn default_customization() -> AvatarCustomization {
        AvatarCustomization {
            skin_tone: "medium".to_string(),
            hair_style: "short".to_string(),
            hair_color: "brown".to_string(),
            eye_color: "blue".to_string(),
            clothing: vec![],
            accessories: vec![],
            height: 1.75,
            proportions: BodyProportions {
                head_scale: 1.0,
                torso_scale: 1.0,
                arm_length: 1.0,
                leg_length: 1.0,
            },
        }
    }

    #[test]
    fn test_create_avatar() {
        let mut manager = AvatarManager::new();
        let user_id = create_test_user();

        let avatar_id = manager.create_avatar(
            user_id.clone(),
            "TestUser".to_string(),
            "model_default".to_string(),
            default_customization(),
        ).unwrap();

        assert!(manager.get_avatar(&user_id).is_some());
    }

    #[test]
    fn test_update_position() {
        let mut manager = AvatarManager::new();
        let user_id = create_test_user();

        manager.create_avatar(
            user_id.clone(),
            "TestUser".to_string(),
            "model_default".to_string(),
            default_customization(),
        ).unwrap();

        let mut transform = Transform::identity();
        transform.position = Vector3::new(5.0, 0.0, 10.0);

        manager.update_position(&user_id, transform).unwrap();

        let avatar = manager.get_avatar(&user_id).unwrap();
        assert_eq!(avatar.transform.position.x, 5.0);
    }

    #[test]
    fn test_play_animation() {
        let mut manager = AvatarManager::new();
        let user_id = create_test_user();

        manager.create_avatar(
            user_id.clone(),
            "TestUser".to_string(),
            "model_default".to_string(),
            default_customization(),
        ).unwrap();

        manager.play_animation(&user_id, AnimationType::Waving, 2000, false).unwrap();

        let avatar = manager.get_avatar(&user_id).unwrap();
        assert_eq!(avatar.current_animation.as_ref().unwrap().animation_type, AnimationType::Waving);
    }

    #[test]
    fn test_get_avatars_in_range() {
        let mut manager = AvatarManager::new();
        let user1 = create_test_user();
        let user2 = create_test_user();

        manager.create_avatar(user1.clone(), "User1".to_string(), "model".to_string(), default_customization()).unwrap();
        manager.create_avatar(user2.clone(), "User2".to_string(), "model".to_string(), default_customization()).unwrap();

        let mut transform = Transform::identity();
        transform.position = Vector3::new(100.0, 0.0, 0.0);
        manager.update_position(&user2, transform).unwrap();

        let nearby = manager.get_avatars_in_range(Vector3::zero(), 10.0);
        assert_eq!(nearby.len(), 1);
    }
}
