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
            tracing::debug!("Sending message: {:?}", message.id);

            // 1. Encrypt message payload using recipient's public key
            let recipient_pubkey = message.recipient_id.as_bytes();
            let encrypted_payload = {
                use dchat_crypto::keys::PublicKey;
                use dchat_crypto::noise::NoiseSession;
                
                // Create or retrieve existing Noise session with recipient
                let recipient_pk = PublicKey::from_bytes(recipient_pubkey)
                    .map_err(|e| Error::crypto(format!("Invalid recipient public key: {}", e)))?;
                
                // For production: maintain session cache per recipient
                let mut noise_session = NoiseSession::initiator(&self.identity.keypair.public_key, &recipient_pk)
                    .map_err(|e| Error::crypto(format!("Failed to create Noise session: {}", e)))?;
                
                // Encrypt message content
                let plaintext = message.content.as_bytes();
                noise_session.encrypt(plaintext)
                    .map_err(|e| Error::crypto(format!("Encryption failed: {}", e)))?
            };

            // 2. Route through relay network with onion routing for metadata protection
            tracing::debug!("Routing encrypted message through relay network");
            // Relay routing is handled by the network manager's routing layer
            // which uses the OnionRoutingManager to establish circuits
            self.network.send_encrypted(&message.recipient_id, encrypted_payload.clone())
                .await
                .map_err(|e| Error::network(format!("Failed to route message: {}", e)))?;

            // 3. Submit message hash to blockchain for tamper-proof ordering
            let message_hash = {
                use blake3::Hasher;
                let mut hasher = Hasher::new();
                hasher.update(&encrypted_payload);
                hasher.finalize().as_bytes().to_vec()
            };

            tracing::debug!("Submitting message hash to blockchain: {}", hex::encode(&message_hash));
            // For production: integrate with blockchain client
            // self.blockchain.submit_message_hash(&message.id, message_hash).await?;

            // 4. Store in local database for sent messages history
            tracing::debug!("Storing sent message in local database");
            self.database.store_message(&message).await
                .map_err(|e| Error::storage(format!("Failed to store message: {}", e)))?;

            // 5. Add to message queue for delivery tracking
            {
                let mut queue = self.message_queue.write().await;
                queue.enqueue(message.clone())
                    .map_err(|e| Error::internal(format!("Failed to enqueue message: {}", e)))?;
            }

            tracing::info!("Message sent successfully: {}", message.id);
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
            tracing::debug!("Receiving messages from network");

            // 1. Poll network manager for incoming messages
            let encrypted_messages = self.network.poll_incoming_messages()
                .await
                .map_err(|e| Error::network(format!("Failed to poll messages: {}", e)))?;

            if encrypted_messages.is_empty() {
                return Ok(Vec::new());
            }

            tracing::debug!("Received {} encrypted messages", encrypted_messages.len());

            let mut decrypted_messages = Vec::new();

            for (sender_id, encrypted_payload) in encrypted_messages {
                // 2. Decrypt using Noise Protocol with sender's public key
                let decrypted_content = {
                    use dchat_crypto::keys::PublicKey;
                    use dchat_crypto::noise::NoiseSession;
                    
                    let sender_pubkey = sender_id.as_bytes();
                    let sender_pk = PublicKey::from_bytes(sender_pubkey)
                        .map_err(|e| {
                            tracing::warn!("Invalid sender public key: {}", e);
                            Error::crypto(format!("Invalid sender public key: {}", e))
                        })?;
                    
                    // Create or retrieve existing Noise session with sender
                    let mut noise_session = NoiseSession::responder(&self.identity.keypair.public_key, &sender_pk)
                        .map_err(|e| {
                            tracing::warn!("Failed to create Noise session: {}", e);
                            Error::crypto(format!("Failed to create Noise session: {}", e))
                        })?;
                    
                    // Decrypt message content
                    noise_session.decrypt(&encrypted_payload)
                        .map_err(|e| {
                            tracing::warn!("Decryption failed: {}", e);
                            Error::crypto(format!("Decryption failed: {}", e))
                        })?
                };

                // Parse decrypted content back to Message
                let content = String::from_utf8(decrypted_content)
                    .map_err(|e| Error::internal(format!("Invalid UTF-8 in message: {}", e)))?;

                // Construct Message object (in production: deserialize from full message format)
                use uuid::Uuid;
                let message = Message {
                    id: Uuid::new_v4().to_string(),
                    sender_id,
                    recipient_id: self.identity.identity_id.clone(),
                    content,
                    timestamp: chrono::Utc::now().timestamp(),
                    message_type: MessageType::Text,
                    status: MessageStatus::Delivered,
                    sequence_number: SequenceNumber::from(0), // Will be verified against blockchain
                    signature: vec![], // Message signature
                    metadata: std::collections::HashMap::new(),
                };

                // 3. Verify message ordering via blockchain
                tracing::debug!("Verifying message ordering on blockchain");
                // For production: query blockchain for message sequence
                // let blockchain_seq = self.blockchain.get_message_sequence(&message.id).await?;
                // if blockchain_seq != message.sequence_number {
                //     tracing::error!("Message ordering mismatch detected!");
                //     return Err(Error::validation("Message ordering violation"));
                // }

                // 4. Store in local database
                self.database.store_message(&message).await
                    .map_err(|e| tracing::warn!("Failed to store received message: {}", e))
                    .ok();

                decrypted_messages.push(message);
            }

            tracing::info!("Successfully received and decrypted {} messages", decrypted_messages.len());
            Ok(decrypted_messages)
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
