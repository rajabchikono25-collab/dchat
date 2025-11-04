//! Message queue for offline and delay-tolerant messaging

use crate::types::Message;
use dchat_core::error::{Error, Result};
use dchat_core::types::UserId;
use std::collections::{HashMap, VecDeque};

/// Message queue for a single user
pub struct MessageQueue {
    /// Messages waiting to be delivered
    pending: VecDeque<Message>,

    /// Maximum queue size
    max_size: usize,

    /// Total bytes in queue
    total_bytes: usize,

    /// Maximum total bytes
    max_bytes: usize,
}

impl MessageQueue {
    pub fn new(max_size: usize, max_bytes: usize) -> Self {
        Self {
            pending: VecDeque::new(),
            max_size,
            total_bytes: 0,
            max_bytes,
        }
    }

    /// Add a message to the queue
    pub fn push(&mut self, message: Message) -> Result<()> {
        if self.pending.len() >= self.max_size {
            return Err(Error::messaging("Queue full".to_string()));
        }

        if self.total_bytes + message.size > self.max_bytes {
            return Err(Error::messaging("Queue size limit exceeded".to_string()));
        }

        self.total_bytes += message.size;
        self.pending.push_back(message);
        Ok(())
    }

    /// Get next message from queue
    pub fn pop(&mut self) -> Option<Message> {
        if let Some(message) = self.pending.pop_front() {
            self.total_bytes = self.total_bytes.saturating_sub(message.size);
            Some(message)
        } else {
            None
        }
    }

    /// Peek at next message without removing
    pub fn peek(&self) -> Option<&Message> {
        self.pending.front()
    }

    /// Get queue length
    pub fn len(&self) -> usize {
        self.pending.len()
    }

    /// Check if queue is empty
    pub fn is_empty(&self) -> bool {
        self.pending.is_empty()
    }

    /// Get total bytes in queue
    pub fn total_bytes(&self) -> usize {
        self.total_bytes
    }

    /// Remove expired messages
    pub fn remove_expired(&mut self) -> usize {
        let initial_len = self.pending.len();

        self.pending.retain(|msg| {
            if msg.is_expired() {
                self.total_bytes = self.total_bytes.saturating_sub(msg.size);
                false
            } else {
                true
            }
        });

        initial_len - self.pending.len()
    }
}

/// Offline message queue manager
pub struct OfflineQueue {
    /// Queues per user
    queues: HashMap<UserId, MessageQueue>,

    /// Default queue configuration
    default_max_messages: usize,
    default_max_bytes: usize,
}

impl OfflineQueue {
    pub fn new(default_max_messages: usize, default_max_bytes: usize) -> Self {
        Self {
            queues: HashMap::new(),
            default_max_messages,
            default_max_bytes,
        }
    }

    /// Queue a message for an offline user
    pub fn enqueue(&mut self, user_id: UserId, message: Message) -> Result<()> {
        let queue = self.queues.entry(user_id).or_insert_with(|| {
            MessageQueue::new(self.default_max_messages, self.default_max_bytes)
        });

        queue.push(message)
    }

    /// Get all pending messages for a user
    pub fn dequeue_all(&mut self, user_id: &UserId) -> Vec<Message> {
        if let Some(mut queue) = self.queues.remove(user_id) {
            let mut messages = Vec::new();
            while let Some(msg) = queue.pop() {
                messages.push(msg);
            }
            messages
        } else {
            Vec::new()
        }
    }

    /// Get pending message count for a user
    pub fn pending_count(&self, user_id: &UserId) -> usize {
        self.queues.get(user_id).map(|q| q.len()).unwrap_or(0)
    }

    /// Get total pending message count
    pub fn total_pending(&self) -> usize {
        self.queues.values().map(|q| q.len()).sum()
    }

    /// Remove expired messages from all queues
    pub fn cleanup_expired(&mut self) -> usize {
        let mut removed = 0;

        for queue in self.queues.values_mut() {
            removed += queue.remove_expired();
        }

        // Remove empty queues
        self.queues.retain(|_, queue| !queue.is_empty());

        removed
    }

    /// Get statistics
    pub fn stats(&self) -> OfflineQueueStats {
        let total_users = self.queues.len();
        let total_messages = self.total_pending();
        let total_bytes: usize = self.queues.values().map(|q| q.total_bytes()).sum();

        OfflineQueueStats {
            total_users,
            total_messages,
            total_bytes,
        }
    }
}

impl Default for OfflineQueue {
    fn default() -> Self {
        Self::new(1000, 10_000_000) // 1000 messages, 10MB per user
    }
}

#[derive(Debug, Clone)]
pub struct OfflineQueueStats {
    pub total_users: usize,
    pub total_messages: usize,
    pub total_bytes: usize,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::MessageBuilder;
    use dchat_core::types::MessageContent;

    #[test]
    fn test_message_queue() {
        let mut queue = MessageQueue::new(10, 10000);

        let sender = UserId(uuid::Uuid::new_v4());
        let recipient = UserId(uuid::Uuid::new_v4());

        let message = MessageBuilder::new()
            .direct(sender, recipient)
            .content(MessageContent::Text("Test".to_string()))
            .encrypted_payload(vec![1, 2, 3, 4])
            .build()
            .unwrap();

        assert!(queue.push(message).is_ok());
        assert_eq!(queue.len(), 1);

        let popped = queue.pop();
        assert!(popped.is_some());
        assert_eq!(queue.len(), 0);
    }

    #[test]
    fn test_offline_queue() {
        let mut queue = OfflineQueue::default();

        let user_id = UserId(uuid::Uuid::new_v4());
        let sender = UserId(uuid::Uuid::new_v4());

        let message = MessageBuilder::new()
            .direct(sender, user_id.clone())
            .content(MessageContent::Text("Test message".to_string()))
            .encrypted_payload(vec![1, 2, 3])
            .build()
            .unwrap();

        assert!(queue.enqueue(user_id.clone(), message).is_ok());
        assert_eq!(queue.pending_count(&user_id), 1);

        let messages = queue.dequeue_all(&user_id);
        assert_eq!(messages.len(), 1);
        assert_eq!(queue.pending_count(&user_id), 0);
    }
}
