//! App manifest - defines mini-app metadata, resources, and permissions

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use url::Url;

use crate::error::{MiniAppError, MiniAppResult};
use crate::permissions::PermissionSet;
use crate::{MAX_MANIFEST_SIZE, PROTOCOL_VERSION};

/// Manifest version
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ManifestVersion {
    /// Major version
    pub major: u16,
    /// Minor version
    pub minor: u16,
}

impl ManifestVersion {
    /// Current manifest version
    pub const CURRENT: Self = Self { major: 1, minor: 0 };

    /// Create new version
    pub fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Check if compatible with current version
    pub fn is_compatible(&self) -> bool {
        self.major == Self::CURRENT.major
    }
}

impl Default for ManifestVersion {
    fn default() -> Self {
        Self::CURRENT
    }
}

impl std::fmt::Display for ManifestVersion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}.{}", self.major, self.minor)
    }
}

/// App metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppMetadata {
    /// App name
    pub name: String,
    /// Short description (max 160 chars)
    pub short_description: String,
    /// Full description (max 4000 chars)
    pub description: String,
    /// App icon URL or data URI
    pub icon: String,
    /// App category
    pub category: AppCategory,
    /// Tags for discovery
    #[serde(default)]
    pub tags: Vec<String>,
    /// Supported languages (ISO 639-1 codes)
    #[serde(default)]
    pub languages: Vec<String>,
    /// Age rating
    #[serde(default)]
    pub age_rating: AgeRating,
    /// Screenshots URLs
    #[serde(default)]
    pub screenshots: Vec<String>,
    /// Privacy policy URL
    pub privacy_policy_url: Option<String>,
    /// Terms of service URL
    pub terms_of_service_url: Option<String>,
    /// Support URL
    pub support_url: Option<String>,
}

impl AppMetadata {
    /// Validate metadata
    pub fn validate(&self) -> MiniAppResult<()> {
        if self.name.is_empty() || self.name.len() > 64 {
            return Err(MiniAppError::InvalidManifest(
                "name must be 1-64 characters".to_string(),
            ));
        }

        if self.short_description.len() > 160 {
            return Err(MiniAppError::InvalidManifest(
                "short_description must be <= 160 characters".to_string(),
            ));
        }

        if self.description.len() > 4000 {
            return Err(MiniAppError::InvalidManifest(
                "description must be <= 4000 characters".to_string(),
            ));
        }

        if self.tags.len() > 10 {
            return Err(MiniAppError::InvalidManifest(
                "maximum 10 tags allowed".to_string(),
            ));
        }

        if self.screenshots.len() > 10 {
            return Err(MiniAppError::InvalidManifest(
                "maximum 10 screenshots allowed".to_string(),
            ));
        }

        Ok(())
    }
}

/// App category
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AppCategory {
    /// Games
    Games,
    /// Social networking
    Social,
    /// Finance and payments
    Finance,
    /// Utilities
    Utilities,
    /// Entertainment
    Entertainment,
    /// Education
    Education,
    /// Productivity
    Productivity,
    /// Shopping
    Shopping,
    /// News and media
    News,
    /// Health and fitness
    Health,
    /// Other
    Other,
}

impl Default for AppCategory {
    fn default() -> Self {
        Self::Other
    }
}

/// Age rating
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AgeRating {
    /// Everyone (all ages)
    Everyone,
    /// Teen (13+)
    Teen,
    /// Mature (17+)
    Mature,
    /// Adults only (18+)
    AdultsOnly,
}

impl Default for AgeRating {
    fn default() -> Self {
        Self::Everyone
    }
}

/// Resource specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSpec {
    /// Entry point URL (relative to bundle root)
    pub entry_point: String,
    /// Additional resources to preload
    #[serde(default)]
    pub preload: Vec<String>,
    /// External domains allowed to access
    #[serde(default)]
    pub allowed_domains: Vec<String>,
    /// Content Security Policy
    pub csp: Option<String>,
}

impl ResourceSpec {
    /// Validate resource spec
    pub fn validate(&self) -> MiniAppResult<()> {
        if self.entry_point.is_empty() {
            return Err(MiniAppError::MissingRequiredField(
                "entry_point".to_string(),
            ));
        }

        // Validate entry point path
        if self.entry_point.contains("..") {
            return Err(MiniAppError::InvalidManifest(
                "entry_point cannot contain path traversal".to_string(),
            ));
        }

        // Validate allowed domains
        for domain in &self.allowed_domains {
            if domain.contains("://") {
                return Err(MiniAppError::InvalidManifest(format!(
                    "allowed_domains should not include protocol: {}",
                    domain
                )));
            }
        }

        Ok(())
    }
}

impl Default for ResourceSpec {
    fn default() -> Self {
        Self {
            entry_point: "index.html".to_string(),
            preload: Vec::new(),
            allowed_domains: Vec::new(),
            csp: None,
        }
    }
}

/// Runtime requirements
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeRequirements {
    /// Minimum protocol version
    #[serde(default)]
    pub min_protocol_version: u32,
    /// Maximum memory in MB
    #[serde(default = "default_max_memory")]
    pub max_memory_mb: u32,
    /// Maximum CPU time per request in ms
    #[serde(default = "default_max_cpu_time")]
    pub max_cpu_time_ms: u32,
    /// Required features
    #[serde(default)]
    pub required_features: Vec<String>,
    /// Requires keyboard
    #[serde(default)]
    pub requires_keyboard: bool,
    /// Requires touch
    #[serde(default)]
    pub requires_touch: bool,
    /// Minimum screen width
    pub min_screen_width: Option<u32>,
    /// Orientation preference
    #[serde(default)]
    pub orientation: Orientation,
}

fn default_max_memory() -> u32 {
    128
}

fn default_max_cpu_time() -> u32 {
    5000
}

impl Default for RuntimeRequirements {
    fn default() -> Self {
        Self {
            min_protocol_version: 1,
            max_memory_mb: default_max_memory(),
            max_cpu_time_ms: default_max_cpu_time(),
            required_features: Vec::new(),
            requires_keyboard: false,
            requires_touch: false,
            min_screen_width: None,
            orientation: Orientation::default(),
        }
    }
}

/// Screen orientation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Orientation {
    /// Any orientation
    #[default]
    Any,
    /// Portrait only
    Portrait,
    /// Landscape only
    Landscape,
}

/// Theme settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ThemeSettings {
    /// Primary color (hex)
    #[serde(default = "default_primary_color")]
    pub primary_color: String,
    /// Background color (hex)
    pub background_color: Option<String>,
    /// Header color (hex)
    pub header_color: Option<String>,
    /// Supports dark mode
    #[serde(default)]
    pub supports_dark_mode: bool,
}

fn default_primary_color() -> String {
    "#007AFF".to_string()
}

impl Default for ThemeSettings {
    fn default() -> Self {
        Self {
            primary_color: default_primary_color(),
            background_color: None,
            header_color: None,
            supports_dark_mode: false,
        }
    }
}

/// Bot configuration (if app has bot component)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotConfig {
    /// Bot username
    pub username: String,
    /// Bot display name
    pub display_name: String,
    /// Bot commands
    #[serde(default)]
    pub commands: Vec<BotCommandSpec>,
    /// Inline query support
    #[serde(default)]
    pub inline_mode: bool,
    /// Group chat support
    #[serde(default)]
    pub supports_groups: bool,
    /// Channel support
    #[serde(default)]
    pub supports_channels: bool,
}

/// Bot command specification
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCommandSpec {
    /// Command name (without /)
    pub command: String,
    /// Command description
    pub description: String,
}

/// Full app manifest
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppManifest {
    /// Manifest version
    #[serde(default)]
    pub manifest_version: ManifestVersion,
    /// App version (semver)
    pub version: String,
    /// App metadata
    pub metadata: AppMetadata,
    /// Resource specification
    #[serde(default)]
    pub resources: ResourceSpec,
    /// Runtime requirements
    #[serde(default)]
    pub runtime: RuntimeRequirements,
    /// Required permissions
    #[serde(default)]
    pub permissions: PermissionSet,
    /// Theme settings
    #[serde(default)]
    pub theme: ThemeSettings,
    /// Bot configuration (optional)
    pub bot: Option<BotConfig>,
    /// Custom metadata (for extensions)
    #[serde(default)]
    pub custom: HashMap<String, serde_json::Value>,
}

impl AppManifest {
    /// Parse manifest from JSON
    pub fn from_json(json: &str) -> MiniAppResult<Self> {
        if json.len() > MAX_MANIFEST_SIZE {
            return Err(MiniAppError::ManifestTooLarge {
                size: json.len(),
                max: MAX_MANIFEST_SIZE,
            });
        }

        let manifest: Self = serde_json::from_str(json)?;
        manifest.validate()?;
        Ok(manifest)
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> MiniAppResult<String> {
        serde_json::to_string(self).map_err(|e| MiniAppError::SerializationError(e.to_string()))
    }

    /// Serialize to JSON (pretty printed)
    pub fn to_json_pretty(&self) -> MiniAppResult<String> {
        serde_json::to_string_pretty(self)
            .map_err(|e| MiniAppError::SerializationError(e.to_string()))
    }

    /// Validate manifest
    pub fn validate(&self) -> MiniAppResult<()> {
        // Check manifest version compatibility
        if !self.manifest_version.is_compatible() {
            return Err(MiniAppError::InvalidManifestVersion(
                self.manifest_version.to_string(),
            ));
        }

        // Validate version is semver
        semver::Version::parse(&self.version)
            .map_err(|e| MiniAppError::InvalidManifestVersion(format!("invalid semver: {}", e)))?;

        // Validate metadata
        self.metadata.validate()?;

        // Validate resources
        self.resources.validate()?;

        // Validate permissions count
        if self.permissions.len() > crate::MAX_PERMISSIONS_PER_APP {
            return Err(MiniAppError::TooManyPermissions {
                count: self.permissions.len(),
                max: crate::MAX_PERMISSIONS_PER_APP,
            });
        }

        // Validate runtime requirements
        if self.runtime.min_protocol_version > PROTOCOL_VERSION {
            return Err(MiniAppError::InvalidManifest(format!(
                "requires protocol version {} but current is {}",
                self.runtime.min_protocol_version, PROTOCOL_VERSION
            )));
        }

        // Validate bot config if present
        if let Some(bot) = &self.bot {
            if bot.username.is_empty() || bot.username.len() > 32 {
                return Err(MiniAppError::InvalidManifest(
                    "bot username must be 1-32 characters".to_string(),
                ));
            }

            if bot.commands.len() > 100 {
                return Err(MiniAppError::InvalidManifest(
                    "maximum 100 bot commands allowed".to_string(),
                ));
            }
        }

        Ok(())
    }

    /// Compute content hash
    pub fn content_hash(&self) -> [u8; 32] {
        let json = self.to_json().expect("manifest serializable");
        blake3::hash(json.as_bytes()).into()
    }
}

/// Builder for creating manifests
pub struct ManifestBuilder {
    manifest: AppManifest,
}

impl ManifestBuilder {
    /// Create new builder with required fields
    pub fn new(name: &str, version: &str, description: &str) -> Self {
        Self {
            manifest: AppManifest {
                manifest_version: ManifestVersion::CURRENT,
                version: version.to_string(),
                metadata: AppMetadata {
                    name: name.to_string(),
                    short_description: description.to_string(),
                    description: description.to_string(),
                    icon: String::new(),
                    category: AppCategory::Other,
                    tags: Vec::new(),
                    languages: vec!["en".to_string()],
                    age_rating: AgeRating::Everyone,
                    screenshots: Vec::new(),
                    privacy_policy_url: None,
                    terms_of_service_url: None,
                    support_url: None,
                },
                resources: ResourceSpec::default(),
                runtime: RuntimeRequirements::default(),
                permissions: PermissionSet::default(),
                theme: ThemeSettings::default(),
                bot: None,
                custom: HashMap::new(),
            },
        }
    }

    /// Set icon
    pub fn icon(mut self, icon: &str) -> Self {
        self.manifest.metadata.icon = icon.to_string();
        self
    }

    /// Set category
    pub fn category(mut self, category: AppCategory) -> Self {
        self.manifest.metadata.category = category;
        self
    }

    /// Add tag
    pub fn tag(mut self, tag: &str) -> Self {
        self.manifest.metadata.tags.push(tag.to_string());
        self
    }

    /// Set entry point
    pub fn entry_point(mut self, entry_point: &str) -> Self {
        self.manifest.resources.entry_point = entry_point.to_string();
        self
    }

    /// Add allowed domain
    pub fn allowed_domain(mut self, domain: &str) -> Self {
        self.manifest
            .resources
            .allowed_domains
            .push(domain.to_string());
        self
    }

    /// Add permission
    pub fn permission(mut self, permission: crate::permissions::Permission) -> Self {
        self.manifest.permissions.insert(permission);
        self
    }

    /// Set theme color
    pub fn theme_color(mut self, color: &str) -> Self {
        self.manifest.theme.primary_color = color.to_string();
        self
    }

    /// Add bot config
    pub fn with_bot(mut self, username: &str, display_name: &str) -> Self {
        self.manifest.bot = Some(BotConfig {
            username: username.to_string(),
            display_name: display_name.to_string(),
            commands: Vec::new(),
            inline_mode: false,
            supports_groups: false,
            supports_channels: false,
        });
        self
    }

    /// Add custom field
    pub fn custom(mut self, key: &str, value: serde_json::Value) -> Self {
        self.manifest.custom.insert(key.to_string(), value);
        self
    }

    /// Build the manifest
    pub fn build(self) -> MiniAppResult<AppManifest> {
        self.manifest.validate()?;
        Ok(self.manifest)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_manifest_version() {
        let v = ManifestVersion::CURRENT;
        assert!(v.is_compatible());

        let v2 = ManifestVersion::new(2, 0);
        assert!(!v2.is_compatible());
    }

    #[test]
    fn test_manifest_builder() {
        let manifest = ManifestBuilder::new("Test App", "1.0.0", "A test app")
            .icon("icon.png")
            .category(AppCategory::Utilities)
            .tag("test")
            .entry_point("index.html")
            .build()
            .unwrap();

        assert_eq!(manifest.metadata.name, "Test App");
        assert_eq!(manifest.version, "1.0.0");
        assert_eq!(manifest.metadata.category, AppCategory::Utilities);
        assert_eq!(manifest.resources.entry_point, "index.html");
    }

    #[test]
    fn test_manifest_serialization() {
        let manifest = ManifestBuilder::new("Test App", "1.0.0", "A test app")
            .build()
            .unwrap();

        let json = manifest.to_json().unwrap();
        let parsed = AppManifest::from_json(&json).unwrap();

        assert_eq!(manifest.metadata.name, parsed.metadata.name);
        assert_eq!(manifest.version, parsed.version);
    }

    #[test]
    fn test_manifest_validation_empty_name() {
        let result = ManifestBuilder::new("", "1.0.0", "Description").build();

        assert!(result.is_err());
    }

    #[test]
    fn test_manifest_validation_invalid_version() {
        let mut manifest = ManifestBuilder::new("Test", "1.0.0", "Desc")
            .build()
            .unwrap();

        manifest.version = "not-semver".to_string();
        assert!(manifest.validate().is_err());
    }

    #[test]
    fn test_resource_spec_validation() {
        let mut spec = ResourceSpec::default();
        spec.entry_point = "../escape.html".to_string();
        assert!(spec.validate().is_err());
    }

    #[test]
    fn test_content_hash() {
        let m1 = ManifestBuilder::new("App", "1.0.0", "Desc")
            .build()
            .unwrap();
        let m2 = ManifestBuilder::new("App", "1.0.0", "Desc")
            .build()
            .unwrap();
        let m3 = ManifestBuilder::new("App", "1.0.1", "Desc")
            .build()
            .unwrap();

        assert_eq!(m1.content_hash(), m2.content_hash());
        assert_ne!(m1.content_hash(), m3.content_hash());
    }
}
