//! Blockchain client for parallel chain architecture
//! Supports Chat Chain, Currency Chain, Cross-Chain Bridge, and Tokenomics

pub mod block_hierarchy;
pub mod chat_chain;
pub mod client;
pub mod cross_chain;
pub mod currency_chain;
pub mod proof_of_relay_work;
pub mod proof_of_transit;
pub mod temporal_stake_consensus;
pub mod rpc;
pub mod tokenomics;

pub use block_hierarchy::{
    Block, Subblock, Miniblock, Transaction, ExecutionResult, StateDelta,
    ValidatorSignature, FinalityProof, BlockError, WorldState,
};
pub use chat_chain::{ChatChainClient, ChatChainConfig};
pub use client::BlockchainClient;
pub use cross_chain::{CrossChainBridge, CrossChainTransaction, CrossChainStatus};
pub use currency_chain::{CurrencyChainClient, CurrencyChainConfig};
pub use proof_of_relay_work::{
    ProofOfRelayWork, RelayScore, GeographicRegion, DeliveryProof,
    BlockVotes, RelayVote, ConsensusError,
};
pub use proof_of_transit::{
    ProofOfTransit, TransitProof, TransitPath, FinalityLevel, GeoLocation,
    HybridSignature, Dilithium3Signature, PoTError,
};
pub use temporal_stake_consensus::{
    TemporalStakeConsensus, TemporalStake, LockupTier, TSCVote, TSCBlockVotes,
    PredictiveOracle, TSCError,
};
pub use rpc::{RpcClient, RpcConfig};
pub use tokenomics::{
    TokenomicsManager, TokenSupplyConfig, MintEvent, MintReason, BurnEvent, BurnReason,
    LiquidityPool, DistributionSchedule, RecipientType, TokenomicsStats,
};
