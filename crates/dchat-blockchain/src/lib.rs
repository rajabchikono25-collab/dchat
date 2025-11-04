//! Blockchain client for parallel chain architecture
//! Supports Chat Chain, Currency Chain, Cross-Chain Bridge, and Tokenomics

pub mod block_hierarchy;
pub mod chat_chain;
pub mod client;
pub mod cross_chain;
pub mod currency_chain;
pub mod proof_of_relay_work;
pub mod proof_of_transit;
pub mod rpc;
pub mod temporal_stake_consensus;
pub mod tokenomics;

// Production gap fixes (Nov 2025)
pub mod geoip;
pub mod oracle_network;
pub mod vote_persistence;

pub use block_hierarchy::{
    Block, BlockError, ExecutionResult, FinalityProof, Miniblock, StateDelta, Subblock,
    Transaction, ValidatorSignature, WorldState,
};
pub use chat_chain::{ChatChainClient, ChatChainConfig};
pub use client::BlockchainClient;
pub use cross_chain::{CrossChainBridge, CrossChainStatus, CrossChainTransaction};
pub use currency_chain::{CurrencyChainClient, CurrencyChainConfig};
pub use proof_of_relay_work::{
    BlockVotes, ConsensusError, DeliveryProof, GeographicRegion, ProofOfRelayWork, RelayScore,
    RelayVote,
};
pub use proof_of_transit::{
    Dilithium3Signature, FinalityLevel, GeoLocation, HybridSignature, PoTError, ProofOfTransit,
    TransitPath, TransitProof,
};
pub use rpc::{RpcClient, RpcConfig};
pub use temporal_stake_consensus::{
    LockupTier, PredictiveOracle, TSCBlockVotes, TSCError, TSCVote, TemporalStake,
    TemporalStakeConsensus,
};
pub use tokenomics::{
    BurnEvent, BurnReason, DistributionSchedule, LiquidityPool, MintEvent, MintReason,
    RecipientType, TokenSupplyConfig, TokenomicsManager, TokenomicsStats,
};

// Production gap fix exports
pub use geoip::{GeoIPError, GeoIPManager, GeoLocation as GeoIPLocation, GeographicQuorum};
pub use oracle_network::{
    OracleConsensus, OracleError, OracleNetwork, OraclePrediction, OracleRegistration,
    PredictionType,
};
pub use vote_persistence::{
    PoRWVoteRecord, PoTProofRecord, TSCVoteRecord, ValidatorStats, VotePersistence,
    VotePersistenceError,
};
