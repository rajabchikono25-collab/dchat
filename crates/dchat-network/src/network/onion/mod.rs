//! Onion Routing Implementation (MVP)
//!
//! Provides metadata-resistant communication through multi-hop routing with layered encryption.
//! Implements Sphinx-style packet format for unlinkability and forward secrecy.

pub mod circuits;
pub mod path_selection;
pub mod sphinx;

pub use circuits::{Circuit, CircuitError, CircuitId, CircuitManager};
pub use path_selection::{PathSelectionError, PathSelector};
pub use sphinx::{SphinxError, SphinxHeader, SphinxPacket};
