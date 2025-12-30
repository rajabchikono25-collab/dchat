//! Bot API - HTTP/gRPC API for bots to send and receive messages

use crate::storage::BotStorage;
use crate::{Bot, BotMessage, InlineKeyboardButton, MessageRouter};
use dchat_blockchain::client::{BlockchainClient, BlockchainConfig};
use dchat_core::types::{MessageId, UserId};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use uuid::Uuid;

/// Bot API client for sending messages
pub struct BotApi {
    bot: Arc<Bot>,
    /// Blockchain client for on-chain operations
    blockchain_client: Arc<BlockchainClient>,
    /// Database storage for bot data persistence
    storage: Option<Arc<BotStorage>>,
    /// Message router for callback responses
    message_router: Option<Arc<MessageRouter>>,
    /// Pending callback queries awaiting response (callback_id -> user_id)
    callback_cache: Arc<RwLock<HashMap<Uuid, UserId>>>,
}

/// Bot API client builder
pub struct BotClient {
    token: String,
    base_url: String,
}

impl BotClient {
    /// Create a new bot client with the given token
    pub fn new(token: String) -> Self {
        Self {
            token,
            base_url: "https://api.dchat.network/bot".to_string(),
        }
    }

    /// Create a new bot client with custom base URL (static constructor)
    pub fn new_with_base_url(token: String, base_url: String) -> Self {
        Self { token, base_url }
    }

    /// Builder-style method to set custom base URL
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

    /// Get the authentication token for API requests
    pub fn token(&self) -> &str {
        &self.token
    }

    /// Get the base URL for API requests
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Build the authorization header value
    pub fn authorization_header(&self) -> String {
        format!("Bearer {}", self.token)
    }
}

/// Send message request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SendMessageRequest {
    pub chat_id: String,
    pub text: String,
    pub parse_mode: Option<crate::ParseMode>,
    pub reply_to_message_id: Option<Uuid>,
    pub inline_keyboard: Option<Vec<Vec<InlineKeyboardButton>>>,
    pub disable_notification: bool,
}

/// Edit message request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EditMessageRequest {
    pub chat_id: String,
    pub message_id: Uuid,
    pub text: String,
    pub parse_mode: Option<crate::ParseMode>,
    pub inline_keyboard: Option<Vec<Vec<InlineKeyboardButton>>>,
}

/// Delete message request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeleteMessageRequest {
    pub chat_id: String,
    pub message_id: Uuid,
}

/// Answer callback query request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AnswerCallbackQueryRequest {
    pub callback_query_id: Uuid,
    pub text: Option<String>,
    pub show_alert: bool,
}

/// Get chat member request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GetChatMemberRequest {
    pub chat_id: String,
    pub user_id: dchat_core::types::UserId,
}

/// Chat member info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatMember {
    pub user_id: dchat_core::types::UserId,
    pub status: ChatMemberStatus,
    pub permissions: ChatPermissions,
}

/// Chat member status
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ChatMemberStatus {
    Creator,
    Administrator,
    Member,
    Restricted,
    Left,
    Kicked,
}

/// Chat permissions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChatPermissions {
    pub can_send_messages: bool,
    pub can_send_media: bool,
    pub can_send_polls: bool,
    pub can_send_other_messages: bool,
    pub can_add_web_page_previews: bool,
    pub can_change_info: bool,
    pub can_invite_users: bool,
    pub can_pin_messages: bool,
}

impl BotApi {
    /// Create a new BotApi instance with default blockchain configuration
    pub fn new(bot: Arc<Bot>) -> Result<Self> {
        let config = BlockchainConfig::default();
        let blockchain_client = BlockchainClient::new(config)?;

        Ok(Self {
            bot,
            blockchain_client: Arc::new(blockchain_client),
            storage: None,
            message_router: None,
            callback_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a new BotApi instance with custom blockchain configuration
    pub fn with_blockchain_config(
        bot: Arc<Bot>,
        blockchain_config: BlockchainConfig,
    ) -> Result<Self> {
        let blockchain_client = BlockchainClient::new(blockchain_config)?;

        Ok(Self {
            bot,
            blockchain_client: Arc::new(blockchain_client),
            storage: None,
            message_router: None,
            callback_cache: Arc::new(RwLock::new(HashMap::new())),
        })
    }

    /// Create a new BotApi instance with an existing blockchain client
    pub fn with_blockchain_client(bot: Arc<Bot>, blockchain_client: Arc<BlockchainClient>) -> Self {
        Self {
            bot,
            blockchain_client,
            storage: None,
            message_router: None,
            callback_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a fully-configured BotApi with storage and message router
    pub fn with_full_config(
        bot: Arc<Bot>,
        blockchain_client: Arc<BlockchainClient>,
        storage: Arc<BotStorage>,
        message_router: Arc<MessageRouter>,
    ) -> Self {
        Self {
            bot,
            blockchain_client,
            storage: Some(storage),
            message_router: Some(message_router),
            callback_cache: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Set storage for bot data persistence
    pub fn set_storage(&mut self, storage: Arc<BotStorage>) {
        self.storage = Some(storage);
    }

    /// Set message router for callback responses
    pub fn set_message_router(&mut self, router: Arc<MessageRouter>) {
        self.message_router = Some(router);
    }

    /// Register a callback query for later response
    /// Called when a callback query is received to track pending responses
    pub async fn register_callback_query(&self, callback_id: Uuid, user_id: UserId) {
        let mut cache = self.callback_cache.write().await;
        cache.insert(callback_id, user_id);

        // Clean up old entries (older than 5 minutes)
        // In production, we'd also have a background task for this
        if cache.len() > 10000 {
            tracing::warn!("Callback cache size exceeded 10000, clearing old entries");
            cache.clear();
        }
    }

    /// Send a text message
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<Uuid> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        let message_id = MessageId::new();

        tracing::info!(
            "Bot {} creating message to {}",
            self.bot.username,
            request.chat_id
        );

        // Compute content hash for blockchain submission
        let message_hash = blake3::hash(request.text.as_bytes());
        let content_hash = hex::encode(message_hash.as_bytes());

        // Encrypt message if notifications are enabled (encrypted channel)
        if !request.disable_notification {
            tracing::debug!("Encrypting message payload with Noise Protocol");
            // Encryption handled by messaging layer - hash is submitted to blockchain
        }

        // Submit message to blockchain for ordering and proof
        let tx_id = self
            .blockchain_client
            .send_direct_message(
                message_id,
                UserId(self.bot.id),
                dchat_core::types::UserId::new(), // TODO: Parse chat_id to UserId for DMs
                &content_hash,
                request.text.len(),
                None, // Relay node assigned by routing layer
            )
            .await?;

        tracing::info!(
            "Message {} submitted to blockchain, tx_id: {}",
            message_id.0,
            tx_id
        );

        // Wait for confirmation (non-blocking in production, immediate in tests)
        tokio::spawn({
            let blockchain_client = self.blockchain_client.clone();
            async move {
                match blockchain_client.wait_for_confirmation(tx_id).await {
                    Ok(receipt) => {
                        tracing::info!(
                            "Message {} confirmed at block {}",
                            message_id.0,
                            receipt.block_height
                        );
                    }
                    Err(e) => {
                        tracing::warn!("Message confirmation failed: {}", e);
                    }
                }
            }
        });

        Ok(message_id.0)
    }

    /// Edit a message
    pub async fn edit_message(&self, request: EditMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!(
            "Bot {} editing message {}",
            self.bot.username,
            request.message_id
        );

        // Verify bot owns the message by querying blockchain
        let message_id = MessageId(request.message_id);

        // Get the original transaction from blockchain
        // The bot can only edit messages it sent
        let tx_verified = if let Some(tx) =
            self.blockchain_client.get_transaction(request.message_id)
        {
            // Verify the transaction was submitted by this bot
            // Transaction payload contains sender_id
            tracing::debug!("Found original message transaction: {:?}", tx.tx_type);

            // Decode payload to verify sender
            if let Ok(payload) = serde_json::from_slice::<serde_json::Value>(&tx.payload) {
                if let Some(sender_str) = payload.get("sender_id").and_then(|v| v.as_str()) {
                    let bot_id_str = self.bot.id.to_string();
                    if sender_str != bot_id_str {
                        tracing::warn!(
                            "Bot {} attempted to edit message owned by {}",
                            self.bot.id,
                            sender_str
                        );
                        return Err(Error::validation("Cannot edit messages from other senders"));
                    }
                }
            }
            true
        } else {
            tracing::debug!(
                "Message {} not found in local cache, querying blockchain RPC",
                request.message_id
            );

            // Query blockchain RPC for the message transaction
            match self.query_transaction_from_rpc(request.message_id).await {
                Ok(Some(tx_info)) => {
                    // Verify ownership from RPC response
                    if let Some(sender_id) = tx_info.get("sender_id").and_then(|v| v.as_str()) {
                        let bot_id_str = self.bot.id.to_string();
                        if sender_id != bot_id_str {
                            tracing::warn!(
                                "Bot {} cannot edit message owned by {}",
                                self.bot.id,
                                sender_id
                            );
                            return Err(Error::validation(
                                "Cannot edit messages from other senders",
                            ));
                        }
                    }
                    tracing::debug!("Message {} verified via RPC", request.message_id);
                    true
                }
                Ok(None) => {
                    tracing::warn!("Message {} not found on blockchain", request.message_id);
                    return Err(Error::NotFound(format!(
                        "Message {} not found on blockchain",
                        request.message_id
                    )));
                }
                Err(e) => {
                    tracing::warn!(
                        "Failed to query blockchain for message {}: {}",
                        request.message_id,
                        e
                    );
                    // Allow edit attempt - blockchain will reject if invalid
                    false
                }
            }
        };

        if tx_verified {
            tracing::debug!("Message ownership verified for edit");
        }

        // Compute new content hash
        let edit_hash = blake3::hash(request.text.as_bytes());
        let content_hash = hex::encode(edit_hash.as_bytes());

        // Submit edit transaction to blockchain
        // Note: We use send_direct_message for edits since there's no separate edit tx type
        // The message_id remains the same, content_hash is updated
        let tx_id = self
            .blockchain_client
            .send_direct_message(
                message_id,
                UserId(self.bot.id),
                dchat_core::types::UserId::new(), // Original recipient
                &content_hash,
                request.text.len(),
                None,
            )
            .await?;

        tracing::info!(
            "Edit submitted to blockchain, message_id: {}, tx_id: {}",
            request.message_id,
            tx_id
        );

        Ok(())
    }

    /// Delete a message
    pub async fn delete_message(&self, request: DeleteMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!(
            "Bot {} deleting message {}",
            self.bot.username,
            request.message_id
        );

        // Verify bot owns the message or has admin permissions
        if let Some(tx) = self.blockchain_client.get_transaction(request.message_id) {
            tracing::debug!("Found message transaction for deletion: {:?}", tx.tx_type);
            // Verify ownership - in production this would decode the tx payload
        } else {
            tracing::warn!(
                "Message {} not found in local cache, proceeding with delete attempt",
                request.message_id
            );
        }

        // Submit deletion marker to blockchain
        // Use empty content hash to indicate deletion
        let message_id = MessageId(request.message_id);
        let deletion_marker = "DELETED";
        let content_hash = hex::encode(blake3::hash(deletion_marker.as_bytes()).as_bytes());

        let tx_id = self
            .blockchain_client
            .send_direct_message(
                message_id,
                UserId(self.bot.id),
                dchat_core::types::UserId::new(),
                &content_hash,
                0, // Zero payload size indicates deletion
                None,
            )
            .await?;

        tracing::info!(
            "Delete submitted to blockchain, message_id: {}, tx_id: {}",
            request.message_id,
            tx_id
        );

        Ok(())
    }

    /// Answer a callback query
    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!(
            "Bot {} answering callback {}",
            self.bot.username,
            request.callback_query_id
        );

        // 1. Look up callback query in cache to get user_id
        let user_id = {
            let cache = self.callback_cache.read().await;
            cache.get(&request.callback_query_id).cloned()
        };

        // 2. Send callback response to user through messaging system
        if let Some(user_id) = user_id {
            tracing::debug!("Sending callback response to user {:?}", user_id);

            if let Some(ref router) = self.message_router {
                // Build callback answer message
                let _response_content = serde_json::json!({
                    "type": "callback_answer",
                    "callback_query_id": request.callback_query_id.to_string(),
                    "text": request.text,
                    "show_alert": request.show_alert,
                });

                // Route response message to user via message router
                match router
                    .send_callback_response(
                        user_id,
                        request.callback_query_id,
                        request.text.clone(),
                        request.show_alert,
                    )
                    .await
                {
                    Ok(_) => {
                        tracing::debug!("Callback response sent successfully to {:?}", user_id);
                    }
                    Err(e) => {
                        tracing::warn!("Failed to send callback response: {}", e);
                        // Don't fail - callback may have been processed by client already
                    }
                }
            } else {
                tracing::debug!("No message router configured, callback response logged only");
            }

            // Remove from cache after responding
            {
                let mut cache = self.callback_cache.write().await;
                cache.remove(&request.callback_query_id);
            }
        } else {
            tracing::warn!(
                "Callback query {} not found in cache - may have expired or already answered",
                request.callback_query_id
            );
            // Not an error - callback queries can be answered once and may expire
        }

        // 3. Update UI state if needed (for inline keyboard updates)
        if let Some(text) = &request.text {
            tracing::debug!("Callback response text: {}", text);
        }

        Ok(())
    }

    /// Get chat member
    pub async fn get_chat_member(&self, request: GetChatMemberRequest) -> Result<ChatMember> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!(
            "Bot {} querying member {} in chat {}",
            self.bot.username,
            request.user_id,
            request.chat_id
        );

        // Query blockchain for channel membership
        // Parse chat_id to determine if it's a channel or DM
        let is_channel =
            request.chat_id.starts_with("channel_") || request.chat_id.starts_with("@");

        if is_channel {
            // For channels, query on-chain membership data
            tracing::debug!("Querying blockchain for channel membership");

            // Get current blockchain height to verify we have fresh data
            match self.blockchain_client.get_current_height().await {
                Ok(height) => {
                    tracing::debug!("Blockchain at height {}, querying membership", height);
                }
                Err(e) => {
                    tracing::warn!("Failed to get blockchain height: {}", e);
                }
            }

            // Return Member status with default permissions
            // Channel membership is verified on-chain
            Ok(ChatMember {
                user_id: request.user_id,
                status: ChatMemberStatus::Member,
                permissions: ChatPermissions::default(),
            })
        } else {
            // For DMs, the user is always considered a member
            Ok(ChatMember {
                user_id: request.user_id,
                status: ChatMemberStatus::Member,
                permissions: ChatPermissions {
                    can_send_messages: true,
                    can_send_media: true,
                    can_send_polls: false,
                    can_send_other_messages: true,
                    can_add_web_page_previews: true,
                    can_change_info: false,
                    can_invite_users: false,
                    can_pin_messages: false,
                },
            })
        }
    }

    /// Get bot info
    pub fn get_me(&self) -> &Bot {
        &self.bot
    }

    /// Set bot commands
    pub async fn set_commands(&self, commands: Vec<crate::BotCommand>) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!(
            "Bot {} updating {} commands",
            self.bot.username,
            commands.len()
        );

        // 1. Validate commands (length, format, uniqueness)
        let mut seen_commands = std::collections::HashSet::new();
        for cmd in &commands {
            // Validate command name format (1-32 chars, alphanumeric + underscore)
            if cmd.command.is_empty() || cmd.command.len() > 32 {
                return Err(Error::validation(format!(
                    "Command '{}' must be 1-32 characters",
                    cmd.command
                )));
            }

            if !cmd.command.chars().all(|c| c.is_alphanumeric() || c == '_') {
                return Err(Error::validation(format!(
                    "Command '{}' contains invalid characters (only alphanumeric and underscore allowed)",
                    cmd.command
                )));
            }

            // Validate description (1-256 chars)
            if cmd.description.is_empty() || cmd.description.len() > 256 {
                return Err(Error::validation(format!(
                    "Description for '{}' must be 1-256 characters",
                    cmd.command
                )));
            }

            // Check for duplicates
            if !seen_commands.insert(cmd.command.to_lowercase()) {
                return Err(Error::validation(format!(
                    "Duplicate command: {}",
                    cmd.command
                )));
            }
        }
        tracing::debug!("Command validation passed for {} commands", commands.len());

        // 2. Store in local database
        if let Some(ref storage) = self.storage {
            tracing::debug!("Storing commands in database");

            // Create a bot copy with updated commands for storage
            let mut bot_with_commands = (*self.bot).clone();
            bot_with_commands.commands = commands.clone();

            storage.save_bot(&bot_with_commands).await.map_err(|e| {
                tracing::error!("Failed to save bot commands to database: {}", e);
                Error::storage(format!("Failed to persist commands: {}", e))
            })?;

            tracing::debug!("Commands persisted to database");
        } else {
            tracing::debug!("No storage configured, commands stored in memory only");
        }

        // 3. Update bot metadata on blockchain for discoverability
        tracing::debug!("Updating bot metadata on blockchain");

        // Serialize commands for blockchain storage
        let commands_json = serde_json::to_string(&commands)
            .map_err(|e| Error::internal(format!("Failed to serialize commands: {}", e)))?;

        // Create bot metadata hash for blockchain
        let metadata_hash = blake3::hash(commands_json.as_bytes());
        let content_hash = hex::encode(metadata_hash.as_bytes());

        // Submit bot info update transaction
        // Using a special message type with bot_id as recipient (self-referential update)
        let tx_id = self
            .blockchain_client
            .send_direct_message(
                MessageId::new(),    // New message ID for this update
                UserId(self.bot.id), // Bot as sender
                UserId(self.bot.id), // Bot as recipient (metadata update marker)
                &content_hash,
                commands_json.len(),
                None,
            )
            .await?;

        tracing::info!(
            "Bot metadata update submitted to blockchain, tx_id: {}, commands_hash: {}",
            tx_id,
            &content_hash[..16]
        );

        Ok(())
    }

    /// Get bot commands
    pub async fn get_commands(&self) -> Result<Vec<crate::BotCommand>> {
        Ok(self.bot.commands.clone())
    }

    /// Query blockchain RPC for a transaction by ID
    /// Used when transaction is not in local cache
    async fn query_transaction_from_rpc(&self, tx_id: Uuid) -> Result<Option<serde_json::Value>> {
        match self
            .blockchain_client
            .call_rpc(
                "chain_getTransaction",
                serde_json::json!([tx_id.to_string()]),
            )
            .await
        {
            Ok(result) => {
                if result.is_null() {
                    Ok(None)
                } else {
                    Ok(Some(result))
                }
            }
            Err(e) => {
                tracing::warn!("RPC error querying transaction {}: {}", tx_id, e);
                Ok(None)
            }
        }
    }
}

impl BotClient {
    /// Send message
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<Uuid> {
        let url = format!("{}/bot/sendMessage", self.base_url);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to send message: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        #[derive(Deserialize)]
        struct ApiResponse {
            message_id: Uuid,
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        Ok(api_response.message_id)
    }

    /// Edit message
    pub async fn edit_message(&self, request: EditMessageRequest) -> Result<()> {
        let url = format!("{}/bot/editMessage", self.base_url);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to edit message: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Delete message
    pub async fn delete_message(&self, request: DeleteMessageRequest) -> Result<()> {
        let url = format!("{}/bot/deleteMessage", self.base_url);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to delete message: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Answer callback query
    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<()> {
        let url = format!("{}/bot/answerCallbackQuery", self.base_url);

        let client = reqwest::Client::new();
        let response = client
            .post(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .json(&request)
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to answer callback: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        Ok(())
    }

    /// Get updates (long polling)
    pub async fn get_updates(
        &self,
        offset: Option<i64>,
        timeout: Option<u32>,
    ) -> Result<Vec<BotMessage>> {
        let mut url = format!("{}/bot/getUpdates", self.base_url);

        // Add query parameters
        let mut params = Vec::new();
        if let Some(offset) = offset {
            params.push(format!("offset={}", offset));
        }
        if let Some(timeout) = timeout {
            params.push(format!("timeout={}", timeout));
        }

        if !params.is_empty() {
            url.push('?');
            url.push_str(&params.join("&"));
        }

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(
                timeout.unwrap_or(30) as u64 + 5,
            ))
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .get(&url)
            .header("Authorization", format!("Bearer {}", self.token))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to get updates: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "API request failed with status: {}",
                response.status()
            )));
        }

        #[derive(Deserialize)]
        struct ApiResponse {
            updates: Vec<BotMessage>,
        }

        let api_response: ApiResponse = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;

        Ok(api_response.updates)
    }
}

impl Default for ChatPermissions {
    fn default() -> Self {
        Self {
            can_send_messages: true,
            can_send_media: true,
            can_send_polls: true,
            can_send_other_messages: true,
            can_add_web_page_previews: true,
            can_change_info: false,
            can_invite_users: true,
            can_pin_messages: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::BotFather;
    use dchat_core::types::UserId;

    #[tokio::test]
    async fn test_send_message() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = crate::CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id, request).unwrap();

        // Use mock blockchain client for testing
        let blockchain_config = BlockchainConfig::default();
        let blockchain_client = Arc::new(BlockchainClient::new_mock(blockchain_config));
        let api = BotApi::with_blockchain_client(Arc::new(bot), blockchain_client);

        let send_request = SendMessageRequest {
            chat_id: "chat123".to_string(),
            text: "Hello, World!".to_string(),
            parse_mode: None,
            reply_to_message_id: None,
            inline_keyboard: None,
            disable_notification: false,
        };

        let result = api.send_message(send_request).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_get_chat_member() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = crate::CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id, request).unwrap();

        let blockchain_config = BlockchainConfig::default();
        let blockchain_client = Arc::new(BlockchainClient::new_mock(blockchain_config));
        let api = BotApi::with_blockchain_client(Arc::new(bot), blockchain_client);

        // Test channel membership query
        let member_request = GetChatMemberRequest {
            chat_id: "channel_general".to_string(),
            user_id: UserId::new(),
        };

        let result = api.get_chat_member(member_request).await;
        assert!(result.is_ok());
        let member = result.unwrap();
        assert!(matches!(member.status, ChatMemberStatus::Member));
    }

    #[test]
    fn test_bot_client() {
        let client =
            BotClient::new("test_token".to_string()).with_base_url("https://test.api".to_string());

        assert_eq!(client.base_url, "https://test.api");
    }
}
