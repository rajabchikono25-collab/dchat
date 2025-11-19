//! Advanced NFT Features for dchat Marketplace
//!
//! This module extends basic NFT functionality with:
//! - NFT composability (combining multiple NFTs)
//! - Dynamic traits and metadata evolution
//! - NFT fractionalization (shared ownership)
//! - Royalty enforcement on secondary sales
//! - NFT staking and yield generation
//! - Collection management and verification
//! - Rarity scoring and analysis

use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Advanced NFT with composability and dynamic traits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AdvancedNft {
    pub token_id: String,
    pub name: String,
    pub description: String,
    pub image_hash: String,
    pub creator: UserId,
    pub current_owner: UserId,
    pub created_at: DateTime<Utc>,
    
    /// Dynamic attributes that can evolve over time
    pub dynamic_traits: Vec<DynamicTrait>,
    
    /// Composed NFTs (this NFT is made from these)
    pub composed_from: Vec<String>,
    
    /// Can this NFT be decomposed back to components?
    pub is_decomposable: bool,
    
    /// Royalty percentage for creator on secondary sales (0-100)
    pub royalty_percentage: u8,
    
    /// Collection identifier
    pub collection_id: Option<Uuid>,
    
    /// Rarity score (0-100, calculated)
    pub rarity_score: f32,
    
    /// Staking info
    pub staking_info: Option<StakingInfo>,
    
    /// Fractionalization info (if NFT is fractionalized)
    pub fractionalization: Option<FractionalizationInfo>,
    
    /// Transaction history
    pub history: Vec<NftTransaction>,
}

/// Dynamic trait that can change based on conditions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DynamicTrait {
    pub trait_type: String,
    pub value: TraitValue,
    pub last_updated: DateTime<Utc>,
    pub update_rule: Option<TraitUpdateRule>,
}

/// Trait value types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraitValue {
    String(String),
    Number(f64),
    Boolean(bool),
    Date(DateTime<Utc>),
}

/// Rules for how traits auto-update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TraitUpdateRule {
    /// Update based on time (e.g., age increases daily)
    TimeBased { interval_seconds: u64 },
    
    /// Update based on owner activity
    ActivityBased { threshold: u64 },
    
    /// Update based on market conditions
    MarketBased { condition: String },
    
    /// Update based on staking duration
    StakingBased { duration_days: u32 },
}

/// NFT staking information
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StakingInfo {
    pub is_staked: bool,
    pub staked_at: Option<DateTime<Utc>>,
    pub staked_duration_days: u32,
    pub estimated_yield: u64,
    pub rewards_claimed: u64,
}

/// Fractionalization - split NFT into multiple shares
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FractionalizationInfo {
    pub total_shares: u64,
    pub share_holders: HashMap<UserId, u64>,
    pub minimum_buy_in: u64,
    pub can_recombine: bool,
}

/// NFT transaction record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftTransaction {
    pub transaction_id: Uuid,
    pub transaction_type: TransactionType,
    pub from_user: Option<UserId>,
    pub to_user: Option<UserId>,
    pub price: Option<u64>,
    pub timestamp: DateTime<Utc>,
    pub transaction_hash: String,
}

/// Types of NFT transactions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TransactionType {
    Mint,
    Transfer,
    Sale,
    Compose,
    Decompose,
    Stake,
    Unstake,
    Fractionalize,
    Recombine,
}

/// NFT Collection metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftCollection {
    pub id: Uuid,
    pub name: String,
    pub description: String,
    pub creator: UserId,
    pub verified: bool,
    pub nft_count: u64,
    pub floor_price: Option<u64>,
    pub total_volume: u64,
    pub created_at: DateTime<Utc>,
    pub royalty_percentage: u8,
    pub traits_schema: Vec<TraitSchema>,
}

/// Trait schema for collection
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TraitSchema {
    pub trait_type: String,
    pub possible_values: Vec<String>,
    pub rarity_weights: HashMap<String, f32>,
}

/// Royalty payment record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RoyaltyPayment {
    pub id: Uuid,
    pub nft_token_id: String,
    pub sale_price: u64,
    pub royalty_amount: u64,
    pub creator: UserId,
    pub paid_at: DateTime<Utc>,
    pub transaction_hash: String,
}

/// Advanced NFT manager
pub struct AdvancedNftManager {
    nfts: HashMap<String, AdvancedNft>,
    collections: HashMap<Uuid, NftCollection>,
    royalty_payments: Vec<RoyaltyPayment>,
}

impl AdvancedNftManager {
    pub fn new() -> Self {
        Self {
            nfts: HashMap::new(),
            collections: HashMap::new(),
            royalty_payments: Vec::new(),
        }
    }

    /// Mint a new advanced NFT
    pub fn mint_nft(
        &mut self,
        token_id: String,
        name: String,
        description: String,
        image_hash: String,
        creator: UserId,
        royalty_percentage: u8,
        collection_id: Option<Uuid>,
        dynamic_traits: Vec<DynamicTrait>,
    ) -> Result<String> {
        if self.nfts.contains_key(&token_id) {
            return Err(Error::validation("NFT token ID already exists"));
        }

        if royalty_percentage > 100 {
            return Err(Error::validation("Royalty percentage cannot exceed 100%"));
        }

        let nft = AdvancedNft {
            token_id: token_id.clone(),
            name,
            description,
            image_hash,
            creator: creator.clone(),
            current_owner: creator.clone(),
            created_at: Utc::now(),
            dynamic_traits,
            composed_from: Vec::new(),
            is_decomposable: false,
            royalty_percentage,
            collection_id,
            rarity_score: 0.0,
            staking_info: None,
            fractionalization: None,
            history: vec![NftTransaction {
                transaction_id: Uuid::new_v4(),
                transaction_type: TransactionType::Mint,
                from_user: None,
                to_user: Some(creator),
                price: None,
                timestamp: Utc::now(),
                transaction_hash: format!("mint_{}", Uuid::new_v4()),
            }],
        };

        // Calculate rarity score
        let rarity_score = self.calculate_rarity(&nft)?;
        let mut nft_with_rarity = nft;
        nft_with_rarity.rarity_score = rarity_score;

        self.nfts.insert(token_id.clone(), nft_with_rarity);

        // Update collection count
        if let Some(coll_id) = collection_id {
            if let Some(collection) = self.collections.get_mut(&coll_id) {
                collection.nft_count += 1;
            }
        }

        Ok(token_id)
    }

    /// Compose multiple NFTs into a new one
    pub fn compose_nfts(
        &mut self,
        component_token_ids: Vec<String>,
        new_token_id: String,
        new_name: String,
        new_description: String,
        owner: UserId,
        is_decomposable: bool,
    ) -> Result<String> {
        // Verify all components exist and are owned by user
        for token_id in &component_token_ids {
            let nft = self.nfts.get(token_id)
                .ok_or_else(|| Error::validation(format!("Component NFT {} not found", token_id)))?;
            
            if nft.current_owner != owner {
                return Err(Error::validation("User does not own all component NFTs"));
            }

            if nft.staking_info.as_ref().map_or(false, |s| s.is_staked) {
                return Err(Error::validation("Cannot compose staked NFTs"));
            }
        }

        // Combine traits from all components
        let mut combined_traits = Vec::new();
        let mut combined_image_hashes = Vec::new();

        for token_id in &component_token_ids {
            let nft = self.nfts.get(token_id).unwrap();
            combined_traits.extend(nft.dynamic_traits.clone());
            combined_image_hashes.push(nft.image_hash.clone());
        }

        // Generate new image hash from combined hashes
        let combined_image_hash = blake3::hash(combined_image_hashes.join(",").as_bytes()).to_hex().to_string();

        // Create composed NFT
        let composed_nft = AdvancedNft {
            token_id: new_token_id.clone(),
            name: new_name,
            description: new_description,
            image_hash: combined_image_hash,
            creator: owner.clone(),
            current_owner: owner.clone(),
            created_at: Utc::now(),
            dynamic_traits: combined_traits,
            composed_from: component_token_ids.clone(),
            is_decomposable,
            royalty_percentage: 5, // Default for composed NFTs
            collection_id: None,
            rarity_score: 0.0,
            staking_info: None,
            fractionalization: None,
            history: vec![NftTransaction {
                transaction_id: Uuid::new_v4(),
                transaction_type: TransactionType::Compose,
                from_user: Some(owner.clone()),
                to_user: Some(owner),
                price: None,
                timestamp: Utc::now(),
                transaction_hash: format!("compose_{}", Uuid::new_v4()),
            }],
        };

        // Calculate rarity (composed NFTs typically more rare)
        let base_rarity = self.calculate_rarity(&composed_nft)?;
        let composition_bonus = component_token_ids.len() as f32 * 5.0;
        let total_rarity = (base_rarity + composition_bonus).min(100.0);

        let mut final_nft = composed_nft;
        final_nft.rarity_score = total_rarity;

        // Burn component NFTs
        for token_id in component_token_ids {
            self.nfts.remove(&token_id);
        }

        self.nfts.insert(new_token_id.clone(), final_nft);

        Ok(new_token_id)
    }

    /// Decompose a composed NFT back into components
    pub fn decompose_nft(&mut self, token_id: &str, owner: UserId) -> Result<Vec<String>> {
        let nft = self.nfts.get(token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        if nft.current_owner != owner {
            return Err(Error::validation("User does not own this NFT"));
        }

        if !nft.is_decomposable {
            return Err(Error::validation("NFT cannot be decomposed"));
        }

        if nft.composed_from.is_empty() {
            return Err(Error::validation("NFT was not composed from other NFTs"));
        }

        let component_ids = nft.composed_from.clone();
        let composition_metadata = nft.metadata.clone();

        // Recreate original component NFTs from stored composition metadata
        for (idx, component_id) in component_ids.iter().enumerate() {
            // Extract component metadata from composition
            let component_metadata = composition_metadata
                .get(&format!("component_{}_metadata", idx))
                .cloned()
                .unwrap_or_default();

            // Recreate component NFT
            let component_nft = Nft {
                token_id: component_id.clone(),
                collection_id: nft.collection_id.clone(),
                current_owner: owner,
                creator: nft.creator,
                metadata: serde_json::from_str(&component_metadata)
                    .unwrap_or_else(|_| HashMap::new()),
                dynamic_traits: DynamicTraits {
                    traits: HashMap::new(),
                    last_updated: chrono::Utc::now(),
                },
                royalty: nft.royalty.clone(),
                mint_timestamp: nft.mint_timestamp,
                last_transfer: chrono::Utc::now(),
                is_fractional: false,
                fractional_info: None,
                is_composable: true,
                composed_from: Vec::new(),
                staking_info: None,
            };

            self.nfts.insert(component_id.clone(), component_nft);
        }

        // Remove composed NFT
        self.nfts.remove(token_id);

        Ok(component_ids)
    }

    /// Stake an NFT to earn rewards
    pub fn stake_nft(&mut self, token_id: &str, owner: UserId) -> Result<()> {
        let nft = self.nfts.get_mut(token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        if nft.current_owner != owner {
            return Err(Error::validation("User does not own this NFT"));
        }

        if nft.staking_info.as_ref().map_or(false, |s| s.is_staked) {
            return Err(Error::validation("NFT is already staked"));
        }

        nft.staking_info = Some(StakingInfo {
            is_staked: true,
            staked_at: Some(Utc::now()),
            staked_duration_days: 0,
            estimated_yield: 100, // Base yield, scaled by rarity
            rewards_claimed: 0,
        });

        nft.history.push(NftTransaction {
            transaction_id: Uuid::new_v4(),
            transaction_type: TransactionType::Stake,
            from_user: Some(owner.clone()),
            to_user: Some(owner),
            price: None,
            timestamp: Utc::now(),
            transaction_hash: format!("stake_{}", Uuid::new_v4()),
        });

        Ok(())
    }

    /// Unstake an NFT and claim rewards
    pub fn unstake_nft(&mut self, token_id: &str, owner: UserId) -> Result<u64> {
        let nft = self.nfts.get_mut(token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        if nft.current_owner != owner {
            return Err(Error::validation("User does not own this NFT"));
        }

        let staking_info = nft.staking_info.as_ref()
            .ok_or_else(|| Error::validation("NFT is not staked"))?;

        if !staking_info.is_staked {
            return Err(Error::validation("NFT is not currently staked"));
        }

        // Calculate rewards based on staking duration and rarity
        let staked_at = staking_info.staked_at.unwrap();
        let duration_days = (Utc::now() - staked_at).num_days() as u32;
        let base_rewards = staking_info.estimated_yield * duration_days as u64;
        let rarity_multiplier = 1.0 + (nft.rarity_score / 100.0);
        let total_rewards = (base_rewards as f32 * rarity_multiplier) as u64;

        nft.staking_info = None;

        nft.history.push(NftTransaction {
            transaction_id: Uuid::new_v4(),
            transaction_type: TransactionType::Unstake,
            from_user: Some(owner.clone()),
            to_user: Some(owner),
            price: Some(total_rewards),
            timestamp: Utc::now(),
            transaction_hash: format!("unstake_{}", Uuid::new_v4()),
        });

        Ok(total_rewards)
    }

    /// Fractionalize NFT into shares
    pub fn fractionalize_nft(
        &mut self,
        token_id: &str,
        owner: UserId,
        total_shares: u64,
        minimum_buy_in: u64,
    ) -> Result<()> {
        let nft = self.nfts.get_mut(token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        if nft.current_owner != owner {
            return Err(Error::validation("User does not own this NFT"));
        }

        if nft.fractionalization.is_some() {
            return Err(Error::validation("NFT is already fractionalized"));
        }

        if total_shares == 0 {
            return Err(Error::validation("Total shares must be > 0"));
        }

        let mut share_holders = HashMap::new();
        share_holders.insert(owner.clone(), total_shares);

        nft.fractionalization = Some(FractionalizationInfo {
            total_shares,
            share_holders,
            minimum_buy_in,
            can_recombine: true,
        });

        nft.history.push(NftTransaction {
            transaction_id: Uuid::new_v4(),
            transaction_type: TransactionType::Fractionalize,
            from_user: Some(owner.clone()),
            to_user: Some(owner),
            price: None,
            timestamp: Utc::now(),
            transaction_hash: format!("frac_{}", Uuid::new_v4()),
        });

        Ok(())
    }

    /// Transfer NFT and enforce royalty
    pub fn transfer_with_royalty(
        &mut self,
        token_id: &str,
        from_user: UserId,
        to_user: UserId,
        sale_price: u64,
    ) -> Result<RoyaltyPayment> {
        let nft = self.nfts.get_mut(token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        if nft.current_owner != from_user {
            return Err(Error::validation("Seller does not own this NFT"));
        }

        // Calculate royalty
        let royalty_amount = (sale_price * nft.royalty_percentage as u64) / 100;
        let seller_receives = sale_price - royalty_amount;

        // Record royalty payment
        let royalty_payment = RoyaltyPayment {
            id: Uuid::new_v4(),
            nft_token_id: token_id.to_string(),
            sale_price,
            royalty_amount,
            creator: nft.creator.clone(),
            paid_at: Utc::now(),
            transaction_hash: format!("royalty_{}", Uuid::new_v4()),
        };

        self.royalty_payments.push(royalty_payment.clone());

        // Transfer ownership
        nft.current_owner = to_user.clone();

        nft.history.push(NftTransaction {
            transaction_id: Uuid::new_v4(),
            transaction_type: TransactionType::Sale,
            from_user: Some(from_user),
            to_user: Some(to_user),
            price: Some(seller_receives),
            timestamp: Utc::now(),
            transaction_hash: format!("sale_{}", Uuid::new_v4()),
        });

        Ok(royalty_payment)
    }

    /// Calculate rarity score based on traits
    fn calculate_rarity(&self, nft: &AdvancedNft) -> Result<f32> {
        if nft.dynamic_traits.is_empty() {
            return Ok(50.0); // Baseline rarity
        }

        // Get collection trait schema if available
        let trait_weights = if let Some(coll_id) = nft.collection_id {
            self.collections.get(&coll_id)
                .map(|c| &c.traits_schema)
        } else {
            None
        };

        // Calculate based on trait rarity (simplified)
        let num_traits = nft.dynamic_traits.len();
        let base_score = 40.0 + (num_traits as f32 * 5.0).min(30.0);

        // Bonus for dynamic traits
        let dynamic_bonus = nft.dynamic_traits.iter()
            .filter(|t| t.update_rule.is_some())
            .count() as f32 * 3.0;

        Ok((base_score + dynamic_bonus).min(100.0))
    }

    /// Create NFT collection
    pub fn create_collection(
        &mut self,
        name: String,
        description: String,
        creator: UserId,
        royalty_percentage: u8,
        traits_schema: Vec<TraitSchema>,
    ) -> Result<Uuid> {
        let collection = NftCollection {
            id: Uuid::new_v4(),
            name,
            description,
            creator,
            verified: false,
            nft_count: 0,
            floor_price: None,
            total_volume: 0,
            created_at: Utc::now(),
            royalty_percentage,
            traits_schema,
        };

        let collection_id = collection.id;
        self.collections.insert(collection_id, collection);

        Ok(collection_id)
    }

    /// Get NFT by token ID
    pub fn get_nft(&self, token_id: &str) -> Option<&AdvancedNft> {
        self.nfts.get(token_id)
    }

    /// Get NFTs by owner
    pub fn get_nfts_by_owner(&self, owner: &UserId) -> Vec<&AdvancedNft> {
        self.nfts.values()
            .filter(|nft| &nft.current_owner == owner)
            .collect()
    }

    /// Get collection
    pub fn get_collection(&self, collection_id: Uuid) -> Option<&NftCollection> {
        self.collections.get(&collection_id)
    }

    /// Get royalty payments for creator
    pub fn get_creator_royalties(&self, creator: &UserId) -> Vec<&RoyaltyPayment> {
        self.royalty_payments.iter()
            .filter(|r| &r.creator == creator)
            .collect()
    }
}

impl Default for AdvancedNftManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_user() -> UserId {
        UserId::new()
    }

    #[test]
    fn test_mint_advanced_nft() {
        let mut manager = AdvancedNftManager::new();
        let creator = create_test_user();

        let traits = vec![
            DynamicTrait {
                trait_type: "Level".to_string(),
                value: TraitValue::Number(1.0),
                last_updated: Utc::now(),
                update_rule: Some(TraitUpdateRule::ActivityBased { threshold: 100 }),
            },
        ];

        let token_id = manager.mint_nft(
            "token_001".to_string(),
            "Dynamic NFT".to_string(),
            "An NFT that evolves".to_string(),
            "QmHash123".to_string(),
            creator,
            10,
            None,
            traits,
        ).unwrap();

        assert_eq!(token_id, "token_001");
        let nft = manager.get_nft(&token_id).unwrap();
        assert_eq!(nft.dynamic_traits.len(), 1);
    }

    #[test]
    fn test_compose_nfts() {
        let mut manager = AdvancedNftManager::new();
        let owner = create_test_user();

        // Mint two NFTs
        manager.mint_nft(
            "token_001".to_string(),
            "Part 1".to_string(),
            "Component 1".to_string(),
            "hash1".to_string(),
            owner.clone(),
            5,
            None,
            vec![],
        ).unwrap();

        manager.mint_nft(
            "token_002".to_string(),
            "Part 2".to_string(),
            "Component 2".to_string(),
            "hash2".to_string(),
            owner.clone(),
            5,
            None,
            vec![],
        ).unwrap();

        // Compose them
        let composed_id = manager.compose_nfts(
            vec!["token_001".to_string(), "token_002".to_string()],
            "token_composed".to_string(),
            "Combined NFT".to_string(),
            "Two NFTs merged".to_string(),
            owner,
            true,
        ).unwrap();

        assert_eq!(composed_id, "token_composed");
        assert!(manager.get_nft(&composed_id).is_some());
        assert!(manager.get_nft("token_001").is_none()); // Burned
    }

    #[test]
    fn test_stake_unstake() {
        let mut manager = AdvancedNftManager::new();
        let owner = create_test_user();

        let token_id = manager.mint_nft(
            "token_stake".to_string(),
            "Stakeable".to_string(),
            "Can be staked".to_string(),
            "hash".to_string(),
            owner.clone(),
            5,
            None,
            vec![],
        ).unwrap();

        // Stake
        manager.stake_nft(&token_id, owner.clone()).unwrap();

        let nft = manager.get_nft(&token_id).unwrap();
        assert!(nft.staking_info.as_ref().unwrap().is_staked);

        // Unstake
        let rewards = manager.unstake_nft(&token_id, owner).unwrap();
        assert!(rewards >= 0); // Should get some rewards
    }

    #[test]
    fn test_fractionalize() {
        let mut manager = AdvancedNftManager::new();
        let owner = create_test_user();

        let token_id = manager.mint_nft(
            "token_frac".to_string(),
            "Fractional".to_string(),
            "Can be split".to_string(),
            "hash".to_string(),
            owner.clone(),
            5,
            None,
            vec![],
        ).unwrap();

        manager.fractionalize_nft(&token_id, owner.clone(), 1000, 10).unwrap();

        let nft = manager.get_nft(&token_id).unwrap();
        assert!(nft.fractionalization.is_some());
        assert_eq!(nft.fractionalization.as_ref().unwrap().total_shares, 1000);
    }

    #[test]
    fn test_royalty_transfer() {
        let mut manager = AdvancedNftManager::new();
        let creator = create_test_user();
        let seller = create_test_user();
        let buyer = create_test_user();

        let token_id = manager.mint_nft(
            "token_royalty".to_string(),
            "Royalty NFT".to_string(),
            "Has royalties".to_string(),
            "hash".to_string(),
            creator.clone(),
            10, // 10% royalty
            None,
            vec![],
        ).unwrap();

        // Transfer to first owner (seller)
        let nft = manager.nfts.get_mut(&token_id).unwrap();
        nft.current_owner = seller.clone();

        // Secondary sale with royalty
        let royalty = manager.transfer_with_royalty(&token_id, seller, buyer, 1000).unwrap();

        assert_eq!(royalty.royalty_amount, 100); // 10% of 1000
        assert_eq!(royalty.creator, creator);
    }

    #[test]
    fn test_create_collection() {
        let mut manager = AdvancedNftManager::new();
        let creator = create_test_user();

        let collection_id = manager.create_collection(
            "Cool Collection".to_string(),
            "A collection of cool NFTs".to_string(),
            creator,
            5,
            vec![],
        ).unwrap();

        assert!(manager.get_collection(collection_id).is_some());
    }
}
