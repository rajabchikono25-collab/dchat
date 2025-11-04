//! Message types and builders

use dchat_core::types::{ChannelId, MessageContent, MessageId, UserId};
use serde::{Deserialize, Serialize};
use std::time::SystemTime;

/// Message status in the system
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageStatus {
    /// Message created locally, not yet sent
    Created,

    /// Message sent to network
    Sent,

    /// Message delivered to recipient (proof received)
    Delivered,

    /// Message read by recipient
    Read,

    /// Message failed to deliver
    Failed,

    /// Message expired
    Expired,
}

/// Message type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageType {
    /// Direct message between two users
    Direct { sender: UserId, recipient: UserId },

    /// Channel message
    Channel {
        sender: UserId,
        channel_id: ChannelId,
    },

    /// System message
    System { content: String },
}

/// Complete message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    /// Unique message identifier
    pub id: MessageId,

    /// Message type
    pub message_type: MessageType,

    /// Message content (encrypted)
    pub content: MessageContent,

    /// Encrypted payload (for wire transmission)
    pub encrypted_payload: Vec<u8>,

    /// Timestamp
    pub timestamp: SystemTime,

    /// Chain sequence number (for ordering)
    pub sequence: Option<u64>,

    /// Message status
    pub status: MessageStatus,

    /// Expiration time
    pub expires_at: Option<SystemTime>,

    /// Message size in bytes
    pub size: usize,
}

impl Message {
    /// Check if message has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires_at) = self.expires_at {
            SystemTime::now() > expires_at
        } else {
            false
        }
    }

    /// Check if message is deliverable
    pub fn is_deliverable(&self) -> bool {
        !self.is_expired() && self.status != MessageStatus::Expired
    }

    /// Get sender user ID
    pub fn sender(&self) -> Option<UserId> {
        match &self.message_type {
            MessageType::Direct { sender, .. } => Some(sender.clone()),
            MessageType::Channel { sender, .. } => Some(sender.clone()),
            MessageType::System { .. } => None,
        }
    }

    /// Get recipient user ID (if direct message)
    pub fn recipient(&self) -> Option<UserId> {
        match &self.message_type {
            MessageType::Direct { recipient, .. } => Some(recipient.clone()),
            _ => None,
        }
    }
}

/// Builder for creating messages
pub struct MessageBuilder {
    message_type: Option<MessageType>,
    content: Option<MessageContent>,
    encrypted_payload: Option<Vec<u8>>,
    expires_at: Option<SystemTime>,
}

impl MessageBuilder {
    pub fn new() -> Self {
        Self {
            message_type: None,
            content: None,
            encrypted_payload: None,
            expires_at: None,
        }
    }

    /// Set as direct message
    pub fn direct(mut self, sender: UserId, recipient: UserId) -> Self {
        self.message_type = Some(MessageType::Direct { sender, recipient });
        self
    }

    /// Set as channel message
    pub fn channel(mut self, sender: UserId, channel_id: ChannelId) -> Self {
        self.message_type = Some(MessageType::Channel { sender, channel_id });
        self
    }

    /// Set message content
    pub fn content(mut self, content: MessageContent) -> Self {
        self.content = Some(content);
        self
    }

    /// Set encrypted payload
    pub fn encrypted_payload(mut self, payload: Vec<u8>) -> Self {
        self.encrypted_payload = Some(payload);
        self
    }

    /// Set expiration time
    pub fn expires_at(mut self, expires_at: SystemTime) -> Self {
        self.expires_at = Some(expires_at);
        self
    }

    /// Set expiration duration from now
    pub fn expires_in(mut self, duration: std::time::Duration) -> Self {
        self.expires_at = Some(SystemTime::now() + duration);
        self
    }

    /// Build the message
    pub fn build(self) -> Result<Message, String> {
        let message_type = self.message_type.ok_or("Message type not set")?;
        let content = self.content.ok_or("Content not set")?;
        let encrypted_payload = self.encrypted_payload.ok_or("Encrypted payload not set")?;

        let size = encrypted_payload.len();

        Ok(Message {
            id: MessageId(uuid::Uuid::new_v4()),
            message_type,
            content,
            encrypted_payload,
            timestamp: SystemTime::now(),
            sequence: None,
            status: MessageStatus::Created,
            expires_at: self.expires_at,
            size,
        })
    }
}

impl Default for MessageBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_builder() {
        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let message = MessageBuilder::new()
            .direct(sender.clone(), recipient.clone())
            .content(MessageContent::Text("Hello".to_string()))
            .encrypted_payload(vec![1, 2, 3, 4])
            .build();

        assert!(message.is_ok());
        let msg = message.unwrap();
        assert_eq!(msg.sender(), Some(sender));
        assert_eq!(msg.recipient(), Some(recipient));
        assert!(msg.is_deliverable());
    }

    #[test]
    fn test_message_expiration() {
        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let past = SystemTime::now() - std::time::Duration::from_secs(10);

        let message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Expired".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .expires_at(past)
            .build()
            .unwrap();

        assert!(message.is_expired());
        assert!(!message.is_deliverable());
    }
}
