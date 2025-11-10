// Slashing module for Byzantine behavior detection and penalties

pub mod detector;
pub mod evidence;
pub mod penalty;

pub use detector::{SlashableOffense, SlashingDetector, SlashingError, SlashingStats};
pub use evidence::{EvidenceBundle, EvidenceError, EvidenceType, SlashingEvidence};
pub use penalty::{
    simulate_penalty, PenaltyApplicator, PenaltyError, PenaltyRecord, PenaltyStats, SlashingPenalty,
};
