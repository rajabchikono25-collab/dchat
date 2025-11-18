// Chain-level modules (both chat and currency chains)

pub mod bootstrap;
pub mod currency_chain;
pub mod genesis;
pub mod slashing;

pub use bootstrap::{BootstrapCoordinator, BootstrapEvent, BootstrapStatus};
pub use currency_chain::staking;
pub use currency_chain::validator_enforcement;
pub use genesis::{
    ChatGenesisBlock, ChatGenesisConfig, CurrencyGenesisBlock, CurrencyGenesisConfig,
    GenesisBuilder, GenesisCoordinator, GenesisValidator,
};
pub use slashing::{
    EvidenceError, EvidenceType, PenaltyApplicator, PenaltyError, SlashableOffense,
    SlashingDetector, SlashingError, SlashingEvidence, SlashingPenalty,
};
