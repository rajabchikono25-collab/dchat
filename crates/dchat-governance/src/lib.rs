// dchat-governance: Decentralized governance and DAO infrastructure
//
// This crate implements voting, proposals, and decentralized moderation
// for the dchat protocol.

pub mod abuse_reporting;
pub mod governance_chain_client;
pub mod moderation;
pub mod protocol_dao;
pub mod upgrade;
pub mod voting;

pub use abuse_reporting::{
    get_zk_keys, AbuseReport, JurySelection, ReportManager, ZkKeyConfig, DEFAULT_ZK_KEYS_DIR,
    PROVING_KEY_FILENAME, VERIFYING_KEY_FILENAME, ZK_KEYS_DIR_ENV,
};
pub use governance_chain_client::HttpGovernanceChainClient;
pub use moderation::{ModerationAction, ModerationManager, SlashingVote};
pub use protocol_dao::{
    GovernanceChainClient, GovernanceEventLog, GovernanceTxReceipt, ProposalStatus,
    ProposalType as ProtocolProposalType, ProtocolDaoConfig, ProtocolDaoManager, ProtocolProposal,
};
pub use upgrade::{
    ForkState, UpgradeManager, UpgradeProposal, UpgradeStatus, UpgradeType, ValidatorSignature,
    Version,
};
pub use voting::{
    compute_vote_commitment, generate_vote_salt, verify_vote_reveal, Proposal, ProposalType, Vote,
    VoteManager, VoteManagerConfig,
};
