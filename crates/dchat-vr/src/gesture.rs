//! Gesture Recognition and Hand Tracking
//!
//! Provides gesture controls and hand tracking for VR/AR interfaces

use crate::{Transform, Vector3};
use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Result};
use serde::{Deserialize, Serialize};

/// Hand type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Hand {
    Left,
    Right,
}

/// Gesture types
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum GestureType {
    /// Pointing at something
    Point { direction: Vector3 },
    /// Thumbs up
    ThumbsUp,
    /// Thumbs down
    ThumbsDown,
    /// Wave hello
    Wave,
    /// Grab/pinch
    Grab,
    /// Release grab
    Release,
    /// Swipe in direction
    Swipe { direction: SwipeDirection },
    /// Peace sign
    PeaceSign,
    /// Fist
    Fist,
    /// Open hand
    OpenPalm,
    /// Custom gesture
    Custom { name: String },
}

/// Swipe direction
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum SwipeDirection {
    Up,
    Down,
    Left,
    Right,
}

/// Detected gesture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedGesture {
    pub user_id: UserId,
    pub hand: Hand,
    pub gesture_type: GestureType,
    pub confidence: f32,
    pub position: Vector3,
    pub detected_at: DateTime<Utc>,
}

/// Hand tracking data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HandTrackingData {
    pub user_id: UserId,
    pub hand: Hand,
    pub palm_position: Vector3,
    pub palm_normal: Vector3,
    pub finger_positions: FingerPositions,
    pub is_tracking: bool,
    pub timestamp: DateTime<Utc>,
}

/// Positions of all fingers
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerPositions {
    pub thumb: Vector3,
    pub index: Vector3,
    pub middle: Vector3,
    pub ring: Vector3,
    pub pinky: Vector3,
}

/// Gesture recognizer
pub struct GestureRecognizer {
    gesture_buffer: Vec<DetectedGesture>,
    tracking_data: Vec<HandTrackingData>,
    min_confidence: f32,
}

impl GestureRecognizer {
    pub fn new(min_confidence: f32) -> Self {
        Self {
            gesture_buffer: Vec::new(),
            tracking_data: Vec::new(),
            min_confidence,
        }
    }

    /// Update hand tracking data
    pub fn update_tracking(&mut self, tracking: HandTrackingData) {
        self.tracking_data.push(tracking);
        
        // Keep only recent tracking data (last 100 frames)
        if self.tracking_data.len() > 100 {
            self.tracking_data.drain(0..self.tracking_data.len() - 100);
        }
    }

    /// Detect gesture from hand position
    pub fn detect_gesture(
        &mut self,
        user_id: UserId,
        hand: Hand,
        hand_position: Vector3,
        finger_positions: FingerPositions,
    ) -> Option<DetectedGesture> {
        // Simple gesture detection logic (would be ML-based in production)
        
        // Check for point gesture (index finger extended)
        let index_extended = self.is_finger_extended(&hand_position, &finger_positions.index);
        let other_fingers_curled = !self.is_finger_extended(&hand_position, &finger_positions.middle)
            && !self.is_finger_extended(&hand_position, &finger_positions.ring)
            && !self.is_finger_extended(&hand_position, &finger_positions.pinky);
        
        if index_extended && other_fingers_curled {
            let direction = Vector3 {
                x: finger_positions.index.x - hand_position.x,
                y: finger_positions.index.y - hand_position.y,
                z: finger_positions.index.z - hand_position.z,
            };
            
            let gesture = DetectedGesture {
                user_id,
                hand,
                gesture_type: GestureType::Point { direction },
                confidence: 0.9,
                position: hand_position,
                detected_at: Utc::now(),
            };
            
            self.gesture_buffer.push(gesture.clone());
            return Some(gesture);
        }
        
        // Check for thumbs up
        let thumb_up = finger_positions.thumb.y > hand_position.y + 0.05;
        if thumb_up && other_fingers_curled {
            let gesture = DetectedGesture {
                user_id,
                hand,
                gesture_type: GestureType::ThumbsUp,
                confidence: 0.85,
                position: hand_position,
                detected_at: Utc::now(),
            };
            
            self.gesture_buffer.push(gesture.clone());
            return Some(gesture);
        }
        
        // Check for open palm
        let all_extended = self.is_finger_extended(&hand_position, &finger_positions.index)
            && self.is_finger_extended(&hand_position, &finger_positions.middle)
            && self.is_finger_extended(&hand_position, &finger_positions.ring)
            && self.is_finger_extended(&hand_position, &finger_positions.pinky);
        
        if all_extended {
            let gesture = DetectedGesture {
                user_id,
                hand,
                gesture_type: GestureType::OpenPalm,
                confidence: 0.95,
                position: hand_position,
                detected_at: Utc::now(),
            };
            
            self.gesture_buffer.push(gesture.clone());
            return Some(gesture);
        }
        
        None
    }

    /// Check if finger is extended
    fn is_finger_extended(&self, palm: &Vector3, finger_tip: &Vector3) -> bool {
        let distance = palm.distance(finger_tip);
        distance > 0.08 // 8cm threshold
    }

    /// Get recent gestures
    pub fn get_recent_gestures(&self, count: usize) -> Vec<&DetectedGesture> {
        self.gesture_buffer.iter()
            .rev()
            .take(count)
            .collect()
    }

    /// Clear old gestures
    pub fn clear_old_gestures(&mut self, older_than_seconds: i64) {
        let cutoff = Utc::now() - chrono::Duration::seconds(older_than_seconds);
        self.gesture_buffer.retain(|g| g.detected_at > cutoff);
    }
}

impl Default for GestureRecognizer {
    fn default() -> Self {
        Self::new(0.7)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    #[test]
    fn test_gesture_recognizer_creation() {
        let recognizer = GestureRecognizer::new(0.8);
        assert_eq!(recognizer.min_confidence, 0.8);
    }

    #[test]
    fn test_detect_point_gesture() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::new(0.0, 0.0, 0.0);
        let fingers = FingerPositions {
            thumb: Vector3::new(0.02, 0.02, 0.02),
            index: Vector3::new(0.15, 0.0, 0.0),
            middle: Vector3::new(0.03, 0.0, 0.0),
            ring: Vector3::new(0.03, 0.0, 0.0),
            pinky: Vector3::new(0.03, 0.0, 0.0),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Right, hand_pos, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert!(matches!(g.gesture_type, GestureType::Point { .. }));
        }
    }

    #[test]
    fn test_detect_open_palm() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::new(0.0, 0.0, 0.0);
        let fingers = FingerPositions {
            thumb: Vector3::new(0.1, 0.05, 0.0),
            index: Vector3::new(0.1, 0.0, 0.0),
            middle: Vector3::new(0.1, 0.0, 0.0),
            ring: Vector3::new(0.1, 0.0, 0.0),
            pinky: Vector3::new(0.1, 0.0, 0.0),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Left, hand_pos, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert_eq!(g.gesture_type, GestureType::OpenPalm);
        }
    }

    #[test]
    fn test_get_recent_gestures() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        recognizer.gesture_buffer.push(DetectedGesture {
            user_id: user_id.clone(),
            hand: Hand::Right,
            gesture_type: GestureType::Wave,
            confidence: 0.9,
            position: Vector3::zero(),
            detected_at: Utc::now(),
        });
        
        let recent = recognizer.get_recent_gestures(1);
        assert_eq!(recent.len(), 1);
    }
}
