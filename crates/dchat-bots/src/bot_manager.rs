//! Bot Manager - The "BotFather" of dchat
//!
//! Manages bot lifecycle, registration, and administration

use crate::{Bot, BotCommand, BotId, CreateBotRequest, UpdateBotRequest};
use dchat_core::types::UserId;
use dchat_core::{Error, Result};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

/// Rate limiter for token regeneration
/// 
/// Prevents brute-force token theft attacks by limiting how often
/// a token can be regenerated for a given bot.
#[derive(Debug)]
struct TokenRegenerationLimiter {
    /// Maximum regenerations allowed per window
    max_regenerations: usize,
    /// Time window duration
    window_duration: Duration,
    /// Map of bot_id to (regeneration_count, window_start)
    state: RwLock<HashMap<BotId, (usize, Instant)>>,
}

impl TokenRegenerationLimiter {
    /// Create a new rate limiter
    /// 
    /// # Arguments
    /// * `max_regenerations` - Maximum regenerations allowed per window
    /// * `window_duration` - Duration of the rate limit window
    fn new(max_regenerations: usize, window_duration: Duration) -> Self {
        Self {
            max_regenerations,
            window_duration,
            state: RwLock::new(HashMap::new()),
        }
    }
    
    /// Create production limiter: 3 regenerations per hour
    fn production() -> Self {
        Self::new(3, Duration::from_secs(3600))
    }
    
    /// Check if regeneration is allowed and record the attempt
    /// 
    /// Returns `Ok(())` if allowed, `Err` with remaining wait time if rate limited
    fn check_and_record(&self, bot_id: &BotId) -> std::result::Result<(), Duration> {
        let mut state = self.state.write().unwrap();
        let now = Instant::now();
        
        let entry = state.entry(*bot_id).or_insert((0, now));
        
        // Check if window has expired
        if now.duration_since(entry.1) >= self.window_duration {
            // Reset window
            entry.0 = 1;
            entry.1 = now;
            return Ok(());
        }
        
        // Check if we've exceeded the limit
        if entry.0 >= self.max_regenerations {
            let remaining = self.window_duration - now.duration_since(entry.1);
            return Err(remaining);
        }
        
        // Record the regeneration
        entry.0 += 1;
        Ok(())
    }
    
    /// Get remaining regenerations for a bot
    pub fn remaining_regenerations(&self, bot_id: &BotId) -> usize {
        let state = self.state.read().unwrap();
        let now = Instant::now();
        
        match state.get(bot_id) {
            Some((count, window_start)) => {
                if now.duration_since(*window_start) >= self.window_duration {
                    self.max_regenerations
                } else {
                    self.max_regenerations.saturating_sub(*count)
                }
            }
            None => self.max_regenerations,
        }
    }
}

impl Default for TokenRegenerationLimiter {
    fn default() -> Self {
        Self::production()
    }
}

/// BotFather - Central bot management system
pub struct BotFather {
    /// All registered bots
    bots: Arc<RwLock<HashMap<BotId, Bot>>>,

    /// Username to bot ID mapping
    username_index: Arc<RwLock<HashMap<String, BotId>>>,

    /// Token to bot ID mapping
    token_index: Arc<RwLock<HashMap<String, BotId>>>,

    /// Owner to bot IDs mapping
    owner_index: Arc<RwLock<HashMap<UserId, Vec<BotId>>>>,
    
    /// Rate limiter for token regeneration
    token_regen_limiter: TokenRegenerationLimiter,
}

/// Bot Manager provides high-level bot operations
pub struct BotManager {
    bot_father: Arc<BotFather>,
}

impl BotFather {
    /// Create a new BotFather instance
    pub fn new() -> Self {
        Self {
            bots: Arc::new(RwLock::new(HashMap::new())),
            username_index: Arc::new(RwLock::new(HashMap::new())),
            token_index: Arc::new(RwLock::new(HashMap::new())),
            owner_index: Arc::new(RwLock::new(HashMap::new())),
            token_regen_limiter: TokenRegenerationLimiter::production(),
        }
    }

    /// Create a new bot
    pub fn create_bot(&self, owner_id: UserId, request: CreateBotRequest) -> Result<Bot> {
        // Validate username uniqueness
        let username_index = self.username_index.read().unwrap();
        if username_index.contains_key(&request.username) {
            return Err(Error::validation("Bot username already exists"));
        }
        drop(username_index);

        // Create bot
        let mut bot = Bot::new(
            request.username.clone(),
            request.display_name,
            owner_id.clone(),
        )?;

        if let Some(desc) = request.description {
            bot.description = Some(desc);
        }

        // Add default commands
        bot.add_command(BotCommand::new(
            "start".to_string(),
            "Start the bot".to_string(),
        ));
        bot.add_command(BotCommand::new(
            "help".to_string(),
            "Show help message".to_string(),
        ));

        // Store bot
        let bot_id = bot.id;
        let token = bot.token.clone();

        let mut bots = self.bots.write().unwrap();
        bots.insert(bot_id, bot.clone());
        drop(bots);

        // Update indices
        let mut username_index = self.username_index.write().unwrap();
        username_index.insert(request.username, bot_id);
        drop(username_index);

        let mut token_index = self.token_index.write().unwrap();
        token_index.insert(token, bot_id);
        drop(token_index);

        let mut owner_index = self.owner_index.write().unwrap();
        owner_index.entry(owner_id).or_default().push(bot_id);
        drop(owner_index);

        Ok(bot)
    }

    /// Get bot by ID
    pub fn get_bot(&self, bot_id: &BotId) -> Option<Bot> {
        let bots = self.bots.read().unwrap();
        bots.get(bot_id).cloned()
    }

    /// Get bot by username
    pub fn get_bot_by_username(&self, username: &str) -> Option<Bot> {
        let username_index = self.username_index.read().unwrap();
        let bot_id = username_index.get(username).cloned()?;
        drop(username_index);

        self.get_bot(&bot_id)
    }

    /// Get bot by token
    pub fn get_bot_by_token(&self, token: &str) -> Option<Bot> {
        let token_index = self.token_index.read().unwrap();
        let bot_id = token_index.get(token).cloned()?;
        drop(token_index);

        self.get_bot(&bot_id)
    }

    /// Get all bots owned by a user
    pub fn get_user_bots(&self, owner_id: &UserId) -> Vec<Bot> {
        let owner_index = self.owner_index.read().unwrap();
        let bot_ids = match owner_index.get(owner_id) {
            Some(ids) => ids.clone(),
            None => return Vec::new(),
        };
        drop(owner_index);

        let bots = self.bots.read().unwrap();
        bot_ids
            .iter()
            .filter_map(|id| bots.get(id).cloned())
            .collect()
    }

    /// Update bot
    pub fn update_bot(
        &self,
        bot_id: &BotId,
        owner_id: &UserId,
        request: UpdateBotRequest,
    ) -> Result<Bot> {
        let mut bots = self.bots.write().unwrap();
        let bot = bots
            .get_mut(bot_id)
            .ok_or_else(|| Error::validation("Bot not found"))?;

        // Verify ownership
        if &bot.owner_id != owner_id {
            return Err(Error::validation("Not authorized to update this bot"));
        }

        // Apply updates
        if let Some(display_name) = request.display_name {
            bot.display_name = display_name;
        }

        if let Some(description) = request.description {
            bot.description = Some(description);
        }

        if let Some(about) = request.about {
            bot.about = Some(about);
        }

        if let Some(commands) = request.commands {
            bot.commands = commands;
        }

        Ok(bot.clone())
    }

    /// Delete bot
    pub fn delete_bot(&self, bot_id: &BotId, owner_id: &UserId) -> Result<()> {
        let mut bots = self.bots.write().unwrap();
        let bot = bots
            .get(bot_id)
            .ok_or_else(|| Error::validation("Bot not found"))?;

        // Verify ownership
        if &bot.owner_id != owner_id {
            return Err(Error::validation("Not authorized to delete this bot"));
        }

        let username = bot.username.clone();
        let token = bot.token.clone();
        let owner = bot.owner_id.clone();

        bots.remove(bot_id);
        drop(bots);

        // Update indices
        let mut username_index = self.username_index.write().unwrap();
        username_index.remove(&username);
        drop(username_index);

        let mut token_index = self.token_index.write().unwrap();
        token_index.remove(&token);
        drop(token_index);

        let mut owner_index = self.owner_index.write().unwrap();
        if let Some(bot_ids) = owner_index.get_mut(&owner) {
            bot_ids.retain(|id| id != bot_id);
        }
        drop(owner_index);

        Ok(())
    }

    /// Regenerate bot token
    /// 
    /// # Security Note
    /// Token regeneration is rate-limited to prevent brute-force attacks.
    /// Maximum 3 regenerations per hour per bot to mitigate:
    /// - Token theft through rapid regeneration
    /// - DoS attacks on bot availability  
    /// - Credential stuffing attempts
    pub fn regenerate_token(&self, bot_id: &BotId, owner_id: &UserId) -> Result<String> {
        // SECURITY FIX: Check rate limit before proceeding
        if let Err(remaining) = self.token_regen_limiter.check_and_record(bot_id) {
            let minutes = remaining.as_secs() / 60;
            return Err(Error::validation(format!(
                "Token regeneration rate limit exceeded. Try again in {} minutes.",
                minutes + 1
            )));
        }
        
        let mut bots = self.bots.write().unwrap();
        let bot = bots
            .get_mut(bot_id)
            .ok_or_else(|| Error::validation("Bot not found"))?;

        // Verify ownership
        if &bot.owner_id != owner_id {
            return Err(Error::validation("Not authorized"));
        }

        let old_token = bot.token.clone();
        bot.token = Bot::new("temp_bot".to_string(), "temp".to_string(), owner_id.clone())
            .unwrap()
            .token;

        let new_token = bot.token.clone();
        drop(bots);

        // Update token index
        let mut token_index = self.token_index.write().unwrap();
        token_index.remove(&old_token);
        token_index.insert(new_token.clone(), *bot_id);
        drop(token_index);

        Ok(new_token)
    }

    /// Get remaining token regenerations allowed for a bot within the current rate limit window
    /// 
    /// This allows users to check how many token regenerations they have left
    /// before hitting the rate limit (3 per hour by default).
    pub fn get_remaining_token_regenerations(&self, bot_id: &BotId) -> usize {
        self.token_regen_limiter.remaining_regenerations(bot_id)
    }

    /// Set bot active/inactive
    pub fn set_bot_active(&self, bot_id: &BotId, owner_id: &UserId, active: bool) -> Result<()> {
        let mut bots = self.bots.write().unwrap();
        let bot = bots
            .get_mut(bot_id)
            .ok_or_else(|| Error::validation("Bot not found"))?;

        // Verify ownership
        if &bot.owner_id != owner_id {
            return Err(Error::validation("Not authorized"));
        }

        bot.is_active = active;
        Ok(())
    }

    /// Get bot statistics
    pub fn get_all_bots_count(&self) -> usize {
        let bots = self.bots.read().unwrap();
        bots.len()
    }

    /// Get active bots count
    pub fn get_active_bots_count(&self) -> usize {
        let bots = self.bots.read().unwrap();
        bots.values().filter(|b| b.is_active).count()
    }
}

impl BotManager {
    /// Create a new BotManager
    pub fn new(bot_father: Arc<BotFather>) -> Self {
        Self { bot_father }
    }

    /// Create bot through manager
    pub fn create_bot(&self, owner_id: UserId, request: CreateBotRequest) -> Result<Bot> {
        self.bot_father.create_bot(owner_id, request)
    }

    /// Get bot
    pub fn get_bot(&self, bot_id: &BotId) -> Option<Bot> {
        self.bot_father.get_bot(bot_id)
    }

    /// Authenticate bot by token
    pub fn authenticate(&self, token: &str) -> Option<Bot> {
        self.bot_father.get_bot_by_token(token)
    }

    /// Get user's bots
    pub fn get_user_bots(&self, owner_id: &UserId) -> Vec<Bot> {
        self.bot_father.get_user_bots(owner_id)
    }
}

impl Default for BotFather {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_create_bot() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: Some("A test bot".to_string()),
        };

        let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
        assert_eq!(bot.username, "testbot");
        assert_eq!(bot.commands.len(), 2); // start and help
    }

    #[test]
    fn test_duplicate_username() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        bot_father
            .create_bot(owner_id.clone(), request.clone())
            .unwrap();
        let result = bot_father.create_bot(owner_id, request);
        assert!(result.is_err());
    }

    #[test]
    fn test_get_bot_by_username() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        bot_father.create_bot(owner_id.clone(), request).unwrap();

        let bot = bot_father.get_bot_by_username("testbot");
        assert!(bot.is_some());
        assert_eq!(bot.unwrap().username, "testbot");
    }

    #[test]
    fn test_get_bot_by_token() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
        let token = bot.token.clone();

        let retrieved = bot_father.get_bot_by_token(&token);
        assert!(retrieved.is_some());
        assert_eq!(retrieved.unwrap().id, bot.id);
    }

    #[test]
    fn test_update_bot() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let create_request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father
            .create_bot(owner_id.clone(), create_request)
            .unwrap();

        let update_request = UpdateBotRequest {
            display_name: Some("Updated Bot".to_string()),
            description: Some("New description".to_string()),
            about: None,
            avatar_data: None,
            commands: None,
        };

        let updated = bot_father
            .update_bot(&bot.id, &owner_id, update_request)
            .unwrap();
        assert_eq!(updated.display_name, "Updated Bot");
        assert_eq!(updated.description, Some("New description".to_string()));
    }

    #[test]
    fn test_delete_bot() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
        let bot_id = bot.id;

        bot_father.delete_bot(&bot_id, &owner_id).unwrap();

        assert!(bot_father.get_bot(&bot_id).is_none());
        assert!(bot_father.get_bot_by_username("testbot").is_none());
    }

    #[test]
    fn test_regenerate_token() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
        let old_token = bot.token.clone();

        let new_token = bot_father.regenerate_token(&bot.id, &owner_id).unwrap();
        assert_ne!(old_token, new_token);

        // Old token should not work
        assert!(bot_father.get_bot_by_token(&old_token).is_none());

        // New token should work
        assert!(bot_father.get_bot_by_token(&new_token).is_some());
    }

    #[test]
    fn test_get_user_bots() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        for i in 1..=3 {
            let request = CreateBotRequest {
                username: format!("test{}bot", i),
                display_name: format!("Test Bot {}", i),
                description: None,
            };
            bot_father.create_bot(owner_id.clone(), request).unwrap();
        }

        let bots = bot_father.get_user_bots(&owner_id);
        assert_eq!(bots.len(), 3);
    }

    #[test]
    fn test_set_bot_active() {
        let bot_father = BotFather::new();
        let owner_id = UserId::new();

        let request = CreateBotRequest {
            username: "testbot".to_string(),
            display_name: "Test Bot".to_string(),
            description: None,
        };

        let bot = bot_father.create_bot(owner_id.clone(), request).unwrap();
        assert!(bot.is_active);

        bot_father
            .set_bot_active(&bot.id, &owner_id, false)
            .unwrap();

        let updated_bot = bot_father.get_bot(&bot.id).unwrap();
        assert!(!updated_bot.is_active);
    }
}
