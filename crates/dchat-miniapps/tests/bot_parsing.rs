//! Bot Command Parsing Tests
//!
//! Tests for bot command parsing, routing, and authentication including:
//! - Command parsing from messages
//! - Argument extraction
//! - Bot routing and lookup
//! - Auth token verification
//! - Edge cases and error handling

use std::sync::Arc;

use chrono::Duration;
use dchat_miniapps::bot::{
    BotAuthToken, BotBridge, BotButton, BotCallbackQuery, BotCommandContext, BotCommandHandler,
    BotCommandResult, BotId, BotIdentity, BotKeyboard,
};
use dchat_miniapps::error::MiniAppResult;
use dchat_miniapps::manifest::BotCommandSpec;
use dchat_miniapps::permissions::{PermissionManager, PermissionSet};
use dchat_miniapps::registry::AppId;
use ed25519_dalek::SigningKey;

// ═══════════════════════════════════════════════════════════════════════════════
// TEST HELPERS
// ═══════════════════════════════════════════════════════════════════════════════

struct EchoHandler;

impl BotCommandHandler for EchoHandler {
    fn handle_command(&self, ctx: &BotCommandContext) -> MiniAppResult<BotCommandResult> {
        Ok(BotCommandResult::text(format!(
            "Echo: /{} {}",
            ctx.command, ctx.raw_args
        )))
    }

    fn handle_callback(&self, query: &BotCallbackQuery) -> MiniAppResult<BotCommandResult> {
        Ok(BotCommandResult::text(format!("Callback: {}", query.data)))
    }
}

fn create_test_signing_key() -> SigningKey {
    SigningKey::from_bytes(&[1u8; 32])
}

// ═══════════════════════════════════════════════════════════════════════════════
// COMMAND PARSING TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_parse_simple_command() {
    let ctx = BotCommandContext::parse("/start", "user123", "channel456", "msg789");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "start");
    assert!(ctx.args.is_empty());
    assert_eq!(ctx.raw_args, "");
    assert_eq!(ctx.user_id, "user123");
    assert_eq!(ctx.channel_id, "channel456");
    assert_eq!(ctx.message_id, "msg789");
}

#[test]
fn test_parse_command_with_single_arg() {
    let ctx = BotCommandContext::parse("/help topic", "user1", "ch1", "m1");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "help");
    assert_eq!(ctx.args, vec!["topic"]);
    assert_eq!(ctx.raw_args, "topic");
}

#[test]
fn test_parse_command_with_multiple_args() {
    let ctx = BotCommandContext::parse("/send 100 coins user123", "u", "c", "m");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "send");
    assert_eq!(ctx.args, vec!["100", "coins", "user123"]);
    assert_eq!(ctx.raw_args, "100 coins user123");
}

#[test]
fn test_parse_command_with_extra_whitespace() {
    let ctx = BotCommandContext::parse("  /start   hello   world  ", "u", "c", "m");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "start");
    assert_eq!(ctx.args, vec!["hello", "world"]);
}

#[test]
fn test_parse_command_case_insensitive() {
    let ctx = BotCommandContext::parse("/START", "u", "c", "m");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "start");
}

#[test]
fn test_parse_not_a_command() {
    let ctx = BotCommandContext::parse("hello world", "u", "c", "m");
    assert!(ctx.is_none());
}

#[test]
fn test_parse_empty_message() {
    let ctx = BotCommandContext::parse("", "u", "c", "m");
    assert!(ctx.is_none());
}

#[test]
fn test_parse_slash_only() {
    let ctx = BotCommandContext::parse("/", "u", "c", "m");
    // Empty command name - should return None or empty command
    if let Some(ctx) = ctx {
        assert!(ctx.command.is_empty());
    }
}

#[test]
fn test_parse_command_with_unicode_args() {
    let ctx = BotCommandContext::parse("/greet こんにちは 世界", "u", "c", "m");

    assert!(ctx.is_some());
    let ctx = ctx.unwrap();
    assert_eq!(ctx.command, "greet");
    assert_eq!(ctx.args, vec!["こんにちは", "世界"]);
}

#[test]
fn test_arg_accessor() {
    let ctx = BotCommandContext::parse("/cmd arg0 arg1 arg2", "u", "c", "m").unwrap();

    assert_eq!(ctx.arg(0), Some("arg0"));
    assert_eq!(ctx.arg(1), Some("arg1"));
    assert_eq!(ctx.arg(2), Some("arg2"));
    assert_eq!(ctx.arg(3), None);
    assert_eq!(ctx.arg(100), None);
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT IDENTITY TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_identity_valid_username() {
    let app_id = AppId([42u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "my_bot".to_string(),
        "My Bot".to_string(),
        [0u8; 32],
    );

    assert!(identity.is_ok());
    let identity = identity.unwrap();
    assert_eq!(identity.username, "my_bot");
    assert_eq!(identity.display_name, "My Bot");
}

#[test]
fn test_bot_identity_username_too_short() {
    let app_id = AppId([42u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "ab".to_string(), // Too short (< 3 chars)
        "Bot".to_string(),
        [0u8; 32],
    );

    assert!(identity.is_err());
}

#[test]
fn test_bot_identity_username_with_special_chars() {
    let app_id = AppId([42u8; 32]);

    // @ symbol not allowed
    let identity = BotIdentity::new(app_id, "my@bot".to_string(), "Bot".to_string(), [0u8; 32]);
    assert!(identity.is_err());

    // Space not allowed
    let identity = BotIdentity::new(app_id, "my bot".to_string(), "Bot".to_string(), [0u8; 32]);
    assert!(identity.is_err());

    // Underscore allowed
    let identity = BotIdentity::new(
        app_id,
        "my_bot_123".to_string(),
        "Bot".to_string(),
        [0u8; 32],
    );
    assert!(identity.is_ok());
}

#[test]
fn test_bot_identity_with_commands() {
    let app_id = AppId([42u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "cmd_bot".to_string(),
        "Command Bot".to_string(),
        [0u8; 32],
    )
    .unwrap()
    .with_commands(vec![
        BotCommandSpec {
            command: "start".to_string(),
            description: "Start the bot".to_string(),
        },
        BotCommandSpec {
            command: "help".to_string(),
            description: "Get help".to_string(),
        },
    ]);

    assert_eq!(identity.commands.len(), 2);
    assert_eq!(identity.commands[0].command, "start");
    assert_eq!(identity.commands[1].command, "help");
}

// ═══════════════════════════════════════════════════════════════════════════════
// BOT BRIDGE ROUTING TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_bridge_register_and_lookup() {
    let pm = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(pm);

    let app_id = AppId([1u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "lookup_bot".to_string(),
        "Lookup Bot".to_string(),
        [0u8; 32],
    )
    .unwrap();

    bridge
        .register(
            identity.clone(),
            Arc::new(EchoHandler),
            PermissionSet::new(),
        )
        .unwrap();

    // Lookup by ID
    let found = bridge.get(&identity.id);
    assert!(found.is_some());
    assert_eq!(found.unwrap().username, "lookup_bot");

    // Lookup by username
    let found = bridge.get_by_username("lookup_bot");
    assert!(found.is_some());
    assert_eq!(found.unwrap().display_name, "Lookup Bot");
}

#[test]
fn test_bot_bridge_duplicate_username() {
    let pm = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(pm);

    let app_id1 = AppId([1u8; 32]);
    let identity1 = BotIdentity::new(
        app_id1,
        "unique_bot".to_string(),
        "Bot 1".to_string(),
        [0u8; 32],
    )
    .unwrap();

    bridge
        .register(identity1, Arc::new(EchoHandler), PermissionSet::new())
        .unwrap();

    // Try to register another bot with same username
    let app_id2 = AppId([2u8; 32]);
    let identity2 = BotIdentity::new(
        app_id2,
        "unique_bot".to_string(), // Same username
        "Bot 2".to_string(),
        [0u8; 32],
    )
    .unwrap();

    let result = bridge.register(identity2, Arc::new(EchoHandler), PermissionSet::new());
    assert!(result.is_err());
}

#[test]
fn test_bot_bridge_unregister() {
    let pm = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(pm);

    let app_id = AppId([3u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "temp_bot".to_string(),
        "Temp Bot".to_string(),
        [0u8; 32],
    )
    .unwrap();

    let bot_id = identity.id;
    bridge
        .register(identity, Arc::new(EchoHandler), PermissionSet::new())
        .unwrap();

    assert!(bridge.get(&bot_id).is_some());

    bridge.unregister(&bot_id).unwrap();

    assert!(bridge.get(&bot_id).is_none());
    assert!(bridge.get_by_username("temp_bot").is_none());
}

#[test]
fn test_bot_bridge_list() {
    let pm = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(pm);

    for i in 0..3 {
        let app_id = AppId([i; 32]);
        let identity = BotIdentity::new(
            app_id,
            format!("list_bot_{}", i),
            format!("Bot {}", i),
            [0u8; 32],
        )
        .unwrap();

        bridge
            .register(identity, Arc::new(EchoHandler), PermissionSet::new())
            .unwrap();
    }

    let bots = bridge.list();
    assert_eq!(bots.len(), 3);
}

#[test]
fn test_bot_bridge_route_direct_command() {
    let pm = Arc::new(PermissionManager::new());
    let bridge = BotBridge::new(pm);

    let app_id = AppId([5u8; 32]);
    let identity = BotIdentity::new(
        app_id,
        "route_bot".to_string(),
        "Route Bot".to_string(),
        [0u8; 32],
    )
    .unwrap()
    .with_commands(vec![BotCommandSpec {
        command: "ping".to_string(),
        description: "Ping".to_string(),
    }]);

    bridge
        .register(identity, Arc::new(EchoHandler), PermissionSet::new())
        .unwrap();

    // Route a /ping command
    let result = bridge.route_message(
        "/ping test",
        "user1".to_string(),
        "ch1".to_string(),
        "m1".to_string(),
    );

    assert!(result.is_some());
    let (bot_id, ctx) = result.unwrap();
    assert_eq!(ctx.command, "ping");
    assert_eq!(ctx.args, vec!["test"]);
}

// ═══════════════════════════════════════════════════════════════════════════════
// AUTH TOKEN TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_auth_token_creation_and_verification() {
    let signing_key = create_test_signing_key();
    let verifying_key = signing_key.verifying_key();
    let bot_id = BotId::from_bytes([99u8; 32]);

    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        Some("channel123".to_string()),
        Some("user456".to_string()),
        Duration::hours(1),
    );

    assert_eq!(token.bot_id.0, bot_id.0);
    assert_eq!(token.channel_id, Some("channel123".to_string()));
    assert_eq!(token.user_id, Some("user456".to_string()));
    assert!(!token.is_expired());

    // Verify token
    let result = token.verify(&verifying_key);
    assert!(result.is_ok());
}

#[test]
fn test_bot_auth_token_expired() {
    let signing_key = create_test_signing_key();
    let verifying_key = signing_key.verifying_key();
    let bot_id = BotId::from_bytes([99u8; 32]);

    // Create token that expires in the past
    let token = BotAuthToken::create(
        bot_id,
        &signing_key,
        None,
        None,
        Duration::seconds(-10), // Already expired
    );

    assert!(token.is_expired());

    // Verification should fail
    let result = token.verify(&verifying_key);
    assert!(result.is_err());
}

#[test]
fn test_bot_auth_token_invalid_signature() {
    let signing_key = create_test_signing_key();
    let wrong_key = SigningKey::from_bytes(&[2u8; 32]);
    let wrong_verifying_key = wrong_key.verifying_key();
    let bot_id = BotId::from_bytes([99u8; 32]);

    let token = BotAuthToken::create(bot_id, &signing_key, None, None, Duration::hours(1));

    // Verify with wrong key
    let result = token.verify(&wrong_verifying_key);
    assert!(result.is_err());
}

// ═══════════════════════════════════════════════════════════════════════════════
// KEYBOARD AND BUTTON TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[test]
fn test_bot_button_callback() {
    let button = BotButton::callback("Click me", "action:click");

    assert_eq!(button.text, "Click me");
    assert_eq!(button.callback_data, Some("action:click".to_string()));
    assert!(button.url.is_none());
    assert!(button.miniapp_url.is_none());
}

#[test]
fn test_bot_button_url() {
    let button = BotButton::url("Visit", "https://example.com");

    assert_eq!(button.text, "Visit");
    assert!(button.callback_data.is_none());
    assert_eq!(button.url, Some("https://example.com".to_string()));
}

#[test]
fn test_bot_button_miniapp() {
    let button = BotButton::miniapp("Open App", "https://app.example.com");

    assert_eq!(button.text, "Open App");
    assert!(button.callback_data.is_none());
    assert!(button.url.is_none());
    assert_eq!(
        button.miniapp_url,
        Some("https://app.example.com".to_string())
    );
}

#[test]
fn test_bot_keyboard_inline() {
    let keyboard = BotKeyboard::inline(vec![
        vec![
            BotButton::callback("Yes", "yes"),
            BotButton::callback("No", "no"),
        ],
        vec![BotButton::callback("Cancel", "cancel")],
    ]);

    assert_eq!(keyboard.rows.len(), 2);
    assert_eq!(keyboard.rows[0].len(), 2);
    assert_eq!(keyboard.rows[1].len(), 1);
}

#[test]
fn test_bot_command_result_chaining() {
    let result = BotCommandResult::text("Hello")
        .with_keyboard(BotKeyboard::inline(vec![]))
        .as_reply();

    assert!(result.text.is_some());
    assert!(result.keyboard.is_some());
    assert!(result.reply_to_message);
    assert!(!result.pin);
}

#[test]
fn test_bot_command_result_html() {
    let result = BotCommandResult::html("<b>Bold</b>");

    assert!(result.text.is_none());
    assert!(result.html.is_some());
    assert_eq!(result.html.unwrap(), "<b>Bold</b>");
}
