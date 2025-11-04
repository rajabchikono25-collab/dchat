//! Database storage for user profiles and statuses

use crate::profile::{
    OnlineStatus, PrivacySettings, ProfilePicture, StatusType, UserProfile, UserStatus,
    VisibilityLevel,
};
use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use sqlx::{Row, SqlitePool};
use std::collections::HashMap;
use uuid::Uuid;

/// Profile storage
pub struct ProfileStorage {
    pool: SqlitePool,
}

impl ProfileStorage {
    /// Create new profile storage
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Initialize database schema
    pub async fn init_schema(&self) -> Result<()> {
        // User profiles table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_profiles (
                user_id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                bio TEXT,
                profile_picture_file_id TEXT,
                profile_picture_unique_id TEXT,
                profile_picture_small TEXT,
                profile_picture_large TEXT,
                profile_picture_uploaded_at TEXT,
                online_status TEXT NOT NULL DEFAULT 'Offline',
                last_seen TEXT,
                created_at TEXT NOT NULL,
                is_verified BOOLEAN NOT NULL DEFAULT FALSE,
                metadata TEXT
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create user_profiles table: {}", e)))?;

        // Privacy settings table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS profile_privacy (
                user_id TEXT PRIMARY KEY,
                profile_picture_visibility TEXT NOT NULL DEFAULT 'Everyone',
                profile_picture_allowed TEXT,
                profile_picture_blocked TEXT,
                status_visibility TEXT NOT NULL DEFAULT 'Everyone',
                status_allowed TEXT,
                status_blocked TEXT,
                last_seen_visibility TEXT NOT NULL DEFAULT 'Everyone',
                last_seen_allowed TEXT,
                last_seen_blocked TEXT,
                bio_visibility TEXT NOT NULL DEFAULT 'Everyone',
                bio_allowed TEXT,
                bio_blocked TEXT,
                message_visibility TEXT NOT NULL DEFAULT 'Everyone',
                message_allowed TEXT,
                message_blocked TEXT,
                FOREIGN KEY (user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create profile_privacy table: {}", e)))?;

        // User statuses table
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS user_statuses (
                id TEXT PRIMARY KEY,
                user_id TEXT NOT NULL,
                status_type TEXT NOT NULL,
                status_data TEXT NOT NULL,
                caption TEXT,
                background_color TEXT,
                created_at TEXT NOT NULL,
                expires_at TEXT NOT NULL,
                view_count INTEGER NOT NULL DEFAULT 0,
                viewers TEXT,
                FOREIGN KEY (user_id) REFERENCES user_profiles(user_id) ON DELETE CASCADE
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create user_statuses table: {}", e)))?;

        // Create indices
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_profiles_username ON user_profiles(username)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create username index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_statuses_user ON user_statuses(user_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create status user index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_statuses_expires ON user_statuses(expires_at)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create status expiry index: {}", e)))?;

        Ok(())
    }

    /// Save or update user profile
    pub async fn save_profile(&self, profile: &UserProfile) -> Result<()> {
        let metadata_json = serde_json::to_string(&profile.metadata)
            .map_err(|e| Error::storage(format!("Failed to serialize metadata: {}", e)))?;

        let (pic_file_id, pic_unique_id, pic_small, pic_large, pic_uploaded) =
            if let Some(ref pic) = profile.profile_picture {
                (
                    Some(pic.file_id.clone()),
                    Some(pic.file_unique_id.clone()),
                    pic.small_file_id.clone(),
                    pic.large_file_id.clone(),
                    Some(pic.uploaded_at.to_rfc3339()),
                )
            } else {
                (None, None, None, None, None)
            };

        sqlx::query(
            r#"
            INSERT INTO user_profiles (
                user_id, username, display_name, bio,
                profile_picture_file_id, profile_picture_unique_id,
                profile_picture_small, profile_picture_large,
                profile_picture_uploaded_at, online_status, last_seen,
                created_at, is_verified, metadata
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                username = excluded.username,
                display_name = excluded.display_name,
                bio = excluded.bio,
                profile_picture_file_id = excluded.profile_picture_file_id,
                profile_picture_unique_id = excluded.profile_picture_unique_id,
                profile_picture_small = excluded.profile_picture_small,
                profile_picture_large = excluded.profile_picture_large,
                profile_picture_uploaded_at = excluded.profile_picture_uploaded_at,
                online_status = excluded.online_status,
                last_seen = excluded.last_seen,
                is_verified = excluded.is_verified,
                metadata = excluded.metadata
            "#,
        )
        .bind(profile.user_id.to_string())
        .bind(&profile.username)
        .bind(&profile.display_name)
        .bind(&profile.bio)
        .bind(&pic_file_id)
        .bind(&pic_unique_id)
        .bind(&pic_small)
        .bind(&pic_large)
        .bind(&pic_uploaded)
        .bind(format!("{:?}", profile.online_status))
        .bind(profile.last_seen.as_ref().map(|dt| dt.to_rfc3339()))
        .bind(profile.created_at.to_rfc3339())
        .bind(profile.is_verified)
        .bind(&metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to save profile: {}", e)))?;

        // Save privacy settings
        self.save_privacy_settings(&profile.user_id, &profile.privacy)
            .await?;

        Ok(())
    }

    /// Save privacy settings
    async fn save_privacy_settings(
        &self,
        user_id: &dchat_core::types::UserId,
        privacy: &PrivacySettings,
    ) -> Result<()> {
        let (pic_vis, pic_allowed, pic_blocked) =
            serialize_visibility(&privacy.profile_picture_visibility);
        let (status_vis, status_allowed, status_blocked) =
            serialize_visibility(&privacy.status_visibility);
        let (last_seen_vis, last_seen_allowed, last_seen_blocked) =
            serialize_visibility(&privacy.last_seen_visibility);
        let (bio_vis, bio_allowed, bio_blocked) = serialize_visibility(&privacy.bio_visibility);
        let (msg_vis, msg_allowed, msg_blocked) = serialize_visibility(&privacy.message_visibility);

        sqlx::query(
            r#"
            INSERT INTO profile_privacy (
                user_id, profile_picture_visibility, profile_picture_allowed, profile_picture_blocked,
                status_visibility, status_allowed, status_blocked,
                last_seen_visibility, last_seen_allowed, last_seen_blocked,
                bio_visibility, bio_allowed, bio_blocked,
                message_visibility, message_allowed, message_blocked
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(user_id) DO UPDATE SET
                profile_picture_visibility = excluded.profile_picture_visibility,
                profile_picture_allowed = excluded.profile_picture_allowed,
                profile_picture_blocked = excluded.profile_picture_blocked,
                status_visibility = excluded.status_visibility,
                status_allowed = excluded.status_allowed,
                status_blocked = excluded.status_blocked,
                last_seen_visibility = excluded.last_seen_visibility,
                last_seen_allowed = excluded.last_seen_allowed,
                last_seen_blocked = excluded.last_seen_blocked,
                bio_visibility = excluded.bio_visibility,
                bio_allowed = excluded.bio_allowed,
                bio_blocked = excluded.bio_blocked,
                message_visibility = excluded.message_visibility,
                message_allowed = excluded.message_allowed,
                message_blocked = excluded.message_blocked
            "#,
        )
        .bind(user_id.to_string())
        .bind(&pic_vis)
        .bind(&pic_allowed)
        .bind(&pic_blocked)
        .bind(&status_vis)
        .bind(&status_allowed)
        .bind(&status_blocked)
        .bind(&last_seen_vis)
        .bind(&last_seen_allowed)
        .bind(&last_seen_blocked)
        .bind(&bio_vis)
        .bind(&bio_allowed)
        .bind(&bio_blocked)
        .bind(&msg_vis)
        .bind(&msg_allowed)
        .bind(&msg_blocked)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to save privacy settings: {}", e)))?;

        Ok(())
    }

    /// Get user profile by user ID
    pub async fn get_profile(
        &self,
        user_id: &dchat_core::types::UserId,
    ) -> Result<Option<UserProfile>> {
        let row = sqlx::query(
            r#"
            SELECT * FROM user_profiles WHERE user_id = ?
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch profile: {}", e)))?;

        if let Some(row) = row {
            let privacy = self.get_privacy_settings(user_id).await?;
            Ok(Some(parse_profile_row(row, privacy)?))
        } else {
            Ok(None)
        }
    }

    /// Get user profile by username
    pub async fn get_profile_by_username(&self, username: &str) -> Result<Option<UserProfile>> {
        let row = sqlx::query(
            r#"
            SELECT * FROM user_profiles WHERE username = ?
            "#,
        )
        .bind(username)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch profile by username: {}", e)))?;

        if let Some(row) = row {
            let user_id_str: String = row.get("user_id");
            let user_id = dchat_core::types::UserId(
                uuid::Uuid::parse_str(&user_id_str)
                    .map_err(|e| Error::storage(format!("Failed to parse user_id: {}", e)))?,
            );
            let privacy = self.get_privacy_settings(&user_id).await?;
            Ok(Some(parse_profile_row(row, privacy)?))
        } else {
            Ok(None)
        }
    }

    /// Get privacy settings
    async fn get_privacy_settings(
        &self,
        user_id: &dchat_core::types::UserId,
    ) -> Result<PrivacySettings> {
        let row = sqlx::query(
            r#"
            SELECT * FROM profile_privacy WHERE user_id = ?
            "#,
        )
        .bind(user_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch privacy settings: {}", e)))?;

        if let Some(row) = row {
            Ok(PrivacySettings {
                profile_picture_visibility: parse_visibility(
                    row.get("profile_picture_visibility"),
                    row.get("profile_picture_allowed"),
                    row.get("profile_picture_blocked"),
                )?,
                status_visibility: parse_visibility(
                    row.get("status_visibility"),
                    row.get("status_allowed"),
                    row.get("status_blocked"),
                )?,
                last_seen_visibility: parse_visibility(
                    row.get("last_seen_visibility"),
                    row.get("last_seen_allowed"),
                    row.get("last_seen_blocked"),
                )?,
                bio_visibility: parse_visibility(
                    row.get("bio_visibility"),
                    row.get("bio_allowed"),
                    row.get("bio_blocked"),
                )?,
                message_visibility: parse_visibility(
                    row.get("message_visibility"),
                    row.get("message_allowed"),
                    row.get("message_blocked"),
                )?,
            })
        } else {
            Ok(PrivacySettings::default())
        }
    }

    /// Save user status
    pub async fn save_status(
        &self,
        user_id: &dchat_core::types::UserId,
        status: &UserStatus,
    ) -> Result<()> {
        let (status_type, status_data) = serialize_status_type(&status.status_type)?;
        let viewers_json = serde_json::to_string(&status.viewers)
            .map_err(|e| Error::storage(format!("Failed to serialize viewers: {}", e)))?;

        sqlx::query(
            r#"
            INSERT INTO user_statuses (
                id, user_id, status_type, status_data, caption,
                background_color, created_at, expires_at, view_count, viewers
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT(id) DO UPDATE SET
                view_count = excluded.view_count,
                viewers = excluded.viewers
            "#,
        )
        .bind(status.id.to_string())
        .bind(user_id.to_string())
        .bind(&status_type)
        .bind(&status_data)
        .bind(&status.caption)
        .bind(&status.background_color)
        .bind(status.created_at.to_rfc3339())
        .bind(status.expires_at.to_rfc3339())
        .bind(status.view_count as i64)
        .bind(&viewers_json)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to save status: {}", e)))?;

        Ok(())
    }

    /// Get active statuses for a user
    pub async fn get_active_statuses(
        &self,
        user_id: &dchat_core::types::UserId,
    ) -> Result<Vec<UserStatus>> {
        let now = Utc::now().to_rfc3339();

        let rows = sqlx::query(
            r#"
            SELECT * FROM user_statuses
            WHERE user_id = ? AND expires_at > ?
            ORDER BY created_at DESC
            "#,
        )
        .bind(user_id.to_string())
        .bind(&now)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch statuses: {}", e)))?;

        rows.into_iter().map(parse_status_row).collect()
    }

    /// Delete expired statuses
    pub async fn cleanup_expired_statuses(&self) -> Result<u64> {
        let now = Utc::now().to_rfc3339();

        let result = sqlx::query(
            r#"
            DELETE FROM user_statuses WHERE expires_at <= ?
            "#,
        )
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to cleanup statuses: {}", e)))?;

        Ok(result.rows_affected())
    }

    /// Search profiles by query
    pub async fn search_profiles(&self, query: &str, limit: usize) -> Result<Vec<UserProfile>> {
        let query_pattern = format!("%{}%", query);

        let rows = sqlx::query(
            r#"
            SELECT * FROM user_profiles
            WHERE username LIKE ? OR display_name LIKE ? OR bio LIKE ?
            ORDER BY is_verified DESC, username ASC
            LIMIT ?
            "#,
        )
        .bind(&query_pattern)
        .bind(&query_pattern)
        .bind(&query_pattern)
        .bind(limit as i64)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to search profiles: {}", e)))?;

        let mut profiles = Vec::new();
        for row in rows {
            let user_id_str: String = row.get("user_id");
            let user_id = dchat_core::types::UserId(
                uuid::Uuid::parse_str(&user_id_str)
                    .map_err(|e| Error::storage(format!("Failed to parse user_id: {}", e)))?,
            );
            let privacy = self.get_privacy_settings(&user_id).await?;
            profiles.push(parse_profile_row(row, privacy)?);
        }

        Ok(profiles)
    }
}

// Helper functions

fn serialize_visibility(vis: &VisibilityLevel) -> (String, Option<String>, Option<String>) {
    match vis {
        VisibilityLevel::Everyone => ("Everyone".to_string(), None, None),
        VisibilityLevel::Contacts => ("Contacts".to_string(), None, None),
        VisibilityLevel::Nobody => ("Nobody".to_string(), None, None),
        VisibilityLevel::Custom { allowed, blocked } => {
            let allowed_json = serde_json::to_string(allowed).ok();
            let blocked_json = serde_json::to_string(blocked).ok();
            ("Custom".to_string(), allowed_json, blocked_json)
        }
    }
}

fn parse_visibility(
    vis: String,
    allowed: Option<String>,
    blocked: Option<String>,
) -> Result<VisibilityLevel> {
    match vis.as_str() {
        "Everyone" => Ok(VisibilityLevel::Everyone),
        "Contacts" => Ok(VisibilityLevel::Contacts),
        "Nobody" => Ok(VisibilityLevel::Nobody),
        "Custom" => {
            let allowed_list = if let Some(json) = allowed {
                serde_json::from_str(&json).unwrap_or_default()
            } else {
                Vec::new()
            };
            let blocked_list = if let Some(json) = blocked {
                serde_json::from_str(&json).unwrap_or_default()
            } else {
                Vec::new()
            };
            Ok(VisibilityLevel::Custom {
                allowed: allowed_list,
                blocked: blocked_list,
            })
        }
        _ => Ok(VisibilityLevel::Everyone),
    }
}

fn serialize_status_type(status_type: &StatusType) -> Result<(String, String)> {
    let (type_name, data) = match status_type {
        StatusType::Text { text, font } => {
            let data = serde_json::json!({
                "text": text,
                "font": font,
            });
            ("Text", data)
        }
        StatusType::Image {
            file_id,
            width,
            height,
        } => {
            let data = serde_json::json!({
                "file_id": file_id,
                "width": width,
                "height": height,
            });
            ("Image", data)
        }
        StatusType::Video {
            file_id,
            width,
            height,
            duration,
        } => {
            let data = serde_json::json!({
                "file_id": file_id,
                "width": width,
                "height": height,
                "duration": duration,
            });
            ("Video", data)
        }
        StatusType::Audio {
            audio_file_id,
            background_image_id,
            duration,
            title,
            artist,
            music_api_track_id,
        } => {
            let data = serde_json::json!({
                "audio_file_id": audio_file_id,
                "background_image_id": background_image_id,
                "duration": duration,
                "title": title,
                "artist": artist,
                "music_api_track_id": music_api_track_id,
            });
            ("Audio", data)
        }
    };

    Ok((
        type_name.to_string(),
        serde_json::to_string(&data)
            .map_err(|e| Error::storage(format!("Failed to serialize status data: {}", e)))?,
    ))
}

fn parse_status_row(row: sqlx::sqlite::SqliteRow) -> Result<UserStatus> {
    let id: String = row.get("id");
    let status_type: String = row.get("status_type");
    let status_data: String = row.get("status_data");
    let viewers_json: String = row.get("viewers");

    let data: serde_json::Value = serde_json::from_str(&status_data)
        .map_err(|e| Error::storage(format!("Failed to parse status data: {}", e)))?;

    let status_type_parsed = match status_type.as_str() {
        "Text" => StatusType::Text {
            text: data["text"].as_str().unwrap_or_default().to_string(),
            font: data["font"].as_str().map(String::from),
        },
        "Image" => StatusType::Image {
            file_id: data["file_id"].as_str().unwrap_or_default().to_string(),
            width: data["width"].as_u64().unwrap_or(0) as u32,
            height: data["height"].as_u64().unwrap_or(0) as u32,
        },
        "Video" => StatusType::Video {
            file_id: data["file_id"].as_str().unwrap_or_default().to_string(),
            width: data["width"].as_u64().unwrap_or(0) as u32,
            height: data["height"].as_u64().unwrap_or(0) as u32,
            duration: data["duration"].as_u64().unwrap_or(0) as u32,
        },
        "Audio" => StatusType::Audio {
            audio_file_id: data["audio_file_id"]
                .as_str()
                .unwrap_or_default()
                .to_string(),
            background_image_id: data["background_image_id"].as_str().map(String::from),
            duration: data["duration"].as_u64().unwrap_or(0) as u32,
            title: data["title"].as_str().map(String::from),
            artist: data["artist"].as_str().map(String::from),
            music_api_track_id: if let Some(track_data) = data.get("music_api_track_id") {
                serde_json::from_value(track_data.clone()).ok()
            } else {
                None
            },
        },
        _ => {
            return Err(Error::storage(format!(
                "Unknown status type: {}",
                status_type
            )))
        }
    };

    let viewers: Vec<dchat_core::types::UserId> = serde_json::from_str(&viewers_json)
        .map_err(|e| Error::storage(format!("Failed to parse viewers: {}", e)))?;

    Ok(UserStatus {
        id: Uuid::parse_str(&id)
            .map_err(|e| Error::storage(format!("Failed to parse status ID: {}", e)))?,
        status_type: status_type_parsed,
        caption: row.get("caption"),
        background_color: row.get("background_color"),
        created_at: DateTime::parse_from_rfc3339(row.get("created_at"))
            .map_err(|e| Error::storage(format!("Failed to parse created_at: {}", e)))?
            .with_timezone(&Utc),
        expires_at: DateTime::parse_from_rfc3339(row.get("expires_at"))
            .map_err(|e| Error::storage(format!("Failed to parse expires_at: {}", e)))?
            .with_timezone(&Utc),
        view_count: row.get::<i64, _>("view_count") as u64,
        viewers,
    })
}

fn parse_profile_row(
    row: sqlx::sqlite::SqliteRow,
    privacy: PrivacySettings,
) -> Result<UserProfile> {
    let user_id_str: String = row.get("user_id");
    let metadata_json: String = row.get("metadata");
    let online_status_str: String = row.get("online_status");

    let profile_picture =
        if let Some(file_id) = row.get::<Option<String>, _>("profile_picture_file_id") {
            Some(ProfilePicture {
                file_id,
                file_unique_id: row
                    .get::<Option<String>, _>("profile_picture_unique_id")
                    .unwrap_or_default(),
                small_file_id: row.get("profile_picture_small"),
                large_file_id: row.get("profile_picture_large"),
                uploaded_at: if let Some(dt_str) =
                    row.get::<Option<String>, _>("profile_picture_uploaded_at")
                {
                    DateTime::parse_from_rfc3339(&dt_str)
                        .map_err(|e| Error::storage(format!("Failed to parse uploaded_at: {}", e)))?
                        .with_timezone(&Utc)
                } else {
                    Utc::now()
                },
            })
        } else {
            None
        };

    let online_status = match online_status_str.as_str() {
        "Online" => OnlineStatus::Online,
        "Offline" => OnlineStatus::Offline,
        "Away" => OnlineStatus::Away,
        "DoNotDisturb" => OnlineStatus::DoNotDisturb,
        _ => OnlineStatus::Offline,
    };

    let last_seen = if let Some(dt_str) = row.get::<Option<String>, _>("last_seen") {
        Some(
            DateTime::parse_from_rfc3339(&dt_str)
                .map_err(|e| Error::storage(format!("Failed to parse last_seen: {}", e)))?
                .with_timezone(&Utc),
        )
    } else {
        None
    };

    let metadata: HashMap<String, String> = serde_json::from_str(&metadata_json)
        .map_err(|e| Error::storage(format!("Failed to parse metadata: {}", e)))?;

    Ok(UserProfile {
        user_id: dchat_core::types::UserId(
            uuid::Uuid::parse_str(&user_id_str)
                .map_err(|e| Error::storage(format!("Failed to parse user_id: {}", e)))?,
        ),
        username: row.get("username"),
        display_name: row.get("display_name"),
        bio: row.get("bio"),
        profile_picture,
        status: None, // Status is loaded separately
        online_status,
        last_seen,
        created_at: DateTime::parse_from_rfc3339(row.get("created_at"))
            .map_err(|e| Error::storage(format!("Failed to parse created_at: {}", e)))?
            .with_timezone(&Utc),
        privacy,
        is_verified: row.get("is_verified"),
        metadata,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_profile_storage_init() {
        let pool = SqlitePool::connect(":memory:").await.unwrap();
        let storage = ProfileStorage::new(pool);
        storage.init_schema().await.unwrap();
    }
}
