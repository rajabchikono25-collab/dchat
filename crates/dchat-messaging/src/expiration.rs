//! Message expiration policies

use crate::types::Message;
use dchat_core::types::MessageId;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::{Duration, SystemTime};

/// Expiration policy for messages
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum ExpirationPolicy {
    /// Never expire
    Never,

    /// Expire after a fixed duration
    Duration(Duration),

    /// Expire at a specific time
    At(SystemTime),

    /// Expire after being read
    AfterRead,

    /// Expire after a certain number of views
    AfterViews(u32),
}

impl ExpirationPolicy {
    /// Check if a message should expire based on this policy
    pub fn should_expire(&self, message: &Message, views: u32) -> bool {
        match self {
            Self::Never => false,
            Self::Duration(duration) => {
                if let Ok(elapsed) = message.timestamp.elapsed() {
                    elapsed >= *duration
                } else {
                    false
                }
            }
            Self::At(time) => SystemTime::now() >= *time,
            Self::AfterRead => views > 0,
            Self::AfterViews(max_views) => views >= *max_views,
        }
    }
}

/// Message expiration manager
pub struct MessageExpiration {
    /// Policies per message
    policies: HashMap<MessageId, ExpirationPolicy>,

    /// View counts per message
    view_counts: HashMap<MessageId, u32>,
}

impl MessageExpiration {
    pub fn new() -> Self {
        Self {
            policies: HashMap::new(),
            view_counts: HashMap::new(),
        }
    }

    /// Set expiration policy for a message
    pub fn set_policy(&mut self, message_id: MessageId, policy: ExpirationPolicy) {
        self.policies.insert(message_id, policy);
    }

    /// Record a message view
    pub fn record_view(&mut self, message_id: MessageId) {
        *self.view_counts.entry(message_id).or_insert(0) += 1;
    }

    /// Check if a message should expire
    pub fn should_expire(&self, message: &Message) -> bool {
        // Check built-in expiration
        if message.is_expired() {
            return true;
        }

        // Check policy-based expiration
        if let Some(policy) = self.policies.get(&message.id) {
            let views = self.view_counts.get(&message.id).copied().unwrap_or(0);
            policy.should_expire(message, views)
        } else {
            false
        }
    }

    /// Get view count for a message
    pub fn view_count(&self, message_id: &MessageId) -> u32 {
        self.view_counts.get(message_id).copied().unwrap_or(0)
    }

    /// Remove expiration data for a message
    pub fn remove(&mut self, message_id: &MessageId) {
        self.policies.remove(message_id);
        self.view_counts.remove(message_id);
    }

    /// Clean up expired messages from tracking
    pub fn cleanup_expired(&mut self, messages: &[Message]) {
        let expired_ids: Vec<_> = messages
            .iter()
            .filter(|msg| self.should_expire(msg))
            .map(|msg| msg.id)
            .collect();

        for id in expired_ids {
            self.remove(&id);
        }
    }
}

impl Default for MessageExpiration {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageBuilder;
    use dchat_core::types::{MessageContent, UserId};

    #[test]
    fn test_expiration_policy_duration() {
        let policy = ExpirationPolicy::Duration(Duration::from_secs(60));

        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let mut message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Test".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .build()
            .unwrap();

        // Fresh message should not expire
        assert!(!policy.should_expire(&message, 0));

        // Old message should expire
        message.timestamp = SystemTime::now() - Duration::from_secs(120);
        assert!(policy.should_expire(&message, 0));
    }

    #[test]
    fn test_expiration_policy_after_read() {
        let policy = ExpirationPolicy::AfterRead;

        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Test".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .build()
            .unwrap();

        assert!(!policy.should_expire(&message, 0)); // Not read
        assert!(policy.should_expire(&message, 1)); // Read once
    }

    #[test]
    fn test_message_expiration_manager() {
        let mut manager = MessageExpiration::new();

        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Test".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .build()
            .unwrap();

        manager.set_policy(message.id.clone(), ExpirationPolicy::AfterViews(3));

        assert!(!manager.should_expire(&message));

        manager.record_view(message.id.clone());
        manager.record_view(message.id.clone());
        assert!(!manager.should_expire(&message));

        manager.record_view(message.id.clone());
        assert!(manager.should_expire(&message));
    }
}
