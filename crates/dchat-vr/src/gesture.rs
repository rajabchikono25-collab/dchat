//! Gesture Recognition and Hand Tracking (Production-Grade)
//!
//! Provides ML-based gesture controls and skeletal hand tracking for VR/AR interfaces.
//! Supports custom gesture training, multi-finger gestures, and gesture-to-action mapping.

use crate::Vector3;
use chrono::{DateTime, Utc};
use dchat_core::types::UserId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Hand type
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub enum Hand {
    Left,
    Right,
}

/// Gesture types
#[derive(Debug, Clone, Serialize, Deserialize)]
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
    /// Pinch
    Pinch,
    /// Custom gesture
    Custom { name: String },
}

impl PartialEq for GestureType {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Point { direction: d1 }, Self::Point { direction: d2 }) => {
                (d1.x - d2.x).abs() < 0.001 && (d1.y - d2.y).abs() < 0.001 && (d1.z - d2.z).abs() < 0.001
            },
            (Self::ThumbsUp, Self::ThumbsUp) => true,
            (Self::ThumbsDown, Self::ThumbsDown) => true,
            (Self::Wave, Self::Wave) => true,
            (Self::Grab, Self::Grab) => true,
            (Self::Release, Self::Release) => true,
            (Self::Swipe { direction: s1 }, Self::Swipe { direction: s2 }) => s1 == s2,
            (Self::PeaceSign, Self::PeaceSign) => true,
            (Self::Fist, Self::Fist) => true,
            (Self::OpenPalm, Self::OpenPalm) => true,
            (Self::Pinch, Self::Pinch) => true,
            (Self::Custom { name: n1 }, Self::Custom { name: n2 }) => n1 == n2,
            _ => false,
        }
    }
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

/// Positions of all fingers with full skeletal data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerPositions {
    pub thumb: FingerJoints,
    pub index: FingerJoints,
    pub middle: FingerJoints,
    pub ring: FingerJoints,
    pub pinky: FingerJoints,
}

/// Skeletal joints for a single finger
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FingerJoints {
    pub metacarpal: Vector3, // Base joint (connects to palm)
    pub proximal: Vector3, // First knuckle
    pub intermediate: Vector3, // Second knuckle
    pub distal: Vector3, // Third knuckle
    pub tip: Vector3, // Fingertip
}

impl FingerJoints {
    /// Get the tip position (shorthand)
    pub fn tip_position(&self) -> Vector3 {
        self.tip
    }

    /// Calculate if finger is extended (straight vs curled)
    pub fn is_extended(&self, palm_position: &Vector3) -> bool {
        // Calculate distance from palm to tip
        let total_distance = palm_position.distance(&self.tip);
        
        // Calculate sum of joint distances (path length)
        let joint_distance = 
            palm_position.distance(&self.metacarpal) +
            self.metacarpal.distance(&self.proximal) +
            self.proximal.distance(&self.intermediate) +
            self.intermediate.distance(&self.distal) +
            self.distal.distance(&self.tip);
        
        // If path length is similar to direct distance, finger is extended
        // Threshold: 1.3 (curled finger has longer path)
        (joint_distance / total_distance) < 1.3
    }

    /// Calculate finger curl amount (0.0 = fully extended, 1.0 = fully curled)
    pub fn curl_amount(&self) -> f32 {
        // Calculate angles between segments
        let angle1 = Self::calculate_angle(&self.metacarpal, &self.proximal, &self.intermediate);
        let angle2 = Self::calculate_angle(&self.proximal, &self.intermediate, &self.distal);
        let angle3 = Self::calculate_angle(&self.intermediate, &self.distal, &self.tip);
        
        // Average of normalized angles (180° = 0.0 extended, 0° = 1.0 curled)
        let avg_angle = (angle1 + angle2 + angle3) / 3.0;
        (std::f32::consts::PI - avg_angle) / std::f32::consts::PI
    }

    /// Calculate angle between three points
    fn calculate_angle(a: &Vector3, b: &Vector3, c: &Vector3) -> f32 {
        let ba = Vector3 {
            x: a.x - b.x,
            y: a.y - b.y,
            z: a.z - b.z,
        };
        let bc = Vector3 {
            x: c.x - b.x,
            y: c.y - b.y,
            z: c.z - b.z,
        };
        
        let dot = ba.x * bc.x + ba.y * bc.y + ba.z * bc.z;
        let mag_ba = (ba.x * ba.x + ba.y * ba.y + ba.z * ba.z).sqrt();
        let mag_bc = (bc.x * bc.x + bc.y * bc.y + bc.z * bc.z).sqrt();
        
        if mag_ba == 0.0 || mag_bc == 0.0 {
            return 0.0;
        }
        
        (dot / (mag_ba * mag_bc)).clamp(-1.0, 1.0).acos()
    }
}

/// Gesture template for ML-based recognition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GestureTemplate {
    pub name: String,
    pub gesture_type: GestureType,
    pub finger_states: HashMap<String, FingerState>,
    pub palm_orientation: Option<PalmOrientation>,
    pub motion_pattern: Option<MotionPattern>,
    pub min_confidence: f32,
}

/// State of a finger for gesture matching
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum FingerState {
    Extended,
    Curled,
    PartiallyExtended,
    Any,
}

/// Palm orientation for gesture matching
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum PalmOrientation {
    Up,
    Down,
    Forward,
    Backward,
    Left,
    Right,
    Any,
}

/// Motion pattern for dynamic gestures (swipe, wave)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MotionPattern {
    pub direction: Vector3,
    pub speed_threshold: f32, // m/s
    pub duration_ms: u32,
}

impl GestureTemplate {
    /// Create a thumbs up gesture template
    pub fn thumbs_up() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Extended);
        finger_states.insert("index".to_string(), FingerState::Curled);
        finger_states.insert("middle".to_string(), FingerState::Curled);
        finger_states.insert("ring".to_string(), FingerState::Curled);
        finger_states.insert("pinky".to_string(), FingerState::Curled);

        Self {
            name: "thumbs_up".to_string(),
            gesture_type: GestureType::ThumbsUp,
            finger_states,
            palm_orientation: Some(PalmOrientation::Forward),
            motion_pattern: None,
            min_confidence: 0.85,
        }
    }

    /// Create a point gesture template
    pub fn point() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::PartiallyExtended);
        finger_states.insert("index".to_string(), FingerState::Extended);
        finger_states.insert("middle".to_string(), FingerState::Curled);
        finger_states.insert("ring".to_string(), FingerState::Curled);
        finger_states.insert("pinky".to_string(), FingerState::Curled);

        Self {
            name: "point".to_string(),
            gesture_type: GestureType::Point { direction: Vector3::zero() },
            finger_states,
            palm_orientation: Some(PalmOrientation::Forward),
            motion_pattern: None,
            min_confidence: 0.9,
        }
    }

    /// Create an open palm gesture template
    pub fn open_palm() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Extended);
        finger_states.insert("index".to_string(), FingerState::Extended);
        finger_states.insert("middle".to_string(), FingerState::Extended);
        finger_states.insert("ring".to_string(), FingerState::Extended);
        finger_states.insert("pinky".to_string(), FingerState::Extended);

        Self {
            name: "open_palm".to_string(),
            gesture_type: GestureType::OpenPalm,
            finger_states,
            palm_orientation: None,
            motion_pattern: None,
            min_confidence: 0.95,
        }
    }

    /// Create a fist gesture template
    pub fn fist() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Curled);
        finger_states.insert("index".to_string(), FingerState::Curled);
        finger_states.insert("middle".to_string(), FingerState::Curled);
        finger_states.insert("ring".to_string(), FingerState::Curled);
        finger_states.insert("pinky".to_string(), FingerState::Curled);

        Self {
            name: "fist".to_string(),
            gesture_type: GestureType::Fist,
            finger_states,
            palm_orientation: None,
            motion_pattern: None,
            min_confidence: 0.9,
        }
    }

    /// Create a peace sign gesture template
    pub fn peace_sign() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Curled);
        finger_states.insert("index".to_string(), FingerState::Extended);
        finger_states.insert("middle".to_string(), FingerState::Extended);
        finger_states.insert("ring".to_string(), FingerState::Curled);
        finger_states.insert("pinky".to_string(), FingerState::Curled);

        Self {
            name: "peace_sign".to_string(),
            gesture_type: GestureType::PeaceSign,
            finger_states,
            palm_orientation: Some(PalmOrientation::Forward),
            motion_pattern: None,
            min_confidence: 0.85,
        }
    }

    /// Create a grab gesture template (pinch)
    pub fn grab() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::PartiallyExtended);
        finger_states.insert("index".to_string(), FingerState::PartiallyExtended);
        finger_states.insert("middle".to_string(), FingerState::Any);
        finger_states.insert("ring".to_string(), FingerState::Any);
        finger_states.insert("pinky".to_string(), FingerState::Any);

        Self {
            name: "grab".to_string(),
            gesture_type: GestureType::Grab,
            finger_states,
            palm_orientation: None,
            motion_pattern: None,
            min_confidence: 0.8,
        }
    }

    /// Create a wave gesture template (dynamic)
    pub fn wave() -> Self {
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Extended);
        finger_states.insert("index".to_string(), FingerState::Extended);
        finger_states.insert("middle".to_string(), FingerState::Extended);
        finger_states.insert("ring".to_string(), FingerState::Extended);
        finger_states.insert("pinky".to_string(), FingerState::Extended);

        Self {
            name: "wave".to_string(),
            gesture_type: GestureType::Wave,
            finger_states,
            palm_orientation: Some(PalmOrientation::Forward),
            motion_pattern: Some(MotionPattern {
                direction: Vector3::new(1.0, 0.0, 0.0), // Side-to-side
                speed_threshold: 0.5,
                duration_ms: 500,
            }),
            min_confidence: 0.75,
        }
    }

    /// Match finger positions against this template
    pub fn match_fingers(&self, fingers: &FingerPositions, palm_position: &Vector3) -> f32 {
        let mut matches = 0;
        let mut total = 0;

        // Check thumb
        if let Some(state) = self.finger_states.get("thumb") {
            total += 1;
            if Self::finger_matches_state(&fingers.thumb, palm_position, state) {
                matches += 1;
            }
        }

        // Check index
        if let Some(state) = self.finger_states.get("index") {
            total += 1;
            if Self::finger_matches_state(&fingers.index, palm_position, state) {
                matches += 1;
            }
        }

        // Check middle
        if let Some(state) = self.finger_states.get("middle") {
            total += 1;
            if Self::finger_matches_state(&fingers.middle, palm_position, state) {
                matches += 1;
            }
        }

        // Check ring
        if let Some(state) = self.finger_states.get("ring") {
            total += 1;
            if Self::finger_matches_state(&fingers.ring, palm_position, state) {
                matches += 1;
            }
        }

        // Check pinky
        if let Some(state) = self.finger_states.get("pinky") {
            total += 1;
            if Self::finger_matches_state(&fingers.pinky, palm_position, state) {
                matches += 1;
            }
        }

        if total == 0 {
            return 0.0;
        }

        matches as f32 / total as f32
    }

    fn finger_matches_state(finger: &FingerJoints, palm_position: &Vector3, state: &FingerState) -> bool {
        match state {
            FingerState::Any => true,
            FingerState::Extended => finger.is_extended(palm_position),
            FingerState::Curled => !finger.is_extended(palm_position),
            FingerState::PartiallyExtended => {
                let curl = finger.curl_amount();
                curl > 0.2 && curl < 0.7 // Partially curled
            }
        }
    }

    /// Match palm orientation
    pub fn match_orientation(&self, palm_normal: &Vector3) -> f32 {
        match &self.palm_orientation {
            None | Some(PalmOrientation::Any) => 1.0,
            Some(orientation) => {
                let target_normal = match orientation {
                    PalmOrientation::Up => Vector3::new(0.0, 1.0, 0.0),
                    PalmOrientation::Down => Vector3::new(0.0, -1.0, 0.0),
                    PalmOrientation::Forward => Vector3::new(0.0, 0.0, -1.0),
                    PalmOrientation::Backward => Vector3::new(0.0, 0.0, 1.0),
                    PalmOrientation::Left => Vector3::new(-1.0, 0.0, 0.0),
                    PalmOrientation::Right => Vector3::new(1.0, 0.0, 0.0),
                    PalmOrientation::Any => return 1.0,
                };

                // Calculate dot product (cosine similarity)
                let dot = palm_normal.x * target_normal.x + 
                         palm_normal.y * target_normal.y + 
                         palm_normal.z * target_normal.z;
                
                // Convert to 0.0-1.0 range (allow 45° deviation)
                ((dot + 1.0) / 2.0).clamp(0.0, 1.0)
            }
        }
    }
}

/// Gesture recognizer with ML-based templates
pub struct GestureRecognizer {
    gesture_buffer: Vec<DetectedGesture>,
    tracking_data: Vec<HandTrackingData>,
    min_confidence: f32,
    gesture_templates: HashMap<String, GestureTemplate>,
    motion_history: HashMap<UserId, Vec<(DateTime<Utc>, Vector3)>>,
}

impl GestureRecognizer {
    pub fn new(min_confidence: f32) -> Self {
        let mut recognizer = Self {
            gesture_buffer: Vec::new(),
            tracking_data: Vec::new(),
            min_confidence,
            gesture_templates: HashMap::new(),
            motion_history: HashMap::new(),
        };

        // Load default gesture templates
        recognizer.load_default_templates();
        recognizer
    }

    fn load_default_templates(&mut self) {
        self.add_template(GestureTemplate::thumbs_up());
        self.add_template(GestureTemplate::point());
        self.add_template(GestureTemplate::open_palm());
        self.add_template(GestureTemplate::fist());
        self.add_template(GestureTemplate::peace_sign());
        self.add_template(GestureTemplate::grab());
        self.add_template(GestureTemplate::wave());
    }

    /// Add a custom gesture template
    pub fn add_template(&mut self, template: GestureTemplate) {
        self.gesture_templates.insert(template.name.clone(), template);
    }

    /// Remove a gesture template
    pub fn remove_template(&mut self, name: &str) {
        self.gesture_templates.remove(name);
    }

    /// Get all registered templates
    pub fn get_templates(&self) -> Vec<&GestureTemplate> {
        self.gesture_templates.values().collect()
    }

    /// Update hand tracking data
    pub fn update_tracking(&mut self, tracking: HandTrackingData) {
        self.tracking_data.push(tracking);
        
        // Keep only recent tracking data (last 100 frames)
        if self.tracking_data.len() > 100 {
            self.tracking_data.drain(0..self.tracking_data.len() - 100);
        }
    }

    /// Detect gesture from hand position using ML-based templates
    pub fn detect_gesture(
        &mut self,
        user_id: UserId,
        hand: Hand,
        hand_position: Vector3,
        palm_normal: Vector3,
        finger_positions: FingerPositions,
    ) -> Option<DetectedGesture> {
        // Track motion for dynamic gestures
        self.track_motion(user_id.clone(), hand_position);

        let mut best_match: Option<(GestureTemplate, f32)> = None;

        // Match against all templates
        for template in self.gesture_templates.values() {
            // Match finger states
            let finger_score = template.match_fingers(&finger_positions, &hand_position);
            
            // Match palm orientation
            let orientation_score = template.match_orientation(&palm_normal);

            // Check motion pattern for dynamic gestures
            let motion_score = if let Some(pattern) = &template.motion_pattern {
                self.match_motion_pattern(&user_id, pattern)
            } else {
                1.0 // Static gestures don't need motion
            };

            // Combined confidence score
            let confidence = finger_score * 0.6 + orientation_score * 0.3 + motion_score * 0.1;

            if confidence >= template.min_confidence && confidence >= self.min_confidence {
                if best_match.is_none() || confidence > best_match.as_ref().unwrap().1 {
                    best_match = Some((template.clone(), confidence));
                }
            }
        }

        if let Some((template, confidence)) = best_match {
            // Create direction for point gesture
            let gesture_type = match &template.gesture_type {
                GestureType::Point { .. } => {
                    let direction = Vector3 {
                        x: finger_positions.index.tip.x - hand_position.x,
                        y: finger_positions.index.tip.y - hand_position.y,
                        z: finger_positions.index.tip.z - hand_position.z,
                    };
                    GestureType::Point { direction }
                },
                other => other.clone(),
            };

            let gesture = DetectedGesture {
                user_id,
                hand,
                gesture_type,
                confidence,
                position: hand_position,
                detected_at: Utc::now(),
            };

            self.gesture_buffer.push(gesture.clone());
            return Some(gesture);
        }

        None
    }

    /// Track hand motion for dynamic gesture detection
    fn track_motion(&mut self, user_id: UserId, position: Vector3) {
        let history = self.motion_history.entry(user_id).or_insert_with(Vec::new);
        history.push((Utc::now(), position));

        // Keep last 30 frames (1 second at 30Hz)
        if history.len() > 30 {
            history.remove(0);
        }
    }

    /// Match motion pattern for dynamic gestures
    fn match_motion_pattern(&self, user_id: &UserId, pattern: &MotionPattern) -> f32 {
        let history = match self.motion_history.get(user_id) {
            Some(h) if h.len() >= 5 => h,
            _ => return 0.0,
        };

        // Calculate average velocity over recent frames
        let mut velocities = Vec::new();
        for i in 1..history.len() {
            let (t1, p1) = &history[i - 1];
            let (t2, p2) = &history[i];
            
            let dt = (t2.timestamp_millis() - t1.timestamp_millis()) as f32 / 1000.0;
            if dt > 0.0 {
                let velocity = Vector3 {
                    x: (p2.x - p1.x) / dt,
                    y: (p2.y - p1.y) / dt,
                    z: (p2.z - p1.z) / dt,
                };
                velocities.push(velocity);
            }
        }

        if velocities.is_empty() {
            return 0.0;
        }

        // Calculate average velocity direction
        let mut avg_velocity = Vector3::zero();
        for v in &velocities {
            avg_velocity.x += v.x;
            avg_velocity.y += v.y;
            avg_velocity.z += v.z;
        }
        avg_velocity.x /= velocities.len() as f32;
        avg_velocity.y /= velocities.len() as f32;
        avg_velocity.z /= velocities.len() as f32;

        let speed = (avg_velocity.x.powi(2) + avg_velocity.y.powi(2) + avg_velocity.z.powi(2)).sqrt();

        // Check if speed meets threshold
        if speed < pattern.speed_threshold {
            return 0.0;
        }

        // Calculate direction similarity (dot product with pattern direction)
        let velocity_magnitude = speed;
        let pattern_magnitude = (pattern.direction.x.powi(2) + 
                                pattern.direction.y.powi(2) + 
                                pattern.direction.z.powi(2)).sqrt();

        if velocity_magnitude == 0.0 || pattern_magnitude == 0.0 {
            return 0.0;
        }

        let dot = (avg_velocity.x * pattern.direction.x +
                  avg_velocity.y * pattern.direction.y +
                  avg_velocity.z * pattern.direction.z) /
                 (velocity_magnitude * pattern_magnitude);

        // Convert to 0.0-1.0 range
        ((dot + 1.0) / 2.0).clamp(0.0, 1.0)
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

    fn create_extended_finger(base_x: f32) -> FingerJoints {
        // Create a realistic extended finger with proper joint spacing
        // Total length ~0.08m (8cm) for an adult index finger
        FingerJoints {
            metacarpal: Vector3::new(base_x, 0.0, 0.0),
            proximal: Vector3::new(base_x + 0.02, 0.0, 0.0),
            intermediate: Vector3::new(base_x + 0.04, 0.0, 0.0),
            distal: Vector3::new(base_x + 0.06, 0.0, 0.0),
            tip: Vector3::new(base_x + 0.08, 0.0, 0.0),
        }
    }

    fn create_curled_finger(base_x: f32) -> FingerJoints {
        // Create a tightly curled finger with sharp angles - tip folds all the way back
        // Each joint bends ~90 degrees to create high curl amount
        FingerJoints {
            metacarpal: Vector3::new(base_x, 0.0, 0.0),
            proximal: Vector3::new(base_x + 0.025, 0.0, 0.0),    // First segment straight out
            intermediate: Vector3::new(base_x + 0.03, 0.025, 0.0), // Bend up 90°
            distal: Vector3::new(base_x + 0.015, 0.03, 0.0),     // Bend back towards palm
            tip: Vector3::new(base_x + 0.005, 0.015, 0.0),       // Curl down into palm
        }
    }

    #[test]
    fn test_gesture_recognizer_creation() {
        let recognizer = GestureRecognizer::new(0.8);
        assert_eq!(recognizer.min_confidence, 0.8);
        assert!(!recognizer.gesture_templates.is_empty()); // Should have default templates
    }

    #[test]
    fn test_finger_is_extended() {
        let extended = create_extended_finger(0.0);
        let curled = create_curled_finger(0.0);
        let palm = Vector3::zero();

        assert!(extended.is_extended(&palm));
        assert!(!curled.is_extended(&palm));
    }

    #[test]
    fn test_finger_curl_amount() {
        let extended = create_extended_finger(0.0);
        let curled = create_curled_finger(0.0);

        let extended_curl = extended.curl_amount();
        let curled_curl = curled.curl_amount();

        println!("Extended curl: {}, Curled curl: {}", extended_curl, curled_curl);
        
        assert!(extended_curl < 0.3); // Extended should have low curl
        assert!(curled_curl > 0.3); // Curled should have noticeably higher curl
        assert!(curled_curl > extended_curl); // Curled should be more than extended
    }

    #[test]
    fn test_detect_point_gesture() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::zero();
        let palm_normal = Vector3::new(0.0, 0.0, -1.0);
        
        // Point gesture: index extended, others curled
        let fingers = FingerPositions {
            thumb: create_curled_finger(0.01),
            index: create_extended_finger(0.03),
            middle: create_curled_finger(0.05),
            ring: create_curled_finger(0.07),
            pinky: create_curled_finger(0.09),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Right, hand_pos, palm_normal, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert!(matches!(g.gesture_type, GestureType::Point { .. }));
            assert!(g.confidence > 0.7);
        }
    }

    #[test]
    fn test_detect_open_palm() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::zero();
        let palm_normal = Vector3::new(0.0, 1.0, 0.0);
        
        // Open palm: all fingers extended
        let fingers = FingerPositions {
            thumb: create_extended_finger(0.01),
            index: create_extended_finger(0.03),
            middle: create_extended_finger(0.05),
            ring: create_extended_finger(0.07),
            pinky: create_extended_finger(0.09),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Left, hand_pos, palm_normal, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert_eq!(g.gesture_type, GestureType::OpenPalm);
        }
    }

    #[test]
    fn test_detect_fist() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::zero();
        let palm_normal = Vector3::new(0.0, 1.0, 0.0);
        
        // Fist: all fingers curled
        let fingers = FingerPositions {
            thumb: create_curled_finger(0.01),
            index: create_curled_finger(0.03),
            middle: create_curled_finger(0.05),
            ring: create_curled_finger(0.07),
            pinky: create_curled_finger(0.09),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Right, hand_pos, palm_normal, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert_eq!(g.gesture_type, GestureType::Fist);
        }
    }

    #[test]
    fn test_detect_peace_sign() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        let hand_pos = Vector3::zero();
        let palm_normal = Vector3::new(0.0, 0.0, -1.0);
        
        // Peace sign: index and middle extended, others curled
        let fingers = FingerPositions {
            thumb: create_curled_finger(0.01),
            index: create_extended_finger(0.03),
            middle: create_extended_finger(0.05),
            ring: create_curled_finger(0.07),
            pinky: create_curled_finger(0.09),
        };
        
        let gesture = recognizer.detect_gesture(user_id, Hand::Right, hand_pos, palm_normal, fingers);
        assert!(gesture.is_some());
        
        if let Some(g) = gesture {
            assert_eq!(g.gesture_type, GestureType::PeaceSign);
        }
    }

    #[test]
    fn test_custom_template() {
        let mut recognizer = GestureRecognizer::new(0.7);
        
        // Create custom "gun" gesture template (index and thumb extended)
        let mut finger_states = HashMap::new();
        finger_states.insert("thumb".to_string(), FingerState::Extended);
        finger_states.insert("index".to_string(), FingerState::Extended);
        finger_states.insert("middle".to_string(), FingerState::Curled);
        finger_states.insert("ring".to_string(), FingerState::Curled);
        finger_states.insert("pinky".to_string(), FingerState::Curled);

        let custom_template = GestureTemplate {
            name: "gun".to_string(),
            gesture_type: GestureType::Custom { name: "gun".to_string() },
            finger_states,
            palm_orientation: None,
            motion_pattern: None,
            min_confidence: 0.8,
        };

        recognizer.add_template(custom_template);
        
        let templates = recognizer.get_templates();
        assert!(templates.iter().any(|t| t.name == "gun"));
    }

    #[test]
    fn test_template_match_fingers() {
        let template = GestureTemplate::thumbs_up();
        let palm = Vector3::zero();
        
        // Perfect match: thumb extended, others curled
        let matching_fingers = FingerPositions {
            thumb: create_extended_finger(0.01),
            index: create_curled_finger(0.03),
            middle: create_curled_finger(0.05),
            ring: create_curled_finger(0.07),
            pinky: create_curled_finger(0.09),
        };
        
        let score = template.match_fingers(&matching_fingers, &palm);
        assert!(score > 0.8); // Should be high match
        
        // Poor match: all fingers extended
        let non_matching_fingers = FingerPositions {
            thumb: create_extended_finger(0.01),
            index: create_extended_finger(0.03),
            middle: create_extended_finger(0.05),
            ring: create_extended_finger(0.07),
            pinky: create_extended_finger(0.09),
        };
        
        let score = template.match_fingers(&non_matching_fingers, &palm);
        assert!(score < 0.5); // Should be low match
    }

    #[test]
    fn test_palm_orientation_matching() {
        let template = GestureTemplate::thumbs_up();
        
        // Forward orientation (perfect match)
        let forward_normal = Vector3::new(0.0, 0.0, -1.0);
        let score_forward = template.match_orientation(&forward_normal);
        assert!(score_forward > 0.8);
        
        // Backward orientation (poor match)
        let backward_normal = Vector3::new(0.0, 0.0, 1.0);
        let score_backward = template.match_orientation(&backward_normal);
        assert!(score_backward < 0.5);
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

    #[test]
    fn test_clear_old_gestures() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        // Add old gesture (1 hour ago)
        let old_time = Utc::now() - chrono::Duration::hours(1);
        recognizer.gesture_buffer.push(DetectedGesture {
            user_id: user_id.clone(),
            hand: Hand::Right,
            gesture_type: GestureType::Wave,
            confidence: 0.9,
            position: Vector3::zero(),
            detected_at: old_time,
        });
        
        // Add recent gesture
        recognizer.gesture_buffer.push(DetectedGesture {
            user_id: user_id.clone(),
            hand: Hand::Left,
            gesture_type: GestureType::Fist,
            confidence: 0.9,
            position: Vector3::zero(),
            detected_at: Utc::now(),
        });
        
        recognizer.clear_old_gestures(1800); // 30 minutes
        
        assert_eq!(recognizer.gesture_buffer.len(), 1);
        assert_eq!(recognizer.gesture_buffer[0].gesture_type, GestureType::Fist);
    }

    #[test]
    fn test_motion_tracking() {
        let mut recognizer = GestureRecognizer::new(0.7);
        let user_id = create_test_user();
        
        // Track motion over several frames
        for i in 0..10 {
            let position = Vector3::new(i as f32 * 0.1, 0.0, 0.0);
            recognizer.track_motion(user_id.clone(), position);
        }
        
        let history = recognizer.motion_history.get(&user_id);
        assert!(history.is_some());
        assert_eq!(history.unwrap().len(), 10);
    }
}
