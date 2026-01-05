//! Accessibility Features for VR/AR
//!
//! Provides accessibility features to make VR/AR experiences inclusive:
//! - Text-to-speech for UI elements and messages
//! - High contrast visual modes
//! - Subtitle display for voice chat
//! - One-handed mode
//! - Colorblind-friendly palettes
//! - Seated mode adjustments

use crate::Vector3;
use serde::{Deserialize, Serialize};

/// Accessibility settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibilitySettings {
    /// Enable text-to-speech for UI elements
    pub tts_enabled: bool,
    /// TTS speech rate (0.5 = half speed, 2.0 = double speed)
    pub tts_rate: f32,
    /// Enable high contrast mode
    pub high_contrast: bool,
    /// Display subtitles for voice chat
    pub subtitles_enabled: bool,
    /// Subtitle font size (1.0 = normal, 2.0 = double size)
    pub subtitle_size: f32,
    /// Enable one-handed mode (remap gestures)
    pub one_handed_mode: bool,
    /// Primary hand for one-handed mode
    pub primary_hand: crate::gesture::Hand,
    /// Colorblind mode
    pub colorblind_mode: ColorblindMode,
    /// Seated mode (adjust interaction heights)
    pub seated_mode: bool,
    /// Seated eye height in meters
    pub seated_eye_height: f32,
    /// Reduce visual effects (particles, bloom, etc.)
    pub reduce_visual_effects: bool,
}

impl Default for AccessibilitySettings {
    fn default() -> Self {
        Self {
            tts_enabled: false,
            tts_rate: 1.0,
            high_contrast: false,
            subtitles_enabled: false,
            subtitle_size: 1.0,
            one_handed_mode: false,
            primary_hand: crate::gesture::Hand::Right,
            colorblind_mode: ColorblindMode::None,
            seated_mode: false,
            seated_eye_height: 1.2, // Typical seated eye height
            reduce_visual_effects: false,
        }
    }
}

/// Colorblind mode options
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub enum ColorblindMode {
    None,
    Protanopia,   // Red-blind
    Deuteranopia, // Green-blind
    Tritanopia,   // Blue-blind
}

/// Text-to-speech message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TtsMessage {
    pub text: String,
    pub priority: TtsPriority,
}

/// TTS priority levels
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum TtsPriority {
    Low,      // Background notifications
    Normal,   // UI feedback
    High,     // Important alerts
    Critical, // Safety warnings
}

/// Subtitle entry for voice chat
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subtitle {
    pub speaker: String,
    pub text: String,
    pub timestamp: chrono::DateTime<chrono::Utc>,
    pub position: Option<Vector3>, // Spatial position of speaker
}

/// Accessibility manager
pub struct AccessibilityManager {
    settings: AccessibilitySettings,
    tts_queue: Vec<TtsMessage>,
    subtitles: Vec<Subtitle>,
    max_subtitle_history: usize,
}

impl AccessibilityManager {
    pub fn new(settings: AccessibilitySettings) -> Self {
        Self {
            settings,
            tts_queue: Vec::new(),
            subtitles: Vec::new(),
            max_subtitle_history: 10,
        }
    }

    /// Queue text-to-speech message
    pub fn speak(&mut self, text: String, priority: TtsPriority) {
        if !self.settings.tts_enabled {
            return;
        }

        let message = TtsMessage { text, priority };

        // Insert based on priority
        let insert_pos = self.tts_queue.iter().position(|m| m.priority < priority);
        if let Some(pos) = insert_pos {
            self.tts_queue.insert(pos, message);
        } else {
            self.tts_queue.push(message);
        }
    }

    /// Get next TTS message to speak
    pub fn next_tts_message(&mut self) -> Option<TtsMessage> {
        if self.tts_queue.is_empty() {
            None
        } else {
            Some(self.tts_queue.remove(0))
        }
    }

    /// Add subtitle
    pub fn add_subtitle(&mut self, speaker: String, text: String, position: Option<Vector3>) {
        if !self.settings.subtitles_enabled {
            return;
        }

        let subtitle = Subtitle {
            speaker,
            text,
            timestamp: chrono::Utc::now(),
            position,
        };

        self.subtitles.push(subtitle);

        // Keep only recent subtitles
        if self.subtitles.len() > self.max_subtitle_history {
            self.subtitles.remove(0);
        }
    }

    /// Get recent subtitles
    pub fn get_subtitles(&self) -> &[Subtitle] {
        &self.subtitles
    }

    /// Adjust interaction height for seated mode
    pub fn adjust_for_seated(&self, position: Vector3) -> Vector3 {
        if !self.settings.seated_mode {
            return position;
        }

        // Adjust vertical position relative to seated eye height
        let standing_eye_height = 1.7; // Average standing eye height
        let height_offset = standing_eye_height - self.settings.seated_eye_height;

        Vector3::new(position.x, position.y - height_offset, position.z)
    }

    /// Apply colorblind filter to color
    pub fn apply_colorblind_filter(&self, rgb: (f32, f32, f32)) -> (f32, f32, f32) {
        match self.settings.colorblind_mode {
            ColorblindMode::None => rgb,
            ColorblindMode::Protanopia => {
                // Simulate red-blindness
                (rgb.1 * 0.567 + rgb.2 * 0.433, rgb.1, rgb.2)
            }
            ColorblindMode::Deuteranopia => {
                // Simulate green-blindness
                (rgb.0, rgb.0 * 0.625 + rgb.2 * 0.375, rgb.2)
            }
            ColorblindMode::Tritanopia => {
                // Simulate blue-blindness
                (rgb.0, rgb.1, rgb.0 * 0.95 + rgb.1 * 0.05)
            }
        }
    }

    /// Update settings
    pub fn set_settings(&mut self, settings: AccessibilitySettings) {
        self.settings = settings;
    }

    /// Get current settings
    pub fn get_settings(&self) -> &AccessibilitySettings {
        &self.settings
    }
}

impl Default for AccessibilityManager {
    fn default() -> Self {
        Self::new(AccessibilitySettings::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tts_queue() {
        let mut manager = AccessibilityManager::default();
        manager.settings.tts_enabled = true;

        manager.speak("Low priority".to_string(), TtsPriority::Low);
        manager.speak("High priority".to_string(), TtsPriority::High);
        manager.speak("Normal priority".to_string(), TtsPriority::Normal);

        // Should get high priority first
        let msg = manager.next_tts_message().unwrap();
        assert_eq!(msg.priority, TtsPriority::High);
    }

    #[test]
    fn test_subtitle_history() {
        let mut manager = AccessibilityManager::default();
        manager.settings.subtitles_enabled = true;
        manager.max_subtitle_history = 3;

        for i in 0..5 {
            manager.add_subtitle(format!("User{}", i), format!("Message {}", i), None);
        }

        assert_eq!(manager.get_subtitles().len(), 3);
    }

    #[test]
    fn test_seated_mode_adjustment() {
        let mut manager = AccessibilityManager::default();
        manager.settings.seated_mode = true;
        manager.settings.seated_eye_height = 1.2;

        let position = Vector3::new(0.0, 1.7, 0.0);
        let adjusted = manager.adjust_for_seated(position);

        assert!((adjusted.y - 1.2).abs() < 0.01);
    }

    #[test]
    fn test_colorblind_filter() {
        let manager = AccessibilityManager::default();

        let red = (1.0, 0.0, 0.0);
        let filtered = manager.apply_colorblind_filter(red);
        assert_eq!(filtered, red); // No filter by default
    }
}
