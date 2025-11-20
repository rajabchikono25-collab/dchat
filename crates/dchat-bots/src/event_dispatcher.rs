//! Event dispatcher for bot message handling
//!
//! Routes incoming messages and events to registered bot handlers

use crate::{Bot, BotMessage, CallbackQuery};
use chrono::Utc;
use dchat_core::types::{ChannelId, MessageId, UserId};
use dchat_core::{Error, Result};
use dchat_messaging::{Message, MessageType};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;

/// Bot event type
#[derive(Debug, Clone)]
pub enum BotEvent {
    /// New message received
    Message(BotMessage),

    /// Message edited
    EditedMessage(BotMessage),

    /// Callback query from inline keyboard
    CallbackQuery(CallbackQuery),

    /// Bot added to channel
    BotAddedToChannel(ChannelId),

    /// Bot removed from channel
    BotRemovedFromChannel(ChannelId),

    /// Channel member updated
    ChannelMemberUpdated {
        channel_id: ChannelId,
        user_id: UserId,
        is_admin: bool,
    },
}

/// Bot event handler trait
#[async_trait::async_trait]
pub trait BotEventHandler: Send + Sync {
    /// Handle a bot event
    async fn handle_event(&self, bot: &Bot, event: BotEvent) -> Result<()>;
}

/// Event subscription
struct EventSubscription {
    bot_id: uuid::Uuid,
    handler: Arc<dyn BotEventHandler>,
}

/// Event dispatcher
pub struct EventDispatcher {
    /// Active subscriptions
    subscriptions: Arc<RwLock<HashMap<uuid::Uuid, Vec<Arc<dyn BotEventHandler>>>>>,

    /// Rate limiting per bot
    rate_limits: Arc<RwLock<HashMap<uuid::Uuid, BotRateLimit>>>,
}

/// Bot rate limit tracker
struct BotRateLimit {
    /// Messages processed in current minute
    messages_this_minute: u32,

    /// Current minute timestamp
    current_minute: i64,

    /// Max messages per minute
    max_per_minute: u32,
}

impl BotRateLimit {
    fn new(max_per_minute: u32) -> Self {
        Self {
            messages_this_minute: 0,
            current_minute: Utc::now().timestamp() / 60,
            max_per_minute,
        }
    }

    fn check_and_increment(&mut self) -> bool {
        let now_minute = Utc::now().timestamp() / 60;

        // Reset counter if we're in a new minute
        if now_minute > self.current_minute {
            self.current_minute = now_minute;
            self.messages_this_minute = 0;
        }

        // Check limit
        if self.messages_this_minute >= self.max_per_minute {
            return false;
        }

        self.messages_this_minute += 1;
        true
    }
}

impl EventDispatcher {
    /// Create a new event dispatcher
    pub fn new() -> Self {
        Self {
            subscriptions: Arc::new(RwLock::new(HashMap::new())),
            rate_limits: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Subscribe a bot to events
    pub async fn subscribe(
        &self,
        bot: &Bot,
        handler: Arc<dyn BotEventHandler>,
        max_events_per_minute: u32,
    ) -> Result<()> {
        let mut subscriptions = self.subscriptions.write().await;
        subscriptions
            .entry(bot.id)
            .or_insert_with(Vec::new)
            .push(handler);

        let mut rate_limits = self.rate_limits.write().await;
        rate_limits
            .entry(bot.id)
            .or_insert_with(|| BotRateLimit::new(max_events_per_minute));

        Ok(())
    }

    /// Unsubscribe a bot from events
    pub async fn unsubscribe(&self, bot_id: &uuid::Uuid) -> Result<()> {
        let mut subscriptions = self.subscriptions.write().await;
        subscriptions.remove(bot_id);

        let mut rate_limits = self.rate_limits.write().await;
        rate_limits.remove(bot_id);

        Ok(())
    }

    /// Dispatch an event to subscribed bots
    pub async fn dispatch_event(&self, bot: &Bot, event: BotEvent) -> Result<()> {
        // Check rate limit
        let mut rate_limits = self.rate_limits.write().await;
        if let Some(limit) = rate_limits.get_mut(&bot.id) {
            if !limit.check_and_increment() {
                return Err(Error::rate_limit(format!(
                    "Bot {} exceeded rate limit ({} events/min)",
                    bot.username, limit.max_per_minute
                )));
            }
        }
        drop(rate_limits);

        // Get handlers
        let subscriptions = self.subscriptions.read().await;
        let handlers = match subscriptions.get(&bot.id) {
            Some(h) => h.clone(),
            None => return Ok(()), // No handlers registered
        };
        drop(subscriptions);

        // Call all handlers concurrently
        let mut tasks = Vec::new();
        for handler in handlers {
            let bot = bot.clone();
            let event = event.clone();
            tasks.push(tokio::spawn(async move {
                handler.handle_event(&bot, event).await
            }));
        }

        // Wait for all handlers to complete
        for task in tasks {
            match task.await {
                Ok(Ok(())) => {}
                Ok(Err(e)) => {
                    tracing::error!("Bot event handler error: {}", e);
                }
                Err(e) => {
                    tracing::error!("Bot event handler panicked: {}", e);
                }
            }
        }

        Ok(())
    }

    /// Convert dchat Message to BotMessage
    pub fn message_to_bot_message(&self, message: &Message) -> Option<BotMessage> {
        let (sender, chat_id) = match &message.message_type {
            MessageType::Direct { sender, recipient } => (sender.clone(), recipient.0.to_string()),
            MessageType::Channel { sender, channel_id } => {
                (sender.clone(), channel_id.0.to_string())
            }
            MessageType::System { .. } => return None,
        };

        // Decrypt content for bot processing
        let text = match &message.content {
            dchat_core::types::MessageContent::Text(t) => t.clone(),
            dchat_core::types::MessageContent::Media { caption, .. } => {
                caption.clone().unwrap_or_default()
            }
            _ => String::new(),
        };

        Some(BotMessage {
            message_id: message.id.0,
            from_user_id: sender.0,
            chat_id,
            text,
            timestamp: message.timestamp,
            reply_to_message_id: None,
            entities: Vec::new(),
            media: None,
        })
    }

    /// Dispatch incoming message to bots
    pub async fn dispatch_message(&self, bot: &Bot, message: &Message) -> Result<()> {
        if let Some(bot_message) = self.message_to_bot_message(message) {
            self.dispatch_event(bot, BotEvent::Message(bot_message))
                .await?;
        }
        Ok(())
    }

    /// Dispatch callback query to bot
    pub async fn dispatch_callback_query(
        &self,
        bot: &Bot,
        query_id: uuid::Uuid,
        user_id: UserId,
        message: BotMessage,
        data: String,
    ) -> Result<()> {
        let callback_query = CallbackQuery {
            id: query_id,
            from: user_id,
            message,
            data,
            timestamp: chrono::Utc::now(),
        };

        self.dispatch_event(bot, BotEvent::CallbackQuery(callback_query))
            .await
    }

    /// Get rate limit stats for a bot
    pub async fn get_rate_limit_stats(&self, bot_id: &uuid::Uuid) -> Option<(u32, u32)> {
        let rate_limits = self.rate_limits.read().await;
        rate_limits
            .get(bot_id)
            .map(|limit| (limit.messages_this_minute, limit.max_per_minute))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct TestHandler {
        events: Arc<RwLock<Vec<BotEvent>>>,
    }

    #[async_trait::async_trait]
    impl BotEventHandler for TestHandler {
        async fn handle_event(&self, _bot: &Bot, event: BotEvent) -> Result<()> {
            let mut events = self.events.write().await;
            events.push(event);
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_event_dispatch() {
        let dispatcher = EventDispatcher::new();
        let bot = Bot::new("test_bot".to_string(), "Test Bot".to_string(), UserId(uuid::Uuid::new_v4())).unwrap();

        let events = Arc::new(RwLock::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            events: events.clone(),
        });

        dispatcher.subscribe(&bot, handler, 100).await.unwrap();

        let message = BotMessage {
            message_id: uuid::Uuid::new_v4(),
            from_user_id: uuid::Uuid::new_v4(),
            chat_id: "test_chat".to_string(),
            text: "Hello bot".to_string(),
            timestamp: std::time::SystemTime::now(),
            reply_to_message_id: None,
            entities: Vec::new(),
            media: None,
        };

        dispatcher
            .dispatch_event(&bot, BotEvent::Message(message))
            .await
            .unwrap();

        let events = events.read().await;
        assert_eq!(events.len(), 1);
    }

    #[tokio::test]
    async fn test_rate_limiting() {
        let dispatcher = EventDispatcher::new();
        let bot = Bot::new("test_bot".to_string(), "Test Bot".to_string(), UserId(uuid::Uuid::new_v4())).unwrap();

        let events = Arc::new(RwLock::new(Vec::new()));
        let handler = Arc::new(TestHandler {
            events: events.clone(),
        });

        // Set low rate limit for testing
        dispatcher.subscribe(&bot, handler, 2).await.unwrap();

        // First two should succeed
        for _ in 0..2 {
            let message = BotMessage {
                message_id: uuid::Uuid::new_v4(),
                from_user_id: uuid::Uuid::new_v4(),
                chat_id: "test_chat".to_string(),
                text: "Hello".to_string(),
                timestamp: std::time::SystemTime::now(),
                reply_to_message_id: None,
                entities: Vec::new(),
                media: None,
            };

            dispatcher
                .dispatch_event(&bot, BotEvent::Message(message))
                .await
                .unwrap();
        }

        // Third should fail
        let message = BotMessage {
            message_id: uuid::Uuid::new_v4(),
            from_user_id: uuid::Uuid::new_v4(),
            chat_id: "test_chat".to_string(),
            text: "Hello".to_string(),
            timestamp: std::time::SystemTime::now(),
            reply_to_message_id: None,
            entities: Vec::new(),
            media: None,
        };

        let result = dispatcher
            .dispatch_event(&bot, BotEvent::Message(message))
            .await;

        assert!(result.is_err());
    }
}
