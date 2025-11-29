//! User profile management with pictures, descriptions, and status

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// User profile
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    /// User ID
    pub user_id: dchat_core::types::UserId,

    /// Username
    pub username: String,

    /// Display name
    pub display_name: String,

    /// Bio/description
    pub bio: Option<String>,

    /// Profile picture
    pub profile_picture: Option<ProfilePicture>,

    /// Current status
    pub status: Option<UserStatus>,

    /// Online status
    pub online_status: OnlineStatus,

    /// Last seen timestamp
    pub last_seen: Option<DateTime<Utc>>,

    /// Account creation date
    pub created_at: DateTime<Utc>,

    /// Privacy settings
    pub privacy: PrivacySettings,

    /// Verified badge
    pub is_verified: bool,

    /// Additional metadata
    pub metadata: HashMap<String, String>,
}

/// Profile picture
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProfilePicture {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Small thumbnail (160x160)
    pub small_file_id: Option<String>,

    /// Large thumbnail (640x640)
    pub large_file_id: Option<String>,

    /// Upload timestamp
    pub uploaded_at: DateTime<Utc>,
}

/// User status (like WhatsApp/Telegram stories)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserStatus {
    /// Status ID
    pub id: Uuid,

    /// Status type
    pub status_type: StatusType,

    /// Caption/text
    pub caption: Option<String>,

    /// Background color (hex)
    pub background_color: Option<String>,

    /// Created timestamp
    pub created_at: DateTime<Utc>,

    /// Expiration timestamp (24 hours by default)
    pub expires_at: DateTime<Utc>,

    /// View count
    pub view_count: u64,

    /// Who viewed (if privacy allows)
    pub viewers: Vec<dchat_core::types::UserId>,
}

/// Status type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StatusType {
    /// Text only status
    Text { text: String, font: Option<String> },

    /// Image status
    Image {
        file_id: String,
        width: u32,
        height: u32,
    },

    /// Video status
    Video {
        file_id: String,
        width: u32,
        height: u32,
        duration: u32,
    },

    /// Audio status (audio with image background)
    Audio {
        audio_file_id: String,
        background_image_id: Option<String>,
        duration: u32,
        title: Option<String>,
        artist: Option<String>,
        /// Spotify/Apple Music track ID for public music
        music_api_track_id: Option<MusicApiTrack>,
    },
}

/// Music API track reference
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MusicApiTrack {
    /// API provider
    pub provider: MusicProvider,

    /// Track ID
    pub track_id: String,

    /// Track name
    pub track_name: String,

    /// Artist name
    pub artist_name: String,

    /// Album name
    pub album_name: Option<String>,

    /// Album art URL
    pub album_art_url: Option<String>,

    /// Preview URL (30-second snippet)
    pub preview_url: Option<String>,
}

/// Music streaming provider
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MusicProvider {
    Spotify,
    AppleMusic,
    YouTubeMusic,
    SoundCloud,
    Deezer,
}

/// Online status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum OnlineStatus {
    Online,
    Offline,
    Away,
    DoNotDisturb,
}

/// Privacy settings
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacySettings {
    /// Who can see profile picture
    pub profile_picture_visibility: VisibilityLevel,

    /// Who can see status
    pub status_visibility: VisibilityLevel,

    /// Who can see last seen
    pub last_seen_visibility: VisibilityLevel,

    /// Who can see bio
    pub bio_visibility: VisibilityLevel,

    /// Who can message
    pub message_visibility: VisibilityLevel,
}

/// Visibility level
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum VisibilityLevel {
    Everyone,
    Contacts,
    Nobody,
    Custom {
        allowed: Vec<dchat_core::types::UserId>,
        blocked: Vec<dchat_core::types::UserId>,
    },
}

impl Default for PrivacySettings {
    fn default() -> Self {
        Self {
            profile_picture_visibility: VisibilityLevel::Everyone,
            status_visibility: VisibilityLevel::Everyone,
            last_seen_visibility: VisibilityLevel::Contacts,
            bio_visibility: VisibilityLevel::Everyone,
            message_visibility: VisibilityLevel::Everyone,
        }
    }
}

/// Maximum number of statuses per user
#[allow(dead_code)]
const MAX_STATUSES_PER_USER: usize = 30;
/// Maximum status caption length
#[allow(dead_code)]
const MAX_CAPTION_LENGTH: usize = 500;
/// Maximum viewers tracked per status
const MAX_VIEWERS_PER_STATUS: usize = 10000;
/// Maximum profiles to return in search
const MAX_SEARCH_RESULTS: usize = 50;

/// Profile manager
pub struct ProfileManager {
    profiles: Arc<RwLock<HashMap<dchat_core::types::UserId, UserProfile>>>,
    username_index: Arc<RwLock<HashMap<String, dchat_core::types::UserId>>>,
}

impl ProfileManager {
    /// Create new profile manager
    pub fn new() -> Self {
        Self {
            profiles: Arc::new(RwLock::new(HashMap::new())),
            username_index: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create or update user profile
    pub fn update_profile(&self, profile: UserProfile) -> Result<()> {
        let user_id = profile.user_id.clone();
        let username = profile.username.clone();

        // Check if username is taken by another user
        {
            let index = self
                .username_index
                .read()
                .map_err(|_| Error::internal("Failed to acquire read lock"))?;

            if let Some(existing_id) = index.get(&username) {
                if existing_id != &user_id {
                    return Err(Error::validation("Username already taken"));
                }
            }
        }

        // Update profile
        {
            let mut profiles = self
                .profiles
                .write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;

            profiles.insert(user_id.clone(), profile);
        }

        // Update username index
        {
            let mut index = self
                .username_index
                .write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;

            index.insert(username, user_id);
        }

        Ok(())
    }

    /// Get profile by user ID
    pub fn get_profile(&self, user_id: &dchat_core::types::UserId) -> Option<UserProfile> {
        self.profiles.read().ok()?.get(user_id).cloned()
    }

    /// Get profile by username
    pub fn get_profile_by_username(&self, username: &str) -> Option<UserProfile> {
        let index = self.username_index.read().ok()?;
        let user_id = index.get(username)?;
        self.get_profile(user_id)
    }

    /// Search profiles by username prefix
    /// 
    /// # Security
    /// - Results are limited to prevent resource exhaustion
    pub fn search_profiles(&self, query: &str) -> Vec<UserProfile> {
        let profiles = match self.profiles.read() {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };
        
        // Limit query length
        let query_lower: String = query.chars().take(100).collect::<String>().to_lowercase();
        if query_lower.is_empty() {
            return Vec::new();
        }

        profiles
            .values()
            .filter(|p| {
                p.username.to_lowercase().contains(&query_lower)
                    || p.display_name.to_lowercase().contains(&query_lower)
            })
            .take(MAX_SEARCH_RESULTS)
            .cloned()
            .collect()
    }

    /// Set profile picture
    pub fn set_profile_picture(
        &self,
        user_id: &dchat_core::types::UserId,
        picture: ProfilePicture,
    ) -> Result<()> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;

        let profile = profiles
            .get_mut(user_id)
            .ok_or_else(|| Error::validation("Profile not found"))?;

        profile.profile_picture = Some(picture);

        Ok(())
    }

    /// Update bio
    pub fn update_bio(
        &self,
        user_id: &dchat_core::types::UserId,
        bio: Option<String>,
    ) -> Result<()> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;

        let profile = profiles
            .get_mut(user_id)
            .ok_or_else(|| Error::validation("Profile not found"))?;

        profile.bio = bio;

        Ok(())
    }

    /// Set user status
    pub fn set_status(
        &self,
        user_id: &dchat_core::types::UserId,
        status: UserStatus,
    ) -> Result<()> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;

        let profile = profiles
            .get_mut(user_id)
            .ok_or_else(|| Error::validation("Profile not found"))?;

        profile.status = Some(status);

        Ok(())
    }

    /// Get active statuses (not expired)
    pub fn get_active_statuses(&self) -> Vec<(dchat_core::types::UserId, UserStatus)> {
        let profiles = match self.profiles.read() {
            Ok(p) => p,
            Err(_) => return Vec::new(),
        };

        let now = Utc::now();

        profiles
            .iter()
            .filter_map(|(user_id, profile)| {
                profile.status.as_ref().and_then(|status| {
                    if status.expires_at > now {
                        Some((user_id.clone(), status.clone()))
                    } else {
                        None
                    }
                })
            })
            .collect()
    }

    /// Increment status view count
    /// 
    /// # Security
    /// - Limits viewer list size to prevent memory exhaustion
    pub fn view_status(
        &self,
        status_owner: &dchat_core::types::UserId,
        viewer: &dchat_core::types::UserId,
    ) -> Result<()> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;

        let profile = profiles
            .get_mut(status_owner)
            .ok_or_else(|| Error::validation("Profile not found"))?;

        if let Some(status) = &mut profile.status {
            status.view_count += 1;
            // Limit viewer list to prevent memory exhaustion
            if status.viewers.len() < MAX_VIEWERS_PER_STATUS && !status.viewers.contains(viewer) {
                status.viewers.push(viewer.clone());
            }
        }

        Ok(())
    }

    /// Update online status
    pub fn update_online_status(
        &self,
        user_id: &dchat_core::types::UserId,
        online_status: OnlineStatus,
    ) -> Result<()> {
        let mut profiles = self
            .profiles
            .write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;

        let profile = profiles
            .get_mut(user_id)
            .ok_or_else(|| Error::validation("Profile not found"))?;

        profile.online_status = online_status;

        if matches!(profile.online_status, OnlineStatus::Offline) {
            profile.last_seen = Some(Utc::now());
        }

        Ok(())
    }
}

impl Default for ProfileManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_and_get_profile() {
        let manager = ProfileManager::new();
        let user_id = dchat_core::types::UserId::new();

        let profile = UserProfile {
            user_id: user_id.clone(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            bio: Some("Hello world!".to_string()),
            profile_picture: None,
            status: None,
            online_status: OnlineStatus::Online,
            last_seen: None,
            created_at: Utc::now(),
            privacy: PrivacySettings::default(),
            is_verified: false,
            metadata: HashMap::new(),
        };

        manager.update_profile(profile.clone()).unwrap();

        let retrieved = manager.get_profile(&user_id).unwrap();
        assert_eq!(retrieved.username, "testuser");
        assert_eq!(retrieved.display_name, "Test User");
    }

    #[test]
    fn test_search_profiles() {
        let manager = ProfileManager::new();

        for i in 1..=5 {
            let user_id = dchat_core::types::UserId::new();
            let profile = UserProfile {
                user_id: user_id.clone(),
                username: format!("user{}", i),
                display_name: format!("User {}", i),
                bio: None,
                profile_picture: None,
                status: None,
                online_status: OnlineStatus::Online,
                last_seen: None,
                created_at: Utc::now(),
                privacy: PrivacySettings::default(),
                is_verified: false,
                metadata: HashMap::new(),
            };
            manager.update_profile(profile).unwrap();
        }

        let results = manager.search_profiles("user");
        assert_eq!(results.len(), 5);

        let results = manager.search_profiles("user3");
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].username, "user3");
    }

    #[test]
    fn test_set_status() {
        let manager = ProfileManager::new();
        let user_id = dchat_core::types::UserId::new();

        let profile = UserProfile {
            user_id: user_id.clone(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            bio: None,
            profile_picture: None,
            status: None,
            online_status: OnlineStatus::Online,
            last_seen: None,
            created_at: Utc::now(),
            privacy: PrivacySettings::default(),
            is_verified: false,
            metadata: HashMap::new(),
        };

        manager.update_profile(profile).unwrap();

        let status = UserStatus {
            id: Uuid::new_v4(),
            status_type: StatusType::Text {
                text: "Hello!".to_string(),
                font: Some("Arial".to_string()),
            },
            caption: None,
            background_color: Some("#FF5733".to_string()),
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
            view_count: 0,
            viewers: Vec::new(),
        };

        manager.set_status(&user_id, status.clone()).unwrap();

        let profile = manager.get_profile(&user_id).unwrap();
        assert!(profile.status.is_some());
        assert_eq!(profile.status.unwrap().id, status.id);
    }

    #[test]
    fn test_music_status() {
        let status = UserStatus {
            id: Uuid::new_v4(),
            status_type: StatusType::Audio {
                audio_file_id: "audio123".to_string(),
                background_image_id: Some("bg456".to_string()),
                duration: 180,
                title: Some("My Favorite Song".to_string()),
                artist: Some("The Artist".to_string()),
                music_api_track_id: Some(MusicApiTrack {
                    provider: MusicProvider::Spotify,
                    track_id: "spotify:track:123456".to_string(),
                    track_name: "My Favorite Song".to_string(),
                    artist_name: "The Artist".to_string(),
                    album_name: Some("The Album".to_string()),
                    album_art_url: Some("https://album-art.com/img.jpg".to_string()),
                    preview_url: Some("https://preview.com/30s.mp3".to_string()),
                }),
            },
            caption: Some("Check out this track!".to_string()),
            background_color: None,
            created_at: Utc::now(),
            expires_at: Utc::now() + chrono::Duration::hours(24),
            view_count: 0,
            viewers: Vec::new(),
        };

        match status.status_type {
            StatusType::Audio {
                ref music_api_track_id,
                ..
            } => {
                assert!(music_api_track_id.is_some());
                let track = music_api_track_id.as_ref().unwrap();
                assert_eq!(track.provider, MusicProvider::Spotify);
                assert_eq!(track.track_name, "My Favorite Song");
            }
            _ => panic!("Test failed: Expected Audio status but got different status type"),
        }
    }
}
