//! Database storage for bots

use crate::{Bot, BotCommand, BotPermissions};
use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use sqlx::{Row, SqlitePool};
use uuid::Uuid;

/// Bot storage
pub struct BotStorage {
    pool: SqlitePool,
}

impl BotStorage {
    /// Create new bot storage
    pub fn new(pool: SqlitePool) -> Self {
        Self { pool }
    }

    /// Initialize database schema
    pub async fn init_schema(&self) -> Result<()> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS bots (
                id TEXT PRIMARY KEY,
                username TEXT NOT NULL UNIQUE,
                display_name TEXT NOT NULL,
                owner_id TEXT NOT NULL,
                token TEXT NOT NULL UNIQUE,
                description TEXT,
                about TEXT,
                webhook_url TEXT,
                is_active BOOLEAN NOT NULL DEFAULT TRUE,
                created_at TEXT NOT NULL,
                last_active_at TEXT,
                total_messages INTEGER NOT NULL DEFAULT 0,
                total_commands INTEGER NOT NULL DEFAULT 0,
                total_inline_queries INTEGER NOT NULL DEFAULT 0,
                total_callback_queries INTEGER NOT NULL DEFAULT 0,
                active_users INTEGER NOT NULL DEFAULT 0,
                avg_response_time_ms INTEGER NOT NULL DEFAULT 0
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create bots table: {}", e)))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS bot_commands (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL,
                command TEXT NOT NULL,
                description TEXT NOT NULL,
                hidden BOOLEAN NOT NULL DEFAULT FALSE,
                FOREIGN KEY (bot_id) REFERENCES bots(id) ON DELETE CASCADE,
                UNIQUE (bot_id, command)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create bot_commands table: {}", e)))?;

        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS bot_permissions (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                bot_id TEXT NOT NULL,
                permission TEXT NOT NULL,
                FOREIGN KEY (bot_id) REFERENCES bots(id) ON DELETE CASCADE,
                UNIQUE (bot_id, permission)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create bot_permissions table: {}", e)))?;

        // Create indices
        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bots_username ON bots(username)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create username index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bots_owner ON bots(owner_id)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create owner index: {}", e)))?;

        sqlx::query("CREATE INDEX IF NOT EXISTS idx_bots_token ON bots(token)")
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to create token index: {}", e)))?;

        Ok(())
    }

    /// Save a bot
    pub async fn save_bot(&self, bot: &Bot) -> Result<()> {
        let id = bot.id.to_string();
        let owner_id = bot.owner_id.to_string();
        let created_at = bot.created_at.to_rfc3339();
        let last_active_at = bot.last_active_at.as_ref().map(|dt| dt.to_rfc3339());

        sqlx::query(
            r#"
            INSERT INTO bots (
                id, username, display_name, owner_id, token, description, about,
                webhook_url, is_active, created_at, last_active_at,
                total_messages, total_commands, total_inline_queries,
                total_callback_queries, active_users, avg_response_time_ms
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
            ON CONFLICT (id) DO UPDATE SET
                username = excluded.username,
                display_name = excluded.display_name,
                description = excluded.description,
                about = excluded.about,
                webhook_url = excluded.webhook_url,
                is_active = excluded.is_active,
                last_active_at = excluded.last_active_at,
                total_messages = excluded.total_messages,
                total_commands = excluded.total_commands,
                total_inline_queries = excluded.total_inline_queries,
                total_callback_queries = excluded.total_callback_queries,
                active_users = excluded.active_users,
                avg_response_time_ms = excluded.avg_response_time_ms
            "#,
        )
        .bind(&id)
        .bind(&bot.username)
        .bind(&bot.display_name)
        .bind(&owner_id)
        .bind(&bot.token)
        .bind(&bot.description)
        .bind(&bot.about)
        .bind(&bot.webhook_url)
        .bind(bot.is_active)
        .bind(&created_at)
        .bind(&last_active_at)
        .bind(bot.stats.total_messages as i64)
        .bind(bot.stats.total_commands as i64)
        .bind(bot.stats.total_inline_queries as i64)
        .bind(bot.stats.total_callback_queries as i64)
        .bind(bot.stats.active_users as i64)
        .bind(bot.stats.avg_response_time_ms as i64)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to save bot: {}", e)))?;

        // Save commands
        self.save_bot_commands(&bot.id, &bot.commands).await?;

        // Save permissions (serialize BotPermissions to JSON)
        let perms_json = serde_json::to_string(&bot.permissions)
            .map_err(|e| Error::storage(format!("Failed to serialize permissions: {}", e)))?;
        self.save_bot_permissions(&bot.id, &[perms_json]).await?;

        Ok(())
    }

    /// Load a bot by ID
    pub async fn load_bot(&self, bot_id: &Uuid) -> Result<Bot> {
        let id = bot_id.to_string();

        let row = sqlx::query(
            r#"
            SELECT id, username, display_name, owner_id, token, description, about,
                   webhook_url, is_active, created_at, last_active_at,
                   total_messages, total_commands, total_inline_queries,
                   total_callback_queries, active_users, avg_response_time_ms
            FROM bots WHERE id = ?
            "#,
        )
        .bind(&id)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to load bot: {}", e)))?
        .ok_or_else(|| Error::storage("Bot not found"))?;

        let bot_id = Uuid::parse_str(row.get("id"))
            .map_err(|e| Error::validation(format!("Invalid bot ID: {}", e)))?;

        let owner_id_str: String = row.get("owner_id");
        let owner_uuid = Uuid::parse_str(&owner_id_str)
            .map_err(|e| Error::validation(format!("Invalid owner UUID: {}", e)))?;
        let owner_id = dchat_core::types::UserId(owner_uuid);

        let created_at: String = row.get("created_at");
        let created_at = DateTime::parse_from_rfc3339(&created_at)
            .map_err(|e| Error::validation(format!("Invalid created_at: {}", e)))?
            .with_timezone(&Utc);

        let last_active_at: Option<String> = row.get("last_active_at");
        let last_active_at = last_active_at
            .map(|s| DateTime::parse_from_rfc3339(&s).map(|dt| dt.with_timezone(&Utc)))
            .transpose()
            .map_err(|e| Error::validation(format!("Invalid last_active_at: {}", e)))?;

        let commands = self.load_bot_commands(&bot_id).await?;
        let perm_strings = self.load_bot_permissions(&bot_id).await?;
        let permissions = if perm_strings.is_empty() {
            BotPermissions::default()
        } else {
            serde_json::from_str(&perm_strings[0])
                .map_err(|e| Error::storage(format!("Failed to deserialize permissions: {}", e)))?
        };

        Ok(Bot {
            id: bot_id,
            username: row.get("username"),
            display_name: row.get("display_name"),
            owner_id,
            token: row.get("token"),
            description: row.get("description"),
            about: row.get("about"),
            permissions,
            commands,
            webhook_url: row.get("webhook_url"),
            is_active: row.get("is_active"),
            avatar_hash: None,
            created_at,
            last_active_at,
            stats: crate::BotStatistics {
                total_messages: row.get::<i64, _>("total_messages") as u64,
                total_commands: row.get::<i64, _>("total_commands") as u64,
                total_inline_queries: row.get::<i64, _>("total_inline_queries") as u64,
                total_callback_queries: row.get::<i64, _>("total_callback_queries") as u64,
                active_users: row.get::<i64, _>("active_users") as u64,
                avg_response_time_ms: row.get::<i64, _>("avg_response_time_ms") as u64,
                total_webhooks: 0,
            },
        })
    }

    /// Load bot by username
    pub async fn load_bot_by_username(&self, username: &str) -> Result<Bot> {
        let row = sqlx::query("SELECT id FROM bots WHERE username = ?")
            .bind(username)
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to query bot: {}", e)))?
            .ok_or_else(|| Error::storage("Bot not found"))?;

        let id = Uuid::parse_str(row.get("id"))
            .map_err(|e| Error::validation(format!("Invalid bot ID: {}", e)))?;

        self.load_bot(&id).await
    }

    /// Delete a bot
    pub async fn delete_bot(&self, bot_id: &Uuid) -> Result<()> {
        let id = bot_id.to_string();

        sqlx::query("DELETE FROM bots WHERE id = ?")
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete bot: {}", e)))?;

        Ok(())
    }

    /// Save bot commands
    async fn save_bot_commands(&self, bot_id: &Uuid, commands: &[BotCommand]) -> Result<()> {
        let id = bot_id.to_string();

        // Delete existing commands
        sqlx::query("DELETE FROM bot_commands WHERE bot_id = ?")
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete commands: {}", e)))?;

        // Insert new commands
        for cmd in commands {
            sqlx::query(
                "INSERT INTO bot_commands (bot_id, command, description, hidden) VALUES (?, ?, ?, ?)"
            )
            .bind(&id)
            .bind(&cmd.command)
            .bind(&cmd.description)
            .bind(cmd.hidden)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to insert command: {}", e)))?;
        }

        Ok(())
    }

    /// Load bot commands
    async fn load_bot_commands(&self, bot_id: &Uuid) -> Result<Vec<BotCommand>> {
        let id = bot_id.to_string();

        let rows =
            sqlx::query("SELECT command, description, hidden FROM bot_commands WHERE bot_id = ?")
                .bind(&id)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to load commands: {}", e)))?;

        Ok(rows
            .into_iter()
            .map(|row| BotCommand {
                command: row.get("command"),
                description: row.get("description"),
                hidden: row.get("hidden"),
                required_permissions: Vec::new(),
            })
            .collect())
    }

    /// Save bot permissions
    async fn save_bot_permissions(&self, bot_id: &Uuid, permissions: &[String]) -> Result<()> {
        let id = bot_id.to_string();

        // Delete existing permissions
        sqlx::query("DELETE FROM bot_permissions WHERE bot_id = ?")
            .bind(&id)
            .execute(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to delete permissions: {}", e)))?;

        // Insert new permissions
        for perm in permissions {
            sqlx::query("INSERT INTO bot_permissions (bot_id, permission) VALUES (?, ?)")
                .bind(&id)
                .bind(perm)
                .execute(&self.pool)
                .await
                .map_err(|e| Error::storage(format!("Failed to insert permission: {}", e)))?;
        }

        Ok(())
    }

    /// Load bot permissions
    async fn load_bot_permissions(&self, bot_id: &Uuid) -> Result<Vec<String>> {
        let id = bot_id.to_string();

        let rows = sqlx::query("SELECT permission FROM bot_permissions WHERE bot_id = ?")
            .bind(&id)
            .fetch_all(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to load permissions: {}", e)))?;

        Ok(rows.into_iter().map(|row| row.get("permission")).collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_core::types::UserId;

    async fn create_test_pool() -> SqlitePool {
        SqlitePool::connect(":memory:").await.unwrap()
    }

    #[tokio::test]
    async fn test_init_schema() {
        let pool = create_test_pool().await;
        let storage = BotStorage::new(pool);

        storage.init_schema().await.unwrap();
    }

    #[tokio::test]
    async fn test_save_and_load_bot() {
        let pool = create_test_pool().await;
        let storage = BotStorage::new(pool);
        storage.init_schema().await.unwrap();

        let bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), UserId::new()).unwrap();

        storage.save_bot(&bot).await.unwrap();

        let loaded = storage.load_bot(&bot.id).await.unwrap();
        assert_eq!(loaded.id, bot.id);
        assert_eq!(loaded.username, bot.username);
        assert_eq!(loaded.display_name, bot.display_name);
    }

    #[tokio::test]
    async fn test_load_by_username() {
        let pool = create_test_pool().await;
        let storage = BotStorage::new(pool);
        storage.init_schema().await.unwrap();

        let bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), UserId::new()).unwrap();

        storage.save_bot(&bot).await.unwrap();

        let loaded = storage.load_bot_by_username("testbot").await.unwrap();
        assert_eq!(loaded.id, bot.id);
    }

    #[tokio::test]
    async fn test_delete_bot() {
        let pool = create_test_pool().await;
        let storage = BotStorage::new(pool);
        storage.init_schema().await.unwrap();

        let bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), UserId::new()).unwrap();

        storage.save_bot(&bot).await.unwrap();
        storage.delete_bot(&bot.id).await.unwrap();

        let result = storage.load_bot(&bot.id).await;
        assert!(result.is_err());
    }
}
