//! Command handling system for bots

use crate::{BotMessage, BotResponse};
use dchat_core::{Error, Result};
use std::collections::HashMap;
use std::sync::Arc;

/// Command handler function type
pub type CommandHandlerFn = Arc<dyn Fn(&BotMessage) -> Result<BotResponse> + Send + Sync>;

/// Command definition
#[derive(Clone)]
pub struct Command {
    /// Command name (without slash)
    pub name: String,

    /// Command description
    pub description: String,

    /// Handler function
    pub handler: CommandHandlerFn,

    /// Is command hidden?
    pub hidden: bool,

    /// Required permissions
    pub required_permissions: Vec<String>,
}

/// Command registry
pub struct CommandRegistry {
    commands: HashMap<String, Command>,
}

/// Command handler trait
pub trait CommandHandler {
    /// Handle a command
    fn handle(&self, message: &BotMessage) -> Result<BotResponse>;

    /// Get command name
    fn command_name(&self) -> &str;

    /// Get command description
    fn description(&self) -> &str;
}

/// Built-in start command handler
pub struct StartCommandHandler;

/// Built-in help command handler
pub struct HelpCommandHandler {
    commands: Vec<crate::BotCommand>,
}

/// Built-in settings command handler
pub struct SettingsCommandHandler;

impl CommandRegistry {
    /// Create a new command registry
    pub fn new() -> Self {
        Self {
            commands: HashMap::new(),
        }
    }

    /// Register a command
    pub fn register(&mut self, command: Command) {
        self.commands.insert(command.name.clone(), command);
    }

    /// Register a command with handler trait
    pub fn register_handler<H: CommandHandler + Send + Sync + 'static>(&mut self, handler: H) {
        let name = handler.command_name().to_string();
        let description = handler.description().to_string();

        let handler = Arc::new(handler);
        let command = Command {
            name: name.clone(),
            description,
            handler: Arc::new(move |msg| handler.handle(msg)),
            hidden: false,
            required_permissions: Vec::new(),
        };

        self.register(command);
    }

    /// Get command by name
    pub fn get(&self, command_name: &str) -> Option<&Command> {
        self.commands.get(command_name)
    }

    /// Handle a command message
    pub fn handle(&self, message: &BotMessage) -> Result<BotResponse> {
        if !message.is_command() {
            return Err(Error::validation("Not a command"));
        }

        let (command_name, _args) = message
            .parse_command()
            .ok_or_else(|| Error::validation("No command specified"))?;

        let command = self
            .get(&command_name)
            .ok_or_else(|| Error::validation(format!("Unknown command: /{}", command_name)))?;

        (command.handler)(message)
    }

    /// Get all registered commands
    pub fn get_all_commands(&self) -> Vec<&Command> {
        self.commands.values().collect()
    }

    /// Get visible commands (non-hidden)
    pub fn get_visible_commands(&self) -> Vec<&Command> {
        self.commands.values().filter(|c| !c.hidden).collect()
    }
}

impl CommandHandler for StartCommandHandler {
    fn handle(&self, message: &BotMessage) -> Result<BotResponse> {
        Ok(BotResponse {
            chat_id: message.chat_id.clone(),
            text: "👋 Welcome to this bot!\n\nUse /help to see available commands.".to_string(),
            parse_mode: None,
            reply_to_message_id: Some(message.message_id),
            inline_keyboard: None,
            disable_notification: false,
        })
    }

    fn command_name(&self) -> &str {
        "start"
    }

    fn description(&self) -> &str {
        "Start the bot"
    }
}

impl HelpCommandHandler {
    /// Create a new help command handler
    pub fn new(commands: Vec<crate::BotCommand>) -> Self {
        Self { commands }
    }
}

impl CommandHandler for HelpCommandHandler {
    fn handle(&self, message: &BotMessage) -> Result<BotResponse> {
        let mut help_text = "📚 **Available Commands**\n\n".to_string();

        for cmd in &self.commands {
            if !cmd.hidden {
                help_text.push_str(&format!("/{} - {}\n", cmd.command, cmd.description));
            }
        }

        Ok(BotResponse {
            chat_id: message.chat_id.clone(),
            text: help_text,
            parse_mode: Some(crate::ParseMode::Markdown),
            reply_to_message_id: Some(message.message_id),
            inline_keyboard: None,
            disable_notification: false,
        })
    }

    fn command_name(&self) -> &str {
        "help"
    }

    fn description(&self) -> &str {
        "Show help message"
    }
}

impl CommandHandler for SettingsCommandHandler {
    fn handle(&self, message: &BotMessage) -> Result<BotResponse> {
        use crate::{ButtonAction, InlineKeyboardButton};

        let keyboard = vec![
            vec![
                InlineKeyboardButton {
                    text: "🔔 Notifications".to_string(),
                    action: ButtonAction::CallbackData("settings:notifications".to_string()),
                },
                InlineKeyboardButton {
                    text: "🔒 Privacy".to_string(),
                    action: ButtonAction::CallbackData("settings:privacy".to_string()),
                },
            ],
            vec![
                InlineKeyboardButton {
                    text: "🌐 Language".to_string(),
                    action: ButtonAction::CallbackData("settings:language".to_string()),
                },
                InlineKeyboardButton {
                    text: "❌ Close".to_string(),
                    action: ButtonAction::CallbackData("settings:close".to_string()),
                },
            ],
        ];

        Ok(BotResponse {
            chat_id: message.chat_id.clone(),
            text: "⚙️ **Settings**\n\nChoose a category:".to_string(),
            parse_mode: Some(crate::ParseMode::Markdown),
            reply_to_message_id: Some(message.message_id),
            inline_keyboard: Some(keyboard),
            disable_notification: false,
        })
    }

    fn command_name(&self) -> &str {
        "settings"
    }

    fn description(&self) -> &str {
        "Bot settings"
    }
}

impl Default for CommandRegistry {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use uuid::Uuid;

    #[test]
    fn test_command_registry() {
        let mut registry = CommandRegistry::new();

        let command = Command {
            name: "test".to_string(),
            description: "Test command".to_string(),
            handler: Arc::new(|msg| {
                Ok(BotResponse {
                    chat_id: msg.chat_id.clone(),
                    text: "Test response".to_string(),
                    parse_mode: None,
                    reply_to_message_id: None,
                    inline_keyboard: None,
                    disable_notification: false,
                })
            }),
            hidden: false,
            required_permissions: Vec::new(),
        };

        registry.register(command);

        assert!(registry.get("test").is_some());
        assert!(registry.get("nonexistent").is_none());
    }

    #[test]
    fn test_start_command() {
        let handler = StartCommandHandler;
        let message = BotMessage::from_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "chat123".to_string(),
            "/start".to_string(),
        );

        let response = handler.handle(&message).unwrap();
        assert!(response.text.contains("Welcome"));
    }

    #[test]
    fn test_help_command() {
        let commands = vec![
            crate::BotCommand::new("start".to_string(), "Start bot".to_string()),
            crate::BotCommand::new("help".to_string(), "Show help".to_string()),
        ];

        let handler = HelpCommandHandler::new(commands);
        let message = BotMessage::from_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "chat123".to_string(),
            "/help".to_string(),
        );

        let response = handler.handle(&message).unwrap();
        assert!(response.text.contains("Available Commands"));
        assert!(response.text.contains("/start"));
    }

    #[test]
    fn test_settings_command() {
        let handler = SettingsCommandHandler;
        let message = BotMessage::from_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "chat123".to_string(),
            "/settings".to_string(),
        );

        let response = handler.handle(&message).unwrap();
        assert!(response.text.contains("Settings"));
        assert!(response.inline_keyboard.is_some());

        let keyboard = response.inline_keyboard.unwrap();
        assert_eq!(keyboard.len(), 2); // 2 rows
    }

    #[test]
    fn test_command_handler_trait() {
        let mut registry = CommandRegistry::new();
        registry.register_handler(StartCommandHandler);

        let message = BotMessage::from_message(
            Uuid::new_v4(),
            Uuid::new_v4(),
            "chat123".to_string(),
            "/start".to_string(),
        );

        let response = registry.handle(&message).unwrap();
        assert!(response.text.contains("Welcome"));
    }
}
