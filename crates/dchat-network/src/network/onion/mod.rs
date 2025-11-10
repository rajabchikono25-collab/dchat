//! Onion Routing Implementation (MVP)
//!
//! Provides metadata-resistant communication through multi-hop routing with layered encryption.
//! Implements Sphinx-style packet format for unlinkability and forward secrecy.

pub mod circuits;
pub mod sphinx;
pub mod path_selection;

pub use circuits::{Circuit, CircuitId, CircuitManager, CircuitError};
pub use sphinx::{SphinxPacket, SphinxHeader, SphinxError};
pub use path_selection::{PathSelector, PathSelectionError};
