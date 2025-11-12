//! dchat-messaging: Message handling and ordering
//!
//! This crate provides:
//! - Message creation and encryption
//! - Blockchain-based message ordering
//! - Delay-tolerant messaging
//! - Proof-of-delivery tracking
//! - Message expiration and lifecycle
//! - Advanced channel access control (token-gating, NFT verification)

pub mod channel_access;
pub mod delivery;
pub mod expiration;
pub mod media;
pub mod ordering;
pub mod queue;
pub mod rate_limit;
pub mod staking_verifier;
pub mod types;

pub use channel_access::{AccessPolicy, ChannelAccessManager};
pub use delivery::{DeliveryProof, DeliveryTracker};
pub use expiration::{ExpirationPolicy, MessageExpiration};
pub use media::{
    Animation, Audio, Contact, Document, EnhancedBotMessage, EntityType, LinkPreview, Location,
    MediaType, MessageEntity, Photo, PhotoSize, Poll, PollOption, PollType, Sticker, StickerType,
    Video, VideoNote, Voice,
};
pub use ordering::{MessageOrder, SequenceNumber};
pub use queue::{MessageQueue, OfflineQueue};
pub use rate_limit::{DropPolicy, RateLimitConfig, RateLimitMetrics, RateLimitResult, RateLimiter};
pub use staking_verifier::{ChainStakingVerifier, StakeStatus, StakingVerifier};

#[cfg(any(test, feature = "test-mocks"))]
pub use staking_verifier::MockStakingVerifier;
pub use types::{Message, MessageBuilder, MessageStatus, MessageType};
