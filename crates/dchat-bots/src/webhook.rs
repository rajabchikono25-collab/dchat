//! Webhook management for bots

use crate::{Bot, BotMessage, CallbackQuery};
use chrono::{DateTime, Utc};
use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};

/// Webhook configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookConfig {
    /// Webhook URL
    pub url: String,

    /// Secret token for verification
    pub secret_token: Option<String>,

    /// Maximum allowed connections
    pub max_connections: u32,

    /// Allowed updates (empty = all)
    pub allowed_updates: Vec<UpdateType>,

    /// Drop pending updates on set
    pub drop_pending_updates: bool,
}

/// Type of update
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum UpdateType {
    Message,
    EditedMessage,
    CallbackQuery,
    InlineQuery,
    ChannelPost,
    EditedChannelPost,
}

/// Webhook update
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookUpdate {
    /// Update ID
    pub update_id: i64,

    /// Update type
    pub update_type: UpdateType,

    /// Message (if update_type = Message)
    pub message: Option<BotMessage>,

    /// Edited message (if update_type = EditedMessage)
    pub edited_message: Option<BotMessage>,

    /// Callback query (if update_type = CallbackQuery)
    pub callback_query: Option<CallbackQuery>,

    /// Timestamp
    pub timestamp: DateTime<Utc>,
}

/// Webhook delivery result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookDeliveryResult {
    /// Was delivery successful?
    pub success: bool,

    /// HTTP status code
    pub status_code: Option<u16>,

    /// Response body
    pub response_body: Option<String>,

    /// Error message (if failed)
    pub error: Option<String>,

    /// Delivery timestamp
    pub timestamp: DateTime<Utc>,
}

/// Webhook manager
pub struct WebhookManager {
    http_client: reqwest::Client,
}

impl WebhookManager {
    /// Create a new webhook manager
    pub fn new() -> Self {
        Self {
            http_client: reqwest::Client::builder()
                .timeout(std::time::Duration::from_secs(30))
                .build()
                .unwrap(),
        }
    }

    /// Set webhook for a bot
    pub async fn set_webhook(&self, bot: &mut Bot, config: WebhookConfig) -> Result<()> {
        // Validate URL
        if !config.url.starts_with("https://") {
            return Err(Error::validation("Webhook URL must use HTTPS"));
        }

        // Test webhook URL
        match self.test_webhook(&config.url).await {
            Ok(_) => {
                bot.webhook_url = Some(config.url);
                Ok(())
            }
            Err(e) => Err(Error::network(format!(
                "Failed to reach webhook URL: {}",
                e
            ))),
        }
    }

    /// Delete webhook
    pub async fn delete_webhook(&self, bot: &mut Bot) -> Result<()> {
        bot.webhook_url = None;
        Ok(())
    }

    /// Get webhook info
    pub fn get_webhook_info(&self, bot: &Bot) -> Option<String> {
        bot.webhook_url.clone()
    }

    /// Send update to webhook
    pub async fn send_update(
        &self,
        webhook_url: &str,
        update: WebhookUpdate,
        secret_token: Option<&str>,
    ) -> Result<WebhookDeliveryResult> {
        let mut request = self.http_client.post(webhook_url).json(&update);

        // Add secret token header if provided
        if let Some(token) = secret_token {
            request = request.header("X-Dchat-Bot-Api-Secret-Token", token);
        }

        // Add signature header
        let signature = self.compute_signature(&update, secret_token);
        request = request.header("X-Dchat-Signature", signature);

        match request.send().await {
            Ok(response) => {
                let status = response.status();
                let body = response.text().await.ok();

                Ok(WebhookDeliveryResult {
                    success: status.is_success(),
                    status_code: Some(status.as_u16()),
                    response_body: body,
                    error: None,
                    timestamp: Utc::now(),
                })
            }
            Err(e) => Ok(WebhookDeliveryResult {
                success: false,
                status_code: None,
                response_body: None,
                error: Some(e.to_string()),
                timestamp: Utc::now(),
            }),
        }
    }

    /// Test webhook URL
    async fn test_webhook(&self, url: &str) -> Result<()> {
        let test_update = WebhookUpdate {
            update_id: 0,
            update_type: UpdateType::Message,
            message: None,
            edited_message: None,
            callback_query: None,
            timestamp: Utc::now(),
        };

        let response = self
            .http_client
            .post(url)
            .json(&test_update)
            .send()
            .await
            .map_err(|e| Error::network(format!("Webhook test failed: {}", e)))?;

        if response.status().is_success() {
            Ok(())
        } else {
            Err(Error::network(format!(
                "Webhook returned status: {}",
                response.status()
            )))
        }
    }

    /// Compute HMAC signature for webhook payload
    fn compute_signature(&self, update: &WebhookUpdate, secret: Option<&str>) -> String {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        let secret = secret.unwrap_or("dchat_default_secret");
        let payload = serde_json::to_string(update).unwrap();

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload.as_bytes());
        let result = mac.finalize();

        format!("sha256={}", hex::encode(result.into_bytes()))
    }

    /// Verify webhook signature
    pub fn verify_signature(&self, payload: &[u8], signature: &str, secret: &str) -> bool {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret.as_bytes()).unwrap();
        mac.update(payload);
        let result = mac.finalize();

        let expected = format!("sha256={}", hex::encode(result.into_bytes()));
        expected == signature
    }
}

impl Default for WebhookManager {
    fn default() -> Self {
        Self::new()
    }
}

impl Default for WebhookConfig {
    fn default() -> Self {
        Self {
            url: String::new(),
            secret_token: None,
            max_connections: 40,
            allowed_updates: Vec::new(),
            drop_pending_updates: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[allow(unused_imports)]
    use crate::BotFather;
    #[allow(unused_imports)]
    use dchat_core::types::UserId;

    #[tokio::test]
    async fn test_webhook_config() {
        let config = WebhookConfig {
            url: "https://example.com/webhook".to_string(),
            secret_token: Some("my_secret".to_string()),
            max_connections: 40,
            allowed_updates: vec![UpdateType::Message, UpdateType::CallbackQuery],
            drop_pending_updates: false,
        };

        assert_eq!(config.url, "https://example.com/webhook");
        assert_eq!(config.max_connections, 40);
    }

    #[test]
    fn test_compute_signature() {
        let manager = WebhookManager::new();

        let update = WebhookUpdate {
            update_id: 1,
            update_type: UpdateType::Message,
            message: None,
            edited_message: None,
            callback_query: None,
            timestamp: Utc::now(),
        };

        let signature = manager.compute_signature(&update, Some("test_secret"));
        assert!(signature.starts_with("sha256="));
    }

    #[test]
    fn test_verify_signature() {
        let manager = WebhookManager::new();
        let payload = b"test payload";

        use hmac::{Hmac, Mac};
        use sha2::Sha256;
        type HmacSha256 = Hmac<Sha256>;

        let mut mac = HmacSha256::new_from_slice(b"test_secret").unwrap();
        mac.update(payload);
        let result = mac.finalize();
        let signature = format!("sha256={}", hex::encode(result.into_bytes()));

        assert!(manager.verify_signature(payload, &signature, "test_secret"));
        assert!(!manager.verify_signature(payload, &signature, "wrong_secret"));
    }
}
