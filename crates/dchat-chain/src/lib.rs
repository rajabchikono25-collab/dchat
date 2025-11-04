//! Blockchain components for dchat
//!
//! This crate provides on-chain functionality including:
//! - On-chain transaction types for user operations
//! - Channel sharding and state partitioning
//! - Cryptographic dispute resolution
//! - Fork arbitration and consensus recovery
//! - Message consensus pruning with Merkle checkpoints
//! - Insurance fund for economic security

pub mod dispute_resolution;
pub mod insurance_fund;
pub mod pruning;
pub mod sharding;
pub mod transactions;

pub use dispute_resolution::{DisputeClaim, DisputeResolver, DisputeStatus};
pub use insurance_fund::{
    ClaimStatus, ClaimType, FundConfiguration, FundStatistics, FundTransaction, InsuranceClaim,
    InsuranceFund, TransactionType as FundTransactionType,
};
pub use pruning::{MerkleCheckpoint, MerkleProof, NodeType, PruningManager, PruningPolicy};
pub use sharding::{ShardConfig, ShardId, ShardManager};
pub use transactions::{
    ChannelVisibility, CreateChannelTx, JoinChannelTx, PostToChannelTx, RegisterUserTx,
    SendDirectMessageTx, Transaction, TransactionReceipt, TransactionStatus, TransactionType,
};
