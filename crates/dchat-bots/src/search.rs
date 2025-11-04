//! Search functionality for users and bots

use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};

use crate::{Bot, BotId};
use dchat_identity::profile::UserProfile;

/// Search result type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SearchResult {
    User(UserProfile),
    Bot(BotSearchResult),
}

/// Bot search result (simplified bot info)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotSearchResult {
    /// Bot ID
    pub id: BotId,
    
    /// Username
    pub username: String,
    
    /// Display name
    pub display_name: String,
    
    /// Description
    pub description: Option<String>,
    
    /// Avatar hash
    pub avatar_hash: Option<String>,
    
    /// Is verified
    pub is_verified: bool,
    
    /// Total users
    pub total_users: u64,
    
    /// Rating (0-5)
    pub rating: f32,
    
    /// Tags/categories
    pub tags: Vec<String>,
}

/// Search filters
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct SearchFilters {
    /// Search type
    pub search_type: Option<SearchType>,
    
    /// Verified only
    pub verified_only: bool,
    
    /// Minimum rating (bots only)
    pub min_rating: Option<f32>,
    
    /// Tags (bots only)
    pub tags: Vec<String>,
    
    /// Online only (users only)
    pub online_only: bool,
    
    /// Maximum results
    pub limit: Option<usize>,
}

/// Search type filter
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum SearchType {
    Users,
    Bots,
    All,
}

impl Default for SearchType {
    fn default() -> Self {
        Self::All
    }
}

/// Global search manager for users and bots
pub struct SearchManager {
    user_profiles: Arc<RwLock<HashMap<dchat_core::types::UserId, UserProfile>>>,
    bots: Arc<RwLock<HashMap<BotId, Bot>>>,
    bot_metadata: Arc<RwLock<HashMap<BotId, BotMetadata>>>,
    username_index: Arc<RwLock<UsernameIndex>>,
}

/// Username index for fast lookups
#[derive(Default)]
struct UsernameIndex {
    users: HashMap<String, dchat_core::types::UserId>,
    bots: HashMap<String, BotId>,
}

/// Additional bot metadata for search
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotMetadata {
    pub bot_id: BotId,
    pub is_verified: bool,
    pub total_users: u64,
    pub rating: f32,
    pub rating_count: u64,
    pub tags: Vec<String>,
}

impl SearchManager {
    /// Create new search manager
    pub fn new() -> Self {
        Self {
            user_profiles: Arc::new(RwLock::new(HashMap::new())),
            bots: Arc::new(RwLock::new(HashMap::new())),
            bot_metadata: Arc::new(RwLock::new(HashMap::new())),
            username_index: Arc::new(RwLock::new(UsernameIndex::default())),
        }
    }
    
    /// Index a user profile
    pub fn index_user(&self, profile: UserProfile) -> Result<()> {
        let user_id = profile.user_id.clone();
        let username = profile.username.clone();
        
        {
            let mut profiles = self.user_profiles.write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;
            profiles.insert(user_id.clone(), profile);
        }
        
        {
            let mut index = self.username_index.write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;
            index.users.insert(username.to_lowercase(), user_id);
        }
        
        Ok(())
    }
    
    /// Index a bot
    pub fn index_bot(&self, bot: Bot, metadata: BotMetadata) -> Result<()> {
        let bot_id = bot.id;
        let username = bot.username.clone();
        
        {
            let mut bots = self.bots.write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;
            bots.insert(bot_id, bot);
        }
        
        {
            let mut meta = self.bot_metadata.write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;
            meta.insert(bot_id, metadata);
        }
        
        {
            let mut index = self.username_index.write()
                .map_err(|_| Error::internal("Failed to acquire write lock"))?;
            index.bots.insert(username.to_lowercase(), bot_id);
        }
        
        Ok(())
    }
    
    /// Search by username (exact match)
    pub fn search_by_username(&self, username: &str) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();
        let username_lower = username.to_lowercase();
        
        let index = self.username_index.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        
        // Search users
        if let Some(user_id) = index.users.get(&username_lower) {
            if let Some(profile) = self.get_user_profile(user_id)? {
                results.push(SearchResult::User(profile));
            }
        }
        
        // Search bots
        if let Some(bot_id) = index.bots.get(&username_lower) {
            if let Some(bot_result) = self.get_bot_search_result(bot_id)? {
                results.push(SearchResult::Bot(bot_result));
            }
        }
        
        Ok(results)
    }
    
    /// Search by query with filters
    pub fn search(&self, query: &str, filters: SearchFilters) -> Result<Vec<SearchResult>> {
        let mut results = Vec::new();
        let query_lower = query.to_lowercase();
        
        let search_users = filters.search_type.is_none() || 
            matches!(filters.search_type, Some(SearchType::All) | Some(SearchType::Users));
        
        let search_bots = filters.search_type.is_none() || 
            matches!(filters.search_type, Some(SearchType::All) | Some(SearchType::Bots));
        
        // Search users
        if search_users {
            let profiles = self.user_profiles.read()
                .map_err(|_| Error::internal("Failed to acquire read lock"))?;
            
            for profile in profiles.values() {
                if self.matches_user_query(profile, &query_lower, &filters) {
                    results.push(SearchResult::User(profile.clone()));
                }
            }
        }
        
        // Search bots
        if search_bots {
            let bots = self.bots.read()
                .map_err(|_| Error::internal("Failed to acquire read lock"))?;
            let metadata = self.bot_metadata.read()
                .map_err(|_| Error::internal("Failed to acquire read lock"))?;
            
            for bot in bots.values() {
                if self.matches_bot_query(bot, &metadata, &query_lower, &filters) {
                    if let Some(bot_result) = self.create_bot_search_result(bot, &metadata) {
                        results.push(SearchResult::Bot(bot_result));
                    }
                }
            }
        }
        
        // Apply limit
        if let Some(limit) = filters.limit {
            results.truncate(limit);
        }
        
        Ok(results)
    }
    
    /// Check if user matches search query
    fn matches_user_query(&self, profile: &UserProfile, query: &str, filters: &SearchFilters) -> bool {
        // Check verified filter
        if filters.verified_only && !profile.is_verified {
            return false;
        }
        
        // Check online filter
        if filters.online_only && !matches!(profile.online_status, dchat_identity::profile::OnlineStatus::Online) {
            return false;
        }
        
        // Check query match
        profile.username.to_lowercase().contains(query) ||
        profile.display_name.to_lowercase().contains(query) ||
        profile.bio.as_ref().map_or(false, |b| b.to_lowercase().contains(query))
    }
    
    /// Check if bot matches search query
    fn matches_bot_query(
        &self,
        bot: &Bot,
        metadata_map: &HashMap<BotId, BotMetadata>,
        query: &str,
        filters: &SearchFilters,
    ) -> bool {
        // Get metadata
        let metadata = match metadata_map.get(&bot.id) {
            Some(m) => m,
            None => return false,
        };
        
        // Check verified filter
        if filters.verified_only && !metadata.is_verified {
            return false;
        }
        
        // Check rating filter
        if let Some(min_rating) = filters.min_rating {
            if metadata.rating < min_rating {
                return false;
            }
        }
        
        // Check tags filter
        if !filters.tags.is_empty() {
            let has_matching_tag = filters.tags.iter()
                .any(|tag| metadata.tags.contains(tag));
            if !has_matching_tag {
                return false;
            }
        }
        
        // Check query match
        bot.username.to_lowercase().contains(query) ||
        bot.display_name.to_lowercase().contains(query) ||
        bot.description.as_ref().map_or(false, |d| d.to_lowercase().contains(query)) ||
        bot.about.as_ref().map_or(false, |a| a.to_lowercase().contains(query))
    }
    
    /// Get user profile by ID
    fn get_user_profile(&self, user_id: &dchat_core::types::UserId) -> Result<Option<UserProfile>> {
        let profiles = self.user_profiles.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        Ok(profiles.get(user_id).cloned())
    }
    
    /// Get bot search result by ID
    fn get_bot_search_result(&self, bot_id: &BotId) -> Result<Option<BotSearchResult>> {
        let bots = self.bots.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        let metadata = self.bot_metadata.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        
        let bot = match bots.get(bot_id) {
            Some(b) => b,
            None => return Ok(None),
        };
        
        Ok(self.create_bot_search_result(bot, &metadata))
    }
    
    /// Create bot search result from bot and metadata
    fn create_bot_search_result(
        &self,
        bot: &Bot,
        metadata_map: &HashMap<BotId, BotMetadata>,
    ) -> Option<BotSearchResult> {
        let metadata = metadata_map.get(&bot.id)?;
        
        Some(BotSearchResult {
            id: bot.id,
            username: bot.username.clone(),
            display_name: bot.display_name.clone(),
            description: bot.description.clone(),
            avatar_hash: bot.avatar_hash.clone(),
            is_verified: metadata.is_verified,
            total_users: metadata.total_users,
            rating: metadata.rating,
            tags: metadata.tags.clone(),
        })
    }
    
    /// Get popular bots
    pub fn get_popular_bots(&self, limit: usize) -> Result<Vec<BotSearchResult>> {
        let bots = self.bots.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        let metadata = self.bot_metadata.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        
        let mut bot_list: Vec<_> = bots.values()
            .filter_map(|bot| {
                let meta = metadata.get(&bot.id)?;
                Some((bot.clone(), meta.clone()))
            })
            .collect();
        
        // Sort by total users
        bot_list.sort_by(|a, b| b.1.total_users.cmp(&a.1.total_users));
        
        Ok(bot_list.iter()
            .take(limit)
            .filter_map(|(bot, _)| self.create_bot_search_result(bot, &metadata))
            .collect())
    }
    
    /// Get bots by category/tag
    pub fn get_bots_by_tag(&self, tag: &str, limit: Option<usize>) -> Result<Vec<BotSearchResult>> {
        let bots = self.bots.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        let metadata = self.bot_metadata.read()
            .map_err(|_| Error::internal("Failed to acquire read lock"))?;
        
        let mut results: Vec<_> = bots.values()
            .filter_map(|bot| {
                let meta = metadata.get(&bot.id)?;
                if meta.tags.iter().any(|t| t.eq_ignore_ascii_case(tag)) {
                    self.create_bot_search_result(bot, &metadata)
                } else {
                    None
                }
            })
            .collect();
        
        if let Some(limit) = limit {
            results.truncate(limit);
        }
        
        Ok(results)
    }
    
    /// Update bot rating
    pub fn update_bot_rating(&self, bot_id: &BotId, rating: f32) -> Result<()> {
        if !(0.0..=5.0).contains(&rating) {
            return Err(Error::validation("Rating must be between 0 and 5"));
        }
        
        let mut metadata = self.bot_metadata.write()
            .map_err(|_| Error::internal("Failed to acquire write lock"))?;
        
        let meta = metadata.get_mut(bot_id)
            .ok_or_else(|| Error::validation("Bot not found"))?;
        
        // Calculate new average rating
        let total = meta.rating * meta.rating_count as f32;
        meta.rating_count += 1;
        meta.rating = (total + rating) / meta.rating_count as f32;
        
        Ok(())
    }
}

impl Default for SearchManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    
    #[test]
    fn test_search_by_username() {
        let manager = SearchManager::new();
        
        // Create and index a user
        let user_id = dchat_core::types::UserId::new();
        let profile = dchat_identity::profile::UserProfile {
            user_id: user_id.clone(),
            username: "testuser".to_string(),
            display_name: "Test User".to_string(),
            bio: None,
            profile_picture: None,
            status: None,
            online_status: dchat_identity::profile::OnlineStatus::Online,
            last_seen: None,
            created_at: Utc::now(),
            privacy: dchat_identity::profile::PrivacySettings::default(),
            is_verified: false,
            metadata: std::collections::HashMap::new(),
        };
        
        manager.index_user(profile).unwrap();
        
        // Search by username
        let results = manager.search_by_username("testuser").unwrap();
        assert_eq!(results.len(), 1);
        
        match &results[0] {
            SearchResult::User(p) => assert_eq!(p.username, "testuser"),
            _ => panic!("Test failed: Expected user result but got different result type"),
        }
    }
    
    #[test]
    fn test_search_with_filters() {
        let manager = SearchManager::new();
        
        // Create multiple users
        for i in 1..=5 {
            let user_id = dchat_core::types::UserId::new();
            let profile = dchat_identity::profile::UserProfile {
                user_id: user_id.clone(),
                username: format!("user{}", i),
                display_name: format!("User {}", i),
                bio: None,
                profile_picture: None,
                status: None,
                online_status: if i % 2 == 0 {
                    dchat_identity::profile::OnlineStatus::Online
                } else {
                    dchat_identity::profile::OnlineStatus::Offline
                },
                last_seen: None,
                created_at: Utc::now(),
                privacy: dchat_identity::profile::PrivacySettings::default(),
                is_verified: i == 1,
                metadata: std::collections::HashMap::new(),
            };
            manager.index_user(profile).unwrap();
        }
        
        // Search all
        let results = manager.search("user", SearchFilters::default()).unwrap();
        assert_eq!(results.len(), 5);
        
        // Search online only
        let results = manager.search("user", SearchFilters {
            online_only: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(results.len(), 2);
        
        // Search verified only
        let results = manager.search("user", SearchFilters {
            verified_only: true,
            ..Default::default()
        }).unwrap();
        assert_eq!(results.len(), 1);
    }
    
    #[test]
    fn test_bot_rating_update() {
        let manager = SearchManager::new();
        let bot_id = uuid::Uuid::new_v4();
        
        let metadata = BotMetadata {
            bot_id,
            is_verified: false,
            total_users: 100,
            rating: 4.0,
            rating_count: 10,
            tags: vec!["utility".to_string()],
        };
        
        {
            let mut meta_map = manager.bot_metadata.write().unwrap();
            meta_map.insert(bot_id, metadata);
        }
        
        // Add new rating
        manager.update_bot_rating(&bot_id, 5.0).unwrap();
        
        let meta_map = manager.bot_metadata.read().unwrap();
        let updated = meta_map.get(&bot_id).unwrap();
        
        assert_eq!(updated.rating_count, 11);
        assert!(updated.rating > 4.0 && updated.rating < 4.1);
    }
}
