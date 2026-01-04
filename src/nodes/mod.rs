//! Node Runners
//!
//! This module contains the extracted node runner implementations for:
//! - Relay nodes (`relay.rs`)
//! - Validator nodes (`validator.rs`)
//! - User/Light Client nodes (`user.rs`)
//!
//! Each module contains the main run loop and initialization logic for its node type,
//! previously located inline in main.rs.
//!
//! ## Common Patterns
//!
//! All node types share common initialization via `NodeContext`:
//! - Peer registry and metrics
//! - Health/readiness endpoints
//! - Graceful shutdown handling
//! - Metrics collection
//!
//! ## Usage
//!
//! ```rust,ignore
//! use dchat::nodes::{NodeContext, run_relay_node, run_validator_node, run_user_node};
//!
//! // Initialize shared context
//! let ctx = NodeContext::new(&config)?;
//!
//! // Run the appropriate node type
//! run_relay_node(ctx, relay_config).await?;
//! ```

mod context;

pub use context::{NodeContext, NodeType, ReadinessState};

// Re-export peer types with node_ prefix to avoid conflicts with dchat_identity::PeerRegistry
pub use context::PeerInfo as NodePeerInfo;
pub use context::PeerMetrics as NodePeerMetrics;
pub use context::PeerRegistry as NodePeerRegistry;

// Node runners will be added as we extract them:
// pub mod relay;
// pub mod validator;
// pub mod user;
