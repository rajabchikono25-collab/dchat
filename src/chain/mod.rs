// Chain-level modules (both chat and currency chains)

pub mod currency_chain;
pub mod slashing;

pub use currency_chain::staking;
pub use slashing::{
    SlashingDetector, SlashableOffense, SlashingError,
    SlashingEvidence, EvidenceType, EvidenceError,
    SlashingPenalty, PenaltyApplicator, PenaltyError
};
