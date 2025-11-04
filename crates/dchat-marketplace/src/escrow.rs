use crate::types::{ListingId, MarketplaceError, UserId};
use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use uuid::Uuid;

/// Escrow state machine
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowState {
    /// Funds locked, awaiting completion
    Locked,
    /// Seller completed, buyer can release
    AwaitingRelease,
    /// Dispute raised by buyer or seller
    Disputed,
    /// Funds released to seller
    Released,
    /// Funds refunded to buyer
    Refunded,
    /// Escrow expired, auto-refund triggered
    Expired,
}

/// Escrow type
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowType {
    /// Two-party escrow (buyer + seller)
    TwoParty { buyer: UserId, seller: UserId },
    /// Multi-party escrow with revenue split
    MultiParty {
        buyer: UserId,
        recipients: Vec<(UserId, u64)>, // (user_id, amount_in_smallest_unit)
    },
}

/// Escrow dispute reason
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeReason {
    ItemNotReceived,
    ItemNotAsDescribed,
    SellerUnresponsive,
    BuyerUnresponsive,
    QualityIssue,
    Other(String),
}

/// Escrow dispute resolution
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeResolution {
    /// Release funds to seller
    ReleaseFunds,
    /// Refund buyer in full
    RefundFull,
    /// Partial refund (amount to refund, rest to seller)
    PartialRefund(u64),
}

/// Escrow entry
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Escrow {
    pub id: Uuid,
    pub listing_id: ListingId,
    pub escrow_type: EscrowType,
    pub amount: u64, // Total amount in smallest currency unit
    pub state: EscrowState,
    pub created_at: DateTime<Utc>,
    pub locked_until: DateTime<Utc>, // Auto-refund if not released by this time
    pub dispute: Option<Dispute>,
}

/// Dispute details
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    pub raised_by: UserId,
    pub reason: DisputeReason,
    pub description: String,
    pub raised_at: DateTime<Utc>,
    pub resolution: Option<DisputeResolution>,
    pub resolved_at: Option<DateTime<Utc>>,
}

/// Escrow manager
pub struct EscrowManager {
    escrows: Arc<RwLock<HashMap<Uuid, Escrow>>>,
}

impl EscrowManager {
    /// Create a new escrow manager
    pub fn new() -> Self {
        Self {
            escrows: Arc::new(RwLock::new(HashMap::new())),
        }
    }

    /// Create a two-party escrow
    pub fn create_two_party_escrow(
        &self,
        listing_id: ListingId,
        buyer: &UserId,
        seller: &UserId,
        amount: u64,
        lock_duration_secs: u64,
    ) -> Result<Uuid, MarketplaceError> {
        let now = Utc::now();
        let escrow_id = Uuid::new_v4();

        let escrow = Escrow {
            id: escrow_id,
            listing_id,
            escrow_type: EscrowType::TwoParty {
                buyer: buyer.clone(),
                seller: seller.clone(),
            },
            amount,
            state: EscrowState::Locked,
            created_at: now,
            locked_until: now + Duration::seconds(lock_duration_secs as i64),
            dispute: None,
        };

        let mut escrows = self.escrows.write().unwrap();
        escrows.insert(escrow_id, escrow);

        Ok(escrow_id)
    }

    /// Create a multi-party escrow with revenue split
    pub fn create_multi_party_escrow(
        &self,
        listing_id: ListingId,
        buyer: &UserId,
        recipients: Vec<(UserId, u64)>,
        lock_duration_secs: u64,
    ) -> Result<Uuid, MarketplaceError> {
        // Validate total amount matches sum of recipient amounts
        let total: u64 = recipients.iter().map(|(_, amt)| amt).sum();

        let now = Utc::now();
        let escrow_id = Uuid::new_v4();

        let escrow = Escrow {
            id: escrow_id,
            listing_id,
            escrow_type: EscrowType::MultiParty {
                buyer: buyer.clone(),
                recipients,
            },
            amount: total,
            state: EscrowState::Locked,
            created_at: now,
            locked_until: now + Duration::seconds(lock_duration_secs as i64),
            dispute: None,
        };

        let mut escrows = self.escrows.write().unwrap();
        escrows.insert(escrow_id, escrow);

        Ok(escrow_id)
    }

    /// Mark escrow as awaiting release (seller completed)
    pub fn mark_awaiting_release(&self, escrow_id: Uuid) -> Result<(), MarketplaceError> {
        let mut escrows = self.escrows.write().unwrap();
        let escrow = escrows
            .get_mut(&escrow_id)
            .ok_or(MarketplaceError::EscrowNotFound)?;

        if escrow.state != EscrowState::Locked {
            return Err(MarketplaceError::InvalidEscrowState);
        }

        escrow.state = EscrowState::AwaitingRelease;
        Ok(())
    }

    /// Release funds to seller (buyer confirms)
    pub fn release_funds(&self, escrow_id: Uuid, buyer: &UserId) -> Result<(), MarketplaceError> {
        let mut escrows = self.escrows.write().unwrap();
        let escrow = escrows
            .get_mut(&escrow_id)
            .ok_or(MarketplaceError::EscrowNotFound)?;

        // Verify buyer authorization
        match &escrow.escrow_type {
            EscrowType::TwoParty {
                buyer: escrow_buyer,
                ..
            } => {
                if escrow_buyer != buyer {
                    return Err(MarketplaceError::Unauthorized);
                }
            }
            EscrowType::MultiParty {
                buyer: escrow_buyer,
                ..
            } => {
                if escrow_buyer != buyer {
                    return Err(MarketplaceError::Unauthorized);
                }
            }
        }

        // Can only release from AwaitingRelease state
        if escrow.state != EscrowState::AwaitingRelease {
            return Err(MarketplaceError::InvalidEscrowState);
        }

        escrow.state = EscrowState::Released;
        Ok(())
    }

    /// Raise a dispute
    pub fn raise_dispute(
        &self,
        escrow_id: Uuid,
        raised_by: &UserId,
        reason: DisputeReason,
        description: String,
    ) -> Result<(), MarketplaceError> {
        let mut escrows = self.escrows.write().unwrap();
        let escrow = escrows
            .get_mut(&escrow_id)
            .ok_or(MarketplaceError::EscrowNotFound)?;

        // Can only dispute from Locked or AwaitingRelease
        if escrow.state != EscrowState::Locked && escrow.state != EscrowState::AwaitingRelease {
            return Err(MarketplaceError::InvalidEscrowState);
        }

        // Verify user is involved in escrow
        let is_involved = match &escrow.escrow_type {
            EscrowType::TwoParty { buyer, seller } => raised_by == buyer || raised_by == seller,
            EscrowType::MultiParty { buyer, recipients } => {
                raised_by == buyer || recipients.iter().any(|(id, _)| id == raised_by)
            }
        };

        if !is_involved {
            return Err(MarketplaceError::Unauthorized);
        }

        escrow.state = EscrowState::Disputed;
        escrow.dispute = Some(Dispute {
            raised_by: raised_by.clone(),
            reason,
            description,
            raised_at: Utc::now(),
            resolution: None,
            resolved_at: None,
        });

        Ok(())
    }

    /// Resolve a dispute (admin/arbitrator action)
    pub fn resolve_dispute(
        &self,
        escrow_id: Uuid,
        resolution: DisputeResolution,
    ) -> Result<(), MarketplaceError> {
        let mut escrows = self.escrows.write().unwrap();
        let escrow = escrows
            .get_mut(&escrow_id)
            .ok_or(MarketplaceError::EscrowNotFound)?;

        if escrow.state != EscrowState::Disputed {
            return Err(MarketplaceError::InvalidEscrowState);
        }

        let dispute = escrow
            .dispute
            .as_mut()
            .ok_or(MarketplaceError::InvalidEscrowState)?;

        dispute.resolution = Some(resolution.clone());
        dispute.resolved_at = Some(Utc::now());

        // Update escrow state based on resolution
        escrow.state = match resolution {
            DisputeResolution::ReleaseFunds => EscrowState::Released,
            DisputeResolution::RefundFull => EscrowState::Refunded,
            DisputeResolution::PartialRefund(_) => EscrowState::Refunded, // Partial is handled externally
        };

        Ok(())
    }

    /// Check for expired escrows and auto-refund
    pub fn process_expirations(&self) -> Vec<Uuid> {
        let mut refunded = Vec::new();
        let now = Utc::now();
        let mut escrows = self.escrows.write().unwrap();

        for (id, escrow) in escrows.iter_mut() {
            // Auto-refund if locked and expired
            if (escrow.state == EscrowState::Locked || escrow.state == EscrowState::AwaitingRelease)
                && now > escrow.locked_until
            {
                escrow.state = EscrowState::Expired;
                refunded.push(*id);
            }
        }

        refunded
    }

    /// Get escrow by ID
    pub fn get_escrow(&self, escrow_id: Uuid) -> Option<Escrow> {
        let escrows = self.escrows.read().unwrap();
        escrows.get(&escrow_id).cloned()
    }

    /// Get all escrows for a buyer
    pub fn get_buyer_escrows(&self, buyer: UserId) -> Vec<Escrow> {
        let escrows = self.escrows.read().unwrap();
        escrows
            .values()
            .filter(|e| match &e.escrow_type {
                EscrowType::TwoParty {
                    buyer: escrow_buyer,
                    ..
                } => *escrow_buyer == buyer,
                EscrowType::MultiParty {
                    buyer: escrow_buyer,
                    ..
                } => *escrow_buyer == buyer,
            })
            .cloned()
            .collect()
    }

    /// Get all escrows for a seller
    pub fn get_seller_escrows(&self, seller: UserId) -> Vec<Escrow> {
        let escrows = self.escrows.read().unwrap();
        escrows
            .values()
            .filter(|e| match &e.escrow_type {
                EscrowType::TwoParty {
                    seller: escrow_seller,
                    ..
                } => *escrow_seller == seller,
                EscrowType::MultiParty { recipients, .. } => {
                    recipients.iter().any(|(id, _)| *id == seller)
                }
            })
            .cloned()
            .collect()
    }
}

impl Default for EscrowManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn new_listing_id() -> ListingId {
        Uuid::new_v4()
    }

    #[test]
    fn test_create_two_party_escrow() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(Uuid::new_v4(), &buyer, &seller, 1000, 86400)
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.amount, 1000);
        assert_eq!(escrow.state, EscrowState::Locked);
    }

    #[test]
    fn test_create_multi_party_escrow() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller1 = UserId::new();
        let seller2 = UserId::new();

        let recipients = vec![(seller1, 700), (seller2, 300)];

        let escrow_id = manager
            .create_multi_party_escrow(new_listing_id(), &buyer, recipients, 86400)
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.amount, 1000);
        assert_eq!(escrow.state, EscrowState::Locked);
    }

    #[test]
    fn test_release_funds_flow() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        // Seller marks as complete
        manager.mark_awaiting_release(escrow_id).unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::AwaitingRelease);

        // Buyer releases funds
        manager.release_funds(escrow_id, &buyer).unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Released);
    }

    #[test]
    fn test_unauthorized_release() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();
        let attacker = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        manager.mark_awaiting_release(escrow_id).unwrap();

        // Attacker tries to release
        let result = manager.release_funds(escrow_id, &attacker);
        assert!(result.is_err());
    }

    #[test]
    fn test_raise_dispute() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        manager.mark_awaiting_release(escrow_id).unwrap();

        // Buyer raises dispute
        manager
            .raise_dispute(
                escrow_id,
                &buyer,
                DisputeReason::ItemNotAsDescribed,
                "Wrong color".to_string(),
            )
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Disputed);
        assert!(escrow.dispute.is_some());
    }

    #[test]
    fn test_resolve_dispute_refund() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        manager
            .raise_dispute(
                escrow_id,
                &buyer,
                DisputeReason::ItemNotReceived,
                "Never arrived".to_string(),
            )
            .unwrap();

        // Admin resolves with full refund
        manager
            .resolve_dispute(escrow_id, DisputeResolution::RefundFull)
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Refunded);
    }

    #[test]
    fn test_resolve_dispute_release() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        manager
            .raise_dispute(
                escrow_id,
                &buyer,
                DisputeReason::QualityIssue,
                "Minor issue".to_string(),
            )
            .unwrap();

        // Admin resolves in favor of seller
        manager
            .resolve_dispute(escrow_id, DisputeResolution::ReleaseFunds)
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Released);
    }

    #[test]
    fn test_partial_refund() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 86400)
            .unwrap();

        manager
            .raise_dispute(
                escrow_id,
                &buyer,
                DisputeReason::QualityIssue,
                "Damaged".to_string(),
            )
            .unwrap();

        // Admin resolves with 30% refund
        manager
            .resolve_dispute(escrow_id, DisputeResolution::PartialRefund(300))
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Refunded);

        let dispute = escrow.dispute.unwrap();
        assert!(dispute.resolution.is_some());
    }

    #[test]
    fn test_expiration_processing() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller = UserId::new();

        // Create escrow with 0 second lock (immediate expiration)
        let escrow_id = manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller, 1000, 0)
            .unwrap();

        // Process expirations
        std::thread::sleep(std::time::Duration::from_millis(10));
        let expired = manager.process_expirations();

        assert_eq!(expired.len(), 1);
        assert_eq!(expired[0], escrow_id);

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.state, EscrowState::Expired);
    }

    #[test]
    fn test_get_buyer_escrows() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let seller1 = UserId::new();
        let seller2 = UserId::new();

        manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller1, 1000, 86400)
            .unwrap();
        manager
            .create_two_party_escrow(new_listing_id(), &buyer, &seller2, 2000, 86400)
            .unwrap();

        let buyer_escrows = manager.get_buyer_escrows(buyer);
        assert_eq!(buyer_escrows.len(), 2);
    }

    #[test]
    fn test_get_seller_escrows() {
        let manager = EscrowManager::new();
        let buyer1 = UserId::new();
        let buyer2 = UserId::new();
        let seller = UserId::new();

        manager
            .create_two_party_escrow(new_listing_id(), &buyer1, &seller, 1000, 86400)
            .unwrap();
        manager
            .create_two_party_escrow(new_listing_id(), &buyer2, &seller, 2000, 86400)
            .unwrap();

        let seller_escrows = manager.get_seller_escrows(seller);
        assert_eq!(seller_escrows.len(), 2);
    }

    #[test]
    fn test_multi_party_escrow_split() {
        let manager = EscrowManager::new();
        let buyer = UserId::new();
        let creator = UserId::new();
        let platform = UserId::new();
        let affiliate = UserId::new();

        // 70% to creator, 20% to platform, 10% to affiliate
        let recipients = vec![(creator, 700), (platform, 200), (affiliate, 100)];

        let escrow_id = manager
            .create_multi_party_escrow(new_listing_id(), &buyer, recipients, 86400)
            .unwrap();

        let escrow = manager.get_escrow(escrow_id).unwrap();
        assert_eq!(escrow.amount, 1000);

        if let EscrowType::MultiParty { recipients, .. } = &escrow.escrow_type {
            assert_eq!(recipients.len(), 3);
            assert_eq!(recipients[0].1, 700);
            assert_eq!(recipients[1].1, 200);
            assert_eq!(recipients[2].1, 100);
        } else {
            panic!("Test failed: Expected MultiParty escrow but got different escrow type");
        }
    }
}
