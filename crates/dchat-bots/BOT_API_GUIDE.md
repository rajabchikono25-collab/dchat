# Bot API Implementation Guide

## Overview

The dchat Bot API provides a comprehensive platform for creating automated bots similar to Telegram's bot system. Bots can send messages, receive events, handle commands, and interact with users through webhooks.

## Architecture

```
┌─────────────────┐         ┌──────────────────┐         ┌─────────────────┐
│   Bot Client    │────────►│  Event Dispatcher │────────►│  Bot Handler    │
│  (BotApi/HTTP)  │         │  (Rate Limiting)  │         │  (User Code)    │
└─────────────────┘         └──────────────────┘         └─────────────────┘
        │                            │
        ▼                            ▼
┌─────────────────┐         ┌──────────────────┐
│  Messaging      │         │    Blockchain    │
│  Integration    │◄────────│  (Ordering/PoD)  │
└─────────────────┘         └──────────────────┘
```

## Core Components

### 1. Bot Management (`bot_manager.rs`)
- **BotFather**: Central bot registration and management system
- **Bot Lifecycle**: Creation, activation, deactivation, deletion
- **Token Management**: Secure token generation and verification
- **Statistics Tracking**: Messages, commands, queries, webhooks

### 2. Bot API (`bot_api.rs`)
- **BotApi**: Low-level API connected to messaging layer
- **BotClient**: High-level HTTP client for external integrations
- **Message Operations**: Send, edit, delete messages
- **Query Handling**: Callback queries, inline queries
- **Member Management**: Get chat members, permissions

### 3. Event System (`event_dispatcher.rs`)
- **EventDispatcher**: Routes incoming messages/events to bots
- **BotEventHandler**: Trait for implementing bot logic
- **Rate Limiting**: Per-bot event rate limits (configurable)
- **Event Types**: Messages, edits, callbacks, channel updates

### 4. Messaging Integration (`messaging_integration.rs`)
- **BotMessagingClient**: Connects bot API to dchat messaging layer
- **Message Routing**: DHT-based message delivery
- **Encryption**: Noise Protocol integration for E2E encryption
- **Queue Management**: Outgoing message queue with flush capabilities

### 5. Webhook System (`webhook.rs`)
- **WebhookManager**: Configure and manage webhooks
- **Webhook Delivery**: HTTPS POST with signatures
- **Update Types**: Message, edited message, callback query, inline query
- **Security**: HMAC-SHA256 signatures, secret tokens

## Quick Start

### Creating a Bot

```rust
use dchat_bots::{BotFather, CreateBotRequest};
use dchat_core::types::UserId;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let bot_father = BotFather::new();
    
    let owner_id = UserId::new();
    let request = CreateBotRequest {
        username: "mybot".to_string(),
        display_name: "My Awesome Bot".to_string(),
        description: Some("A helpful bot".to_string()),
    };
    
    let bot = bot_father.create_bot(owner_id, request)?;
    println!("Bot created! Token: {}", bot.token);
    
    Ok(())
}
```

### Sending Messages

```rust
use dchat_bots::{BotMessagingClient, SendMessageRequest};

let bot = bot_father.get_bot(&bot_id).unwrap();
let client = BotMessagingClient::new(bot);

let request = SendMessageRequest {
    chat_id: "user:550e8400-e29b-41d4-a716-446655440000".to_string(),
    text: "Hello from bot!".to_string(),
    parse_mode: None,
    reply_to_message_id: None,
    inline_keyboard: None,
    disable_notification: false,
};

let message_id = client.send_message(request).await?;
println!("Message sent: {:?}", message_id);
```

### Receiving Events

```rust
use dchat_bots::{EventDispatcher, BotEventHandler, BotEvent};
use async_trait::async_trait;

struct MyBotHandler;

#[async_trait]
impl BotEventHandler for MyBotHandler {
    async fn handle_event(&self, bot: &Bot, event: BotEvent) -> Result<()> {
        match event {
            BotEvent::Message(msg) => {
                println!("Received: {}", msg.text);
                // Handle message
            }
            BotEvent::CallbackQuery(query) => {
                println!("Button clicked: {}", query.data);
                // Handle callback
            }
            _ => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dispatcher = EventDispatcher::new();
    let handler = Arc::new(MyBotHandler);
    
    dispatcher.subscribe(&bot, handler, 100).await?; // 100 events/min
    
    // Dispatcher will now route events to the handler
    Ok(())
}
```

### Setting Up Webhooks

```rust
use dchat_bots::{WebhookManager, WebhookConfig};

let manager = WebhookManager::new();

let config = WebhookConfig {
    url: "https://myserver.com/webhook".to_string(),
    secret_token: Some("my_secret".to_string()),
    max_connections: 100,
    allowed_updates: vec![UpdateType::Message, UpdateType::CallbackQuery],
    drop_pending_updates: false,
};

manager.set_webhook(&mut bot, config).await?;
```

## Chat ID Format

Chat IDs follow a specific format:

- **Direct Messages**: `user:{uuid}`
  - Example: `user:550e8400-e29b-41d4-a716-446655440000`
  
- **Channel Messages**: `channel:{uuid}`
  - Example: `channel:6ba7b810-9dad-11d1-80b4-00c04fd430c8`

## Event Types

### BotEvent::Message
Triggered when a user sends a message to the bot or mentions it in a channel.

```rust
pub struct BotMessage {
    pub message_id: Uuid,
    pub from_user_id: Uuid,
    pub chat_id: String,
    pub text: String,
    pub timestamp: SystemTime,
    pub reply_to_message_id: Option<Uuid>,
    pub entities: Vec<MessageEntity>,
    pub media: Option<EnhancedBotMessage>,
}
```

### BotEvent::EditedMessage
Triggered when a user edits a message sent to the bot.

### BotEvent::CallbackQuery
Triggered when a user clicks an inline keyboard button.

```rust
pub struct CallbackQuery {
    pub id: Uuid,
    pub from: UserId,
    pub message: BotMessage,
    pub data: String,
    pub timestamp: DateTime<Utc>,
}
```

### BotEvent::BotAddedToChannel
Triggered when the bot is added to a channel.

### BotEvent::BotRemovedFromChannel
Triggered when the bot is removed from a channel.

### BotEvent::ChannelMemberUpdated
Triggered when channel member permissions change.

## Rate Limiting

Rate limiting is enforced per bot on the event dispatcher:

- **Default**: 100 events per minute per bot
- **Configurable**: Set custom limit during subscription
- **Sliding Window**: Resets every minute
- **Error Handling**: Returns `Error::rate_limit` when exceeded

```rust
// Set custom rate limit
dispatcher.subscribe(&bot, handler, 200).await?; // 200 events/min

// Check current stats
if let Some((current, max)) = dispatcher.get_rate_limit_stats(&bot.id).await {
    println!("Rate limit: {}/{}", current, max);
}
```

## Inline Keyboards

Create interactive buttons in messages:

```rust
use dchat_bots::{InlineKeyboardButton, ButtonAction};

let keyboard = vec![
    vec![
        InlineKeyboardButton {
            text: "Option 1".to_string(),
            action: ButtonAction::CallbackData("option_1".to_string()),
        },
        InlineKeyboardButton {
            text: "Option 2".to_string(),
            action: ButtonAction::CallbackData("option_2".to_string()),
        },
    ],
    vec![
        InlineKeyboardButton {
            text: "Visit Website".to_string(),
            action: ButtonAction::Url("https://dchat.network".to_string()),
        },
    ],
];

let request = SendMessageRequest {
    chat_id: chat_id,
    text: "Choose an option:".to_string(),
    parse_mode: None,
    reply_to_message_id: None,
    inline_keyboard: Some(keyboard),
    disable_notification: false,
};
```

## Commands

Bots can define commands that appear in the user interface:

```rust
let mut bot = Bot::new("mybot".to_string(), "My Bot".to_string(), owner_id)?;

bot.add_command(BotCommand::new(
    "start".to_string(),
    "Start the bot".to_string(),
));

bot.add_command(BotCommand::new(
    "help".to_string(),
    "Show help message".to_string(),
));

bot.add_command(
    BotCommand::new(
        "admin".to_string(),
        "Admin commands".to_string(),
    )
    .hidden(true)
    .require_permission("admin".to_string())
);
```

Parsing commands from messages:

```rust
async fn handle_event(&self, bot: &Bot, event: BotEvent) -> Result<()> {
    if let BotEvent::Message(msg) = event {
        if msg.is_command() {
            if let Some((cmd, args)) = msg.parse_command() {
                match cmd.as_str() {
                    "start" => self.handle_start(bot, &msg).await?,
                    "help" => self.handle_help(bot, &msg).await?,
                    _ => {}
                }
            }
        }
    }
    Ok(())
}
```

## Security

### Token Security
- Tokens are generated using SHA-256 hashing of random UUIDs
- Format: `dchat_bot_{base64_encoded_hash}`
- Tokens should be stored securely and never exposed in logs
- Tokens are verified on every API call

### Webhook Security
- HTTPS required for all webhook URLs
- HMAC-SHA256 signature in `X-Dchat-Signature` header
- Optional secret token in `X-Dchat-Bot-Api-Secret-Token` header
- Signature verification prevents spoofing

### Permission Management
Bots have permission scopes that control their capabilities:

```rust
pub struct BotPermissions {
    pub can_send_messages: bool,
    pub can_edit_messages: bool,
    pub can_delete_messages: bool,
    pub can_pin_messages: bool,
    pub can_invite_users: bool,
    pub can_manage_channel: bool,
    pub max_messages_per_minute: u32,
}
```

## Integration with dchat Messaging

The bot API integrates with the core dchat messaging system:

1. **Message Creation**: Uses `MessageBuilder` to create structured messages
2. **Encryption**: Integrates with Noise Protocol for E2E encryption
3. **Routing**: Uses DHT (libp2p/Kademlia) for peer discovery and message routing
4. **Blockchain**: Submits message hashes for ordering and proof-of-delivery
5. **Queue Management**: Outgoing messages queued and batched for efficiency

```
Bot sends message
    ↓
BotMessagingClient.send_message()
    ↓
MessageBuilder creates Message
    ↓
Noise Protocol encrypts payload
    ↓
Message added to outgoing queue
    ↓
Queue flushed to MessageRouter
    ↓
DHT lookup for recipient
    ↓
Message routed to relay nodes
    ↓
Blockchain submission for ordering
    ↓
Proof-of-delivery tracked
```

## Testing

### Unit Tests
```bash
cd crates/dchat-bots
cargo test
```

### Integration Tests
```rust
#[tokio::test]
async fn test_bot_lifecycle() {
    let bot_father = BotFather::new();
    let owner_id = UserId::new();
    
    // Create bot
    let request = CreateBotRequest {
        username: "testbot".to_string(),
        display_name: "Test Bot".to_string(),
        description: None,
    };
    let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
    
    // Verify bot
    assert_eq!(bot.username, "testbot");
    assert!(bot.is_active);
    
    // Get bot
    let retrieved = bot_father.get_bot(&bot.id).unwrap();
    assert_eq!(retrieved.id, bot.id);
    
    // Delete bot
    bot_father.delete_bot(&bot.id, &owner_id).unwrap();
    assert!(bot_father.get_bot(&bot.id).is_none());
}
```

## Best Practices

1. **Rate Limiting**: Respect rate limits to avoid being blocked
2. **Error Handling**: Always handle errors gracefully
3. **Token Security**: Never expose bot tokens in public repositories
4. **Webhook Reliability**: Implement retry logic for webhook failures
5. **Event Processing**: Keep event handlers fast; use background tasks for heavy work
6. **Logging**: Use structured logging with `tracing` crate
7. **Testing**: Write comprehensive tests for bot logic
8. **Documentation**: Document bot commands and behavior clearly
9. **Privacy**: Respect user privacy; don't store unnecessary data
10. **Permissions**: Request minimal permissions required

## Example: Echo Bot

Complete example of a simple echo bot:

```rust
use dchat_bots::{
    Bot, BotEvent, BotEventHandler, BotFather, BotMessagingClient,
    CreateBotRequest, EventDispatcher, SendMessageRequest,
};
use dchat_core::types::UserId;
use dchat_core::Result;
use std::sync::Arc;

struct EchoBot {
    messaging_client: Arc<BotMessagingClient>,
}

#[async_trait::async_trait]
impl BotEventHandler for EchoBot {
    async fn handle_event(&self, _bot: &Bot, event: BotEvent) -> Result<()> {
        match event {
            BotEvent::Message(msg) => {
                if msg.is_command() {
                    if let Some((cmd, _)) = msg.parse_command() {
                        if cmd == "start" {
                            let response = SendMessageRequest {
                                chat_id: msg.chat_id,
                                text: "Hello! I'm an echo bot. Send me a message and I'll echo it back!".to_string(),
                                parse_mode: None,
                                reply_to_message_id: None,
                                inline_keyboard: None,
                                disable_notification: false,
                            };
                            self.messaging_client.send_message(response).await?;
                        }
                    }
                } else {
                    // Echo the message
                    let response = SendMessageRequest {
                        chat_id: msg.chat_id,
                        text: format!("You said: {}", msg.text),
                        parse_mode: None,
                        reply_to_message_id: Some(msg.message_id),
                        inline_keyboard: None,
                        disable_notification: false,
                    };
                    self.messaging_client.send_message(response).await?;
                }
            }
            _ => {}
        }
        Ok(())
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    // Create bot
    let bot_father = BotFather::new();
    let owner_id = UserId::new();
    
    let request = CreateBotRequest {
        username: "echobot".to_string(),
        display_name: "Echo Bot".to_string(),
        description: Some("Echoes your messages".to_string()),
    };
    
    let bot = bot_father.create_bot(owner_id, request)?;
    println!("Bot created! Token: {}", bot.token);
    
    // Set up messaging
    let messaging_client = Arc::new(BotMessagingClient::new(bot.clone()));
    
    // Set up event handling
    let dispatcher = EventDispatcher::new();
    let handler = Arc::new(EchoBot { messaging_client });
    
    dispatcher.subscribe(&bot, handler, 100).await?;
    
    println!("Echo bot is running!");
    
    // Keep running
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}
```

## API Reference

For complete API documentation, run:
```bash
cargo doc --open --package dchat-bots
```

## Troubleshooting

### Bot not receiving messages
- Check if bot is active: `bot.is_active == true`
- Verify event subscription: `dispatcher.subscribe()` called
- Check rate limits: `dispatcher.get_rate_limit_stats()`

### Message sending fails
- Verify chat ID format (`user:` or `channel:` prefix)
- Check bot token authentication
- Ensure bot has send permissions
- Check outgoing queue: `client.queue_length()`

### Webhook not working
- Ensure HTTPS is used
- Verify webhook URL is reachable
- Check secret token matches
- Validate HMAC signature

## Contributing

When contributing to the bot API:
1. Follow existing code patterns
2. Add tests for new features
3. Update documentation
4. Run `cargo fmt` and `cargo clippy`
5. Ensure all tests pass: `cargo test`

## License

See main dchat project license.
