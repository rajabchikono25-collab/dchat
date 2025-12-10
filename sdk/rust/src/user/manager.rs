//! User management for creating users and handling profiles

use crate::blockchain::{BlockchainClient, ChannelVisibility, Result};
use crate::crypto::{hash_content, KeyPair};
use crate::user::models::{
    CreateChannelResponse, CreateUserResponse,
    DirectMessageResponse,
};
use chrono::Utc;
use uuid::Uuid;

/// User manager for user operations
pub struct UserManager {
    blockchain: BlockchainClient,
    /// Base URL for user management API
    pub base_url: String,
}

impl UserManager {
    /// Create a new user manager
    pub fn new(blockchain: BlockchainClient, base_url: String) -> Self {
        Self {
            blockchain,
            base_url,
        }
    }

    /// Create a new user with blockchain registration
    pub async fn create_user(&self, username: &str) -> Result<CreateUserResponse> {
        // Generate unique user ID
        let user_id = Uuid::new_v4().to_string();

        // Generate Ed25519 key pair
        let keypair = KeyPair::generate();

        // Submit blockchain transaction
        let tx_id = self
            .blockchain
            .register_user(user_id.clone(), username.to_string(), keypair.public_key_hex())
            .await?;

        // Wait for blockchain confirmation
        let receipt = self.blockchain.wait_for_confirmation(&tx_id).await?;
        let on_chain_confirmed = receipt.success;

        // Return response with actual blockchain status
        Ok(CreateUserResponse {
            user_id,
            username: username.to_string(),
            public_key: keypair.public_key_hex(),
            private_key: keypair.private_key_hex(),
            created_at: Utc::now(),
            on_chain_confirmed,
            tx_id: Some(tx_id),
        })
    }

    /// Send a direct message
    pub async fn send_direct_message(
        &self,
        sender_id: &str,
        recipient_id: &str,
        content: &str,
        relay_node_id: Option<String>,
    ) -> Result<DirectMessageResponse> {
        // Generate message ID
        let message_id = Uuid::new_v4().to_string();

        // Hash the content
        let content_hash = hash_content(content);

        // Submit blockchain transaction
        let tx_id = self
            .blockchain
            .send_direct_message(
                message_id.clone(),
                sender_id.to_string(),
                recipient_id.to_string(),
                content_hash.clone(),
                content.len(),
                relay_node_id,
            )
            .await?;

        // Wait for confirmation
        let receipt = self.blockchain.wait_for_confirmation(&tx_id).await?;
        let on_chain_confirmed = receipt.success;

        Ok(DirectMessageResponse {
            message_id,
            sender_id: sender_id.to_string(),
            recipient_id: recipient_id.to_string(),
            content_hash,
            created_at: Utc::now(),
            on_chain_confirmed,
            tx_id: Some(tx_id),
        })
    }

    /// Create a new channel
    pub async fn create_channel(
        &self,
        creator_id: &str,
        channel_name: &str,
        description: Option<&str>,
    ) -> Result<CreateChannelResponse> {
        // Generate channel ID
        let channel_id = Uuid::new_v4().to_string();

        // Submit blockchain transaction
        let tx_id = self
            .blockchain
            .create_channel(
                channel_id.clone(),
                channel_name.to_string(),
                description.unwrap_or("").to_string(),
                creator_id.to_string(),
                ChannelVisibility::Public,
                None,
            )
            .await?;

        // Wait for confirmation
        let receipt = self.blockchain.wait_for_confirmation(&tx_id).await?;
        let on_chain_confirmed = receipt.success;

        Ok(CreateChannelResponse {
            channel_id,
            name: channel_name.to_string(),
            description: description.map(String::from),
            creator_id: creator_id.to_string(),
            created_at: Utc::now(),
            on_chain_confirmed,
            tx_id: Some(tx_id),
        })
    }

    /// Post a message to a channel
    pub async fn post_to_channel(
        &self,
        sender_id: &str,
        channel_id: &str,
        content: &str,
    ) -> Result<DirectMessageResponse> {
        // Generate message ID
        let message_id = Uuid::new_v4().to_string();

        // Hash the content
        let content_hash = hash_content(content);

        // Submit blockchain transaction
        let tx_id = self
            .blockchain
            .post_to_channel(
                message_id.clone(),
                channel_id.to_string(),
                sender_id.to_string(),
                content_hash.clone(),
                content.len(),
            )
            .await?;

        // Wait for confirmation
        let receipt = self.blockchain.wait_for_confirmation(&tx_id).await?;
        let on_chain_confirmed = receipt.success;

        Ok(DirectMessageResponse {
            message_id,
            sender_id: sender_id.to_string(),
            recipient_id: channel_id.to_string(),
            content_hash,
            created_at: Utc::now(),
            on_chain_confirmed,
            tx_id: Some(tx_id),
        })
    }
}
