//! Haptic Feedback System
//!
//! Provides haptic feedback for VR interactions and gestures

use crate::gesture::{Hand, GestureType};
use serde::{Deserialize, Serialize};

/// Haptic event types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum HapticEvent {
    /// Short tap (like a button press)
    Tap,
    /// Continuous buzz for specified duration
    Buzz { duration_ms: u32 },
    /// Pulsing haptic with intensity
    Pulse { intensity: f32, duration_ms: u32 },
    /// Custom waveform pattern
    Custom { waveform: Vec<(f32, u32)> }, // (intensity, duration_ms) pairs
}

/// Haptic manager for triggering feedback
#[derive(Debug)]
pub struct HapticManager {
    enabled: bool,
    intensity_multiplier: f32, // User preference: 0.0 to 1.0
}

impl HapticManager {
    /// Create new haptic manager
    pub fn new() -> Self {
        Self {
            enabled: true,
            intensity_multiplier: 1.0,
        }
    }

    /// Enable/disable haptics
    pub fn set_enabled(&mut self, enabled: bool) {
        self.enabled = enabled;
    }

    /// Set global intensity multiplier (user preference)
    pub fn set_intensity(&mut self, multiplier: f32) {
        self.intensity_multiplier = multiplier.clamp(0.0, 1.0);
    }

    /// Trigger haptic event on specified hand
    pub fn trigger(&self, hand: Hand, event: HapticEvent) -> Option<(Hand, f32, u32)> {
        if !self.enabled {
            return None;
        }

        match event {
            HapticEvent::Tap => Some((hand, 0.5 * self.intensity_multiplier, 50)),
            HapticEvent::Buzz { duration_ms } => {
                Some((hand, 0.7 * self.intensity_multiplier, duration_ms))
            }
            HapticEvent::Pulse { intensity, duration_ms } => {
                Some((hand, intensity * self.intensity_multiplier, duration_ms))
            }
            HapticEvent::Custom { waveform } => {
                // For custom waveforms, return first segment
                // In production: handle full waveform sequence
                if let Some((intensity, duration)) = waveform.first() {
                    Some((hand, intensity * self.intensity_multiplier, *duration))
                } else {
                    None
                }
            }
        }
    }

    /// Get haptic feedback for gesture detection
    pub fn haptic_for_gesture(&self, gesture_type: &GestureType) -> HapticEvent {
        match gesture_type {
            GestureType::ThumbsUp | GestureType::ThumbsDown => {
                HapticEvent::Tap
            }
            GestureType::Point { .. } => HapticEvent::Buzz { duration_ms: 50 },
            GestureType::Grab => HapticEvent::Pulse {
                intensity: 0.8,
                duration_ms: 100,
            },
            GestureType::Release => HapticEvent::Tap,
            GestureType::Wave => HapticEvent::Custom {
                waveform: vec![(0.3, 100), (0.6, 100), (0.3, 100)],
            },
            GestureType::Fist => HapticEvent::Pulse {
                intensity: 1.0,
                duration_ms: 150,
            },
            GestureType::OpenPalm => HapticEvent::Tap,
            GestureType::PeaceSign => HapticEvent::Buzz { duration_ms: 75 },
            GestureType::Pinch => HapticEvent::Pulse {
                intensity: 0.5,
                duration_ms: 80,
            },
            GestureType::Swipe { .. } => HapticEvent::Custom {
                waveform: vec![(0.5, 50), (0.7, 50), (0.5, 50)],
            },
            GestureType::Custom { .. } => HapticEvent::Tap,
        }
    }
}

impl Default for HapticManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_haptic_trigger() {
        let manager = HapticManager::new();
        let result = manager.trigger(Hand::Right, HapticEvent::Tap);
        
        assert!(result.is_some());
        let (hand, intensity, duration) = result.unwrap();
        assert_eq!(hand, Hand::Right);
        assert!(intensity > 0.0);
        assert_eq!(duration, 50);
    }

    #[test]
    fn test_intensity_multiplier() {
        let mut manager = HapticManager::new();
        manager.set_intensity(0.5);
        
        let result = manager.trigger(Hand::Left, HapticEvent::Pulse {
            intensity: 1.0,
            duration_ms: 100,
        });
        
        assert!(result.is_some());
        let (_, intensity, _) = result.unwrap();
        assert_eq!(intensity, 0.5); // 1.0 * 0.5 multiplier
    }

    #[test]
    fn test_disabled_haptics() {
        let mut manager = HapticManager::new();
        manager.set_enabled(false);
        
        let result = manager.trigger(Hand::Right, HapticEvent::Tap);
        assert!(result.is_none());
    }

    #[test]
    fn test_gesture_haptics() {
        let manager = HapticManager::new();
        
        let thumbs_up_haptic = manager.haptic_for_gesture(&GestureType::ThumbsUp);
        assert!(matches!(thumbs_up_haptic, HapticEvent::Tap));
        
        let grab_haptic = manager.haptic_for_gesture(&GestureType::Grab);
        assert!(matches!(grab_haptic, HapticEvent::Pulse { .. }));
    }
}
