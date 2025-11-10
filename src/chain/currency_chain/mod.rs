// Currency chain modules (economic layer)

pub mod staking;

pub use staking::{submit_validator_stake, submit_validator_unstake, StakeRequest, StakeReceipt};
