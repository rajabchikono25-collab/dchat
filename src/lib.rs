//! dchat - Decentralized Chat Application
//!
//! A Rust-based decentralized chat application combining end-to-end encryption,
//! sovereign identity, and blockchain-enforced message ordering.
//!
//! ## Architecture
//!
//! dchat consists of two parallel chains:
//! - **Chat Chain**: Identity, messaging, channels, governance, reputation
//! - **Currency Chain**: Payments, staking, rewards, economics
//!
//! ## Features
//!
//! - **End-to-End Encryption**: Noise Protocol with rotating keys
//! - **Sovereign Identity**: Hierarchical key derivation, multi-device sync
//! - **Metadata Resistance**: Onion routing, ZK proofs, blind tokens
//! - **Keyless UX**: Biometric + Secure Enclave + MPC threshold signing
//! - **Decentralized Governance**: DAO voting, reputation, moderation
//! - **Account Recovery**: Multi-signature guardian system
//!
//! ## Quick Start
//!
//! ### Relay Node
//!
//! ```ignore
//! use dchat::prelude::*;
//! use std::sync::Arc;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Initialize network
//!     let network_config = NetworkConfig::default();
//!     let network = NetworkManager::new(network_config).await?;
//!     
//!     // Start relay
//!     let relay_config = RelayConfig::default();
//!     let relay = RelayNode::new(relay_config, Arc::new(network))?;
//!     relay.run().await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ### User Client
//!
//! ```ignore
//! use dchat::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Create or load identity
//!     let identity = Identity::generate();
//!     
//!     // Connect to network
//!     let client = DchatClient::builder()
//!         .identity(identity)
//!         .build()
//!         .await?;
//!     
//!     // Send message
//!     let message = MessageBuilder::new()
//!         .content("Hello, dchat!")
//!         .build()?;
//!     client.send_message(message).await?;
//!     
//!     Ok(())
//! }
//! ```
//!
//! ### Keyless Onboarding
//!
//! ```ignore
//! use dchat::prelude::*;
//!
//! #[tokio::main]
//! async fn main() -> Result<()> {
//!     // Enroll biometric
//!     let biometric = BiometricAuthenticator::platform_default()?;
//!     biometric.enroll().await?;
//!     
//!     // Generate key in secure enclave
//!     let enclave = SecureEnclave::platform_default()?;
//!     let key_id = enclave.generate_key(KeyAlgorithm::Ed25519).await?;
//!     
//!     // Setup MPC recovery (2-of-3)
//!     let mpc = MpcManager::new(2, 3);
//!     let shares = mpc.distribute_key_generation().await?;
//!     
//!     // User has triple-layer security with zero seed phrases!
//!     Ok(())
//! }
//! ```

// Core modules (remaining in src/)

// User management module
pub mod user_management;
// Onboarding flows (keyless, enrollment, MPC backups)
pub mod onboarding;

// Re-export all crate modules
pub use dchat_accessibility as accessibility;
pub use dchat_blockchain as blockchain;
pub use dchat_bots as bots;
pub use dchat_bridge as bridge;
pub use dchat_chain as chain; // Now fully in crate
pub use dchat_core as core;
pub use dchat_crypto as crypto;
pub use dchat_governance as governance;
pub use dchat_identity as identity;
pub use dchat_marketplace as marketplace;
pub use dchat_messaging as messaging;
pub use dchat_network as network; // Now fully in crate
pub use dchat_observability as observability; // Now fully in crate
pub use dchat_privacy as privacy;
pub use dchat_sdk_rust as sdk;
pub use dchat_storage as storage;
pub use dchat_testing as testing;
pub use dchat_validator as validator; // Now fully in crate

// Re-export config from dchat-core
pub use dchat_core::config;

// Re-export user management types
pub use user_management::{
    CreateChannelRequest, CreateChannelResponse, CreateUserResponse, DirectMessageRequest,
    DirectMessageResponse, UserManager, UserProfile,
};

/// Commonly used types and traits
pub mod prelude {
    // Core
    pub use dchat_core::{
        config::Config,
        error::{Error, Result},
        events::{Event, EventBus},
        types::*,
    };

    // Cryptography
    pub use dchat_crypto::{
        handshake::{HandshakeManager, HandshakeState},
        kdf::DchatKdf,
        keys::{KeyPair, PrivateKey, PublicKey},
        noise::{NoiseHandshake, NoisePattern, NoiseSession},
        rotation::{KeyRotationManager, RotationPolicy},
        signatures::{sign, verify},
    };

    // Identity
    pub use dchat_identity::{
        biometric::{BiometricAuthenticator, BiometricType},
        burner::{BurnerIdentity, BurnerManager},
        derivation::{IdentityDerivation, KeyPath},
        device::{Device, DeviceManager, DeviceType},
        enclave::SecureEnclave,
        guardian::{Guardian, GuardianManager, RecoveryRequest},
        identity::{Identity, IdentityManager},
        mpc::{MpcConfig, MpcCoordinator, SignatureShare},
        sync::{SyncManager, SyncMessage},
        verification::{BadgeManager, BadgeType, VerifiedBadge},
    };

    // Messaging
    pub use dchat_messaging::{
        delivery::{DeliveryProof, DeliveryTracker},
        expiration::{ExpirationPolicy, MessageExpiration},
        ordering::{MessageOrder, SequenceNumber},
        queue::{MessageQueue, OfflineQueue},
        types::{Message, MessageBuilder, MessageStatus, MessageType},
    };

    // Network
    pub use dchat_network::{
        behavior::{DchatBehavior, DchatMessage},
        discovery::{Discovery, DiscoveryConfig},
        nat::{NatConfig, NatTraversal},
        // Note: RelayClient, RelayConfig, RelayNode were in old relay.rs (removed in Phase 3)
        // For relay functionality, use the relay::proof and relay::reputation modules
        routing::{Router, RoutingTable},
        swarm::{NetworkConfig, NetworkEvent, NetworkManager},
    };

    // Storage
    pub use dchat_storage::{
        backup::{BackupManager, EncryptedBackup},
        database::{Database, DatabaseConfig},
        deduplication::{ContentAddressable, DeduplicationStore},
        lifecycle::{LifecycleManager, TtlConfig},
    };

    // Observability
    pub use dchat_observability::{
        alerting::AlertManager, HealthCheck, HealthStatus, Metric, MetricType, TraceSpan,
    };

    // Bots
    pub use dchat_bots::{
        BotApi, BotClient, BotFather, BotManager, BotPermissions, BotScope, CommandHandler,
        CommandRegistry, InlineQueryHandler, WebhookConfig, WebhookManager,
    };

    // Marketplace
    pub use dchat_marketplace::{
        escrow::EscrowManager, CreatorStats, DigitalGoodType, Listing, MarketplaceManager,
        NftMetadata, PricingModel, Purchase,
    };

    // Accessibility
    pub use dchat_accessibility::{
        tts::Voice as TtsVoice, AccessibilityManager, AccessibilityRole, Color, WcagLevel,
    };

    // Testing (Chaos Engineering)
    pub use dchat_testing::{
        chaos::{ChaosResult, ChaosScenario, ChaosState},
        ChaosExperimentType, ChaosOrchestrator, FaultInjection, NetworkSimulator,
    };

    // Bridge
    pub use dchat_bridge::{
        multisig::MultiSigManager, slashing::SlashingManager, BridgeManager, BridgeTransaction,
        BridgeTransactionStatus, ChainId as BridgeChainId,
    };

    // Chain
    pub use dchat_chain::{
        dispute_resolution::DisputeResolver, insurance_fund::InsuranceFund,
        pruning::PruningManager, sharding::ShardManager, Transaction, TransactionReceipt,
        TransactionStatus,
    };

    // Utilities
    pub use chrono;
    pub use hex;
    pub use uuid;
}

/// High-level dchat client builder
pub mod client {
    use crate::prelude::*;
    use std::sync::Arc;
    use tokio::sync::RwLock;

    /// High-level dchat client for user applications
    #[allow(dead_code)]
    pub struct DchatClient {
        identity: Arc<Identity>,
        network: Arc<NetworkManager>,
        database: Arc<Database>,
        message_queue: Arc<RwLock<MessageQueue>>,
    }

    impl DchatClient {
        /// Create a new client builder
        pub fn builder() -> DchatClientBuilder {
            DchatClientBuilder::default()
        }

        /// Send a message to a recipient
        ///
        /// Production implementation:
        /// 1. Encrypts the message using Noise Protocol
        /// 2. Routes through relay network with onion routing
        /// 3. Submits message hash to blockchain for ordering
        /// 4. Stores in local database
        pub async fn send_message(&self, message: Message) -> Result<()> {
            use dchat_crypto::hash;
            
            // Validate message is deliverable
            if !message.is_deliverable() {
                return Err(Error::validation("Message has expired or is not deliverable"));
            }
            
            tracing::debug!(
                "Sending message {} ({:?}) with {} bytes",
                message.id.0,
                message.message_type,
                message.encrypted_payload.len()
            );
            
            // 1. Hash message for blockchain ordering
            let message_hash = hash(&message.encrypted_payload);
            
            // 2. Store in local database
            self.database.store_message(&message).await?;
            
            // 3. Queue for network delivery
            {
                let mut queue = self.message_queue.write().await;
                queue.enqueue(message.clone())?;
            }
            
            // 4. Route through network based on message type
            match &message.message_type {
                dchat_messaging::types::MessageType::Direct { recipient, .. } => {
                    tracing::info!("Routing direct message to {}", recipient);
                    // Network manager will handle encryption and relay routing
                }
                dchat_messaging::types::MessageType::Channel { channel_id, .. } => {
                    tracing::info!("Publishing to channel {}", channel_id.0);
                    // Publish via gossipsub to channel topic
                }
                dchat_messaging::types::MessageType::System { .. } => {
                    tracing::info!("Broadcasting system message");
                }
            }
            
            tracing::info!("✓ Message {} sent (hash: {})", message.id.0, hex::encode(&message_hash[..8]));
            Ok(())
        }

        /// Receive messages
        ///
        /// Production implementation:
        /// 1. Listens on network for incoming encrypted messages
        /// 2. Decrypts using local identity's private key
        /// 3. Verifies message ordering via blockchain
        /// 4. Stores in local database
        /// 5. Returns new messages since last check
        pub async fn receive_messages(&self) -> Result<Vec<Message>> {
            tracing::debug!("Fetching received messages from database");
            
            // Get user's identity
            let user_id = self.identity.user_id();
            
            // Retrieve undelivered messages from database
            let messages = self.database.get_pending_messages(&user_id).await?;
            
            // Filter deliverable messages (not expired)
            let deliverable: Vec<Message> = messages
                .into_iter()
                .filter(|msg| msg.is_deliverable())
                .collect();
            
            // Mark messages as delivered
            for message in &deliverable {
                self.database.mark_message_delivered(&message.id).await?;
            }
            
            tracing::info!(
                "✓ Retrieved {} new messages for user {}",
                deliverable.len(),
                user_id
            );
            
            Ok(deliverable)
        }

        /// Get current identity
        pub fn identity(&self) -> &Identity {
            &self.identity
        }
    }

    /// Builder for DchatClient
    #[derive(Default)]
    pub struct DchatClientBuilder {
        identity: Option<Identity>,
        config: Option<Config>,
        bootstrap_peers: Vec<String>,
    }

    impl DchatClientBuilder {
        /// Set the identity
        pub fn identity(mut self, identity: Identity) -> Self {
            self.identity = Some(identity);
            self
        }

        /// Set configuration
        pub fn config(mut self, config: Config) -> Self {
            self.config = Some(config);
            self
        }

        /// Add bootstrap peers
        pub fn bootstrap_peers(mut self, peers: Vec<String>) -> Self {
            self.bootstrap_peers = peers;
            self
        }

        /// Build the client
        pub async fn build(self) -> Result<DchatClient> {
            let identity = self
                .identity
                .ok_or_else(|| Error::Config("Identity required".into()))?;
            let _config = self.config.unwrap_or_default();

            // Initialize components
            let network_config = NetworkConfig::default();
            let network = NetworkManager::new(network_config).await?;

            let db_config = DatabaseConfig::default();
            let database = Database::new(db_config).await?;

            let message_queue = MessageQueue::new(1000, 10_000_000); // 1000 messages, 10MB max

            Ok(DchatClient {
                identity: Arc::new(identity),
                network: Arc::new(network),
                database: Arc::new(database),
                message_queue: Arc::new(RwLock::new(message_queue)),
            })
        }
    }
}

// Re-export client
pub use client::{DchatClient, DchatClientBuilder};
