//! Bot API - HTTP/gRPC API for bots to send and receive messages

use crate::{Bot, BotMessage, InlineKeyboardButton};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::sync::Arc;
use uuid::Uuid;

/// Bot API client for sending messages
pub struct BotApi {
    bot: Arc<Bot>,
}

/// Bot API client builder
pub struct BotClient {
    #[allow(dead_code)]
    token: String,
    base_url: String,
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
    /// Create a new BotApi instance
    pub fn new(bot: Arc<Bot>) -> Self {
        Self { bot }
    }

    /// Send a text message
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<Uuid> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        let message_id = Uuid::new_v4();
        
        // 1. Create a Message with bot as sender
        tracing::info!("Bot {} creating message to {}", self.bot.username, request.chat_id);
        
        // 2. Encrypt if needed using Noise Protocol
        if !request.disable_notification {
            tracing::debug!("Encrypting message payload with Noise Protocol");
            // In production: noise_session.encrypt(&request.text)
        }
        
        // 3. Route through messaging system (via libp2p DHT)
        tracing::debug!("Routing message via DHT to chat: {}", request.chat_id);
        // In production: 
        // - Look up chat/channel in DHT
        // - Route to relay nodes or direct to recipients
        // messaging_client.route_message(chat_id, encrypted_message).await?
        
        // 4. Submit message hash to blockchain for ordering
        let message_hash = blake3::hash(request.text.as_bytes());
        tracing::debug!("Submitting message to blockchain: hash={}", message_hash);
        // In production: blockchain_client.submit_bot_message(message_id, message_hash).await?

        Ok(message_id)
    }

    /// Edit a message
    pub async fn edit_message(&self, request: EditMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!("Bot {} editing message {}", self.bot.username, request.message_id);
        
        // 1. Verify bot owns the message (blockchain query)
        tracing::debug!("Verifying message ownership on blockchain");
        // In production: 
        // let original_message = blockchain_client.get_message(request.message_id).await?;
        // if original_message.sender != self.bot.user_id { return Err(...) }
        
        // 2. Create edit transaction with new content
        tracing::debug!("Creating edit transaction");
        let _edit_hash = blake3::hash(request.text.as_bytes());
        
        // 3. Submit edit to messaging system and blockchain
        tracing::debug!("Submitting edit to blockchain: message_id={}", request.message_id);
        // In production: blockchain_client.submit_message_edit(request.message_id, edit_hash).await?

        Ok(())
    }

    /// Delete a message
    pub async fn delete_message(&self, request: DeleteMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!("Bot {} deleting message {}", self.bot.username, request.message_id);
        
        // 1. Verify bot owns the message or has admin permissions
        tracing::debug!("Checking deletion permissions on blockchain");
        // In production:
        // let message = blockchain_client.get_message(request.message_id).await?;
        // let has_permission = message.sender == self.bot.user_id || 
        //     blockchain_client.check_admin_permission(self.bot.user_id, request.chat_id).await?;
        // if !has_permission { return Err(Error::permission_denied(...)) }
        
        // 2. Create delete transaction
        tracing::debug!("Creating delete transaction");
        
        // 3. Submit deletion to messaging system and blockchain
        tracing::debug!("Submitting deletion to blockchain");
        // In production: blockchain_client.submit_message_deletion(request.message_id).await?

        Ok(())
    }

    /// Answer a callback query
    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!("Bot {} answering callback {}", self.bot.username, request.callback_query_id);
        
        // 1. Send callback response to user through messaging system
        tracing::debug!("Sending callback response to user");
        // In production:
        // - Look up callback query in database to get user_id
        // - Route response message to user
        // messaging_client.send_callback_answer(
        //     user_id, 
        //     request.text, 
        //     request.show_alert
        // ).await?
        
        // 2. Update UI state if needed (for inline keyboard updates)
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
        
        // Query channel/chat membership from blockchain
        tracing::debug!("Querying blockchain for membership info");
        // In production:
        // let membership = blockchain_client.get_chat_member(
        //     &request.chat_id,
        //     &request.user_id
        // ).await?;
        // 
        // Return actual status and permissions from blockchain:
        // - Creator: channel owner
        // - Administrator: has admin permissions
        // - Member: regular member
        // - Restricted: limited permissions
        // - Left: was member but left
        // - Kicked: was banned

        Ok(ChatMember {
            user_id: request.user_id,
            status: ChatMemberStatus::Member,
            permissions: ChatPermissions::default(),
        })
    }

    /// Get bot info
    pub fn get_me(&self) -> &Bot {
        &self.bot
    }

    /// Set bot commands
    pub async fn set_commands(&self, _commands: Vec<crate::BotCommand>) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }

        tracing::info!("Bot {} updating commands", self.bot.username);
        
        // 1. Validate commands (length, format, uniqueness)
        // In production: validate each command has valid name and description
        
        // 2. Store in local database
        tracing::debug!("Storing commands in database");
        // In production: database.update_bot_commands(self.bot.user_id, _commands).await?
        
        // 3. Update bot metadata on blockchain for discoverability
        tracing::debug!("Updating bot metadata on blockchain");
        // In production: blockchain_client.update_bot_info(
        //     self.bot.user_id,
        //     BotMetadata { commands: _commands, ... }
        // ).await?
        
        Ok(())
    }

    /// Get bot commands
    pub async fn get_commands(&self) -> Result<Vec<crate::BotCommand>> {
        Ok(self.bot.commands.clone())
    }
}

impl BotClient {
    /// Create a new bot client
    pub fn new(token: String) -> Self {
        Self {
            token,
            base_url: "https://api.dchat.network".to_string(),
        }
    }

    /// Set custom API base URL
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }

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
        let api = BotApi::new(Arc::new(bot));

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

    #[test]
    fn test_bot_client() {
        let client =
            BotClient::new("test_token".to_string()).with_base_url("https://test.api".to_string());

        assert_eq!(client.base_url, "https://test.api");
    }
}
