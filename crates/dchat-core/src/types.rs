//! Core types for dchat

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Unique identifier for users
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct UserId(pub Uuid);

impl UserId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        self.0.as_bytes()
    }
}

impl Default for UserId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for UserId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for channels
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ChannelId(pub Uuid);

impl ChannelId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ChannelId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ChannelId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Unique identifier for messages
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct MessageId(pub Uuid);

impl MessageId {
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for MessageId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Public key representation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PublicKey(pub Vec<u8>);

impl PublicKey {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Digital signature representation
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature(pub Vec<u8>);

impl Signature {
    #[must_use]
    pub fn new(bytes: Vec<u8>) -> Self {
        Self(bytes)
    }

    #[must_use]
    pub fn as_bytes(&self) -> &[u8] {
        &self.0
    }
}

/// Reputation score for users
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReputationScore {
    pub total: u64,
    pub messaging: u64,
    pub governance: u64,
    pub relay: u64,
    pub last_updated: DateTime<Utc>,
}

impl Default for ReputationScore {
    fn default() -> Self {
        Self {
            total: 0,
            messaging: 0,
            governance: 0,
            relay: 0,
            last_updated: Utc::now(),
        }
    }
}

/// Channel types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ChannelType {
    Public,
    Private,
    TokenGated {
        required_tokens: u64,
        token_contract: String,
    },
}

/// Message content types
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageContent {
    Text(String),
    Image {
        data: Vec<u8>,
        mime_type: String,
    },
    File {
        data: Vec<u8>,
        filename: String,
        mime_type: String,
    },
    Audio {
        data: Vec<u8>,
        duration_ms: u64,
    },
    Video {
        data: Vec<u8>,
        duration_ms: u64,
        width: u32,
        height: u32,
    },
    Sticker {
        pack_id: String,
        sticker_id: String,
    },
    System(String),
}

/// User profile information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UserProfile {
    pub user_id: UserId,
    pub username: String,
    pub display_name: Option<String>,
    pub avatar_hash: Option<String>,
    pub bio: Option<String>,
    pub public_key: PublicKey,
    pub reputation: ReputationScore,
    pub created_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub badges: Vec<String>,
    pub verified: bool,
}

/// Channel information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Channel {
    pub id: ChannelId,
    pub name: String,
    pub description: Option<String>,
    pub channel_type: ChannelType,
    pub creator: UserId,
    pub created_at: DateTime<Utc>,
    pub member_count: u64,
    pub metadata: HashMap<String, String>,
}

/// Message structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub channel_id: ChannelId,
    pub sender_id: UserId,
    pub content: MessageContent,
    pub timestamp: DateTime<Utc>,
    pub sequence_number: u64,
    pub reply_to: Option<MessageId>,
    pub edited_at: Option<DateTime<Utc>>,
    pub signature: Option<Signature>,
}

/// Network node information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodeInfo {
    pub peer_id: String,
    pub addresses: Vec<String>,
    pub protocol_version: String,
    pub uptime: u64,
    pub relay_score: u64,
    pub last_seen: DateTime<Utc>,
}
