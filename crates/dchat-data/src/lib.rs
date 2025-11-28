//! dchat-data: Shared data models and utilities
//!
//! This crate provides core data types and utilities used across dchat crates:
//!
//! - **TTL (Time-To-Live)**: Message expiration policies and lifecycle management
//! - **Content Addressing**: BLAKE3-based content-addressable identifiers
//! - **Deduplication**: Content deduplication utilities with delta encoding support
//! - **Serialization**: Efficient binary serialization for network and storage
//! - **Data Integrity**: Hash verification and corruption detection

pub mod content_id;
pub mod dedup;
pub mod error;
pub mod serialization;
pub mod ttl;

pub use content_id::{ContentId, ContentIdError};
pub use dedup::{DedupEntry, DedupStore, DeltaEncoded};
pub use error::{DataError, DataResult};
pub use serialization::{DataFormat, Serializable};
pub use ttl::{TtlConfig, TtlPolicy, ExpirationTime};
