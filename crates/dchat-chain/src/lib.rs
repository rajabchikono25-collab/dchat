//! Blockchain components for dchat
//!
//! This crate provides on-chain functionality including:
//! - On-chain transaction types for user operations
//! - Channel sharding and state partitioning
//! - Cryptographic dispute resolution
//! - Fork arbitration and consensus recovery
//! - Message consensus pruning with Merkle checkpoints
//! - Insurance fund for economic security

pub mod chain; // Currency chain and slashing modules
pub mod currency_chain_client;
pub mod dispute_resolution;
pub mod insurance_fund;
pub mod pruning;
pub mod sharding;
pub mod transactions;

// Re-export chain submodules
pub use chain::currency_chain;
pub use chain::slashing;

pub use currency_chain_client::HttpCurrencyChainClient;
pub use dispute_resolution::{
    CurrencyChainClient, DisputeClaim, DisputeResolver, DisputeStatus, SlashingConfig,
    SlashingEvent,
};
pub use insurance_fund::{
    ClaimStatus, ClaimType, FundConfiguration, FundStatistics, FundTransaction, InsuranceClaim,
    InsuranceFund, TransactionType as FundTransactionType,
};
pub use pruning::{MerkleCheckpoint, MerkleProof, NodeType, PruningManager, PruningPolicy};
pub use sharding::{ShardConfig, ShardId, ShardManager};
pub use transactions::{
    ChannelVisibility, CreateChannelTx, JoinChannelTx, PostToChannelTx, RegisterUserTx,
    SendDirectMessageTx, SubmitDeliveryProofTx, Transaction, TransactionReceipt,
    TransactionStatus, TransactionType,
};
