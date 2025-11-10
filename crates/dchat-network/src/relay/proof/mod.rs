/// Proof-of-delivery system for relay reward distribution.
///
/// This module implements cryptographic proofs that enable relays to claim rewards
/// for successfully delivering messages. Proofs are batched for efficiency and
/// submitted on-chain for transparent reward distribution.

pub mod batching;
pub mod delivery;

pub use batching::{BatchAccumulator, BatchId, ProofBatch, BATCH_SIZE, BATCH_TIMEOUT};
pub use delivery::{create_recipient_acknowledgment, DeliveryProof, MessageId};
