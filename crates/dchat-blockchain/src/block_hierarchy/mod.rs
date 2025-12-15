//! Hierarchical Block Structure for High-Throughput Consensus
//!
//! Architecture:
//! Block (2 seconds) - Main consensus unit, BFT finality
//! ├── Subblock 1 (200ms) - Parallel execution unit
//! │   ├── Miniblock 1 (20ms) - Transaction batch (100-500 txs)
//! │   ├── Miniblock 2 (20ms) - Transaction batch (100-500 txs)
//! │   └── ... (10 miniblocks per subblock)
//! ├── Subblock 2 (200ms)
//! │   └── ... (10 miniblocks)
//! └── ... (10 subblocks per block)
//!
//! Throughput Calculation:
//! - 1 miniblock = 250 transactions (average)
//! - 10 miniblocks per subblock = 2,500 transactions
//! - 10 subblocks per block = 25,000 transactions
//! - 1 block per 2 seconds = **12,500 TPS base**
//! - With parallel processing (4x) = **50,000 TPS**
//! - With SIMD optimizations (1.5x) = **75,000 TPS**

mod core_types;
mod data_availability;
mod erasure_coding;
mod execution;
mod fraud_proofs;
mod lane_sharding;
mod metrics;
mod signature_aggregation;
mod subblock_certificates;

// Re-export core types (includes ExecutionTransaction, Address, Hash, etc.)
pub use core_types::*;
pub use data_availability::*;
pub use erasure_coding::*;
// Execution module exports
pub use execution::{
    AccountState, ExecutionContext, ExecutionEngine, StateTransition, Transaction, WorldState,
    WorldStateSnapshot, BASE_TX_GAS, GAS_PER_BYTE, MAX_GAS_PER_MINIBLOCK,
};
pub use fraud_proofs::*;
pub use lane_sharding::*;
pub use metrics::*;
pub use signature_aggregation::*;
pub use subblock_certificates::*;
