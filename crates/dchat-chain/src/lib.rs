//! Blockchain components for dchat
//!
//! This crate provides on-chain functionality including:
//! - On-chain transaction types for user operations
//! - Channel sharding and state partitioning
//! - Cryptographic dispute resolution
//! - Fork arbitration and consensus recovery
//! - Message consensus pruning with Merkle checkpoints
//! - Insurance fund for economic security
//! - Guardian-based account recovery with on-chain timelock verification

pub mod chain; // Currency chain, slashing, and guardian modules
pub mod currency_chain_client;
pub mod currency_transactions;
pub mod currency_transaction_parser;
pub mod balance_tracker;
pub mod dispute_resolution;
pub mod insurance_fund;
pub mod pruning;
pub mod sharding;
pub mod transactions;

// Re-export chain submodules
pub use chain::currency_chain;
pub use chain::guardians;
pub use chain::slashing;

pub use currency_chain_client::HttpCurrencyChainClient;
pub use currency_transaction_parser::{CurrencyTransactionParser, ParsedTransaction, TransactionData};
pub use balance_tracker::BalanceTracker;
pub use currency_transactions::{
    Balance, BlockRewardTx, ChannelAccessTx, ClaimRewardsTx, CurrencyTransactionType,
    DelegateTx, Delegation, PendingUnstake, RelayPaymentTx, RewardType, SlashReason, SlashTx,
    StakeTx, StakeType, StakingInfo, TransferTx, UndelegateTx, UnstakeTx,
};
pub use dispute_resolution::{
    CurrencyChainClient, DisputeClaim, DisputeResolver, DisputeStatus, SlashingConfig,
    SlashingEvent,
};
pub use guardians::{
    GuardianChainState, InitiateRecoveryTx, RegisterGuardianTx, SubmitGuardianSignatureTx,
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
