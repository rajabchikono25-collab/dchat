//! Media types and handling for bot messages

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// Media type in messages
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum MediaType {
    /// Photo/image
    Photo,
    /// Video file
    Video,
    /// Audio file
    Audio,
    /// Voice message
    Voice,
    /// Document/file
    Document,
    /// Sticker
    Sticker,
    /// Animation/GIF
    Animation,
    /// Video note (round video)
    VideoNote,
    /// Location
    Location,
    /// Contact
    Contact,
    /// Poll
    Poll,
}

/// Photo attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Photo {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Photo width
    pub width: u32,

    /// Photo height
    pub height: u32,

    /// File size in bytes
    pub file_size: Option<u64>,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,
}

/// Photo size variant
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhotoSize {
    /// File ID
    pub file_id: String,

    /// Width
    pub width: u32,

    /// Height
    pub height: u32,

    /// File size
    pub file_size: Option<u64>,
}

/// Video attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Video {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Video width
    pub width: u32,

    /// Video height
    pub height: u32,

    /// Duration in seconds
    pub duration: u32,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,

    /// MIME type
    pub mime_type: Option<String>,

    /// File size
    pub file_size: Option<u64>,
}

/// Audio attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Audio {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Duration in seconds
    pub duration: u32,

    /// Performer
    pub performer: Option<String>,

    /// Title
    pub title: Option<String>,

    /// MIME type
    pub mime_type: Option<String>,

    /// File size
    pub file_size: Option<u64>,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,
}

/// Voice message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Voice {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Duration in seconds
    pub duration: u32,

    /// MIME type
    pub mime_type: Option<String>,

    /// File size
    pub file_size: Option<u64>,
}

/// Document/file attachment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Document {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Original filename
    pub file_name: Option<String>,

    /// MIME type
    pub mime_type: Option<String>,

    /// File size
    pub file_size: Option<u64>,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,
}

/// Sticker
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Sticker {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Sticker type (regular, mask, custom emoji)
    pub sticker_type: StickerType,

    /// Width
    pub width: u32,

    /// Height
    pub height: u32,

    /// Is animated
    pub is_animated: bool,

    /// Is video
    pub is_video: bool,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,

    /// Emoji associated with sticker
    pub emoji: Option<String>,

    /// Sticker set name
    pub set_name: Option<String>,

    /// File size
    pub file_size: Option<u64>,
}

/// Sticker type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum StickerType {
    Regular,
    Mask,
    CustomEmoji,
}

/// Animation/GIF
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Animation {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Width
    pub width: u32,

    /// Height
    pub height: u32,

    /// Duration
    pub duration: u32,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,

    /// Original filename
    pub file_name: Option<String>,

    /// MIME type
    pub mime_type: Option<String>,

    /// File size
    pub file_size: Option<u64>,
}

/// Video note (round video)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoNote {
    /// File ID
    pub file_id: String,

    /// File unique ID
    pub file_unique_id: String,

    /// Video length (diameter)
    pub length: u32,

    /// Duration in seconds
    pub duration: u32,

    /// Thumbnail
    pub thumbnail: Option<PhotoSize>,

    /// File size
    pub file_size: Option<u64>,
}

/// Location
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Location {
    /// Longitude
    pub longitude: f64,

    /// Latitude
    pub latitude: f64,

    /// Horizontal accuracy (meters)
    pub horizontal_accuracy: Option<f64>,

    /// Live location period (seconds)
    pub live_period: Option<u32>,

    /// Heading (direction)
    pub heading: Option<u16>,

    /// Proximity alert radius
    pub proximity_alert_radius: Option<u32>,
}

/// Contact
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Contact {
    /// Phone number
    pub phone_number: String,

    /// First name
    pub first_name: String,

    /// Last name
    pub last_name: Option<String>,

    /// User ID
    pub user_id: Option<dchat_core::types::UserId>,

    /// vCard
    pub vcard: Option<String>,
}

/// Poll
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Poll {
    /// Poll ID
    pub id: String,

    /// Question
    pub question: String,

    /// Options
    pub options: Vec<PollOption>,

    /// Total voter count
    pub total_voter_count: u32,

    /// Is closed
    pub is_closed: bool,

    /// Is anonymous
    pub is_anonymous: bool,

    /// Poll type
    pub poll_type: PollType,

    /// Multiple answers allowed
    pub allows_multiple_answers: bool,

    /// Correct option ID (quiz mode)
    pub correct_option_id: Option<u32>,

    /// Explanation (quiz mode)
    pub explanation: Option<String>,
}

/// Poll option
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PollOption {
    /// Option text
    pub text: String,

    /// Voter count
    pub voter_count: u32,
}

/// Poll type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PollType {
    Regular,
    Quiz,
}

/// Link/URL preview
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LinkPreview {
    /// URL
    pub url: String,

    /// Title
    pub title: Option<String>,

    /// Description
    pub description: Option<String>,

    /// Image URL
    pub image_url: Option<String>,

    /// Site name
    pub site_name: Option<String>,

    /// Favicon URL
    pub favicon_url: Option<String>,
}

/// Message entities (formatting, mentions, links, etc.)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MessageEntity {
    /// Entity type
    pub entity_type: EntityType,

    /// Offset in UTF-16 code units
    pub offset: u32,

    /// Length in UTF-16 code units
    pub length: u32,

    /// Optional data (URL, user, etc.)
    pub data: Option<String>,
}

/// Entity type
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum EntityType {
    /// @username mention
    Mention,
    /// #hashtag
    Hashtag,
    /// $cashtag
    Cashtag,
    /// /command
    BotCommand,
    /// URL
    Url,
    /// Email address
    Email,
    /// Phone number
    PhoneNumber,
    /// Bold text
    Bold,
    /// Italic text
    Italic,
    /// Underlined text
    Underline,
    /// Strikethrough text
    Strikethrough,
    /// Spoiler text
    Spoiler,
    /// Inline code
    Code,
    /// Code block
    Pre,
    /// Text link (with custom URL)
    TextLink,
    /// Text mention (user without username)
    TextMention,
}

/// Enhanced message with all media types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EnhancedBotMessage {
    /// Message ID
    pub message_id: Uuid,

    /// Sender user ID
    pub from: dchat_core::types::UserId,

    /// Chat ID (channel or DM)
    pub chat_id: String,

    /// Message text
    pub text: Option<String>,

    /// Message caption (for media)
    pub caption: Option<String>,

    /// Message entities (formatting, links, mentions)
    pub entities: Vec<MessageEntity>,

    /// Caption entities
    pub caption_entities: Vec<MessageEntity>,

    /// Photo attachment
    pub photo: Option<Vec<PhotoSize>>,

    /// Video attachment
    pub video: Option<Video>,

    /// Audio attachment
    pub audio: Option<Audio>,

    /// Voice message
    pub voice: Option<Voice>,

    /// Document attachment
    pub document: Option<Document>,

    /// Sticker
    pub sticker: Option<Sticker>,

    /// Animation/GIF
    pub animation: Option<Animation>,

    /// Video note
    pub video_note: Option<VideoNote>,

    /// Location
    pub location: Option<Location>,

    /// Contact
    pub contact: Option<Contact>,

    /// Poll
    pub poll: Option<Poll>,

    /// Link preview
    pub link_preview: Option<LinkPreview>,

    /// Message timestamp
    pub timestamp: DateTime<Utc>,

    /// Edit timestamp
    pub edit_timestamp: Option<DateTime<Utc>>,

    /// Is forwarded
    pub is_forwarded: bool,

    /// Forward from user
    pub forward_from: Option<dchat_core::types::UserId>,

    /// Forward from chat
    pub forward_from_chat: Option<String>,

    /// Forward date
    pub forward_date: Option<DateTime<Utc>>,

    /// Reply to message ID
    pub reply_to_message_id: Option<Uuid>,

    /// Is command
    pub is_command: bool,

    /// Parsed command
    pub command: Option<String>,

    /// Command arguments
    pub command_args: Vec<String>,
}

impl EnhancedBotMessage {
    /// Get media type if any
    pub fn get_media_type(&self) -> Option<MediaType> {
        if self.photo.is_some() {
            Some(MediaType::Photo)
        } else if self.video.is_some() {
            Some(MediaType::Video)
        } else if self.audio.is_some() {
            Some(MediaType::Audio)
        } else if self.voice.is_some() {
            Some(MediaType::Voice)
        } else if self.document.is_some() {
            Some(MediaType::Document)
        } else if self.sticker.is_some() {
            Some(MediaType::Sticker)
        } else if self.animation.is_some() {
            Some(MediaType::Animation)
        } else if self.video_note.is_some() {
            Some(MediaType::VideoNote)
        } else if self.location.is_some() {
            Some(MediaType::Location)
        } else if self.contact.is_some() {
            Some(MediaType::Contact)
        } else if self.poll.is_some() {
            Some(MediaType::Poll)
        } else {
            None
        }
    }

    /// Has any media attachment
    pub fn has_media(&self) -> bool {
        self.get_media_type().is_some()
    }

    /// Extract all URLs from entities
    pub fn extract_urls(&self) -> Vec<String> {
        let mut urls = Vec::new();

        for entity in &self.entities {
            if entity.entity_type == EntityType::Url {
                if let Some(text_slice) = self.text.as_ref().and_then(|t| {
                    t.chars()
                        .skip(entity.offset as usize)
                        .take(entity.length as usize)
                        .collect::<String>()
                        .into()
                }) {
                    urls.push(text_slice);
                }
            } else if entity.entity_type == EntityType::TextLink {
                if let Some(url) = &entity.data {
                    urls.push(url.clone());
                }
            }
        }

        urls
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_media_type_detection() {
        let mut msg = EnhancedBotMessage {
            message_id: Uuid::new_v4(),
            from: dchat_core::types::UserId::new(),
            chat_id: "test".to_string(),
            text: None,
            caption: None,
            entities: Vec::new(),
            caption_entities: Vec::new(),
            photo: Some(vec![PhotoSize {
                file_id: "photo123".to_string(),
                width: 1920,
                height: 1080,
                file_size: Some(1024000),
            }]),
            video: None,
            audio: None,
            voice: None,
            document: None,
            sticker: None,
            animation: None,
            video_note: None,
            location: None,
            contact: None,
            poll: None,
            link_preview: None,
            timestamp: Utc::now(),
            edit_timestamp: None,
            is_forwarded: false,
            forward_from: None,
            forward_from_chat: None,
            forward_date: None,
            reply_to_message_id: None,
            is_command: false,
            command: None,
            command_args: Vec::new(),
        };

        assert_eq!(msg.get_media_type(), Some(MediaType::Photo));
        assert!(msg.has_media());

        msg.photo = None;
        msg.video = Some(Video {
            file_id: "video123".to_string(),
            file_unique_id: "vid_unique".to_string(),
            width: 1920,
            height: 1080,
            duration: 60,
            thumbnail: None,
            mime_type: Some("video/mp4".to_string()),
            file_size: Some(5000000),
        });

        assert_eq!(msg.get_media_type(), Some(MediaType::Video));
    }

    #[test]
    fn test_extract_urls() {
        let msg = EnhancedBotMessage {
            message_id: Uuid::new_v4(),
            from: dchat_core::types::UserId::new(),
            chat_id: "test".to_string(),
            text: Some("Check out https://example.com and this link".to_string()),
            caption: None,
            entities: vec![
                MessageEntity {
                    entity_type: EntityType::Url,
                    offset: 10,
                    length: 19,
                    data: None,
                },
                MessageEntity {
                    entity_type: EntityType::TextLink,
                    offset: 34,
                    length: 9,
                    data: Some("https://hidden.com".to_string()),
                },
            ],
            caption_entities: Vec::new(),
            photo: None,
            video: None,
            audio: None,
            voice: None,
            document: None,
            sticker: None,
            animation: None,
            video_note: None,
            location: None,
            contact: None,
            poll: None,
            link_preview: None,
            timestamp: Utc::now(),
            edit_timestamp: None,
            is_forwarded: false,
            forward_from: None,
            forward_from_chat: None,
            forward_date: None,
            reply_to_message_id: None,
            is_command: false,
            command: None,
            command_args: Vec::new(),
        };

        let urls = msg.extract_urls();
        assert_eq!(urls.len(), 2);
        assert!(urls.contains(&"https://hidden.com".to_string()));
    }
}
