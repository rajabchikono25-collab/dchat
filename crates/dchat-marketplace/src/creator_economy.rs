//! Creator Economy Features for dchat Marketplace
//!
//! This module implements revenue streams and monetization tools for content creators:
//! - Tipping system (one-time and recurring)
//! - Revenue sharing from content sales
//! - Subscription management
//! - Creator tier benefits
//! - Analytics and earnings dashboard
//! - Payout management
//! - Fan engagement tools

use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Tip transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Tip {
    pub id: Uuid,
    pub from_user: UserId,
    pub to_creator: UserId,
    pub amount: u64,
    pub message: Option<String>,
    pub timestamp: DateTime<Utc>,
    pub transaction_hash: String,
    pub is_recurring: bool,
    pub recurring_interval_days: Option<u32>,
}

/// Subscription plan
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SubscriptionPlan {
    pub id: Uuid,
    pub creator: UserId,
    pub name: String,
    pub description: String,
    pub price_per_month: u64,
    pub benefits: Vec<String>,
    pub tier: SubscriptionTier,
    pub is_active: bool,
    pub subscriber_count: u64,
}

/// Subscription tier levels
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum SubscriptionTier {
    Basic,
    Silver,
    Gold,
    Platinum,
    Diamond,
}

/// Active subscription
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Subscription {
    pub id: Uuid,
    pub subscriber: UserId,
    pub plan_id: Uuid,
    pub creator: UserId,
    pub started_at: DateTime<Utc>,
    pub current_period_end: DateTime<Utc>,
    pub is_active: bool,
    pub auto_renew: bool,
    pub total_paid: u64,
}

/// Revenue share configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RevenueShare {
    /// Creator's percentage (e.g., 90%)
    pub creator_percentage: u8,
    /// Platform's percentage (e.g., 10%)
    pub platform_percentage: u8,
}

impl Default for RevenueShare {
    fn default() -> Self {
        Self {
            creator_percentage: 90,
            platform_percentage: 10,
        }
    }
}

/// Sale revenue breakdown
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SaleRevenue {
    pub sale_id: Uuid,
    pub total_amount: u64,
    pub creator_earnings: u64,
    pub platform_fee: u64,
    pub creator: UserId,
    pub timestamp: DateTime<Utc>,
}

/// Creator earnings summary
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorEarnings {
    pub creator: UserId,
    pub total_tips: u64,
    pub total_subscriptions: u64,
    pub total_sales: u64,
    pub total_earnings: u64,
    pub pending_payout: u64,
    pub paid_out: u64,
    pub active_subscribers: u64,
    pub total_supporters: u64,
}

/// Payout request
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PayoutRequest {
    pub id: Uuid,
    pub creator: UserId,
    pub amount: u64,
    pub status: PayoutStatus,
    pub requested_at: DateTime<Utc>,
    pub processed_at: Option<DateTime<Utc>>,
    pub transaction_hash: Option<String>,
    pub notes: Option<String>,
}

/// Payout status
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum PayoutStatus {
    Pending,
    Processing,
    Completed,
    Failed,
    Cancelled,
}

/// Creator tier based on performance
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum CreatorTier {
    Novice,       // 0-10 subscribers
    Rising,       // 11-50 subscribers
    Established,  // 51-200 subscribers
    Professional, // 201-1000 subscribers
    Celebrity,    // 1000+ subscribers
}

/// Creator profile with tier benefits
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreatorProfile {
    pub creator: UserId,
    pub tier: CreatorTier,
    pub total_subscribers: u64,
    pub total_earnings: u64,
    pub join_date: DateTime<Utc>,
    pub verified: bool,
    pub featured: bool,
    pub badges: Vec<String>,
}

/// Fan engagement metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EngagementMetrics {
    pub creator: UserId,
    pub total_tips_received: u64,
    pub total_tip_amount: u64,
    pub unique_tippers: u64,
    pub recurring_supporters: u64,
    pub average_tip_amount: u64,
    pub tips_this_month: u64,
    pub new_subscribers_this_month: u64,
}

/// Creator economy manager
pub struct CreatorEconomyManager {
    tips: Vec<Tip>,
    subscription_plans: Vec<SubscriptionPlan>,
    subscriptions: Vec<Subscription>,
    sale_revenues: Vec<SaleRevenue>,
    payout_requests: Vec<PayoutRequest>,
    creator_profiles: HashMap<UserId, CreatorProfile>,
    revenue_share: RevenueShare,
}

impl CreatorEconomyManager {
    pub fn new() -> Self {
        Self {
            tips: Vec::new(),
            subscription_plans: Vec::new(),
            subscriptions: Vec::new(),
            sale_revenues: Vec::new(),
            payout_requests: Vec::new(),
            creator_profiles: HashMap::new(),
            revenue_share: RevenueShare::default(),
        }
    }

    /// Send a tip to a creator
    pub fn send_tip(
        &mut self,
        from_user: UserId,
        to_creator: UserId,
        amount: u64,
        message: Option<String>,
        is_recurring: bool,
        recurring_interval_days: Option<u32>,
    ) -> Result<Uuid> {
        if amount == 0 {
            return Err(Error::validation("Tip amount must be greater than 0"));
        }

        if is_recurring && recurring_interval_days.is_none() {
            return Err(Error::validation(
                "Recurring tips must specify interval",
            ));
        }

        // Generate transaction hash (in production, use actual blockchain tx)
        let transaction_hash = format!("tip_tx_{}", Uuid::new_v4());

        let tip = Tip {
            id: Uuid::new_v4(),
            from_user,
            to_creator,
            amount,
            message,
            timestamp: Utc::now(),
            transaction_hash,
            is_recurring,
            recurring_interval_days,
        };

        let tip_id = tip.id;
        self.tips.push(tip);

        Ok(tip_id)
    }

    /// Create a subscription plan
    pub fn create_subscription_plan(
        &mut self,
        creator: UserId,
        name: String,
        description: String,
        price_per_month: u64,
        benefits: Vec<String>,
        tier: SubscriptionTier,
    ) -> Result<Uuid> {
        if price_per_month == 0 {
            return Err(Error::validation("Subscription price must be > 0"));
        }

        let plan = SubscriptionPlan {
            id: Uuid::new_v4(),
            creator,
            name,
            description,
            price_per_month,
            benefits,
            tier,
            is_active: true,
            subscriber_count: 0,
        };

        let plan_id = plan.id;
        self.subscription_plans.push(plan);

        Ok(plan_id)
    }

    /// Subscribe to a creator's plan
    pub fn subscribe(
        &mut self,
        subscriber: UserId,
        plan_id: Uuid,
        auto_renew: bool,
    ) -> Result<Uuid> {
        let plan = self
            .subscription_plans
            .iter_mut()
            .find(|p| p.id == plan_id)
            .ok_or_else(|| Error::validation("Subscription plan not found"))?;

        if !plan.is_active {
            return Err(Error::validation("Subscription plan is not active"));
        }

        let subscription = Subscription {
            id: Uuid::new_v4(),
            subscriber,
            plan_id,
            creator: plan.creator.clone(),
            started_at: Utc::now(),
            current_period_end: Utc::now() + chrono::Duration::days(30),
            is_active: true,
            auto_renew,
            total_paid: plan.price_per_month,
        };

        let subscription_id = subscription.id;
        plan.subscriber_count += 1;
        self.subscriptions.push(subscription);

        Ok(subscription_id)
    }

    /// Cancel a subscription
    pub fn cancel_subscription(&mut self, subscription_id: Uuid) -> Result<()> {
        let subscription = self
            .subscriptions
            .iter_mut()
            .find(|s| s.id == subscription_id)
            .ok_or_else(|| Error::validation("Subscription not found"))?;

        subscription.is_active = false;
        subscription.auto_renew = false;

        // Decrement subscriber count
        if let Some(plan) = self
            .subscription_plans
            .iter_mut()
            .find(|p| p.id == subscription.plan_id)
        {
            plan.subscriber_count = plan.subscriber_count.saturating_sub(1);
        }

        Ok(())
    }

    /// Process a sale and calculate revenue share
    pub fn process_sale(
        &mut self,
        sale_id: Uuid,
        creator: UserId,
        total_amount: u64,
    ) -> Result<SaleRevenue> {
        let creator_earnings = (total_amount * self.revenue_share.creator_percentage as u64) / 100;
        let platform_fee = total_amount - creator_earnings;

        let revenue = SaleRevenue {
            sale_id,
            total_amount,
            creator_earnings,
            platform_fee,
            creator,
            timestamp: Utc::now(),
        };

        self.sale_revenues.push(revenue.clone());
        Ok(revenue)
    }

    /// Get creator earnings summary
    pub fn get_creator_earnings(&self, creator: &UserId) -> CreatorEarnings {
        let total_tips: u64 = self
            .tips
            .iter()
            .filter(|t| &t.to_creator == creator)
            .map(|t| t.amount)
            .sum();

        let active_subscriptions: Vec<_> = self
            .subscriptions
            .iter()
            .filter(|s| &s.creator == creator && s.is_active)
            .collect();

        let total_subscriptions: u64 = active_subscriptions.iter().map(|s| s.total_paid).sum();

        let total_sales: u64 = self
            .sale_revenues
            .iter()
            .filter(|r| &r.creator == creator)
            .map(|r| r.creator_earnings)
            .sum();

        let total_earnings = total_tips + total_subscriptions + total_sales;

        let paid_out: u64 = self
            .payout_requests
            .iter()
            .filter(|p| &p.creator == creator && p.status == PayoutStatus::Completed)
            .map(|p| p.amount)
            .sum();

        let pending_payout = total_earnings.saturating_sub(paid_out);

        let unique_tippers = self
            .tips
            .iter()
            .filter(|t| &t.to_creator == creator)
            .map(|t| &t.from_user)
            .collect::<std::collections::HashSet<_>>()
            .len() as u64;

        let active_subscribers = active_subscriptions.len() as u64;
        let total_supporters = unique_tippers + active_subscribers;

        CreatorEarnings {
            creator: creator.clone(),
            total_tips,
            total_subscriptions,
            total_sales,
            total_earnings,
            pending_payout,
            paid_out,
            active_subscribers,
            total_supporters,
        }
    }

    /// Request a payout
    pub fn request_payout(&mut self, creator: UserId, amount: u64) -> Result<Uuid> {
        let earnings = self.get_creator_earnings(&creator);

        if amount > earnings.pending_payout {
            return Err(Error::validation("Insufficient funds for payout"));
        }

        let payout = PayoutRequest {
            id: Uuid::new_v4(),
            creator,
            amount,
            status: PayoutStatus::Pending,
            requested_at: Utc::now(),
            processed_at: None,
            transaction_hash: None,
            notes: None,
        };

        let payout_id = payout.id;
        self.payout_requests.push(payout);

        Ok(payout_id)
    }

    /// Process a payout request
    pub fn process_payout(&mut self, payout_id: Uuid, transaction_hash: String) -> Result<()> {
        let payout = self
            .payout_requests
            .iter_mut()
            .find(|p| p.id == payout_id)
            .ok_or_else(|| Error::validation("Payout request not found"))?;

        if payout.status != PayoutStatus::Pending {
            return Err(Error::validation("Payout already processed"));
        }

        payout.status = PayoutStatus::Completed;
        payout.processed_at = Some(Utc::now());
        payout.transaction_hash = Some(transaction_hash);

        Ok(())
    }

    /// Get or create creator profile
    pub fn get_or_create_profile(&mut self, creator: UserId) -> &mut CreatorProfile {
        self.creator_profiles
            .entry(creator.clone())
            .or_insert_with(|| CreatorProfile {
                creator,
                tier: CreatorTier::Novice,
                total_subscribers: 0,
                total_earnings: 0,
                join_date: Utc::now(),
                verified: false,
                featured: false,
                badges: Vec::new(),
            })
    }

    /// Update creator tier based on performance
    pub fn update_creator_tier(&mut self, creator: &UserId) -> Result<CreatorTier> {
        let active_subs = self
            .subscriptions
            .iter()
            .filter(|s| &s.creator == creator && s.is_active)
            .count() as u64;

        let tier = match active_subs {
            0..=10 => CreatorTier::Novice,
            11..=50 => CreatorTier::Rising,
            51..=200 => CreatorTier::Established,
            201..=1000 => CreatorTier::Professional,
            _ => CreatorTier::Celebrity,
        };

        if let Some(profile) = self.creator_profiles.get_mut(creator) {
            profile.tier = tier;
            profile.total_subscribers = active_subs;
        }

        Ok(tier)
    }

    /// Get engagement metrics
    pub fn get_engagement_metrics(&self, creator: &UserId) -> EngagementMetrics {
        let tips_for_creator: Vec<_> = self
            .tips
            .iter()
            .filter(|t| &t.to_creator == creator)
            .collect();

        let total_tips_received = tips_for_creator.len() as u64;
        let total_tip_amount: u64 = tips_for_creator.iter().map(|t| t.amount).sum();
        let unique_tippers = tips_for_creator
            .iter()
            .map(|t| &t.from_user)
            .collect::<std::collections::HashSet<_>>()
            .len() as u64;

        let recurring_supporters = tips_for_creator
            .iter()
            .filter(|t| t.is_recurring)
            .map(|t| &t.from_user)
            .collect::<std::collections::HashSet<_>>()
            .len() as u64;

        let average_tip_amount = if total_tips_received > 0 {
            total_tip_amount / total_tips_received
        } else {
            0
        };

        let one_month_ago = Utc::now() - chrono::Duration::days(30);
        let tips_this_month = tips_for_creator
            .iter()
            .filter(|t| t.timestamp > one_month_ago)
            .count() as u64;

        let new_subscribers_this_month = self
            .subscriptions
            .iter()
            .filter(|s| &s.creator == creator && s.started_at > one_month_ago)
            .count() as u64;

        EngagementMetrics {
            creator: creator.clone(),
            total_tips_received,
            total_tip_amount,
            unique_tippers,
            recurring_supporters,
            average_tip_amount,
            tips_this_month,
            new_subscribers_this_month,
        }
    }

    /// Get all subscriptions for a user
    pub fn get_user_subscriptions(&self, user: &UserId) -> Vec<&Subscription> {
        self.subscriptions
            .iter()
            .filter(|s| &s.subscriber == user && s.is_active)
            .collect()
    }

    /// Get all subscription plans for a creator
    pub fn get_creator_plans(&self, creator: &UserId) -> Vec<&SubscriptionPlan> {
        self.subscription_plans
            .iter()
            .filter(|p| &p.creator == creator && p.is_active)
            .collect()
    }

    /// Get tips received by a creator
    pub fn get_creator_tips(&self, creator: &UserId) -> Vec<&Tip> {
        self.tips
            .iter()
            .filter(|t| &t.to_creator == creator)
            .collect()
    }

    /// Get pending payouts
    pub fn get_pending_payouts(&self) -> Vec<&PayoutRequest> {
        self.payout_requests
            .iter()
            .filter(|p| p.status == PayoutStatus::Pending)
            .collect()
    }
}

impl Default for CreatorEconomyManager {
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
    fn test_send_tip() {
        let mut manager = CreatorEconomyManager::new();
        let tipper = create_test_user();
        let creator = create_test_user();

        let tip_id = manager
            .send_tip(tipper, creator.clone(), 1000, Some("Great content!".to_string()), false, None)
            .unwrap();

        assert_ne!(tip_id, Uuid::nil());

        let earnings = manager.get_creator_earnings(&creator);
        assert_eq!(earnings.total_tips, 1000);
    }

    #[test]
    fn test_recurring_tip() {
        let mut manager = CreatorEconomyManager::new();
        let tipper = create_test_user();
        let creator = create_test_user();

        let result = manager.send_tip(tipper, creator, 500, None, true, Some(30));
        assert!(result.is_ok());
    }

    #[test]
    fn test_create_subscription_plan() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();

        let plan_id = manager
            .create_subscription_plan(
                creator,
                "Gold Plan".to_string(),
                "Premium access".to_string(),
                1000,
                vec!["Exclusive content".to_string()],
                SubscriptionTier::Gold,
            )
            .unwrap();

        assert_ne!(plan_id, Uuid::nil());
        assert_eq!(manager.subscription_plans.len(), 1);
    }

    #[test]
    fn test_subscribe() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();
        let subscriber = create_test_user();

        let plan_id = manager
            .create_subscription_plan(
                creator,
                "Basic".to_string(),
                "Basic access".to_string(),
                500,
                vec![],
                SubscriptionTier::Basic,
            )
            .unwrap();

        let sub_id = manager.subscribe(subscriber, plan_id, true).unwrap();
        assert_ne!(sub_id, Uuid::nil());
    }

    #[test]
    fn test_cancel_subscription() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();
        let subscriber = create_test_user();

        let plan_id = manager
            .create_subscription_plan(
                creator,
                "Silver".to_string(),
                "Silver access".to_string(),
                750,
                vec![],
                SubscriptionTier::Silver,
            )
            .unwrap();

        let sub_id = manager.subscribe(subscriber, plan_id, true).unwrap();
        manager.cancel_subscription(sub_id).unwrap();

        let sub = manager.subscriptions.iter().find(|s| s.id == sub_id).unwrap();
        assert!(!sub.is_active);
    }

    #[test]
    fn test_revenue_share() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();

        let revenue = manager.process_sale(Uuid::new_v4(), creator.clone(), 1000).unwrap();

        assert_eq!(revenue.creator_earnings, 900); // 90%
        assert_eq!(revenue.platform_fee, 100); // 10%
    }

    #[test]
    fn test_creator_earnings() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();
        let supporter = create_test_user();

        // Send tip
        manager
            .send_tip(supporter.clone(), creator.clone(), 500, None, false, None)
            .unwrap();

        // Make sale
        manager.process_sale(Uuid::new_v4(), creator.clone(), 1000).unwrap();

        let earnings = manager.get_creator_earnings(&creator);
        assert_eq!(earnings.total_tips, 500);
        assert_eq!(earnings.total_sales, 900); // 90% of 1000
        assert_eq!(earnings.total_earnings, 1400);
    }

    #[test]
    fn test_payout_request() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();
        let supporter = create_test_user();

        // Generate some earnings
        manager
            .send_tip(supporter, creator.clone(), 5000, None, false, None)
            .unwrap();

        // Request payout
        let payout_id = manager.request_payout(creator, 3000).unwrap();
        assert_ne!(payout_id, Uuid::nil());

        // Process payout
        manager.process_payout(payout_id, "tx_hash_123".to_string()).unwrap();

        let payout = manager
            .payout_requests
            .iter()
            .find(|p| p.id == payout_id)
            .unwrap();
        assert_eq!(payout.status, PayoutStatus::Completed);
    }

    #[test]
    fn test_creator_tier_update() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();

        // Create profile
        manager.get_or_create_profile(creator.clone());

        // Initially novice
        let tier = manager.update_creator_tier(&creator).unwrap();
        assert_eq!(tier, CreatorTier::Novice);

        // Add subscribers to upgrade tier
        let plan_id = manager
            .create_subscription_plan(
                creator.clone(),
                "Plan".to_string(),
                "Description".to_string(),
                100,
                vec![],
                SubscriptionTier::Basic,
            )
            .unwrap();

        // Add 15 subscribers (Rising tier)
        for _ in 0..15 {
            let subscriber = create_test_user();
            manager.subscribe(subscriber, plan_id, true).unwrap();
        }

        let tier = manager.update_creator_tier(&creator).unwrap();
        assert_eq!(tier, CreatorTier::Rising);
    }

    #[test]
    fn test_engagement_metrics() {
        let mut manager = CreatorEconomyManager::new();
        let creator = create_test_user();

        // Send multiple tips
        for _ in 0..5 {
            let tipper = create_test_user();
            manager
                .send_tip(tipper, creator.clone(), 200, None, false, None)
                .unwrap();
        }

        let metrics = manager.get_engagement_metrics(&creator);
        assert_eq!(metrics.total_tips_received, 5);
        assert_eq!(metrics.total_tip_amount, 1000);
        assert_eq!(metrics.unique_tippers, 5);
        assert_eq!(metrics.average_tip_amount, 200);
    }
}
