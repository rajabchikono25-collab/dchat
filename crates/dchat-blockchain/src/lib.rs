//! Blockchain client for parallel chain architecture
//! Supports Chat Chain, Currency Chain, Cross-Chain Bridge, and Tokenomics

pub mod block_hierarchy;
pub mod chain_synchronizer;
pub mod chat_chain;
pub mod client;
pub mod consensus_types;
pub mod cross_chain;
pub mod currency_chain;
pub mod currency_chain_block_sync;
pub mod hardened_consensus;
pub mod proof_of_relay_work;
pub mod proof_of_transit;
pub mod rpc;
pub mod staking;
pub mod state_validation;
pub mod temporal_stake_consensus;
pub mod tokenomics;

// Production gap fixes (Nov 2025)
pub mod geoip;
pub mod oracle_network;
pub mod vote_persistence;

// Economic infrastructure (plan2.md implementation)
pub mod faucet;
pub mod payment_channels;
pub mod payment_processor;
pub mod staking_backend;
pub mod watchtower;

// Wallet infrastructure (production wallets with Solana compatibility)
pub mod solana;
pub mod solana_bridge;
pub mod wallet;

pub use block_hierarchy::{
    Block, BlockError, ExecutionResult, FinalityProof, Miniblock, StateDelta, Subblock,
    ValidatorSignature, WorldState,
};
pub use chain_synchronizer::{
    ChainSyncConfig, ChainSynchronizer, ChainThroughput, ChainType, CrossChainFinalityStatus,
    CurrencyHierarchicalBlock, CurrencyMiniblock, CurrencySubblock, CurrencyTransaction,
    CurrencyTxType, FinalityAnchor, SyncEpoch, SyncError, SyncStatusReport, ThroughputReport,
};
pub use chat_chain::{ChatChainClient, ChatChainConfig};
pub use client::{BlockchainClient, BlockchainConfig};
pub use consensus_types::{
    BlockVotes, ConsensusError, DeliveryProof, GeographicRegion, RelayScore, RelayVote,
};
pub use cross_chain::{CrossChainBridge, CrossChainStatus, CrossChainTransaction};
pub use currency_chain::{
    CreateStorageBondResult, CurrencyChainClient, CurrencyChainConfig, StorageBondRecord,
    StorageBondStatus,
};
pub use currency_chain_block_sync::{
    BlockSyncConfig, BlockSyncManager, CurrencyBlock, CurrencyBlockHeader, ForkInfo, SyncStatus,
};
pub use dchat_chain::{Transaction, TransactionStatus, TransactionType};
pub use proof_of_relay_work::ProofOfRelayWork;
pub use proof_of_transit::{
    Dilithium3Signature, FinalityLevel, GeoLocation, HybridSignature, PoTError, ProofOfTransit,
    TransitPath, TransitProof,
};
pub use rpc::{RpcClient, RpcConfig};
pub use staking::{
    ClaimReceipt, SlashingEvent, SlashingSeverity, StakingManager, StakingTransaction,
    StakingTxType, ValidatorStake, ValidatorStatus, MAX_ACTIVE_VALIDATORS, MAX_VALIDATOR_STAKE,
    MIN_VALIDATOR_STAKE, UNSTAKE_COOLDOWN_SECONDS,
};
pub use state_validation::{
    MerkleNode, MerkleProof, MerkleTree, StateValidationError, StateValidator,
};
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

// Economic infrastructure exports (plan2.md implementation)
pub use faucet::{Faucet, FaucetConfig, FaucetError, FaucetState};
pub use payment_channels::{
    ChannelError, ChannelState, CloseChallenge, FraudEvent, MessageCreditsChannel, PaymentChannel,
    PaymentChannelManager, SignedStateUpdate, UnilateralCloseRequest,
};
pub use payment_processor::{
    PaymentProcessor, PaymentProcessorConfig, PaymentProcessorStats, PaymentReceipt,
};
pub use staking_backend::CurrencyChainStakingBackend;
pub use watchtower::{
    AlertSeverity, AlertType, FraudAttemptRecord, WatchedChannel, WatchedChannelState, Watchtower,
    WatchtowerAlert, WatchtowerConfig, WatchtowerError, WatchtowerMonitor, WatchtowerStats,
};

// Wallet infrastructure exports (production wallets with Solana compatibility)
pub use wallet::{
    AddressFormat,
    AddressMapping,
    BurnerStats,
    // Burner wallet
    BurnerWallet,
    BurnerWalletConfig,
    BurnerWalletManager,
    DestructionReason,
    MultiSigConfig,
    // Multi-sig wallet
    MultiSigWallet,
    PendingMultiSigTx,
    SignedTransaction,
    SignerInfo,
    // Solana compatibility
    SolanaAddress,
    SolanaCompatible,
    SolanaSignature,
    TokenBalance,
    TokenMint,
    TransactionSignature,
    // Universal addressing
    UniversalAddress,
    // Normal wallet
    Wallet,
    // Core wallet types
    WalletBalance,
    WalletConfig,
    WalletExport,
    WalletTransaction,
    WalletType,
};

pub use solana_bridge::{
    BridgeDirection, BridgeStatistics, BridgeTransfer, BridgeTransferStatus, BridgeValidator,
    SolanaBridge, SolanaBridgeConfig, ValidatorSignature as BridgeValidatorSignature,
};

// Solana integration exports
pub use solana::{
    create_shared_client,
    lamports_to_sol,
    sol_to_lamports,
    AccountInfo as SolanaAccountInfo,
    AccountMeta,
    AssociatedTokenAccount,
    BridgeInstruction,
    // Bridge program
    BridgeProgram,
    BridgeState,
    // Helpers
    Cluster,
    Commitment,
    CompiledInstruction,
    DepositRecord,
    Instruction,
    LockAccounts,
    MessageHeader,
    MintInfo,
    Rent,
    RpcError,
    SharedSolanaClient,
    // Accounts
    SolanaAccount,
    // High-level client
    SolanaClient,
    // RPC client
    SolanaRpcClient,
    SolanaRpcConfig,
    // Transaction building
    SolanaTransaction,
    SolanaTxStatus,
    // SPL Token
    SplToken,
    SystemProgram,
    TokenAccount,
    TokenInstruction,
    TokenTransfer,
    TransactionBuilder,
    TransactionMessage,
    UnlockAccounts,
    WithdrawRecord,
    LAMPORTS_PER_SOL,
};

// Re-export privacy trait implementations for integration
// ChatChainClient implements dchat_privacy::zk_proofs::BlockchainClient
// CurrencyChainClient implements dchat_privacy::blind_tokens::CurrencyChainClient
pub use dchat_privacy::blind_tokens::CurrencyChainClient as PrivacyCurrencyChainClient;
pub use dchat_privacy::zk_proofs::BlockchainClient as PrivacyBlockchainClient;
