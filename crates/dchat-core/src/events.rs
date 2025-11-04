//! Event system for dchat

use crate::error::{Error, Result};
use crate::types::*;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::{broadcast, RwLock};

/// Event types in the dchat system
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Event {
    // User events
    UserRegistered {
        user_id: UserId,
        username: String,
        public_key: PublicKey,
    },
    UserUpdated {
        user_id: UserId,
        profile: UserProfile,
    },

    // Channel events
    ChannelCreated {
        channel: Channel,
    },
    ChannelUpdated {
        channel_id: ChannelId,
        name: Option<String>,
        description: Option<String>,
    },
    ChannelMemberJoined {
        channel_id: ChannelId,
        user_id: UserId,
    },
    ChannelMemberLeft {
        channel_id: ChannelId,
        user_id: UserId,
    },

    // Message events
    MessageReceived {
        message: Message,
    },
    MessageDelivered {
        message_id: MessageId,
        recipient: UserId,
    },
    MessageEdited {
        message_id: MessageId,
        new_content: MessageContent,
        edited_at: chrono::DateTime<chrono::Utc>,
    },

    // Network events
    PeerConnected {
        peer_id: String,
        addresses: Vec<String>,
    },
    PeerDisconnected {
        peer_id: String,
    },
    RelayNodeDiscovered {
        node_info: NodeInfo,
    },

    // Governance events
    ProposalCreated {
        proposal_id: String,
        creator: UserId,
        title: String,
        description: String,
    },
    VoteCast {
        proposal_id: String,
        voter: UserId,
        vote: bool,
        stake: u64,
    },
    ProposalExecuted {
        proposal_id: String,
        result: bool,
    },

    // System events
    SystemStarted,
    SystemShutdown,
    ConfigUpdated,
    ApplicationStarted,
    ApplicationStopped,

    // Error events
    Error {
        error: String,
        context: HashMap<String, String>,
    },
}

/// Event handler trait
#[async_trait]
pub trait EventHandler: Send + Sync {
    async fn handle_event(&self, event: &Event) -> Result<()>;
}

/// Event bus for managing events and handlers
pub struct EventBus {
    sender: broadcast::Sender<Event>,
    handlers: Arc<RwLock<Vec<Arc<dyn EventHandler>>>>,
}

impl EventBus {
    /// Create a new event bus
    pub fn new(capacity: usize) -> Self {
        let (sender, _) = broadcast::channel(capacity);

        Self {
            sender,
            handlers: Arc::new(RwLock::new(Vec::new())),
        }
    }

    /// Subscribe to events
    pub fn subscribe(&self) -> broadcast::Receiver<Event> {
        self.sender.subscribe()
    }

    /// Add an event handler
    pub async fn add_handler(&self, handler: Arc<dyn EventHandler>) {
        let mut handlers = self.handlers.write().await;
        handlers.push(handler);
    }

    /// Remove an event handler by index
    pub async fn remove_handler(&self, index: usize) -> Result<()> {
        let mut handlers = self.handlers.write().await;
        if index >= handlers.len() {
            return Err(Error::InvalidInput(
                "Handler index out of bounds".to_string(),
            ));
        }
        handlers.remove(index);
        Ok(())
    }

    /// Publish an event
    pub async fn publish(&self, event: Event) -> Result<()> {
        // Send to broadcast subscribers
        if let Err(e) = self.sender.send(event.clone()) {
            tracing::warn!("Failed to broadcast event: {}", e);
        }

        // Call registered handlers
        let handlers = self.handlers.read().await;
        for handler in handlers.iter() {
            if let Err(e) = handler.handle_event(&event).await {
                tracing::error!("Event handler failed: {}", e);
            }
        }

        Ok(())
    }

    /// Get the number of active subscribers
    pub fn subscriber_count(&self) -> usize {
        self.sender.receiver_count()
    }
}

impl Clone for EventBus {
    fn clone(&self) -> Self {
        Self {
            sender: self.sender.clone(),
            handlers: self.handlers.clone(),
        }
    }
}

/// A simple logging event handler for debugging
pub struct LoggingEventHandler;

#[async_trait]
impl EventHandler for LoggingEventHandler {
    async fn handle_event(&self, event: &Event) -> Result<()> {
        tracing::info!("Event: {:?}", event);
        Ok(())
    }
}
