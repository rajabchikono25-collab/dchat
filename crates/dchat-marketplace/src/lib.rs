//! Marketplace and Digital Goods Infrastructure
//!
//! This module implements the dchat marketplace for:
//! - Digital goods (sticker packs, themes, bots)
//! - NFT integration and trading
//! - Creator economy (tips, subscriptions)
//! - Listing management and discovery
//! - Escrow system with dispute resolution

use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::fmt;
use uuid::Uuid;

pub mod escrow;

// Re-export types for escrow module
pub mod types {
    use super::*;
    pub use dchat_core::types::UserId;
    pub type ListingId = Uuid;

    /// Marketplace-specific error types
    #[derive(Debug, Clone)]
    pub enum MarketplaceError {
        EscrowNotFound,
        InvalidEscrowState,
        Unauthorized,
    }

    impl fmt::Display for MarketplaceError {
        fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
            match self {
                MarketplaceError::EscrowNotFound => write!(f, "Escrow not found"),
                MarketplaceError::InvalidEscrowState => write!(f, "Invalid escrow state"),
                MarketplaceError::Unauthorized => write!(f, "Unauthorized operation"),
            }
        }
    }

    impl std::error::Error for MarketplaceError {}
}

/// Types of digital goods available in the marketplace
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum DigitalGoodType {
    /// Sticker pack with multiple stickers
    StickerPack,
    /// Emoji pack with custom emojis
    EmojiPack,
    /// Theme/skin for the UI
    Theme,
    /// Bot or automation script (transferable ownership)
    Bot,
    /// NFT collectible
    Nft,
    /// Image or artwork
    Image,
    /// Subscription to premium features
    Subscription,
    /// Channel badge or role
    Badge,
    /// Channel ownership transfer
    Channel,
    /// Channel membership (access pass)
    Membership,
}

/// Where the digital good is stored
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub enum OnChainStorageType {
    /// Stored on chat chain (ownership, metadata)
    ChatChain,
    /// Stored on currency chain (financial data)
    CurrencyChain,
    /// Stored on IPFS (content files)
    Ipfs,
    /// Hybrid (metadata on-chain, content on IPFS)
    Hybrid,
}

/// Represents a transferable asset
pub trait TransferableAsset {
    /// Get unique asset identifier
    fn asset_id(&self) -> String;
    /// Get current owner
    fn current_owner(&self) -> UserId;
    /// Transfer ownership to new owner
    fn transfer_ownership(&mut self, new_owner: UserId) -> Result<()>;
    /// Get on-chain address or identifier
    fn on_chain_address(&self) -> Option<String>;
}

/// Pricing model for digital goods
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PricingModel {
    /// One-time purchase
    OneTime { price: u64 },
    /// Subscription with recurring payments
    Subscription { price_per_month: u64 },
    /// Free (with optional tips)
    Free,
    /// Pay-what-you-want with minimum
    PayWhatYouWant { minimum: u64 },
}

/// A digital good listing in the marketplace
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Listing {
    pub id: Uuid,
    pub creator: UserId,
    pub title: String,
    pub description: String,
    pub good_type: DigitalGoodType,
    pub pricing: PricingModel,
    pub created_at: DateTime<Utc>,
    pub downloads: u64,
    pub rating: f32,
    pub is_verified: bool,
    /// IPFS CID or content hash
    pub content_hash: String,
    /// Storage location type
    pub storage_type: OnChainStorageType,
    /// On-chain contract address or identifier
    pub on_chain_address: Option<String>,
    /// NFT token ID if applicable
    pub nft_token_id: Option<String>,
    /// Bot ID if selling a bot
    pub bot_id: Option<Uuid>,
    /// Channel ID if selling channel or membership
    pub channel_id: Option<Uuid>,
    /// Membership duration in days (if applicable)
    pub membership_duration_days: Option<u32>,
    /// Whether item is currently in escrow
    pub in_escrow: bool,
    /// Escrow ID if currently escrowed
    pub escrow_id: Option<Uuid>,
}

/// A purchase transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Purchase {
    pub id: Uuid,
    pub buyer: UserId,
    pub listing_id: Uuid,
    pub amount_paid: u64,
    pub purchased_at: DateTime<Utc>,
    pub transaction_hash: String,
}

/// Creator earnings and statistics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorStats {
    pub creator: UserId,
    pub total_sales: u64,
    pub total_earnings: u64,
    pub active_listings: u64,
    pub total_downloads: u64,
    pub average_rating: f32,
}

/// NFT metadata for on-chain collectibles
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftMetadata {
    pub token_id: String,
    pub name: String,
    pub description: String,
    pub image_hash: String,
    pub attributes: Vec<NftAttribute>,
    pub creator: UserId,
    pub owner: UserId,
    pub created_at: DateTime<Utc>,
}

/// NFT attribute (trait)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NftAttribute {
    pub trait_type: String,
    pub value: String,
}

/// Bot ownership transfer record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BotOwnership {
    pub bot_id: Uuid,
    pub bot_username: String,
    pub current_owner: UserId,
    pub previous_owners: Vec<(UserId, DateTime<Utc>)>,
    pub on_chain_address: String,
    pub transfer_count: u32,
}

/// Channel ownership transfer record
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelOwnership {
    pub channel_id: Uuid,
    pub channel_name: String,
    pub current_owner: UserId,
    pub previous_owners: Vec<(UserId, DateTime<Utc>)>,
    pub on_chain_address: String,
    pub member_count: u64,
    pub transfer_count: u32,
}

/// Emoji pack metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EmojiPack {
    pub pack_id: Uuid,
    pub name: String,
    pub description: String,
    pub emoji_count: u32,
    pub creator: UserId,
    pub content_hash: String,
    pub preview_emojis: Vec<String>,
    pub is_animated: bool,
}

/// Image artwork metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ImageArtwork {
    pub image_id: Uuid,
    pub title: String,
    pub description: String,
    pub creator: UserId,
    pub content_hash: String,
    pub width: u32,
    pub height: u32,
    pub format: String,
    pub license_type: LicenseType,
}

/// License types for images
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum LicenseType {
    /// All rights reserved
    AllRightsReserved,
    /// Creative Commons Attribution
    CcBy,
    /// Creative Commons Attribution-ShareAlike
    CcBySa,
    /// Creative Commons Attribution-NoDerivs
    CcByNd,
    /// Creative Commons Attribution-NonCommercial
    CcByNc,
    /// Public domain
    PublicDomain,
}

/// Channel membership pass
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelMembership {
    pub membership_id: Uuid,
    pub channel_id: Uuid,
    pub holder: UserId,
    pub purchased_at: DateTime<Utc>,
    pub expires_at: DateTime<Utc>,
    pub is_transferable: bool,
    pub access_level: MembershipAccessLevel,
}

/// Membership access levels
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MembershipAccessLevel {
    /// Basic read access
    Basic,
    /// Can post messages
    Contributor,
    /// Can moderate
    Moderator,
    /// VIP with special perks
    Vip,
}

/// Marketplace manager for handling listings and purchases
pub struct MarketplaceManager {
    listings: Vec<Listing>,
    purchases: Vec<Purchase>,
    nft_registry: Vec<NftMetadata>,
    bot_ownership: Vec<BotOwnership>,
    channel_ownership: Vec<ChannelOwnership>,
    emoji_packs: Vec<EmojiPack>,
    images: Vec<ImageArtwork>,
    memberships: Vec<ChannelMembership>,
    pub escrow: escrow::EscrowManager,
}

impl MarketplaceManager {
    /// Create a new marketplace manager
    pub fn new() -> Self {
        Self {
            listings: Vec::new(),
            purchases: Vec::new(),
            nft_registry: Vec::new(),
            bot_ownership: Vec::new(),
            channel_ownership: Vec::new(),
            emoji_packs: Vec::new(),
            images: Vec::new(),
            memberships: Vec::new(),
            escrow: escrow::EscrowManager::new(),
        }
    }

    /// Create a new listing with optional on-chain storage
    pub fn create_listing(
        &mut self,
        creator: UserId,
        title: String,
        description: String,
        good_type: DigitalGoodType,
        pricing: PricingModel,
        content_hash: String,
        storage_type: OnChainStorageType,
        nft_token_id: Option<String>,
        bot_id: Option<Uuid>,
        channel_id: Option<Uuid>,
        membership_duration_days: Option<u32>,
    ) -> Result<Uuid> {
        // Generate on-chain address based on storage type
        let on_chain_address = match storage_type {
            OnChainStorageType::ChatChain
            | OnChainStorageType::CurrencyChain
            | OnChainStorageType::Hybrid => Some(format!("0x{:x}", Uuid::new_v4())),
            OnChainStorageType::Ipfs => None,
        };

        let listing = Listing {
            id: Uuid::new_v4(),
            creator,
            title,
            description,
            good_type,
            pricing,
            created_at: Utc::now(),
            downloads: 0,
            rating: 0.0,
            is_verified: false,
            content_hash,
            storage_type,
            on_chain_address,
            nft_token_id,
            bot_id,
            channel_id,
            membership_duration_days,
            in_escrow: false,
            escrow_id: None,
        };

        let listing_id = listing.id;
        self.listings.push(listing);
        Ok(listing_id)
    }

    /// Purchase a digital good with automatic escrow creation
    pub fn purchase(
        &mut self,
        buyer: UserId,
        listing_id: Uuid,
        amount_paid: u64,
        transaction_hash: String,
    ) -> Result<Uuid> {
        // Find the listing
        let listing = self
            .listings
            .iter_mut()
            .find(|l| l.id == listing_id)
            .ok_or_else(|| Error::validation("Listing not found"))?;

        // Check if already in escrow
        if listing.in_escrow {
            return Err(Error::validation(
                "Item currently in escrow for another transaction",
            ));
        }

        // Verify payment amount matches pricing
        match &listing.pricing {
            PricingModel::OneTime { price } => {
                if amount_paid < *price {
                    return Err(Error::validation("Insufficient payment"));
                }
            }
            PricingModel::Subscription { price_per_month } => {
                if amount_paid < *price_per_month {
                    return Err(Error::validation("Insufficient payment"));
                }
            }
            PricingModel::PayWhatYouWant { minimum } => {
                if amount_paid < *minimum {
                    return Err(Error::validation("Payment below minimum"));
                }
            }
            PricingModel::Free => {}
        }

        let seller = listing.creator.clone();

        // Create escrow for the transaction (30 days lock)
        let escrow_id = self
            .escrow
            .create_two_party_escrow(
                listing_id,
                &buyer,
                &seller,
                amount_paid,
                30 * 24 * 60 * 60, // 30 days in seconds
            )
            .map_err(|e| Error::validation(&format!("Escrow creation failed: {:?}", e)))?;

        // Mark listing as in escrow
        listing.in_escrow = true;
        listing.escrow_id = Some(escrow_id);

        // Create purchase record
        let purchase = Purchase {
            id: Uuid::new_v4(),
            buyer: buyer.clone(),
            listing_id,
            amount_paid,
            purchased_at: Utc::now(),
            transaction_hash,
        };

        let purchase_id = purchase.id;
        listing.downloads += 1;
        self.purchases.push(purchase);

        Ok(purchase_id)
    }

    /// Complete purchase and transfer asset ownership (called after escrow release)
    pub fn complete_purchase_transfer(
        &mut self,
        purchase_id: Uuid,
        _escrow_id: Uuid,
    ) -> Result<()> {
        // Find the purchase and extract needed data
        let (buyer, listing_id) = {
            let purchase = self
                .purchases
                .iter()
                .find(|p| p.id == purchase_id)
                .ok_or_else(|| Error::validation("Purchase not found"))?;
            (purchase.buyer.clone(), purchase.listing_id)
        };

        // Extract listing data before mutable borrows
        let (good_type, bot_id, channel_id, nft_token_id, membership_duration) = {
            let listing = self
                .listings
                .iter()
                .find(|l| l.id == listing_id)
                .ok_or_else(|| Error::validation("Listing not found"))?;

            (
                listing.good_type,
                listing.bot_id,
                listing.channel_id,
                listing.nft_token_id.clone(),
                listing.membership_duration_days,
            )
        };

        // Transfer ownership based on asset type (now we can mutably borrow self)
        match good_type {
            DigitalGoodType::Bot => {
                if let Some(bot_id) = bot_id {
                    self.transfer_bot_ownership(bot_id, buyer)?;
                }
            }
            DigitalGoodType::Channel => {
                if let Some(channel_id) = channel_id {
                    self.transfer_channel_ownership(channel_id, buyer)?;
                }
            }
            DigitalGoodType::Nft => {
                if let Some(ref token_id) = nft_token_id {
                    self.transfer_nft(token_id, buyer)?;
                }
            }
            DigitalGoodType::Membership => {
                if let Some(channel_id) = channel_id {
                    let duration_days = membership_duration.unwrap_or(30);
                    self.grant_membership(channel_id, buyer, duration_days)?;
                }
            }
            // Other types don't require ownership transfer (content delivery only)
            _ => {}
        }

        // Clear escrow status
        let listing = self
            .listings
            .iter_mut()
            .find(|l| l.id == listing_id)
            .ok_or_else(|| Error::validation("Listing not found"))?;

        listing.in_escrow = false;
        listing.escrow_id = None;

        Ok(())
    }

    /// Get a listing by ID
    pub fn get_listing(&self, listing_id: Uuid) -> Option<&Listing> {
        self.listings.iter().find(|l| l.id == listing_id)
    }

    /// Get all listings by creator
    pub fn get_listings_by_creator(&self, creator: &UserId) -> Vec<&Listing> {
        self.listings
            .iter()
            .filter(|l| &l.creator == creator)
            .collect()
    }

    /// Get listings by type
    pub fn get_listings_by_type(&self, good_type: &DigitalGoodType) -> Vec<&Listing> {
        self.listings
            .iter()
            .filter(|l| &l.good_type == good_type)
            .collect()
    }

    /// Get creator statistics
    pub fn get_creator_stats(&self, creator: &UserId) -> CreatorStats {
        let creator_listings: Vec<_> = self.get_listings_by_creator(creator);
        let active_listings = creator_listings.len() as u64;

        let total_downloads: u64 = creator_listings.iter().map(|l| l.downloads).sum();
        let avg_rating = if active_listings > 0 {
            creator_listings.iter().map(|l| l.rating).sum::<f32>() / active_listings as f32
        } else {
            0.0
        };

        let creator_listing_ids: Vec<_> = creator_listings.iter().map(|l| l.id).collect();
        let creator_purchases: Vec<_> = self
            .purchases
            .iter()
            .filter(|p| creator_listing_ids.contains(&p.listing_id))
            .collect();

        let total_sales = creator_purchases.len() as u64;
        let total_earnings: u64 = creator_purchases.iter().map(|p| p.amount_paid).sum();

        CreatorStats {
            creator: creator.clone(),
            total_sales,
            total_earnings,
            active_listings,
            total_downloads,
            average_rating: avg_rating,
        }
    }

    /// Register an NFT
    pub fn register_nft(
        &mut self,
        token_id: String,
        name: String,
        description: String,
        image_hash: String,
        attributes: Vec<NftAttribute>,
        creator: UserId,
        owner: UserId,
    ) -> Result<()> {
        // Check if token ID already exists
        if self.nft_registry.iter().any(|n| n.token_id == token_id) {
            return Err(Error::validation("NFT token ID already exists"));
        }

        let nft = NftMetadata {
            token_id,
            name,
            description,
            image_hash,
            attributes,
            creator,
            owner,
            created_at: Utc::now(),
        };

        self.nft_registry.push(nft);
        Ok(())
    }

    /// Transfer NFT ownership
    pub fn transfer_nft(&mut self, token_id: &str, new_owner: UserId) -> Result<()> {
        let nft = self
            .nft_registry
            .iter_mut()
            .find(|n| n.token_id == token_id)
            .ok_or_else(|| Error::validation("NFT not found"))?;

        nft.owner = new_owner;
        Ok(())
    }

    /// Get NFT by token ID
    pub fn get_nft(&self, token_id: &str) -> Option<&NftMetadata> {
        self.nft_registry.iter().find(|n| n.token_id == token_id)
    }

    /// Get all NFTs owned by a user
    pub fn get_nfts_by_owner(&self, owner: &UserId) -> Vec<&NftMetadata> {
        self.nft_registry
            .iter()
            .filter(|n| &n.owner == owner)
            .collect()
    }

    // ========== Bot Ownership Methods ==========

    /// Register bot for marketplace trading
    pub fn register_bot_ownership(
        &mut self,
        bot_id: Uuid,
        bot_username: String,
        owner: UserId,
    ) -> Result<String> {
        // Check if bot already registered
        if self.bot_ownership.iter().any(|b| b.bot_id == bot_id) {
            return Err(Error::validation("Bot already registered"));
        }

        let on_chain_address = format!("0xbot{:x}", bot_id);

        let ownership = BotOwnership {
            bot_id,
            bot_username,
            current_owner: owner,
            previous_owners: Vec::new(),
            on_chain_address: on_chain_address.clone(),
            transfer_count: 0,
        };

        self.bot_ownership.push(ownership);
        Ok(on_chain_address)
    }

    /// Transfer bot ownership
    pub fn transfer_bot_ownership(&mut self, bot_id: Uuid, new_owner: UserId) -> Result<()> {
        let bot = self
            .bot_ownership
            .iter_mut()
            .find(|b| b.bot_id == bot_id)
            .ok_or_else(|| Error::validation("Bot ownership not found"))?;

        let old_owner = bot.current_owner.clone();
        bot.previous_owners.push((old_owner, Utc::now()));
        bot.current_owner = new_owner;
        bot.transfer_count += 1;

        Ok(())
    }

    /// Get bot ownership info
    pub fn get_bot_ownership(&self, bot_id: Uuid) -> Option<&BotOwnership> {
        self.bot_ownership.iter().find(|b| b.bot_id == bot_id)
    }

    /// Get all bots owned by user
    pub fn get_bots_by_owner(&self, owner: &UserId) -> Vec<&BotOwnership> {
        self.bot_ownership
            .iter()
            .filter(|b| &b.current_owner == owner)
            .collect()
    }

    // ========== Channel Ownership Methods ==========

    /// Register channel for marketplace trading
    pub fn register_channel_ownership(
        &mut self,
        channel_id: Uuid,
        channel_name: String,
        owner: UserId,
        member_count: u64,
    ) -> Result<String> {
        // Check if channel already registered
        if self
            .channel_ownership
            .iter()
            .any(|c| c.channel_id == channel_id)
        {
            return Err(Error::validation("Channel already registered"));
        }

        let on_chain_address = format!("0xchan{:x}", channel_id);

        let ownership = ChannelOwnership {
            channel_id,
            channel_name,
            current_owner: owner,
            previous_owners: Vec::new(),
            on_chain_address: on_chain_address.clone(),
            member_count,
            transfer_count: 0,
        };

        self.channel_ownership.push(ownership);
        Ok(on_chain_address)
    }

    /// Transfer channel ownership
    pub fn transfer_channel_ownership(
        &mut self,
        channel_id: Uuid,
        new_owner: UserId,
    ) -> Result<()> {
        let channel = self
            .channel_ownership
            .iter_mut()
            .find(|c| c.channel_id == channel_id)
            .ok_or_else(|| Error::validation("Channel ownership not found"))?;

        let old_owner = channel.current_owner.clone();
        channel.previous_owners.push((old_owner, Utc::now()));
        channel.current_owner = new_owner;
        channel.transfer_count += 1;

        Ok(())
    }

    /// Get channel ownership info
    pub fn get_channel_ownership(&self, channel_id: Uuid) -> Option<&ChannelOwnership> {
        self.channel_ownership
            .iter()
            .find(|c| c.channel_id == channel_id)
    }

    /// Get all channels owned by user
    pub fn get_channels_by_owner(&self, owner: &UserId) -> Vec<&ChannelOwnership> {
        self.channel_ownership
            .iter()
            .filter(|c| &c.current_owner == owner)
            .collect()
    }

    // ========== Emoji Pack Methods ==========

    /// Register emoji pack
    pub fn register_emoji_pack(
        &mut self,
        name: String,
        description: String,
        emoji_count: u32,
        creator: UserId,
        content_hash: String,
        preview_emojis: Vec<String>,
        is_animated: bool,
    ) -> Result<Uuid> {
        let pack = EmojiPack {
            pack_id: Uuid::new_v4(),
            name,
            description,
            emoji_count,
            creator,
            content_hash,
            preview_emojis,
            is_animated,
        };

        let pack_id = pack.pack_id;
        self.emoji_packs.push(pack);
        Ok(pack_id)
    }

    /// Get emoji pack by ID
    pub fn get_emoji_pack(&self, pack_id: Uuid) -> Option<&EmojiPack> {
        self.emoji_packs.iter().find(|p| p.pack_id == pack_id)
    }

    /// Get all emoji packs by creator
    pub fn get_emoji_packs_by_creator(&self, creator: &UserId) -> Vec<&EmojiPack> {
        self.emoji_packs
            .iter()
            .filter(|p| &p.creator == creator)
            .collect()
    }

    // ========== Image Artwork Methods ==========

    /// Register image artwork
    pub fn register_image(
        &mut self,
        title: String,
        description: String,
        creator: UserId,
        content_hash: String,
        width: u32,
        height: u32,
        format: String,
        license_type: LicenseType,
    ) -> Result<Uuid> {
        let image = ImageArtwork {
            image_id: Uuid::new_v4(),
            title,
            description,
            creator,
            content_hash,
            width,
            height,
            format,
            license_type,
        };

        let image_id = image.image_id;
        self.images.push(image);
        Ok(image_id)
    }

    /// Get image by ID
    pub fn get_image(&self, image_id: Uuid) -> Option<&ImageArtwork> {
        self.images.iter().find(|i| i.image_id == image_id)
    }

    /// Get all images by creator
    pub fn get_images_by_creator(&self, creator: &UserId) -> Vec<&ImageArtwork> {
        self.images
            .iter()
            .filter(|i| &i.creator == creator)
            .collect()
    }

    // ========== Channel Membership Methods ==========

    /// Grant channel membership
    pub fn grant_membership(
        &mut self,
        channel_id: Uuid,
        holder: UserId,
        duration_days: u32,
    ) -> Result<Uuid> {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::days(duration_days as i64);

        let membership = ChannelMembership {
            membership_id: Uuid::new_v4(),
            channel_id,
            holder,
            purchased_at: now,
            expires_at,
            is_transferable: true,
            access_level: MembershipAccessLevel::Basic,
        };

        let membership_id = membership.membership_id;
        self.memberships.push(membership);
        Ok(membership_id)
    }

    /// Transfer membership to another user
    pub fn transfer_membership(&mut self, membership_id: Uuid, new_holder: UserId) -> Result<()> {
        let membership = self
            .memberships
            .iter_mut()
            .find(|m| m.membership_id == membership_id)
            .ok_or_else(|| Error::validation("Membership not found"))?;

        if !membership.is_transferable {
            return Err(Error::validation("Membership is not transferable"));
        }

        if membership.expires_at < Utc::now() {
            return Err(Error::validation("Membership has expired"));
        }

        membership.holder = new_holder;
        Ok(())
    }

    /// Get membership by ID
    pub fn get_membership(&self, membership_id: Uuid) -> Option<&ChannelMembership> {
        self.memberships
            .iter()
            .find(|m| m.membership_id == membership_id)
    }

    /// Get all memberships for a user
    pub fn get_memberships_by_holder(&self, holder: &UserId) -> Vec<&ChannelMembership> {
        self.memberships
            .iter()
            .filter(|m| &m.holder == holder && m.expires_at > Utc::now())
            .collect()
    }

    /// Get all memberships for a channel
    pub fn get_memberships_by_channel(&self, channel_id: Uuid) -> Vec<&ChannelMembership> {
        self.memberships
            .iter()
            .filter(|m| m.channel_id == channel_id && m.expires_at > Utc::now())
            .collect()
    }

    /// Check if user has active membership
    pub fn has_active_membership(&self, channel_id: Uuid, holder: &UserId) -> bool {
        self.memberships
            .iter()
            .any(|m| m.channel_id == channel_id && &m.holder == holder && m.expires_at > Utc::now())
    }

    /// Update listing rating
    pub fn update_rating(&mut self, listing_id: Uuid, new_rating: f32) -> Result<()> {
        let listing = self
            .listings
            .iter_mut()
            .find(|l| l.id == listing_id)
            .ok_or_else(|| Error::validation("Listing not found"))?;

        if !(0.0..=5.0).contains(&new_rating) {
            return Err(Error::validation("Rating must be between 0.0 and 5.0"));
        }

        listing.rating = new_rating;
        Ok(())
    }

    /// Verify a listing (creator badge)
    pub fn verify_listing(&mut self, listing_id: Uuid) -> Result<()> {
        let listing = self
            .listings
            .iter_mut()
            .find(|l| l.id == listing_id)
            .ok_or_else(|| Error::validation("Listing not found"))?;

        listing.is_verified = true;
        Ok(())
    }
}

impl Default for MarketplaceManager {
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
    fn test_create_listing() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();

        let listing_id = marketplace
            .create_listing(
                creator.clone(),
                "Cool Sticker Pack".to_string(),
                "Awesome stickers".to_string(),
                DigitalGoodType::StickerPack,
                PricingModel::OneTime { price: 1000 },
                "QmHash123".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let listing = marketplace.get_listing(listing_id).unwrap();
        assert_eq!(listing.title, "Cool Sticker Pack");
        assert_eq!(listing.creator, creator);
        assert_eq!(listing.downloads, 0);
    }

    #[test]
    fn test_purchase_listing() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let buyer = UserId::new();

        let listing_id = marketplace
            .create_listing(
                creator,
                "Theme".to_string(),
                "Dark theme".to_string(),
                DigitalGoodType::Theme,
                PricingModel::OneTime { price: 500 },
                "QmHash456".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let purchase_id = marketplace
            .purchase(buyer, listing_id, 500, "tx123".to_string())
            .unwrap();

        assert!(purchase_id != Uuid::nil());
        let listing = marketplace.get_listing(listing_id).unwrap();
        assert_eq!(listing.downloads, 1);
    }

    #[test]
    fn test_insufficient_payment() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let buyer = UserId::new();

        let listing_id = marketplace
            .create_listing(
                creator,
                "Bot".to_string(),
                "Automation bot".to_string(),
                DigitalGoodType::Bot,
                PricingModel::OneTime { price: 1000 },
                "QmHash789".to_string(),
                OnChainStorageType::Hybrid,
                None,
                Some(Uuid::new_v4()),
                None,
                None,
            )
            .unwrap();

        let result = marketplace.purchase(buyer, listing_id, 500, "tx456".to_string());
        assert!(result.is_err());
    }

    #[test]
    fn test_get_listings_by_creator() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();

        marketplace
            .create_listing(
                creator.clone(),
                "Item 1".to_string(),
                "Desc 1".to_string(),
                DigitalGoodType::StickerPack,
                PricingModel::Free,
                "hash1".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        marketplace
            .create_listing(
                creator.clone(),
                "Item 2".to_string(),
                "Desc 2".to_string(),
                DigitalGoodType::Theme,
                PricingModel::Free,
                "hash2".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let listings = marketplace.get_listings_by_creator(&creator);
        assert_eq!(listings.len(), 2);
    }

    #[test]
    fn test_get_listings_by_type() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();

        marketplace
            .create_listing(
                creator.clone(),
                "Stickers 1".to_string(),
                "Pack 1".to_string(),
                DigitalGoodType::StickerPack,
                PricingModel::Free,
                "hash1".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        marketplace
            .create_listing(
                creator,
                "Theme 1".to_string(),
                "Dark mode".to_string(),
                DigitalGoodType::Theme,
                PricingModel::Free,
                "hash2".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let sticker_listings = marketplace.get_listings_by_type(&DigitalGoodType::StickerPack);
        assert_eq!(sticker_listings.len(), 1);
    }

    #[test]
    fn test_creator_stats() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let buyer = UserId::new();

        let listing_id = marketplace
            .create_listing(
                creator.clone(),
                "Product".to_string(),
                "Description".to_string(),
                DigitalGoodType::Bot,
                PricingModel::OneTime { price: 2000 },
                "hash".to_string(),
                OnChainStorageType::Hybrid,
                None,
                Some(Uuid::new_v4()),
                None,
                None,
            )
            .unwrap();

        marketplace
            .purchase(buyer, listing_id, 2000, "tx1".to_string())
            .unwrap();

        let stats = marketplace.get_creator_stats(&creator);
        assert_eq!(stats.total_sales, 1);
        assert_eq!(stats.total_earnings, 2000);
        assert_eq!(stats.active_listings, 1);
    }

    #[test]
    fn test_register_nft() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let owner = creator.clone();

        marketplace
            .register_nft(
                "token_001".to_string(),
                "Rare Badge".to_string(),
                "Limited edition".to_string(),
                "QmImage123".to_string(),
                vec![NftAttribute {
                    trait_type: "Rarity".to_string(),
                    value: "Legendary".to_string(),
                }],
                creator,
                owner.clone(),
            )
            .unwrap();

        let nft = marketplace.get_nft("token_001").unwrap();
        assert_eq!(nft.name, "Rare Badge");
        assert_eq!(nft.owner, owner);
    }

    #[test]
    fn test_transfer_nft() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let owner1 = creator.clone();
        let owner2 = UserId::new();

        marketplace
            .register_nft(
                "token_002".to_string(),
                "Badge".to_string(),
                "Collectible".to_string(),
                "QmImg".to_string(),
                vec![],
                creator,
                owner1,
            )
            .unwrap();

        marketplace
            .transfer_nft("token_002", owner2.clone())
            .unwrap();

        let nft = marketplace.get_nft("token_002").unwrap();
        assert_eq!(nft.owner, owner2);
    }

    #[test]
    fn test_get_nfts_by_owner() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();
        let owner = creator.clone();

        marketplace
            .register_nft(
                "token_003".to_string(),
                "NFT 1".to_string(),
                "First".to_string(),
                "hash1".to_string(),
                vec![],
                creator.clone(),
                owner.clone(),
            )
            .unwrap();

        marketplace
            .register_nft(
                "token_004".to_string(),
                "NFT 2".to_string(),
                "Second".to_string(),
                "hash2".to_string(),
                vec![],
                creator,
                owner.clone(),
            )
            .unwrap();

        let nfts = marketplace.get_nfts_by_owner(&owner);
        assert_eq!(nfts.len(), 2);
    }

    #[test]
    fn test_update_rating() {
        let mut marketplace = MarketplaceManager::new();
        let creator = create_test_user();

        let listing_id = marketplace
            .create_listing(
                creator,
                "Product".to_string(),
                "Test".to_string(),
                DigitalGoodType::Theme,
                PricingModel::Free,
                "hash".to_string(),
                OnChainStorageType::Ipfs,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        marketplace.update_rating(listing_id, 4.5).unwrap();

        let listing = marketplace.get_listing(listing_id).unwrap();
        assert_eq!(listing.rating, 4.5);
    }
}
