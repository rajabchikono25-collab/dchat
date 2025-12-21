//! Bot bridge - authenticated bot identities and command handling

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::manifest::BotCommandSpec;
use crate::permissions::{Permission, PermissionManager, PermissionSet};
use crate::registry::AppId;

/// Bot ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BotId(pub [u8; 32]);

impl BotId {
    /// Create from bytes
    pub fn from_bytes(bytes: [u8; 32]) -> Self {
        Self(bytes)
    }

    /// Create from app ID (bot is tied to app)
    pub fn from_app_id(app_id: &AppId) -> Self {
        Self(app_id.0)
    }

    /// Get bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }
}

impl std::fmt::Display for BotId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bot:{}", hex::encode(&self.0[..8]))
    }
}

/// Bot identity with authentication
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotIdentity {
    /// Bot ID
    pub id: BotId,
    /// Associated app ID
    pub app_id: AppId,
    /// Bot username (unique)
    pub username: String,
    /// Display name
    pub display_name: String,
    /// Bot description
    pub description: String,
    /// Avatar URL
    pub avatar_url: Option<String>,
    /// Public key for verification
    pub public_key: [u8; 32],
    /// Supported commands
    pub commands: Vec<BotCommandSpec>,
    /// Is verified bot
    pub is_verified: bool,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Last active timestamp
    pub last_active: DateTime<Utc>,
}

impl BotIdentity {
    /// Create new bot identity
    pub fn new(
        app_id: AppId,
        username: String,
        display_name: String,
        public_key: [u8; 32],
    ) -> MiniAppResult<Self> {
        // Validate username
        if username.len() < 3 || username.len() > 32 {
            return Err(MiniAppError::InvalidBotConfig(
                "username must be 3-32 characters".to_string(),
            ));
        }

        if !username.chars().all(|c| c.is_alphanumeric() || c == '_') {
            return Err(MiniAppError::InvalidBotConfig(
                "username must be alphanumeric with underscores".to_string(),
            ));
        }

        let now = Utc::now();
        Ok(Self {
            id: BotId::from_app_id(&app_id),
            app_id,
            username,
            display_name,
            description: String::new(),
            avatar_url: None,
            public_key,
            commands: Vec::new(),
            is_verified: false,
            created_at: now,
            last_active: now,
        })
    }

    /// Set description
    pub fn with_description(mut self, description: String) -> Self {
        self.description = description;
        self
    }

    /// Set avatar
    pub fn with_avatar(mut self, url: String) -> Self {
        self.avatar_url = Some(url);
        self
    }

    /// Set commands
    pub fn with_commands(mut self, commands: Vec<BotCommandSpec>) -> Self {
        self.commands = commands;
        self
    }
}

/// Bot authentication token
#[serde_as]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAuthToken {
    /// Token ID
    pub id: String,
    /// Bot ID
    pub bot_id: BotId,
    /// Channel ID (if scoped)
    pub channel_id: Option<String>,
    /// User ID (if scoped)
    pub user_id: Option<String>,
    /// Issued timestamp
    pub issued_at: DateTime<Utc>,
    /// Expiry timestamp
    pub expires_at: DateTime<Utc>,
    /// Signature (Ed25519 64-byte signature)
    #[serde_as(as = "Bytes")]
    pub signature: [u8; 64],
}

impl BotAuthToken {
    /// Create new token
    pub fn create(
        bot_id: BotId,
        signing_key: &SigningKey,
        channel_id: Option<String>,
        user_id: Option<String>,
        duration: chrono::Duration,
    ) -> Self {
        let id = Uuid::new_v4().to_string();
        let now = Utc::now();
        let expires_at = now + duration;

        // Create signing payload
        let payload = Self::signing_payload(
            &id,
            &bot_id,
            channel_id.as_deref(),
            user_id.as_deref(),
            &expires_at,
        );

        let signature = signing_key.sign(&payload);

        Self {
            id,
            bot_id,
            channel_id,
            user_id,
            issued_at: now,
            expires_at,
            signature: signature.to_bytes(),
        }
    }

    /// Verify token
    pub fn verify(&self, public_key: &VerifyingKey) -> MiniAppResult<()> {
        // Check expiry
        if self.expires_at < Utc::now() {
            return Err(MiniAppError::BotAuthenticationFailed(
                "token expired".to_string(),
            ));
        }

        // Verify signature
        let payload = Self::signing_payload(
            &self.id,
            &self.bot_id,
            self.channel_id.as_deref(),
            self.user_id.as_deref(),
            &self.expires_at,
        );

        let signature = Signature::from_bytes(&self.signature);
        public_key
            .verify(&payload, &signature)
            .map_err(|_| MiniAppError::BotAuthenticationFailed("invalid signature".to_string()))
    }

    /// Create signing payload
    fn signing_payload(
        id: &str,
        bot_id: &BotId,
        channel_id: Option<&str>,
        user_id: Option<&str>,
        expires_at: &DateTime<Utc>,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(id.as_bytes());
        payload.extend_from_slice(&bot_id.0);
        if let Some(ch) = channel_id {
            payload.extend_from_slice(ch.as_bytes());
        }
        if let Some(u) = user_id {
            payload.extend_from_slice(u.as_bytes());
        }
        payload.extend_from_slice(&expires_at.timestamp().to_le_bytes());
        payload
    }

    /// Check if token is expired
    pub fn is_expired(&self) -> bool {
        self.expires_at < Utc::now()
    }
}

/// Bot command context
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCommandContext {
    /// Command name (without /)
    pub command: String,
    /// Arguments
    pub args: Vec<String>,
    /// Raw argument string
    pub raw_args: String,
    /// Sender user ID
    pub user_id: String,
    /// Channel ID
    pub channel_id: String,
    /// Message ID
    pub message_id: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
    /// Reply-to message ID
    pub reply_to: Option<String>,
}

impl BotCommandContext {
    /// Parse command from message
    pub fn parse(message: &str, user_id: &str, channel_id: &str, message_id: &str) -> Option<Self> {
        let message = message.trim();
        if !message.starts_with('/') {
            return None;
        }

        let mut parts = message[1..].splitn(2, char::is_whitespace);
        let command = parts.next()?.to_lowercase();
        let raw_args = parts.next().unwrap_or("").to_string();
        let args: Vec<String> = raw_args.split_whitespace().map(String::from).collect();

        Some(Self {
            command,
            args,
            raw_args,
            user_id: user_id.to_string(),
            channel_id: channel_id.to_string(),
            message_id: message_id.to_string(),
            timestamp: Utc::now(),
            reply_to: None,
        })
    }

    /// Set reply-to
    pub fn with_reply_to(mut self, message_id: String) -> Self {
        self.reply_to = Some(message_id);
        self
    }

    /// Get first argument
    pub fn arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).map(String::as_str)
    }
}

/// Bot command result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCommandResult {
    /// Response text
    pub text: Option<String>,
    /// Response HTML
    pub html: Option<String>,
    /// Attachments
    pub attachments: Vec<BotAttachment>,
    /// Keyboard
    pub keyboard: Option<BotKeyboard>,
    /// Reply to original message
    pub reply_to_message: bool,
    /// Pin response
    pub pin: bool,
}

impl BotCommandResult {
    /// Create text response
    pub fn text(message: impl Into<String>) -> Self {
        Self {
            text: Some(message.into()),
            html: None,
            attachments: Vec::new(),
            keyboard: None,
            reply_to_message: false,
            pin: false,
        }
    }

    /// Create HTML response
    pub fn html(content: impl Into<String>) -> Self {
        Self {
            text: None,
            html: Some(content.into()),
            attachments: Vec::new(),
            keyboard: None,
            reply_to_message: false,
            pin: false,
        }
    }

    /// Add keyboard
    pub fn with_keyboard(mut self, keyboard: BotKeyboard) -> Self {
        self.keyboard = Some(keyboard);
        self
    }

    /// Set reply flag
    pub fn as_reply(mut self) -> Self {
        self.reply_to_message = true;
        self
    }
}

/// Bot attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotAttachment {
    /// Attachment type
    pub attachment_type: AttachmentType,
    /// URL or data
    pub url: String,
    /// Caption
    pub caption: Option<String>,
}

/// Attachment type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttachmentType {
    /// Image attachment (JPEG, PNG, GIF, WebP)
    Image,
    /// Video attachment (MP4, WebM)
    Video,
    /// Audio attachment (MP3, OGG)
    Audio,
    /// Document attachment (PDF, DOC, etc.)
    Document,
    /// Sticker attachment
    Sticker,
}

/// Bot keyboard
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotKeyboard {
    /// Keyboard type
    pub keyboard_type: KeyboardType,
    /// Rows of buttons
    pub rows: Vec<Vec<BotButton>>,
    /// Resize keyboard to fit buttons
    pub resize: bool,
    /// One-time keyboard
    pub one_time: bool,
}

impl BotKeyboard {
    /// Create inline keyboard
    pub fn inline(rows: Vec<Vec<BotButton>>) -> Self {
        Self {
            keyboard_type: KeyboardType::Inline,
            rows,
            resize: false,
            one_time: false,
        }
    }

    /// Create reply keyboard
    pub fn reply(rows: Vec<Vec<BotButton>>) -> Self {
        Self {
            keyboard_type: KeyboardType::Reply,
            rows,
            resize: true,
            one_time: true,
        }
    }
}

/// Keyboard type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum KeyboardType {
    /// Inline keyboard (buttons attached to message)
    Inline,
    /// Reply keyboard (custom keyboard below input)
    Reply,
    /// Remove keyboard (hide custom keyboard)
    Remove,
}

/// Bot button
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotButton {
    /// Button text
    pub text: String,
    /// Callback data
    pub callback_data: Option<String>,
    /// URL to open
    pub url: Option<String>,
    /// Mini app URL
    pub miniapp_url: Option<String>,
}

impl BotButton {
    /// Create callback button
    pub fn callback(text: impl Into<String>, data: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: Some(data.into()),
            url: None,
            miniapp_url: None,
        }
    }

    /// Create URL button
    pub fn url(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: None,
            url: Some(url.into()),
            miniapp_url: None,
        }
    }

    /// Create miniapp button
    pub fn miniapp(text: impl Into<String>, url: impl Into<String>) -> Self {
        Self {
            text: text.into(),
            callback_data: None,
            url: None,
            miniapp_url: Some(url.into()),
        }
    }
}

/// Bot callback query
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotCallbackQuery {
    /// Query ID
    pub id: String,
    /// User ID
    pub user_id: String,
    /// Message ID
    pub message_id: String,
    /// Channel ID
    pub channel_id: String,
    /// Callback data
    pub data: String,
    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Bot command handler trait
pub trait BotCommandHandler: Send + Sync {
    /// Handle command
    fn handle_command(&self, ctx: &BotCommandContext) -> MiniAppResult<BotCommandResult>;

    /// Handle callback query
    fn handle_callback(&self, query: &BotCallbackQuery) -> MiniAppResult<BotCommandResult>;
}

/// Registered bot with handlers
struct RegisteredBot {
    identity: BotIdentity,
    handler: Arc<dyn BotCommandHandler>,
    permissions: PermissionSet,
}

/// Bot bridge - manages bot identities and routing
pub struct BotBridge {
    /// Registered bots
    bots: RwLock<HashMap<BotId, RegisteredBot>>,
    /// Username to ID mapping
    usernames: RwLock<HashMap<String, BotId>>,
    /// Permission manager
    permission_manager: Arc<PermissionManager>,
}

impl BotBridge {
    /// Create new bot bridge
    pub fn new(permission_manager: Arc<PermissionManager>) -> Self {
        Self {
            bots: RwLock::new(HashMap::new()),
            usernames: RwLock::new(HashMap::new()),
            permission_manager,
        }
    }

    /// Register bot
    pub fn register(
        &self,
        identity: BotIdentity,
        handler: Arc<dyn BotCommandHandler>,
        permissions: PermissionSet,
    ) -> MiniAppResult<()> {
        let mut bots = self.bots.write();
        let mut usernames = self.usernames.write();

        // Check username uniqueness
        if usernames.contains_key(&identity.username) {
            return Err(MiniAppError::BotAlreadyRegistered(
                identity.username.clone(),
            ));
        }

        let bot_id = identity.id;
        let username = identity.username.clone();

        bots.insert(
            bot_id,
            RegisteredBot {
                identity,
                handler,
                permissions,
            },
        );
        usernames.insert(username, bot_id);

        Ok(())
    }

    /// Unregister bot
    pub fn unregister(&self, bot_id: &BotId) -> MiniAppResult<()> {
        let mut bots = self.bots.write();
        let mut usernames = self.usernames.write();

        if let Some(bot) = bots.remove(bot_id) {
            usernames.remove(&bot.identity.username);
            Ok(())
        } else {
            Err(MiniAppError::BotNotFound(bot_id.to_string()))
        }
    }

    /// Get bot by ID
    pub fn get(&self, bot_id: &BotId) -> Option<BotIdentity> {
        self.bots.read().get(bot_id).map(|b| b.identity.clone())
    }

    /// Get bot by username
    pub fn get_by_username(&self, username: &str) -> Option<BotIdentity> {
        let usernames = self.usernames.read();
        if let Some(id) = usernames.get(username) {
            self.get(id)
        } else {
            None
        }
    }

    /// Handle command
    pub fn handle_command(
        &self,
        bot_id: &BotId,
        ctx: BotCommandContext,
    ) -> MiniAppResult<BotCommandResult> {
        let bots = self.bots.read();
        let bot = bots
            .get(bot_id)
            .ok_or(MiniAppError::BotNotFound(bot_id.to_string()))?;

        // Check if command is supported
        let command_found = bot
            .identity
            .commands
            .iter()
            .any(|c| c.command == ctx.command);
        if !command_found && !bot.identity.commands.is_empty() {
            return Err(MiniAppError::BotCommandNotFound(ctx.command.clone()));
        }

        // Check permissions
        // (In production, we'd check if bot has permission for this operation)

        // Handle command
        bot.handler.handle_command(&ctx)
    }

    /// Handle callback query
    pub fn handle_callback(
        &self,
        bot_id: &BotId,
        query: BotCallbackQuery,
    ) -> MiniAppResult<BotCommandResult> {
        let bots = self.bots.read();
        let bot = bots
            .get(bot_id)
            .ok_or(MiniAppError::BotNotFound(bot_id.to_string()))?;

        bot.handler.handle_callback(&query)
    }

    /// Route message to appropriate bot
    pub fn route_message(
        &self,
        message: &str,
        user_id: String,
        channel_id: String,
        message_id: String,
    ) -> Option<(BotId, BotCommandContext)> {
        // Check if message starts with @botname command
        let message = message.trim();

        // Parse @mention at start
        if message.starts_with('@') {
            let parts: Vec<&str> = message.splitn(2, char::is_whitespace).collect();
            if let Some(mention) = parts.first() {
                let username = &mention[1..]; // Remove @
                if let Some(bot_id) = self.usernames.read().get(username) {
                    let remainder = parts.get(1).unwrap_or(&"");
                    if let Some(ctx) =
                        BotCommandContext::parse(remainder, &user_id, &channel_id, &message_id)
                    {
                        return Some((*bot_id, ctx));
                    }
                }
            }
        }

        // Otherwise, check if it's a direct command
        if message.starts_with('/') {
            // Find which bot handles this command
            let parts: Vec<&str> = message[1..].splitn(2, char::is_whitespace).collect();
            let command = parts.first()?.to_lowercase();

            for (bot_id, bot) in self.bots.read().iter() {
                if bot.identity.commands.iter().any(|c| c.command == command) {
                    if let Some(ctx) =
                        BotCommandContext::parse(message, &user_id, &channel_id, &message_id)
                    {
                        return Some((*bot_id, ctx));
                    }
                }
            }
        }

        None
    }

    /// List all bots
    pub fn list(&self) -> Vec<BotIdentity> {
        self.bots
            .read()
            .values()
            .map(|b| b.identity.clone())
            .collect()
    }

    /// List verified bots
    pub fn list_verified(&self) -> Vec<BotIdentity> {
        self.bots
            .read()
            .values()
            .filter(|b| b.identity.is_verified)
            .map(|b| b.identity.clone())
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_bot_id() {
        let app_id = AppId([1u8; 32]);
        let bot_id = BotId::from_app_id(&app_id);
        assert_eq!(bot_id.0, app_id.0);
    }

    #[test]
    fn test_bot_identity() {
        let app_id = AppId([1u8; 32]);
        let identity = BotIdentity::new(
            app_id,
            "test_bot".to_string(),
            "Test Bot".to_string(),
            [0u8; 32],
        );
        assert!(identity.is_ok());
    }

    #[test]
    fn test_invalid_username() {
        let app_id = AppId([1u8; 32]);

        // Too short
        let result = BotIdentity::new(app_id, "ab".to_string(), "Test".to_string(), [0u8; 32]);
        assert!(result.is_err());

        // Invalid chars
        let result = BotIdentity::new(
            app_id,
            "test@bot".to_string(),
            "Test".to_string(),
            [0u8; 32],
        );
        assert!(result.is_err());
    }

    #[test]
    fn test_command_parse() {
        let ctx = BotCommandContext::parse(
            "/start hello world",
            "user1".to_string(),
            "channel1".to_string(),
            "msg1".to_string(),
        );

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.command, "start");
        assert_eq!(ctx.args, vec!["hello", "world"]);
        assert_eq!(ctx.raw_args, "hello world");
    }

    #[test]
    fn test_command_parse_no_args() {
        let ctx = BotCommandContext::parse(
            "/help",
            "user1".to_string(),
            "channel1".to_string(),
            "msg1".to_string(),
        );

        assert!(ctx.is_some());
        let ctx = ctx.unwrap();
        assert_eq!(ctx.command, "help");
        assert!(ctx.args.is_empty());
    }

    #[test]
    fn test_not_a_command() {
        let ctx = BotCommandContext::parse(
            "hello world",
            "user1".to_string(),
            "channel1".to_string(),
            "msg1".to_string(),
        );
        assert!(ctx.is_none());
    }

    #[test]
    fn test_bot_keyboard() {
        let keyboard = BotKeyboard::inline(vec![vec![
            BotButton::callback("Option 1", "opt1"),
            BotButton::callback("Option 2", "opt2"),
        ]]);

        assert_eq!(keyboard.rows.len(), 1);
        assert_eq!(keyboard.rows[0].len(), 2);
    }

    #[test]
    fn test_command_result() {
        let result = BotCommandResult::text("Hello!")
            .with_keyboard(BotKeyboard::inline(vec![]))
            .as_reply();

        assert!(result.text.is_some());
        assert!(result.keyboard.is_some());
        assert!(result.reply_to_message);
    }

    struct TestHandler;

    impl BotCommandHandler for TestHandler {
        fn handle_command(&self, ctx: &BotCommandContext) -> MiniAppResult<BotCommandResult> {
            Ok(BotCommandResult::text(format!(
                "Received: /{}",
                ctx.command
            )))
        }

        fn handle_callback(&self, query: &BotCallbackQuery) -> MiniAppResult<BotCommandResult> {
            Ok(BotCommandResult::text(format!("Callback: {}", query.data)))
        }
    }

    #[test]
    fn test_bot_bridge() {
        let pm = Arc::new(PermissionManager::new());
        let bridge = BotBridge::new(pm);

        let app_id = AppId([1u8; 32]);
        let identity = BotIdentity::new(
            app_id,
            "test_bot".to_string(),
            "Test Bot".to_string(),
            [0u8; 32],
        )
        .unwrap()
        .with_commands(vec![BotCommandSpec {
            command: "start".to_string(),
            description: "Start the bot".to_string(),
        }]);

        bridge
            .register(identity, Arc::new(TestHandler), PermissionSet::new())
            .unwrap();

        // Get by username
        let bot = bridge.get_by_username("test_bot");
        assert!(bot.is_some());

        // List bots
        let bots = bridge.list();
        assert_eq!(bots.len(), 1);
    }
}
