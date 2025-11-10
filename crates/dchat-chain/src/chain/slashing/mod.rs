// Slashing module for Byzantine behavior detection and penalties

pub mod detector;
pub mod evidence;
pub mod penalty;

pub use detector::{SlashingDetector, SlashableOffense, SlashingError, SlashingStats};
pub use evidence::{SlashingEvidence, EvidenceType, EvidenceError, EvidenceBundle};
pub use penalty::{SlashingPenalty, PenaltyApplicator, PenaltyError, PenaltyRecord, PenaltyStats, simulate_penalty};
