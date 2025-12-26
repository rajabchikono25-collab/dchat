//! SQLite-backed persistence for marketplace state.
//!
//! This module is used by CLI/service entrypoints so marketplace commands are not
//! ephemeral per process invocation.
//!
//! Security note:
//! - Attestation replay protection is enforced via a unique nullifier per (escrow_id, kind).
//! - Entitlements are only minted from verified `EscrowLocked` attestations.

use crate::attestations::{
    AttestationValidatorSet, MarketplaceAttestation, MarketplaceAttestationKind,
};
use crate::{DigitalGoodType, Listing, OnChainStorageType, PricingModel, Purchase};
use chrono::{DateTime, Utc};
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use sqlx::sqlite::{SqliteConnectOptions, SqlitePool, SqlitePoolOptions};
use sqlx::Row;
use std::path::{Path, PathBuf};
use uuid::Uuid;

/// File-backed marketplace store.
#[derive(Clone)]
pub struct MarketplaceStore {
    pool: SqlitePool,
}

/// Minimal persisted record proving a buyer has a right to the listing contents.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct Entitlement {
    pub escrow_id: Uuid,
    pub listing_id: Uuid,
    pub buyer: UserId,
    pub seller: UserId,
    pub amount: u64,
    pub expires_at: DateTime<Utc>,
    pub locked_at: DateTime<Utc>,
    pub lock_tx_hash: String,
    pub lock_block_hash: String,
    pub lock_block_number: u64,
}

/// A persisted marker preventing re-use of the same escrow event.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NullifierInsertOutcome {
    Inserted,
    AlreadyExists,
}

impl MarketplaceStore {
    pub async fn open(path: impl AsRef<Path>) -> Result<Self> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| Error::storage(format!("Failed to create db dir: {e}")))?;
            }
        }

        // Use filename-based options to avoid Windows path/URI edge cases.
        let options = SqliteConnectOptions::new()
            .filename(path)
            .create_if_missing(true);

        let pool = SqlitePoolOptions::new()
            .max_connections(10)
            .connect_with(options)
            .await
            .map_err(|e| Error::storage(format!("Failed to connect sqlite db: {e}")))?;

        let store = Self { pool };
        store.initialize_schema().await?;
        Ok(store)
    }

    pub fn default_path() -> PathBuf {
        // Match the storage crate default filename for operator convenience.
        PathBuf::from("dchat.db")
    }

    async fn initialize_schema(&self) -> Result<()> {
        // Listings table (store JSON + indexed columns for basic filtering).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketplace_listings (
                id TEXT PRIMARY KEY,
                creator_id TEXT NOT NULL,
                good_type INTEGER NOT NULL,
                created_at INTEGER NOT NULL,
                json TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create marketplace_listings: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_listings_creator ON marketplace_listings(creator_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create listings index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_listings_type ON marketplace_listings(good_type)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create listings type index: {e}")))?;

        // Purchases table (store JSON + indexed columns).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketplace_purchases (
                id TEXT PRIMARY KEY,
                buyer_id TEXT NOT NULL,
                listing_id TEXT NOT NULL,
                purchased_at INTEGER NOT NULL,
                json TEXT NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create marketplace_purchases: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_purchases_buyer ON marketplace_purchases(buyer_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create purchases buyer index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_purchases_listing ON marketplace_purchases(listing_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create purchases listing index: {e}")))?;

        // Attestation nullifiers (replay protection).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketplace_attestation_nullifiers (
                escrow_id TEXT NOT NULL,
                kind INTEGER NOT NULL,
                payload_hash BLOB NOT NULL,
                created_at INTEGER NOT NULL,
                PRIMARY KEY (escrow_id, kind)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create nullifiers table: {e}")))?;

        // Entitlements (minted from EscrowLocked attestations).
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS marketplace_entitlements (
                escrow_id TEXT PRIMARY KEY,
                listing_id TEXT NOT NULL,
                buyer_id TEXT NOT NULL,
                seller_id TEXT NOT NULL,
                amount INTEGER NOT NULL,
                expires_at INTEGER NOT NULL,
                locked_at INTEGER NOT NULL,
                lock_tx_hash TEXT NOT NULL,
                lock_block_hash TEXT NOT NULL,
                lock_block_number INTEGER NOT NULL
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create entitlements table: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_entitlements_buyer ON marketplace_entitlements(buyer_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create entitlements buyer index: {e}")))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_marketplace_entitlements_listing ON marketplace_entitlements(listing_id)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to create entitlements listing index: {e}")))?;

        Ok(())
    }

    pub async fn create_listing(
        &self,
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
        // SECURITY/PRODUCTION: Do not fabricate on-chain identifiers.
        let listing = Listing {
            id: Uuid::new_v4(),
            creator: creator.clone(),
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
            on_chain_address: None,
            nft_token_id,
            bot_id,
            channel_id,
            membership_duration_days,
            in_escrow: false,
            escrow_id: None,
        };

        let created_at = listing.created_at.timestamp();
        let json = serde_json::to_string(&listing)?;

        sqlx::query(
            "INSERT INTO marketplace_listings (id, creator_id, good_type, created_at, json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(listing.id.to_string())
        .bind(listing.creator.to_string())
        .bind(digital_good_type_to_i64(listing.good_type))
        .bind(created_at)
        .bind(json)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert listing: {e}")))?;

        Ok(listing.id)
    }

    pub async fn get_listing(&self, listing_id: Uuid) -> Result<Option<Listing>> {
        let row = sqlx::query("SELECT json FROM marketplace_listings WHERE id = ?")
            .bind(listing_id.to_string())
            .fetch_optional(&self.pool)
            .await
            .map_err(|e| Error::storage(format!("Failed to get listing: {e}")))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let json: String = row
            .try_get("json")
            .map_err(|e| Error::storage(format!("Failed to read listing json: {e}")))?;

        let listing: Listing = serde_json::from_str(&json)?;
        Ok(Some(listing))
    }

    pub async fn list_listings(&self, good_type: Option<DigitalGoodType>) -> Result<Vec<Listing>> {
        let rows = if let Some(t) = good_type {
            sqlx::query("SELECT json FROM marketplace_listings WHERE good_type = ? ORDER BY created_at DESC")
                .bind(digital_good_type_to_i64(t))
                .fetch_all(&self.pool)
                .await
        } else {
            sqlx::query("SELECT json FROM marketplace_listings ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await
        }
        .map_err(|e| Error::storage(format!("Failed to list listings: {e}")))?;

        let mut out = Vec::with_capacity(rows.len());
        for row in rows {
            let json: String = row
                .try_get("json")
                .map_err(|e| Error::storage(format!("Failed to read listing json: {e}")))?;
            let listing: Listing = serde_json::from_str(&json)?;
            out.push(listing);
        }
        Ok(out)
    }

    pub async fn record_purchase(&self, purchase: &Purchase) -> Result<()> {
        let json = serde_json::to_string(purchase)?;

        sqlx::query(
            "INSERT INTO marketplace_purchases (id, buyer_id, listing_id, purchased_at, json) VALUES (?, ?, ?, ?, ?)",
        )
        .bind(purchase.id.to_string())
        .bind(purchase.buyer.to_string())
        .bind(purchase.listing_id.to_string())
        .bind(purchase.purchased_at.timestamp())
        .bind(json)
        .execute(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert purchase: {e}")))?;

        Ok(())
    }

    pub async fn mint_entitlement_from_attestation(
        &self,
        attestation: &MarketplaceAttestation,
        validator_set: &AttestationValidatorSet,
    ) -> Result<Entitlement> {
        attestation.verify(validator_set)?;

        if attestation.payload.kind != MarketplaceAttestationKind::EscrowLocked {
            return Err(Error::validation(
                "Only EscrowLocked attestations can mint entitlements",
            ));
        }

        let payload_hash = attestation.payload.payload_hash();
        let now = Utc::now();
        let expires_at =
            DateTime::<Utc>::from_timestamp(attestation.payload.expiry_unix_seconds as i64, 0)
                .ok_or_else(|| Error::validation("Invalid expiry_unix_seconds"))?;

        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| Error::storage(format!("Failed to start transaction: {e}")))?;

        let nullifier_outcome = insert_nullifier(
            &mut tx,
            attestation.payload.escrow_id,
            attestation.payload.kind,
            payload_hash.as_bytes(),
            now.timestamp(),
        )
        .await?;

        if nullifier_outcome == NullifierInsertOutcome::AlreadyExists {
            return Err(Error::validation(
                "Attestation already consumed (replay detected)",
            ));
        }

        // Ensure listing exists before minting entitlement.
        let listing = self
            .get_listing(attestation.payload.listing_id)
            .await?
            .ok_or_else(|| Error::NotFound("Listing not found".to_string()))?;

        // Extra binding checks.
        if listing.creator != attestation.payload.seller {
            return Err(Error::validation(
                "Attestation seller does not match listing creator",
            ));
        }

        let entitlement = Entitlement {
            escrow_id: attestation.payload.escrow_id,
            listing_id: attestation.payload.listing_id,
            buyer: attestation.payload.buyer.clone(),
            seller: attestation.payload.seller.clone(),
            amount: attestation.payload.amount,
            expires_at,
            locked_at: attestation.created_at,
            lock_tx_hash: attestation.payload.tx_hash.clone(),
            lock_block_hash: attestation.payload.block_hash.clone(),
            lock_block_number: attestation.payload.block_number,
        };

        sqlx::query(
            r#"INSERT INTO marketplace_entitlements (
                escrow_id, listing_id, buyer_id, seller_id, amount, expires_at, locked_at,
                lock_tx_hash, lock_block_hash, lock_block_number
            ) VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)"#,
        )
        .bind(entitlement.escrow_id.to_string())
        .bind(entitlement.listing_id.to_string())
        .bind(entitlement.buyer.to_string())
        .bind(entitlement.seller.to_string())
        .bind(entitlement.amount as i64)
        .bind(entitlement.expires_at.timestamp())
        .bind(entitlement.locked_at.timestamp())
        .bind(&entitlement.lock_tx_hash)
        .bind(&entitlement.lock_block_hash)
        .bind(entitlement.lock_block_number as i64)
        .execute(&mut *tx)
        .await
        .map_err(|e| Error::storage(format!("Failed to insert entitlement: {e}")))?;

        tx.commit()
            .await
            .map_err(|e| Error::storage(format!("Failed to commit transaction: {e}")))?;

        Ok(entitlement)
    }

    pub async fn get_entitlement(&self, escrow_id: Uuid) -> Result<Option<Entitlement>> {
        let row = sqlx::query(
            r#"SELECT
                escrow_id, listing_id, buyer_id, seller_id, amount, expires_at, locked_at,
                lock_tx_hash, lock_block_hash, lock_block_number
               FROM marketplace_entitlements
               WHERE escrow_id = ?"#,
        )
        .bind(escrow_id.to_string())
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| Error::storage(format!("Failed to fetch entitlement: {e}")))?;

        let Some(row) = row else {
            return Ok(None);
        };

        let escrow_id: String = row
            .try_get("escrow_id")
            .map_err(|e| Error::storage(format!("Failed to read escrow_id: {e}")))?;
        let listing_id: String = row
            .try_get("listing_id")
            .map_err(|e| Error::storage(format!("Failed to read listing_id: {e}")))?;
        let buyer_id: String = row
            .try_get("buyer_id")
            .map_err(|e| Error::storage(format!("Failed to read buyer_id: {e}")))?;
        let seller_id: String = row
            .try_get("seller_id")
            .map_err(|e| Error::storage(format!("Failed to read seller_id: {e}")))?;

        let amount: i64 = row
            .try_get("amount")
            .map_err(|e| Error::storage(format!("Failed to read amount: {e}")))?;
        let expires_at: i64 = row
            .try_get("expires_at")
            .map_err(|e| Error::storage(format!("Failed to read expires_at: {e}")))?;
        let locked_at: i64 = row
            .try_get("locked_at")
            .map_err(|e| Error::storage(format!("Failed to read locked_at: {e}")))?;
        let lock_tx_hash: String = row
            .try_get("lock_tx_hash")
            .map_err(|e| Error::storage(format!("Failed to read lock_tx_hash: {e}")))?;
        let lock_block_hash: String = row
            .try_get("lock_block_hash")
            .map_err(|e| Error::storage(format!("Failed to read lock_block_hash: {e}")))?;
        let lock_block_number: i64 = row
            .try_get("lock_block_number")
            .map_err(|e| Error::storage(format!("Failed to read lock_block_number: {e}")))?;

        let entitlement = Entitlement {
            escrow_id: Uuid::parse_str(&escrow_id)
                .map_err(|_| Error::validation("Invalid entitlement escrow_id"))?,
            listing_id: Uuid::parse_str(&listing_id)
                .map_err(|_| Error::validation("Invalid entitlement listing_id"))?,
            buyer: UserId(
                Uuid::parse_str(&buyer_id)
                    .map_err(|_| Error::validation("Invalid entitlement buyer_id"))?,
            ),
            seller: UserId(
                Uuid::parse_str(&seller_id)
                    .map_err(|_| Error::validation("Invalid entitlement seller_id"))?,
            ),
            amount: amount as u64,
            expires_at: DateTime::<Utc>::from_timestamp(expires_at, 0)
                .ok_or_else(|| Error::validation("Invalid entitlement expires_at"))?,
            locked_at: DateTime::<Utc>::from_timestamp(locked_at, 0)
                .ok_or_else(|| Error::validation("Invalid entitlement locked_at"))?,
            lock_tx_hash,
            lock_block_hash,
            lock_block_number: lock_block_number as u64,
        };

        Ok(Some(entitlement))
    }
}

async fn insert_nullifier(
    tx: &mut sqlx::Transaction<'_, sqlx::Sqlite>,
    escrow_id: Uuid,
    kind: MarketplaceAttestationKind,
    payload_hash: &[u8; 32],
    created_at: i64,
) -> Result<NullifierInsertOutcome> {
    let kind_i64 = kind as i64;

    let res = sqlx::query(
        "INSERT OR IGNORE INTO marketplace_attestation_nullifiers (escrow_id, kind, payload_hash, created_at) VALUES (?, ?, ?, ?)",
    )
    .bind(escrow_id.to_string())
    .bind(kind_i64)
    .bind(payload_hash.as_slice())
    .bind(created_at)
    .execute(&mut **tx)
    .await
    .map_err(|e| Error::storage(format!("Failed to insert nullifier: {e}")))?;

    if res.rows_affected() == 0 {
        Ok(NullifierInsertOutcome::AlreadyExists)
    } else {
        Ok(NullifierInsertOutcome::Inserted)
    }
}

fn digital_good_type_to_i64(t: DigitalGoodType) -> i64 {
    t as i64
}
