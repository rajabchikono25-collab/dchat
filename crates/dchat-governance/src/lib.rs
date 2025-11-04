// dchat-governance: Decentralized governance and DAO infrastructure
//
// This crate implements voting, proposals, and decentralized moderation
// for the dchat protocol.

pub mod abuse_reporting;
pub mod moderation;
pub mod upgrade;
pub mod voting;

pub use abuse_reporting::{AbuseReport, JurySelection, ReportManager};
pub use moderation::{ModerationAction, ModerationManager, SlashingVote};
pub use upgrade::{
    ForkState, UpgradeManager, UpgradeProposal, UpgradeStatus, UpgradeType, ValidatorSignature,
    Version,
};
pub use voting::{Proposal, ProposalType, Vote, VoteManager};
