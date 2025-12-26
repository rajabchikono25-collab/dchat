//! Cross-chain marketplace attestations (Option B: threshold-signed finality attestations)
//!
//! Security goals:
//! - Prevent forged entitlements: entitlements can only be minted from a threshold-signed
//!   currency-chain escrow event attestation.
//! - Prevent replay/double-mint: consumer must persist a nullifier keyed by `escrow_id`.
//! - Domain separation: signatures are scoped to marketplace attestations via a dedicated DST.
//!
//! This module is intentionally opinionated and strict:
//! - Canonical message encoding is fixed and versioned.
//! - Signature verification requires a known validator set (BLS pubkeys) and a minimum threshold.

use blake3::Hash;
use blst::min_pk::{PublicKey, Signature};
use chrono::{DateTime, Utc};
use dchat_bridge::ChainId;
use dchat_core::{types::UserId, Error, Result};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeSet, HashMap};
use uuid::Uuid;

/// Domain separation tag (DST) used by BLS signing/verifying for marketplace attestations.
///
/// Changing this requires a version bump and coordinated rollout.
pub const MARKETPLACE_ATTESTATION_DST: &[u8] = b"DCHAT_MARKETPLACE_ATTESTATION_V1";

/// Canonical encoding version for `MarketplaceAttestationPayload`.
pub const MARKETPLACE_ATTESTATION_ENCODING_VERSION: u16 = 1;

/// Maximum number of signers allowed in an attestation.
/// Prevents adversarially large signer lists from causing verification DoS.
pub const MAX_ATTESTATION_SIGNERS: usize = 128;

/// The specific marketplace event being attested to.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Hash)]
pub enum MarketplaceAttestationKind {
    /// Currency chain escrow funds were locked for a listing purchase.
    EscrowLocked = 1,
    /// Currency chain escrow funds were released to the seller.
    EscrowReleased = 2,
    /// Currency chain escrow funds were refunded to the buyer.
    EscrowRefunded = 3,
}

impl MarketplaceAttestationKind {
    fn as_u8(self) -> u8 {
        self as u8
    }
}

/// Canonical payload that validators sign.
///
/// This should be treated as the single source of truth for the attested facts.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceAttestationPayload {
    pub kind: MarketplaceAttestationKind,
    pub chain: ChainId,

    /// Currency chain tx hash (human-readable). Included for auditability.
    pub tx_hash: String,
    /// Currency chain block hash (human-readable). Included for auditability.
    pub block_hash: String,
    pub block_number: u64,

    /// Unique escrow identifier on the currency chain.
    pub escrow_id: Uuid,
    /// Marketplace listing identifier on the chat chain.
    pub listing_id: Uuid,

    /// Participants
    pub buyer: UserId,
    pub seller: UserId,

    /// Amount locked/released/refunded (smallest unit).
    pub amount: u64,

    /// Expiry timestamp for lock-based flows (seconds since epoch).
    /// For non-lock events this still binds the attestation to a specific purchase policy.
    pub expiry_unix_seconds: u64,

    /// Blinded event hash (optional) for compact commitment to chain-specific event fields.
    /// If present, it MUST be the exact commitment the validator committee used.
    pub event_hash: Option<[u8; 32]>,
}

impl MarketplaceAttestationPayload {
    /// Canonical bytes to be signed.
    ///
    /// Format (all little-endian where applicable):
    /// - magic (16 bytes): "DCHAT_MKT_ATTEST"
    /// - encoding_version (u16)
    /// - kind (u8)
    /// - chain (u8)
    /// - block_number (u64)
    /// - escrow_id (16 bytes)
    /// - listing_id (16 bytes)
    /// - buyer (16 bytes)
    /// - seller (16 bytes)
    /// - amount (u64)
    /// - expiry_unix_seconds (u64)
    /// - tx_hash_hash (32 bytes) = blake3(tx_hash)
    /// - block_hash_hash (32 bytes) = blake3(block_hash)
    /// - event_hash_present (u8)
    /// - event_hash (32 bytes if present)
    pub fn canonical_message(&self) -> Vec<u8> {
        const MAGIC: &[u8; 16] = b"DCHAT_MKT_ATTEST";

        let mut out = Vec::with_capacity(16 + 2 + 1 + 1 + 8 + (16 * 4) + 8 + 8 + 32 + 32 + 1 + 32);
        out.extend_from_slice(MAGIC);
        out.extend_from_slice(&MARKETPLACE_ATTESTATION_ENCODING_VERSION.to_le_bytes());
        out.push(self.kind.as_u8());
        out.push(chain_id_to_u8(&self.chain));
        out.extend_from_slice(&self.block_number.to_le_bytes());
        out.extend_from_slice(self.escrow_id.as_bytes());
        out.extend_from_slice(self.listing_id.as_bytes());
        out.extend_from_slice(self.buyer.0.as_bytes());
        out.extend_from_slice(self.seller.0.as_bytes());
        out.extend_from_slice(&self.amount.to_le_bytes());
        out.extend_from_slice(&self.expiry_unix_seconds.to_le_bytes());
        out.extend_from_slice(blake3::hash(self.tx_hash.as_bytes()).as_bytes());
        out.extend_from_slice(blake3::hash(self.block_hash.as_bytes()).as_bytes());

        match self.event_hash {
            Some(h) => {
                out.push(1);
                out.extend_from_slice(&h);
            }
            None => {
                out.push(0);
            }
        }

        out
    }

    /// Convenience commitment for logging/storage.
    pub fn payload_hash(&self) -> Hash {
        blake3::hash(&self.canonical_message())
    }
}

fn chain_id_to_u8(chain: &ChainId) -> u8 {
    match chain {
        ChainId::ChatChain => 1,
        ChainId::CurrencyChain => 2,
        ChainId::Solana => 3,
    }
}

/// Threshold configuration + known validator pubkeys.
#[derive(Debug, Clone)]
pub struct AttestationValidatorSet {
    pub required_signers: usize,
    pub validators: HashMap<UserId, Vec<u8>>, // 48-byte compressed BLS min_pk pubkeys
}

impl AttestationValidatorSet {
    pub fn new(required_signers: usize, validators: HashMap<UserId, Vec<u8>>) -> Result<Self> {
        if required_signers == 0 {
            return Err(Error::validation("required_signers must be > 0"));
        }
        if validators.is_empty() {
            return Err(Error::validation("validator set must not be empty"));
        }
        if required_signers > validators.len() {
            return Err(Error::validation(
                "required_signers exceeds validator set size",
            ));
        }

        // Validate all keys up-front.
        for (id, pk) in &validators {
            if pk.len() != 48 {
                return Err(Error::validation(format!(
                    "invalid BLS pubkey length for validator {}: {}",
                    id,
                    pk.len()
                )));
            }
            PublicKey::from_bytes(pk).map_err(|_| Error::validation("invalid BLS pubkey bytes"))?;
        }

        Ok(Self {
            required_signers,
            validators,
        })
    }

    fn pubkeys_for_signers(&self, signer_ids: &[UserId]) -> Result<Vec<PublicKey>> {
        let mut out = Vec::with_capacity(signer_ids.len());
        for id in signer_ids {
            let pk_bytes = self
                .validators
                .get(id)
                .ok_or_else(|| Error::validation("unknown attestation signer"))?;
            let pk = PublicKey::from_bytes(pk_bytes)
                .map_err(|_| Error::validation("invalid signer BLS pubkey"))?;
            out.push(pk);
        }
        Ok(out)
    }
}

/// A threshold-signed attestation over a `MarketplaceAttestationPayload`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceAttestation {
    pub payload: MarketplaceAttestationPayload,
    /// Signers who contributed to the aggregated signature.
    pub signer_ids: Vec<UserId>,
    /// Aggregated BLS signature (96 bytes, min_pk compressed).
    pub aggregated_signature: Vec<u8>,
    pub created_at: DateTime<Utc>,
}

/// Verification errors specific to marketplace attestations.
#[derive(Debug, thiserror::Error)]
pub enum AttestationError {
    #[error("invalid attestation: {0}")]
    Invalid(&'static str),
    #[error("invalid attestation: {0}")]
    InvalidOwned(String),
}

impl MarketplaceAttestation {
    /// Verify attestation signature + basic payload sanity.
    ///
    /// Replay protection (nullifier storage) is intentionally NOT done here: callers must persist
    /// a record keyed by `payload.escrow_id` (or `payload.payload_hash()`) after successful verify.
    pub fn verify(&self, set: &AttestationValidatorSet) -> Result<()> {
        // Basic checks
        if self.payload.chain != ChainId::CurrencyChain {
            return Err(Error::validation(
                "marketplace attestations must originate from CurrencyChain",
            ));
        }
        if self.payload.amount == 0 {
            return Err(Error::validation("attested amount must be > 0"));
        }
        if self.payload.listing_id.is_nil() || self.payload.escrow_id.is_nil() {
            return Err(Error::validation("listing_id/escrow_id must be non-nil"));
        }

        if self.signer_ids.len() < set.required_signers {
            return Err(Error::validation("insufficient signer count for threshold"));
        }
        if self.signer_ids.len() > MAX_ATTESTATION_SIGNERS {
            return Err(Error::validation("too many signers"));
        }

        // Ensure signer uniqueness
        let mut uniq = BTreeSet::new();
        for id in &self.signer_ids {
            if !uniq.insert(id.0) {
                return Err(Error::validation("duplicate signer in attestation"));
            }
        }

        if self.aggregated_signature.len() != 96 {
            return Err(Error::validation("invalid aggregated BLS signature length"));
        }

        // Canonical message
        let msg = self.payload.canonical_message();

        // Parse aggregated signature
        let agg_sig = Signature::from_bytes(&self.aggregated_signature)
            .map_err(|_| Error::validation("invalid aggregated BLS signature bytes"))?;

        // Resolve signer pubkeys
        let pubkeys = set.pubkeys_for_signers(&self.signer_ids)?;
        let pk_refs: Vec<&PublicKey> = pubkeys.iter().collect();

        // Verify using fast_aggregate_verify for same-message multi-sig.
        // NOTE: This assumes the validator set uses proof-of-possession or otherwise trusted key registration.
        let result =
            agg_sig.fast_aggregate_verify(true, &msg, MARKETPLACE_ATTESTATION_DST, &pk_refs);
        if result != blst::BLST_ERROR::BLST_SUCCESS {
            return Err(Error::validation(
                "attestation signature verification failed",
            ));
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use blst::min_pk::AggregateSignature;
    use blst::min_pk::SecretKey;

    fn generate_bls_keypair() -> (SecretKey, blst::min_pk::PublicKey) {
        let mut ikm = [0u8; 32];
        getrandom::getrandom(&mut ikm).unwrap();
        let sk = SecretKey::key_gen(&ikm, &[]).unwrap();
        let pk = sk.sk_to_pk();
        (sk, pk)
    }

    fn payload() -> MarketplaceAttestationPayload {
        MarketplaceAttestationPayload {
            kind: MarketplaceAttestationKind::EscrowLocked,
            chain: ChainId::CurrencyChain,
            tx_hash: "0xdeadbeef".to_string(),
            block_hash: "0xabc".to_string(),
            block_number: 123,
            escrow_id: Uuid::new_v4(),
            listing_id: Uuid::new_v4(),
            buyer: UserId::new(),
            seller: UserId::new(),
            amount: 1_000,
            expiry_unix_seconds: 1_800_000_000,
            event_hash: Some([7u8; 32]),
        }
    }

    #[test]
    fn verifies_threshold_attestation() {
        let p = payload();
        let msg = p.canonical_message();

        let (sk1, pk1) = generate_bls_keypair();
        let (sk2, pk2) = generate_bls_keypair();
        let (_sk3, pk3) = generate_bls_keypair();

        let v1 = UserId::new();
        let v2 = UserId::new();
        let v3 = UserId::new();

        let mut validators = HashMap::new();
        validators.insert(v1.clone(), pk1.to_bytes().to_vec());
        validators.insert(v2.clone(), pk2.to_bytes().to_vec());
        validators.insert(v3.clone(), pk3.to_bytes().to_vec());

        let set = AttestationValidatorSet::new(2, validators).unwrap();

        let sig1 = sk1.sign(&msg, MARKETPLACE_ATTESTATION_DST, &[]);
        let sig2 = sk2.sign(&msg, MARKETPLACE_ATTESTATION_DST, &[]);

        let s1 = Signature::from_bytes(&sig1.to_bytes()).unwrap();
        let s2 = Signature::from_bytes(&sig2.to_bytes()).unwrap();
        let sig_refs: Vec<&Signature> = vec![&s1, &s2];
        let agg = AggregateSignature::aggregate(&sig_refs, true).unwrap();
        let agg_bytes = agg.to_signature().to_bytes().to_vec();

        let att = MarketplaceAttestation {
            payload: p,
            signer_ids: vec![v1, v2],
            aggregated_signature: agg_bytes,
            created_at: Utc::now(),
        };

        att.verify(&set).unwrap();
    }

    #[test]
    fn rejects_wrong_dst_signature() {
        let p = payload();
        let msg = p.canonical_message();

        let (sk1, pk1) = generate_bls_keypair();
        let (sk2, pk2) = generate_bls_keypair();

        let v1 = UserId::new();
        let v2 = UserId::new();

        let mut validators = HashMap::new();
        validators.insert(v1.clone(), pk1.to_bytes().to_vec());
        validators.insert(v2.clone(), pk2.to_bytes().to_vec());
        let set = AttestationValidatorSet::new(2, validators).unwrap();

        // Sign with bridge finality DST instead of marketplace DST
        let bad_dst = b"DCHAT_BRIDGE_FINALITY_V1";
        let sig1 = sk1.sign(&msg, bad_dst, &[]);
        let sig2 = sk2.sign(&msg, bad_dst, &[]);

        let s1 = Signature::from_bytes(&sig1.to_bytes()).unwrap();
        let s2 = Signature::from_bytes(&sig2.to_bytes()).unwrap();
        let sig_refs: Vec<&Signature> = vec![&s1, &s2];
        let agg = AggregateSignature::aggregate(&sig_refs, true).unwrap();

        let att = MarketplaceAttestation {
            payload: p,
            signer_ids: vec![v1, v2],
            aggregated_signature: agg.to_signature().to_bytes().to_vec(),
            created_at: Utc::now(),
        };

        assert!(att.verify(&set).is_err());
    }
}
