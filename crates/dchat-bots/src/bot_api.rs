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
        
        // In production, this would:
        // 1. Create a Message with bot as sender
        // 2. Encrypt if needed using Noise Protocol
        // 3. Route through messaging system
        // 4. Submit to blockchain for ordering
        let message_id = Uuid::new_v4();
        tracing::info!("Bot {} sending message to {}", self.bot.username, request.chat_id);
        
        Ok(message_id)
    }
    
    /// Edit a message
    pub async fn edit_message(&self, request: EditMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }
        
        // In production, this would:
        // 1. Verify bot owns the message
        // 2. Create edit transaction
        // 3. Submit to messaging system
        tracing::info!("Bot {} editing message {}", self.bot.username, request.message_id);
        
        Ok(())
    }
    
    /// Delete a message
    pub async fn delete_message(&self, request: DeleteMessageRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }
        
        // In production, this would:
        // 1. Verify bot owns the message or has permissions
        // 2. Create delete transaction
        // 3. Submit to messaging system
        tracing::info!("Bot {} deleting message {}", self.bot.username, request.message_id);
        
        Ok(())
    }
    
    /// Answer a callback query
    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<()> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }
        
        // In production, this would:
        // 1. Send callback response to user through messaging system
        // 2. Update UI state if needed
        tracing::info!("Bot {} answering callback {}", self.bot.username, request.callback_query_id);
        
        Ok(())
    }
    
    /// Get chat member
    pub async fn get_chat_member(&self, request: GetChatMemberRequest) -> Result<ChatMember> {
        if !self.bot.is_active {
            return Err(Error::validation("Bot is not active"));
        }
        
        // In production, this would query channel/chat membership from blockchain
        tracing::info!("Bot {} querying member {} in chat {}", 
            self.bot.username, request.user_id, request.chat_id);
        
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
        
        // In production, this would:
        // 1. Validate commands
        // 2. Store in database
        // 3. Update bot metadata on blockchain
        tracing::info!("Bot {} updating commands", self.bot.username);
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
    pub async fn send_message(&self, _request: SendMessageRequest) -> Result<Uuid> {
        // TODO: HTTP request to API
        Ok(Uuid::new_v4())
    }
    
    /// Edit message
    pub async fn edit_message(&self, _request: EditMessageRequest) -> Result<()> {
        // TODO: HTTP request to API
        Ok(())
    }
    
    /// Delete message
    pub async fn delete_message(&self, _request: DeleteMessageRequest) -> Result<()> {
        // TODO: HTTP request to API
        Ok(())
    }
    
    /// Answer callback query
    pub async fn answer_callback_query(&self, _request: AnswerCallbackQueryRequest) -> Result<()> {
        // TODO: HTTP request to API
        Ok(())
    }
    
    /// Get updates (long polling)
    pub async fn get_updates(&self, _offset: Option<i64>, _timeout: Option<u32>) -> Result<Vec<BotMessage>> {
        // TODO: HTTP request to API for updates
        Ok(Vec::new())
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
        let client = BotClient::new("test_token".to_string())
            .with_base_url("https://test.api".to_string());
        
        assert_eq!(client.base_url, "https://test.api");
    }
}
