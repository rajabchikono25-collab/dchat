//! dchat Rust SDK
//!
//! High-level API for building decentralized chat applications.
//!
//! # Quick Start
//!
//! ```no_run
//! use dchat_sdk_rust::{Client, ClientConfig};
//!
//! #[tokio::main]
//! async fn main() -> anyhow::Result<()> {
//!     let client = Client::builder()
//!         .name("Alice")
//!         .build()
//!         .await?;
//!
//!     client.send_message("Hello, dchat!").await?;
//!     Ok(())
//! }
//! ```

pub mod client;
pub mod config;
pub mod error;
pub mod network;
pub mod relay;

pub use client::{Client, ClientBuilder};
pub use config::{ClientConfig, NetworkConfig, StorageConfig};
pub use error::{Result, SdkError};
pub use network::{NetworkEvent, NetworkManager};
pub use relay::{RelayConfig, RelayNode};

use dchat_crypto::keys::KeyPair;
use dchat_identity::Identity;

/// SDK version
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// Initialize the SDK with default configuration
pub async fn init() -> Result<()> {
    // Set up logging, tracing, etc.
    Ok(())
}

/// Generate a new identity keypair
pub fn generate_keypair() -> KeyPair {
    #[allow(deprecated)]
    KeyPair::generate()
}

/// Create an identity from a keypair
pub fn create_identity(name: String, keypair: &KeyPair) -> Identity {
    Identity::new(name, keypair)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_version() {
        assert!(!VERSION.is_empty());
    }

    #[test]
    fn test_generate_keypair() {
        let keypair = generate_keypair();
        assert_eq!(keypair.public_key().as_bytes().len(), 32);
    }

    #[tokio::test]
    async fn test_init() {
        assert!(init().await.is_ok());
    }
}
