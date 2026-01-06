//! Mainnet-Critical Hardened Consensus Infrastructure
//!
//! This module implements the production-hardened triple-layer consensus with:
//! - Fraud-provable commitment-based proofs with Merkle roots + deterministic sampling
//! - VRF-selected relay committees for bounded consensus work
//! - VRF-based single leader selection per slot for deterministic block production
//! - Validator chain synchronization with GHOST fork choice rule
//! - Batch signature verification and sharded state
//! - Admission control with backpressure and priority lanes
//! - Fast-path transport framing with stateless cookies
//! - Normalized thresholds against epoch snapshots
//! - Two-stage finality model with attack escalation presets
//!
//! CRITICAL FOR MAINNET: This entire stack must be treated as mandatory.
//!
//! # Integration Layer
//!
//! The `integration` module provides the unified coordinator that wires all
//! hardened components into the existing PoRW, PoT, and TSC consensus engines.
//! Use `HardenedConsensusCoordinator` as the single entry point for all
//! consensus operations in production.
//!
//! # Slot Leader Selection
//!
//! Each slot has exactly one leader selected via VRF:
//! - `slot_leader_selection` - VRF-based single leader per slot
//! - `validator_chain_sync` - Chain synchronization across validators

pub mod admission_control;
pub mod batch_verification;
pub mod challenge_response;
pub mod epoch_snapshot;
#[cfg(feature = "hardened-consensus-integration")]
pub mod integration;
pub mod merkle_commitments;
pub mod sharded_state;
pub mod slot_leader_selection;
pub mod threshold_normalization;
pub mod transport_framing;
pub mod two_stage_finality;
pub mod validator_chain_sync;
pub mod vrf_committees;

// Re-export all critical types
pub use admission_control::*;
pub use batch_verification::*;
pub use challenge_response::*;
pub use epoch_snapshot::*;
#[cfg(feature = "hardened-consensus-integration")]
pub use integration::*;
pub use merkle_commitments::*;
pub use sharded_state::*;
pub use slot_leader_selection::*;
pub use threshold_normalization::*;
pub use transport_framing::*;
pub use two_stage_finality::*;
pub use validator_chain_sync::*;
pub use vrf_committees::*;
