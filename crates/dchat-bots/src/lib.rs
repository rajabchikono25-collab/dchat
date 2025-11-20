//! Bot Management System for dchat
//!
//! This module provides a comprehensive bot platform similar to Telegram's BotFather:
//! - Bot creation and registration
//! - Bot authentication tokens
//! - Webhook management
//! - Command handling
//! - Inline queries
//! - Callback queries
//! - Bot API endpoints
//! - Permission management

pub mod api;
pub mod bot_api;
pub mod bot_manager;
pub mod commands;
pub mod event_dispatcher;
pub mod inline;
pub mod messaging_integration;
pub mod music_api;
pub mod permissions;
pub mod search;
pub mod storage;
pub mod token_security;
pub mod webhook;

pub use api::BotHttpClient;
pub use bot_api::{BotApi, BotClient};
pub use event_dispatcher::{BotEvent, BotEventHandler, EventDispatcher};
pub use messaging_integration::{BotMessagingClient, MessageRouter};
pub use bot_manager::{BotFather, BotManager};
pub use commands::{Command, CommandHandler, CommandRegistry};
pub use dchat_identity::profile::{
    MusicApiTrack, MusicProvider, OnlineStatus, PrivacySettings, ProfileManager, ProfilePicture,
    StatusType, UserProfile, UserStatus, VisibilityLevel,
};
pub use dchat_identity::storage::ProfileStorage;
pub use dchat_messaging::media::{
    Audio, Document, EnhancedBotMessage, EntityType, LinkPreview, Location, MediaType,
    MessageEntity, Photo, PhotoSize, Poll, Sticker, Video, Voice,
};
pub use dchat_storage::file_upload::{
    FileUploadManager, MediaFileType, StorageStats, UploadConfig, UploadedFile,
};
pub use inline::{InlineQuery, InlineQueryHandler, InlineResult};
pub use music_api::MusicApiClient;
pub use permissions::{BotPermissions, BotScope};
pub use search::{
    BotMetadata, BotSearchResult, SearchFilters, SearchManager, SearchResult, SearchType,
};
pub use token_security::{BotToken, TokenError, TokenManager, WebhookVerifier};
pub use webhook::{WebhookConfig, WebhookManager};

use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Bot unique identifier
pub type BotId = Uuid;

/// Bot information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Bot {
    /// Unique bot ID
    pub id: BotId,

    /// Bot username (must be unique, ends with "bot")
    pub username: String,

    /// Display name
    pub display_name: String,

    /// Bot description
    pub description: Option<String>,

    /// Bot about text
    pub about: Option<String>,

    /// Owner user ID
    pub owner_id: dchat_core::types::UserId,

    /// Authentication token
    pub token: String,

    /// Bot avatar hash
    pub avatar_hash: Option<String>,

    /// Is bot active?
    pub is_active: bool,

    /// Bot permissions
    pub permissions: BotPermissions,

    /// Creation timestamp
    pub created_at: DateTime<Utc>,

    /// Last activity timestamp
    pub last_active_at: Option<DateTime<Utc>>,

    /// Webhook configuration
    pub webhook_url: Option<String>,

    /// Commands supported by this bot
    pub commands: Vec<BotCommand>,

    /// Statistics
    pub stats: BotStatistics,
}

/// Bot command definition
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCommand {
    /// Command name (without leading slash)
    pub command: String,

    /// Command description
    pub description: String,

    /// Is command hidden from command list?
    pub hidden: bool,

    /// Required permissions to use this command
    pub required_permissions: Vec<String>,
}

/// Bot statistics
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct BotStatistics {
    /// Total messages received
    pub total_messages: u64,

    /// Total commands executed
    pub total_commands: u64,

    /// Total inline queries
    pub total_inline_queries: u64,

    /// Total callback queries
    pub total_callback_queries: u64,

    /// Total webhooks sent
    pub total_webhooks: u64,

    /// Active users (last 30 days)
    pub active_users: u64,

    /// Average response time (milliseconds)
    pub avg_response_time_ms: u64,
}

/// Bot creation request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreateBotRequest {
    /// Bot username (must end with "bot")
    pub username: String,

    /// Display name
    pub display_name: String,

    /// Optional description
    pub description: Option<String>,
}

/// Bot update request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpdateBotRequest {
    /// New display name
    pub display_name: Option<String>,

    /// New description
    pub description: Option<String>,

    /// New about text
    pub about: Option<String>,

    /// Update avatar
    pub avatar_data: Option<Vec<u8>>,

    /// Update commands
    pub commands: Option<Vec<BotCommand>>,
}

/// Bot message (received by bot)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotMessage {
    /// Message ID
    pub message_id: Uuid,

    /// Sender user ID
    pub from_user_id: Uuid,

    /// Chat ID (channel or DM)
    pub chat_id: String,

    /// Message text
    pub text: String,

    /// Message timestamp
    pub timestamp: std::time::SystemTime,

    /// Reply to message ID
    pub reply_to_message_id: Option<Uuid>,

    /// Message entities
    pub entities: Vec<MessageEntity>,

    /// Media content
    pub media: Option<EnhancedBotMessage>,
}

/// Bot response message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotResponse {
    /// Chat ID to send to
    pub chat_id: String,

    /// Response text
    pub text: String,

    /// Parse mode (Markdown, HTML, or None)
    pub parse_mode: Option<ParseMode>,

    /// Reply to message ID
    pub reply_to_message_id: Option<Uuid>,

    /// Inline keyboard
    pub inline_keyboard: Option<Vec<Vec<InlineKeyboardButton>>>,

    /// Disable notification
    pub disable_notification: bool,
}

/// Parse mode for bot messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ParseMode {
    Markdown,
    HTML,
}

/// Inline keyboard button
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InlineKeyboardButton {
    /// Button text
    pub text: String,

    /// Button action
    pub action: ButtonAction,
}

/// Button action type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ButtonAction {
    /// Callback data (sent back to bot when clicked)
    CallbackData(String),

    /// URL to open
    Url(String),

    /// Inline query
    InlineQuery(String),
}

/// Callback query (button click)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CallbackQuery {
    /// Query ID
    pub id: Uuid,

    /// User who clicked
    pub from: dchat_core::types::UserId,

    /// Original message
    pub message: BotMessage,

    /// Callback data
    pub data: String,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

impl Bot {
    /// Create a new bot
    pub fn new(
        username: String,
        display_name: String,
        owner_id: dchat_core::types::UserId,
    ) -> Result<Self> {
        if !username.ends_with("bot") {
            return Err(Error::validation("Bot username must end with 'bot'"));
        }

        if username.len() < 5 || username.len() > 32 {
            return Err(Error::validation("Bot username must be 5-32 characters"));
        }

        let token = Self::generate_token();

        Ok(Self {
            id: BotId::new_v4(),
            username,
            display_name,
            description: None,
            about: None,
            owner_id,
            token,
            avatar_hash: None,
            is_active: true,
            permissions: BotPermissions::default(),
            created_at: Utc::now(),
            last_active_at: None,
            webhook_url: None,
            commands: Vec::new(),
            stats: BotStatistics::default(),
        })
    }

    /// Generate a secure authentication token
    fn generate_token() -> String {
        use sha2::{Digest, Sha256};
        let uuid = uuid::Uuid::new_v4();
        let random_bytes = uuid.as_bytes();
        let mut hasher = Sha256::new();
        hasher.update(random_bytes);
        let result = hasher.finalize();
        use base64::Engine;
        format!(
            "dchat_bot_{}",
            base64::engine::general_purpose::STANDARD.encode(result)
        )
    }

    /// Verify token
    pub fn verify_token(&self, token: &str) -> bool {
        self.token == token
    }

    /// Add command
    pub fn add_command(&mut self, command: BotCommand) {
        self.commands.push(command);
    }

    /// Remove command
    pub fn remove_command(&mut self, command_name: &str) {
        self.commands.retain(|c| c.command != command_name);
    }

    /// Get command
    pub fn get_command(&self, command_name: &str) -> Option<&BotCommand> {
        self.commands.iter().find(|c| c.command == command_name)
    }

    /// Update statistics
    pub fn record_message(&mut self) {
        self.stats.total_messages += 1;
        self.last_active_at = Some(Utc::now());
    }

    pub fn record_command(&mut self) {
        self.stats.total_commands += 1;
    }

    pub fn record_inline_query(&mut self) {
        self.stats.total_inline_queries += 1;
    }

    pub fn record_callback_query(&mut self) {
        self.stats.total_callback_queries += 1;
    }
}

impl BotCommand {
    /// Create a new bot command
    pub fn new(command: String, description: String) -> Self {
        Self {
            command,
            description,
            hidden: false,
            required_permissions: Vec::new(),
        }
    }

    /// Set command as hidden
    pub fn hidden(mut self, hidden: bool) -> Self {
        self.hidden = hidden;
        self
    }

    /// Add required permission
    pub fn require_permission(mut self, permission: String) -> Self {
        self.required_permissions.push(permission);
        self
    }
}

impl BotMessage {
    /// Parse a message into a BotMessage
    pub fn from_message(
        message_id: Uuid,
        from_user_id: Uuid,
        chat_id: String,
        text: String,
    ) -> Self {
        Self {
            message_id,
            from_user_id,
            chat_id,
            text,
            timestamp: std::time::SystemTime::now(),
            reply_to_message_id: None,
            entities: Vec::new(),
            media: None,
        }
    }

    /// Check if message is a command
    pub fn is_command(&self) -> bool {
        self.text.starts_with('/')
    }

    /// Parse command from text
    pub fn parse_command(&self) -> Option<(String, Vec<String>)> {
        if !self.text.starts_with('/') {
            return None;
        }

        let parts: Vec<&str> = self.text[1..].split_whitespace().collect();
        if parts.is_empty() {
            return None;
        }

        let command = parts[0].to_string();
        let args = parts[1..].iter().map(|s| s.to_string()).collect();

        Some((command, args))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_bot() {
        let owner_id = dchat_core::types::UserId::new();
        let bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), owner_id).unwrap();

        assert_eq!(bot.username, "testbot");
        assert_eq!(bot.display_name, "Test Bot");
        assert!(!bot.token.is_empty());
        assert!(bot.is_active);
    }

    #[test]
    fn test_bot_username_validation() {
        let owner_id = dchat_core::types::UserId::new();

        // Should fail - doesn't end with "bot"
        let result = Bot::new("test".to_string(), "Test".to_string(), owner_id.clone());
        assert!(result.is_err());

        // Should fail - too short
        let result = Bot::new("bot".to_string(), "Test".to_string(), owner_id);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_command() {
        let message = BotMessage::from_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "chat123".to_string(),
            "/start hello world".to_string(),
        );

        assert!(message.is_command());
        if let Some((cmd, args)) = message.parse_command() {
            assert_eq!(cmd, "start");
            assert_eq!(args, vec!["hello", "world"]);
        } else {
            panic!("Failed to parse command");
        }
    }

    #[test]
    fn test_add_remove_commands() {
        let owner_id = dchat_core::types::UserId::new();
        let mut bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), owner_id).unwrap();

        let cmd = BotCommand::new("start".to_string(), "Start the bot".to_string());

        bot.add_command(cmd);
        assert_eq!(bot.commands.len(), 1);

        assert!(bot.get_command("start").is_some());
        assert!(bot.get_command("help").is_none());

        bot.remove_command("start");
        assert_eq!(bot.commands.len(), 0);
    }

    #[test]
    fn test_token_verification() {
        let owner_id = dchat_core::types::UserId::new();
        let bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), owner_id).unwrap();

        assert!(bot.verify_token(&bot.token));
        assert!(!bot.verify_token("invalid_token"));
    }

    #[test]
    fn test_record_statistics() {
        let owner_id = dchat_core::types::UserId::new();
        let mut bot = Bot::new("testbot".to_string(), "Test Bot".to_string(), owner_id).unwrap();

        assert_eq!(bot.stats.total_messages, 0);

        bot.record_message();
        assert_eq!(bot.stats.total_messages, 1);
        assert!(bot.last_active_at.is_some());

        bot.record_command();
        assert_eq!(bot.stats.total_commands, 1);
    }
}
