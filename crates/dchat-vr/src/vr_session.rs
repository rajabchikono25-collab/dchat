//! VR/AR Session Management
//!
//! Manages active VR/AR sessions for users

use crate::{DeviceType, Transform, Vector3};
use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Active VR/AR session
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VrSession {
    pub session_id: Uuid,
    pub user_id: UserId,
    pub device_type: DeviceType,
    pub environment_id: Option<Uuid>,
    pub started_at: DateTime<Utc>,
    pub last_activity: DateTime<Utc>,
    pub head_transform: Transform,
    pub left_hand_transform: Option<Transform>,
    pub right_hand_transform: Option<Transform>,
    pub connection_quality: ConnectionQuality,
}

/// Connection quality metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConnectionQuality {
    pub latency_ms: f32,
    pub packet_loss_percent: f32,
    pub frame_rate: f32,
    pub bandwidth_mbps: f32,
}

/// Session event
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SessionEvent {
    UserJoined { user_id: UserId, environment_id: Uuid },
    UserLeft { user_id: UserId },
    UserMoved { user_id: UserId, new_position: Vector3 },
    UserGesture { user_id: UserId, gesture: String },
    UserSpoke { user_id: UserId },
    EnvironmentChanged { old_env: Uuid, new_env: Uuid },
}

/// Session manager
pub struct VrSessionManager {
    sessions: HashMap<UserId, VrSession>,
    events: Vec<(DateTime<Utc>, SessionEvent)>,
}

impl VrSessionManager {
    pub fn new() -> Self {
        Self {
            sessions: HashMap::new(),
            events: Vec::new(),
        }
    }

    /// Start a new VR session
    pub fn start_session(
        &mut self,
        user_id: UserId,
        device_type: DeviceType,
        environment_id: Option<Uuid>,
    ) -> Result<Uuid> {
        // Check if user already has active session
        if self.sessions.contains_key(&user_id) {
            return Err(dchat_core::Error::validation(
                "User already has an active VR session",
            ));
        }

        let session = VrSession {
            session_id: Uuid::new_v4(),
            user_id: user_id.clone(),
            device_type,
            environment_id,
            started_at: Utc::now(),
            last_activity: Utc::now(),
            head_transform: Transform::identity(),
            left_hand_transform: None,
            right_hand_transform: None,
            connection_quality: ConnectionQuality {
                latency_ms: 0.0,
                packet_loss_percent: 0.0,
                frame_rate: 90.0,
                bandwidth_mbps: 10.0,
            },
        };

        let session_id = session.session_id;
        self.sessions.insert(user_id.clone(), session);

        // Record event
        if let Some(env_id) = environment_id {
            self.record_event(SessionEvent::UserJoined {
                user_id,
                environment_id: env_id,
            });
        }

        Ok(session_id)
    }

    /// End a VR session
    pub fn end_session(&mut self, user_id: &UserId) -> Result<()> {
        self.sessions
            .remove(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Session not found"))?;

        self.record_event(SessionEvent::UserLeft {
            user_id: user_id.clone(),
        });

        Ok(())
    }

    /// Update head tracking
    pub fn update_head_transform(&mut self, user_id: &UserId, transform: Transform) -> Result<()> {
        let session = self
            .sessions
            .get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Session not found"))?;

        let position_changed = session.head_transform.position.distance(&transform.position) > 0.1;

        session.head_transform = transform;
        session.last_activity = Utc::now();

        if position_changed {
            self.record_event(SessionEvent::UserMoved {
                user_id: user_id.clone(),
                new_position: transform.position,
            });
        }

        Ok(())
    }

    /// Update hand tracking
    pub fn update_hand_transform(
        &mut self,
        user_id: &UserId,
        hand: crate::gesture::Hand,
        transform: Transform,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Session not found"))?;

        match hand {
            crate::gesture::Hand::Left => session.left_hand_transform = Some(transform),
            crate::gesture::Hand::Right => session.right_hand_transform = Some(transform),
        }

        session.last_activity = Utc::now();

        Ok(())
    }

    /// Update connection quality
    pub fn update_connection_quality(
        &mut self,
        user_id: &UserId,
        quality: ConnectionQuality,
    ) -> Result<()> {
        let session = self
            .sessions
            .get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Session not found"))?;

        session.connection_quality = quality;
        session.last_activity = Utc::now();

        Ok(())
    }

    /// Change environment
    pub fn change_environment(&mut self, user_id: &UserId, new_environment_id: Uuid) -> Result<()> {
        let session = self
            .sessions
            .get_mut(user_id)
            .ok_or_else(|| dchat_core::Error::validation("Session not found"))?;

        let old_env = session.environment_id;
        session.environment_id = Some(new_environment_id);
        session.last_activity = Utc::now();

        if let Some(old_id) = old_env {
            self.record_event(SessionEvent::EnvironmentChanged {
                old_env: old_id,
                new_env: new_environment_id,
            });
        }

        Ok(())
    }

    /// Get session
    pub fn get_session(&self, user_id: &UserId) -> Option<&VrSession> {
        self.sessions.get(user_id)
    }

    /// Get all sessions in environment
    pub fn get_sessions_in_environment(&self, environment_id: Uuid) -> Vec<&VrSession> {
        self.sessions
            .values()
            .filter(|s| s.environment_id == Some(environment_id))
            .collect()
    }

    /// Get all active sessions
    pub fn get_active_sessions(&self) -> Vec<&VrSession> {
        self.sessions.values().collect()
    }

    /// Record session event
    fn record_event(&mut self, event: SessionEvent) {
        self.events.push((Utc::now(), event));

        // Keep last 1000 events
        if self.events.len() > 1000 {
            self.events.drain(0..self.events.len() - 1000);
        }
    }

    /// Get recent events
    pub fn get_recent_events(&self, count: usize) -> Vec<&(DateTime<Utc>, SessionEvent)> {
        self.events.iter().rev().take(count).collect()
    }

    /// Clean up inactive sessions
    pub fn cleanup_inactive(&mut self, timeout_minutes: i64) {
        let cutoff = Utc::now() - chrono::Duration::minutes(timeout_minutes);
        let inactive: Vec<UserId> = self
            .sessions
            .iter()
            .filter(|(_, s)| s.last_activity < cutoff)
            .map(|(id, _)| id.clone())
            .collect();

        for user_id in inactive {
            let _ = self.end_session(&user_id);
        }
    }
}

impl Default for VrSessionManager {
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
    fn test_start_session() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();

        let session_id = manager
            .start_session(user_id.clone(), DeviceType::MetaQuest, None)
            .unwrap();

        assert!(manager.get_session(&user_id).is_some());
    }

    #[test]
    fn test_end_session() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();

        manager
            .start_session(user_id.clone(), DeviceType::MetaQuest, None)
            .unwrap();

        manager.end_session(&user_id).unwrap();
        assert!(manager.get_session(&user_id).is_none());
    }

    #[test]
    fn test_duplicate_session() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();

        manager
            .start_session(user_id.clone(), DeviceType::MetaQuest, None)
            .unwrap();

        let result = manager.start_session(user_id, DeviceType::PSVR2, None);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_head_transform() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();

        manager
            .start_session(user_id.clone(), DeviceType::ValveIndex, None)
            .unwrap();

        let mut transform = Transform::identity();
        transform.position = Vector3::new(1.0, 1.5, 0.0);

        manager.update_head_transform(&user_id, transform).unwrap();

        let session = manager.get_session(&user_id).unwrap();
        assert_eq!(session.head_transform.position.x, 1.0);
    }

    #[test]
    fn test_change_environment() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();
        let env1 = Uuid::new_v4();
        let env2 = Uuid::new_v4();

        manager
            .start_session(user_id.clone(), DeviceType::VisionPro, Some(env1))
            .unwrap();

        manager.change_environment(&user_id, env2).unwrap();

        let session = manager.get_session(&user_id).unwrap();
        assert_eq!(session.environment_id, Some(env2));
    }

    #[test]
    fn test_get_sessions_in_environment() {
        let mut manager = VrSessionManager::new();
        let user1 = create_test_user();
        let user2 = create_test_user();
        let env_id = Uuid::new_v4();

        manager
            .start_session(user1, DeviceType::MetaQuest, Some(env_id))
            .unwrap();
        manager
            .start_session(user2, DeviceType::PSVR2, Some(env_id))
            .unwrap();

        let sessions = manager.get_sessions_in_environment(env_id);
        assert_eq!(sessions.len(), 2);
    }

    #[test]
    fn test_cleanup_inactive() {
        let mut manager = VrSessionManager::new();
        let user_id = create_test_user();

        manager
            .start_session(user_id.clone(), DeviceType::HTCVive, None)
            .unwrap();

        // Set last activity to 2 hours ago
        if let Some(session) = manager.sessions.get_mut(&user_id) {
            session.last_activity = Utc::now() - chrono::Duration::hours(2);
        }

        manager.cleanup_inactive(60); // 60 minute timeout

        assert!(manager.get_session(&user_id).is_none());
    }
}
