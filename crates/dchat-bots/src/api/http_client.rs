//! HTTP client for Bot API
//! 
//! Provides HTTP-based communication with the dchat Bot API,
//! including automatic retries, rate limiting, and error handling.

use crate::{
    BotMessage, SendMessageRequest, EditMessageRequest, DeleteMessageRequest,
    AnswerCallbackQueryRequest,
};
use dchat_core::{Error, Result};
use reqwest::{Client, StatusCode};
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tracing::{debug, error, info, warn};
use uuid::Uuid;

/// HTTP client for Bot API with automatic retries and error handling
pub struct BotHttpClient {
    client: Client,
    base_url: String,
    token: String,
    max_retries: u32,
}

/// API response wrapper
#[derive(Debug, Clone, Serialize, Deserialize)]
struct ApiResponse<T> {
    success: bool,
    data: Option<T>,
    error: Option<String>,
}

/// Message response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct MessageResponse {
    message_id: Uuid,
}

/// Updates response
#[derive(Debug, Clone, Serialize, Deserialize)]
struct UpdatesResponse {
    updates: Vec<BotMessage>,
    offset: i64,
}

/// Webhook info
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebhookInfo {
    pub url: String,
    pub pending_update_count: u32,
    pub last_error_date: Option<i64>,
    pub last_error_message: Option<String>,
}

impl BotHttpClient {
    /// Create a new HTTP client for Bot API
    pub fn new(token: String) -> Result<Self> {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .connect_timeout(Duration::from_secs(10))
            .pool_max_idle_per_host(10)
            .user_agent("dchat-bot-sdk/0.1.0")
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;
        
        Ok(Self {
            client,
            base_url: "https://api.dchat.network".to_string(),
            token,
            max_retries: 3,
        })
    }
    
    /// Create with custom base URL
    pub fn with_base_url(mut self, base_url: String) -> Self {
        self.base_url = base_url;
        self
    }
    
    /// Set maximum retry attempts
    pub fn with_max_retries(mut self, max_retries: u32) -> Self {
        self.max_retries = max_retries;
        self
    }
    
    /// Send a text message
    pub async fn send_message(&self, request: SendMessageRequest) -> Result<Uuid> {
        debug!("Sending message to chat {}", request.chat_id);
        
        let url = format!("{}/bot{}/sendMessage", self.base_url, self.token);
        
        let response = self.post_with_retry(&url, &request).await?;
        let api_response: ApiResponse<MessageResponse> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            api_response.data
                .map(|d| d.message_id)
                .ok_or_else(|| Error::network("No message ID in response"))
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Edit an existing message
    pub async fn edit_message(&self, request: EditMessageRequest) -> Result<()> {
        debug!("Editing message {} in chat {}", request.message_id, request.chat_id);
        
        let url = format!("{}/bot{}/editMessage", self.base_url, self.token);
        
        let response = self.post_with_retry(&url, &request).await?;
        let api_response: ApiResponse<()> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            Ok(())
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Delete a message
    pub async fn delete_message(&self, request: DeleteMessageRequest) -> Result<()> {
        debug!("Deleting message {} from chat {}", request.message_id, request.chat_id);
        
        let url = format!("{}/bot{}/deleteMessage", self.base_url, self.token);
        
        let response = self.post_with_retry(&url, &request).await?;
        let api_response: ApiResponse<()> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            Ok(())
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Answer callback query
    pub async fn answer_callback_query(&self, request: AnswerCallbackQueryRequest) -> Result<()> {
        debug!("Answering callback query {}", request.callback_query_id);
        
        let url = format!("{}/bot{}/answerCallbackQuery", self.base_url, self.token);
        
        let response = self.post_with_retry(&url, &request).await?;
        let api_response: ApiResponse<()> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            Ok(())
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Get updates using long polling
    pub async fn get_updates(&self, offset: Option<i64>, timeout: Option<u32>) -> Result<Vec<BotMessage>> {
        let timeout_secs = timeout.unwrap_or(30);
        debug!("Getting updates with offset {:?}, timeout {}s", offset, timeout_secs);
        
        let mut url = format!("{}/bot{}/getUpdates?timeout={}", self.base_url, self.token, timeout_secs);
        if let Some(offset) = offset {
            url.push_str(&format!("&offset={}", offset));
        }
        
        // Don't retry get_updates - it uses long polling
        let response = self.client
            .get(&url)
            .timeout(Duration::from_secs(timeout_secs as u64 + 10))
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to get updates: {}", e)))?;
        
        let api_response: ApiResponse<UpdatesResponse> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse updates: {}", e)))?;
        
        if api_response.success {
            Ok(api_response.data.map(|d| d.updates).unwrap_or_default())
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Set webhook URL for receiving updates
    pub async fn set_webhook(&self, url: String) -> Result<()> {
        info!("Setting webhook URL: {}", url);
        
        #[derive(Serialize)]
        struct SetWebhookRequest {
            url: String,
        }
        
        let api_url = format!("{}/bot{}/setWebhook", self.base_url, self.token);
        let request = SetWebhookRequest { url };
        
        let response = self.post_with_retry(&api_url, &request).await?;
        let api_response: ApiResponse<()> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            info!("Webhook successfully set");
            Ok(())
        } else {
            Err(Error::network(format!("Failed to set webhook: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Delete webhook
    pub async fn delete_webhook(&self) -> Result<()> {
        info!("Deleting webhook");
        
        let url = format!("{}/bot{}/deleteWebhook", self.base_url, self.token);
        
        let response = self.post_with_retry(&url, &()).await?;
        let api_response: ApiResponse<()> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse response: {}", e)))?;
        
        if api_response.success {
            info!("Webhook successfully deleted");
            Ok(())
        } else {
            Err(Error::network(format!("Failed to delete webhook: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// Get webhook info
    pub async fn get_webhook_info(&self) -> Result<WebhookInfo> {
        debug!("Getting webhook info");
        
        let url = format!("{}/bot{}/getWebhookInfo", self.base_url, self.token);
        
        let response = self.client
            .get(&url)
            .send()
            .await
            .map_err(|e| Error::network(format!("Failed to get webhook info: {}", e)))?;
        
        let api_response: ApiResponse<WebhookInfo> = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse webhook info: {}", e)))?;
        
        if api_response.success {
            api_response.data
                .ok_or_else(|| Error::network("No webhook info in response"))
        } else {
            Err(Error::network(format!("API error: {}", 
                api_response.error.unwrap_or_else(|| "Unknown error".to_string()))))
        }
    }
    
    /// POST request with automatic retry and exponential backoff
    async fn post_with_retry<T: Serialize>(&self, url: &str, body: &T) -> Result<reqwest::Response> {
        let mut attempts = 0;
        let mut delay = Duration::from_millis(100);
        
        loop {
            attempts += 1;
            
            match self.client
                .post(url)
                .json(body)
                .send()
                .await
            {
                Ok(response) => {
                    let status = response.status();
                    
                    // Success
                    if status.is_success() {
                        return Ok(response);
                    }
                    
                    // Rate limit - always retry after delay
                    if status == StatusCode::TOO_MANY_REQUESTS {
                        if let Some(retry_after) = response.headers().get("retry-after") {
                            if let Ok(retry_str) = retry_after.to_str() {
                                if let Ok(retry_secs) = retry_str.parse::<u64>() {
                                    delay = Duration::from_secs(retry_secs);
                                }
                            }
                        }
                        
                        if attempts >= self.max_retries {
                            return Err(Error::network("Rate limit exceeded, max retries reached"));
                        }
                        
                        warn!("Rate limited, retrying after {:?} (attempt {}/{})", delay, attempts, self.max_retries);
                        tokio::time::sleep(delay).await;
                        delay *= 2; // Exponential backoff
                        continue;
                    }
                    
                    // Server error - retry
                    if status.is_server_error() && attempts < self.max_retries {
                        warn!("Server error {}, retrying (attempt {}/{})", status, attempts, self.max_retries);
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    
                    // Client error or max retries - don't retry
                    return Err(Error::network(format!("HTTP error: {}", status)));
                }
                Err(e) => {
                    // Network error - retry
                    if attempts < self.max_retries {
                        warn!("Network error: {}, retrying (attempt {}/{})", e, attempts, self.max_retries);
                        tokio::time::sleep(delay).await;
                        delay *= 2;
                        continue;
                    }
                    
                    return Err(Error::network(format!("Network error after {} attempts: {}", attempts, e)));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_client_creation() {
        let client = BotHttpClient::new("test_token".to_string()).unwrap();
        assert_eq!(client.base_url, "https://api.dchat.network");
        assert_eq!(client.max_retries, 3);
    }
    
    #[test]
    fn test_custom_base_url() {
        let client = BotHttpClient::new("test_token".to_string())
            .unwrap()
            .with_base_url("https://custom.api".to_string())
            .with_max_retries(5);
        
        assert_eq!(client.base_url, "https://custom.api");
        assert_eq!(client.max_retries, 5);
    }
}
