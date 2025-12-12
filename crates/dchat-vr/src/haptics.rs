//! Haptic Feedback System
//!
//! Provides haptic feedback for VR interactions and gestures

use crate::gesture::{GestureType, Hand};
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

/// Represents a full haptic waveform with multiple segments
#[derive(Debug, Clone)]
pub struct HapticWaveform {
    /// The hand to apply the waveform to
    pub hand: Hand,
    /// Sequence of (intensity, duration_ms) segments
    pub segments: Vec<(f32, u32)>,
    /// Total duration of the waveform in milliseconds
    pub total_duration_ms: u32,
}

impl HapticWaveform {
    /// Create a new waveform from segments with applied intensity multiplier
    pub fn new(hand: Hand, segments: Vec<(f32, u32)>, intensity_multiplier: f32) -> Self {
        let adjusted_segments: Vec<(f32, u32)> = segments
            .iter()
            .map(|(intensity, duration)| (intensity * intensity_multiplier, *duration))
            .collect();

        let total_duration_ms = adjusted_segments.iter().map(|(_, d)| d).sum();

        Self {
            hand,
            segments: adjusted_segments,
            total_duration_ms,
        }
    }

    /// Get an iterator over the waveform segments for playback
    pub fn iter(&self) -> impl Iterator<Item = &(f32, u32)> {
        self.segments.iter()
    }

    /// Check if the waveform is empty
    pub fn is_empty(&self) -> bool {
        self.segments.is_empty()
    }
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
    ///
    /// Returns a simple trigger result for single-segment events,
    /// or the first segment for custom waveforms. For full waveform
    /// playback, use `trigger_waveform` instead.
    pub fn trigger(&self, hand: Hand, event: HapticEvent) -> Option<(Hand, f32, u32)> {
        if !self.enabled {
            return None;
        }

        match event {
            HapticEvent::Tap => Some((hand, 0.5 * self.intensity_multiplier, 50)),
            HapticEvent::Buzz { duration_ms } => {
                Some((hand, 0.7 * self.intensity_multiplier, duration_ms))
            }
            HapticEvent::Pulse {
                intensity,
                duration_ms,
            } => Some((hand, intensity * self.intensity_multiplier, duration_ms)),
            HapticEvent::Custom { waveform } => {
                // Return first segment for backward compatibility
                // For full waveform playback, use trigger_waveform()
                if let Some((intensity, duration)) = waveform.first() {
                    Some((hand, intensity * self.intensity_multiplier, *duration))
                } else {
                    None
                }
            }
        }
    }

    /// Trigger a haptic event and return full waveform for custom events
    ///
    /// This method handles the complete waveform sequence for custom haptic events,
    /// allowing VR platforms to play back multi-segment feedback patterns.
    pub fn trigger_waveform(&self, hand: Hand, event: HapticEvent) -> Option<HapticWaveform> {
        if !self.enabled {
            return None;
        }

        let segments = match event {
            HapticEvent::Tap => vec![(0.5, 50)],
            HapticEvent::Buzz { duration_ms } => vec![(0.7, duration_ms)],
            HapticEvent::Pulse {
                intensity,
                duration_ms,
            } => vec![(intensity, duration_ms)],
            HapticEvent::Custom { waveform } => {
                if waveform.is_empty() {
                    return None;
                }
                waveform
            }
        };

        Some(HapticWaveform::new(
            hand,
            segments,
            self.intensity_multiplier,
        ))
    }

    /// Get haptic feedback for gesture detection
    pub fn haptic_for_gesture(&self, gesture_type: &GestureType) -> HapticEvent {
        match gesture_type {
            GestureType::ThumbsUp | GestureType::ThumbsDown => HapticEvent::Tap,
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

        let result = manager.trigger(
            Hand::Left,
            HapticEvent::Pulse {
                intensity: 1.0,
                duration_ms: 100,
            },
        );

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
    fn test_custom_waveform() {
        let manager = HapticManager::new();
        let wave_haptic = manager.haptic_for_gesture(&GestureType::Wave);

        if let HapticEvent::Custom { waveform } = wave_haptic {
            // Test full waveform handling
            let result = manager.trigger_waveform(
                Hand::Right,
                HapticEvent::Custom {
                    waveform: waveform.clone(),
                },
            );
            assert!(result.is_some());

            let haptic_waveform = result.unwrap();
            assert_eq!(haptic_waveform.segments.len(), 3);
            assert_eq!(haptic_waveform.total_duration_ms, 300); // 100 + 100 + 100
        } else {
            panic!("Expected Custom haptic event");
        }
    }

    #[test]
    fn test_waveform_intensity_multiplier() {
        let mut manager = HapticManager::new();
        manager.set_intensity(0.5);

        let waveform = vec![(1.0, 100), (0.8, 50)];
        let result = manager.trigger_waveform(Hand::Left, HapticEvent::Custom { waveform });

        assert!(result.is_some());
        let haptic_waveform = result.unwrap();
        assert_eq!(haptic_waveform.segments[0].0, 0.5); // 1.0 * 0.5
        assert_eq!(haptic_waveform.segments[1].0, 0.4); // 0.8 * 0.5
    }
}
