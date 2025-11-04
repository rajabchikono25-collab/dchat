//! Accessibility Compliance Infrastructure
//!
//! This module implements WCAG 2.1 AA+ compliance features:
//! - Screen reader support
//! - Keyboard navigation
//! - ARIA labels and roles
//! - Color contrast validation
//! - Focus management
//! - Text-to-speech (TTS) integration

pub mod tts;

use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// WCAG compliance level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum WcagLevel {
    /// Level A (minimum)
    A,
    /// Level AA (mid-range)
    AA,
    /// Level AAA (highest)
    AAA,
}

/// Accessibility role for UI elements (ARIA)
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AccessibilityRole {
    Button,
    Link,
    Heading,
    TextBox,
    List,
    ListItem,
    Navigation,
    Main,
    Complementary,
    Banner,
    ContentInfo,
    Dialog,
    Alert,
    Status,
    Checkbox,
    Radio,
    Tab,
    TabPanel,
    Menu,
    MenuItem,
}

/// UI element with accessibility metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccessibleElement {
    pub id: String,
    pub role: AccessibilityRole,
    pub label: String,
    pub description: Option<String>,
    pub is_focusable: bool,
    pub tab_index: Option<i32>,
    pub aria_attributes: HashMap<String, String>,
}

/// Color with RGB values
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Color {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Color {
    /// Create a new color
    pub fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }

    /// Calculate relative luminance (WCAG formula)
    pub fn relative_luminance(&self) -> f64 {
        let r = Self::linearize(self.r as f64 / 255.0);
        let g = Self::linearize(self.g as f64 / 255.0);
        let b = Self::linearize(self.b as f64 / 255.0);
        0.2126 * r + 0.7152 * g + 0.0722 * b
    }

    fn linearize(val: f64) -> f64 {
        if val <= 0.03928 {
            val / 12.92
        } else {
            ((val + 0.055) / 1.055).powf(2.4)
        }
    }
}

/// Keyboard shortcut definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyboardShortcut {
    pub id: String,
    pub key: String,
    pub modifiers: Vec<String>,
    pub action: String,
    pub description: String,
}

/// Screen reader announcement
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub enum AnnouncementPriority {
    /// Polite - wait for current speech to finish
    Polite,
    /// Assertive - interrupt current speech
    Assertive,
    /// Off - don't announce
    Off,
}

/// Accessibility manager
pub struct AccessibilityManager {
    elements: HashMap<String, AccessibleElement>,
    shortcuts: Vec<KeyboardShortcut>,
    pub tts: tts::TtsEngine,
}

impl AccessibilityManager {
    /// Create a new accessibility manager
    pub fn new() -> Self {
        Self {
            elements: HashMap::new(),
            shortcuts: Vec::new(),
            tts: tts::TtsEngine::new(),
        }
    }

    /// Register an accessible element
    pub fn register_element(&mut self, element: AccessibleElement) -> Result<()> {
        if element.label.is_empty() {
            return Err(Error::validation("Element must have a label"));
        }

        self.elements.insert(element.id.clone(), element);
        Ok(())
    }

    /// Get element by ID
    pub fn get_element(&self, id: &str) -> Option<&AccessibleElement> {
        self.elements.get(id)
    }

    /// Update element label
    pub fn update_label(&mut self, id: &str, new_label: String) -> Result<()> {
        let element = self
            .elements
            .get_mut(id)
            .ok_or_else(|| Error::validation("Element not found"))?;

        element.label = new_label;
        Ok(())
    }

    /// Add ARIA attribute to element
    pub fn add_aria_attribute(&mut self, id: &str, key: String, value: String) -> Result<()> {
        let element = self
            .elements
            .get_mut(id)
            .ok_or_else(|| Error::validation("Element not found"))?;

        element.aria_attributes.insert(key, value);
        Ok(())
    }

    /// Register keyboard shortcut
    pub fn register_shortcut(&mut self, shortcut: KeyboardShortcut) -> Result<()> {
        // Check for duplicate shortcuts
        let conflict = self
            .shortcuts
            .iter()
            .any(|s| s.key == shortcut.key && s.modifiers == shortcut.modifiers);

        if conflict {
            return Err(Error::validation("Shortcut already registered"));
        }

        self.shortcuts.push(shortcut);
        Ok(())
    }

    /// Get all keyboard shortcuts
    pub fn get_shortcuts(&self) -> &[KeyboardShortcut] {
        &self.shortcuts
    }

    /// Calculate contrast ratio between two colors
    pub fn contrast_ratio(color1: &Color, color2: &Color) -> f64 {
        let l1 = color1.relative_luminance();
        let l2 = color2.relative_luminance();

        let lighter = l1.max(l2);
        let darker = l1.min(l2);

        (lighter + 0.05) / (darker + 0.05)
    }

    /// Check if contrast ratio meets WCAG level
    pub fn check_contrast(
        foreground: &Color,
        background: &Color,
        level: WcagLevel,
        is_large_text: bool,
    ) -> bool {
        let ratio = Self::contrast_ratio(foreground, background);

        match level {
            WcagLevel::A => true, // No specific requirement
            WcagLevel::AA => {
                if is_large_text {
                    ratio >= 3.0
                } else {
                    ratio >= 4.5
                }
            }
            WcagLevel::AAA => {
                if is_large_text {
                    ratio >= 4.5
                } else {
                    ratio >= 7.0
                }
            }
        }
    }

    /// Get focusable elements in tab order
    pub fn get_focus_order(&self) -> Vec<&AccessibleElement> {
        let mut focusable: Vec<_> = self.elements.values().filter(|e| e.is_focusable).collect();

        focusable.sort_by_key(|e| e.tab_index.unwrap_or(0));
        focusable
    }

    /// Validate element accessibility
    pub fn validate_element(&self, id: &str) -> Vec<String> {
        let mut issues = Vec::new();

        if let Some(element) = self.elements.get(id) {
            if element.label.is_empty() {
                issues.push("Element missing label".to_string());
            }

            if element.is_focusable && element.tab_index.is_none() {
                issues.push("Focusable element missing tab index".to_string());
            }

            // Check for proper ARIA attributes based on role
            match element.role {
                AccessibilityRole::Button => {
                    if !element.aria_attributes.contains_key("aria-label")
                        && element.label.is_empty()
                    {
                        issues.push("Button missing accessible name".to_string());
                    }
                }
                AccessibilityRole::Dialog => {
                    if !element.aria_attributes.contains_key("aria-labelledby") {
                        issues.push("Dialog missing aria-labelledby".to_string());
                    }
                }
                _ => {}
            }
        }

        issues
    }
}

impl Default for AccessibilityManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_register_element() {
        let mut manager = AccessibilityManager::new();

        let element = AccessibleElement {
            id: "btn1".to_string(),
            role: AccessibilityRole::Button,
            label: "Submit".to_string(),
            description: Some("Submit the form".to_string()),
            is_focusable: true,
            tab_index: Some(1),
            aria_attributes: HashMap::new(),
        };

        manager.register_element(element).unwrap();

        let retrieved = manager.get_element("btn1").unwrap();
        assert_eq!(retrieved.label, "Submit");
    }

    #[test]
    fn test_empty_label_rejected() {
        let mut manager = AccessibilityManager::new();

        let element = AccessibleElement {
            id: "btn2".to_string(),
            role: AccessibilityRole::Button,
            label: "".to_string(),
            description: None,
            is_focusable: true,
            tab_index: Some(1),
            aria_attributes: HashMap::new(),
        };

        let result = manager.register_element(element);
        assert!(result.is_err());
    }

    #[test]
    fn test_update_label() {
        let mut manager = AccessibilityManager::new();

        let element = AccessibleElement {
            id: "link1".to_string(),
            role: AccessibilityRole::Link,
            label: "Old Label".to_string(),
            description: None,
            is_focusable: true,
            tab_index: Some(2),
            aria_attributes: HashMap::new(),
        };

        manager.register_element(element).unwrap();
        manager
            .update_label("link1", "New Label".to_string())
            .unwrap();

        let updated = manager.get_element("link1").unwrap();
        assert_eq!(updated.label, "New Label");
    }

    #[test]
    fn test_add_aria_attribute() {
        let mut manager = AccessibilityManager::new();

        let element = AccessibleElement {
            id: "div1".to_string(),
            role: AccessibilityRole::Main,
            label: "Main Content".to_string(),
            description: None,
            is_focusable: false,
            tab_index: None,
            aria_attributes: HashMap::new(),
        };

        manager.register_element(element).unwrap();
        manager
            .add_aria_attribute("div1", "aria-live".to_string(), "polite".to_string())
            .unwrap();

        let updated = manager.get_element("div1").unwrap();
        assert_eq!(updated.aria_attributes.get("aria-live").unwrap(), "polite");
    }

    #[test]
    fn test_register_shortcut() {
        let mut manager = AccessibilityManager::new();

        let shortcut = KeyboardShortcut {
            id: "save".to_string(),
            key: "S".to_string(),
            modifiers: vec!["Ctrl".to_string()],
            action: "save_document".to_string(),
            description: "Save the current document".to_string(),
        };

        manager.register_shortcut(shortcut).unwrap();

        let shortcuts = manager.get_shortcuts();
        assert_eq!(shortcuts.len(), 1);
        assert_eq!(shortcuts[0].key, "S");
    }

    #[test]
    fn test_duplicate_shortcut_rejected() {
        let mut manager = AccessibilityManager::new();

        let shortcut1 = KeyboardShortcut {
            id: "action1".to_string(),
            key: "C".to_string(),
            modifiers: vec!["Ctrl".to_string()],
            action: "copy".to_string(),
            description: "Copy".to_string(),
        };

        let shortcut2 = KeyboardShortcut {
            id: "action2".to_string(),
            key: "C".to_string(),
            modifiers: vec!["Ctrl".to_string()],
            action: "something_else".to_string(),
            description: "Other action".to_string(),
        };

        manager.register_shortcut(shortcut1).unwrap();
        let result = manager.register_shortcut(shortcut2);
        assert!(result.is_err());
    }

    #[test]
    fn test_contrast_ratio() {
        let black = Color::new(0, 0, 0);
        let white = Color::new(255, 255, 255);

        let ratio = AccessibilityManager::contrast_ratio(&black, &white);
        assert!(ratio > 20.0); // Should be 21:1
    }

    #[test]
    fn test_check_contrast_aa() {
        let black = Color::new(0, 0, 0);
        let white = Color::new(255, 255, 255);

        let passes = AccessibilityManager::check_contrast(&black, &white, WcagLevel::AA, false);
        assert!(passes);
    }

    #[test]
    fn test_check_contrast_fails() {
        let light_gray = Color::new(200, 200, 200);
        let white = Color::new(255, 255, 255);

        let passes =
            AccessibilityManager::check_contrast(&light_gray, &white, WcagLevel::AA, false);
        assert!(!passes);
    }

    #[test]
    fn test_get_focus_order() {
        let mut manager = AccessibilityManager::new();

        let elem1 = AccessibleElement {
            id: "elem1".to_string(),
            role: AccessibilityRole::Button,
            label: "First".to_string(),
            description: None,
            is_focusable: true,
            tab_index: Some(2),
            aria_attributes: HashMap::new(),
        };

        let elem2 = AccessibleElement {
            id: "elem2".to_string(),
            role: AccessibilityRole::Button,
            label: "Second".to_string(),
            description: None,
            is_focusable: true,
            tab_index: Some(1),
            aria_attributes: HashMap::new(),
        };

        manager.register_element(elem1).unwrap();
        manager.register_element(elem2).unwrap();

        let focus_order = manager.get_focus_order();
        assert_eq!(focus_order[0].id, "elem2"); // Tab index 1 comes first
        assert_eq!(focus_order[1].id, "elem1"); // Tab index 2 comes second
    }

    #[test]
    fn test_validate_element() {
        let mut manager = AccessibilityManager::new();

        let element = AccessibleElement {
            id: "btn_test".to_string(),
            role: AccessibilityRole::Button,
            label: "Test Button".to_string(), // Has label but missing other attributes
            description: None,
            is_focusable: true,
            tab_index: None,                 // Missing tab index
            aria_attributes: HashMap::new(), // Missing aria-label
        };

        manager.register_element(element).unwrap();

        let issues = manager.validate_element("btn_test");
        assert!(!issues.is_empty()); // Should have issue about missing tab index
    }
}
