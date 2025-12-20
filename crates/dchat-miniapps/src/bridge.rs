//! Message bridge - communication between sandbox and host

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::sandbox::{SandboxId, SandboxMessage};

/// Bridge message ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct MessageId(pub u64);

impl MessageId {
    /// Generate new ID
    pub fn new() -> Self {
        static COUNTER: AtomicU64 = AtomicU64::new(1);
        Self(COUNTER.fetch_add(1, Ordering::Relaxed))
    }
}

impl Default for MessageId {
    fn default() -> Self {
        Self::new()
    }
}

/// Bridge message type
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "payload")]
pub enum BridgeMessageType {
    // ─── Request/Response ───────────────────────────────────────────────────
    /// Request from sandbox to host
    Request {
        method: String,
        params: serde_json::Value,
        id: String,
    },
    /// Response from host to sandbox
    Response {
        id: String,
        result: Option<serde_json::Value>,
        error: Option<BridgeError>,
    },

    // ─── Events ─────────────────────────────────────────────────────────────
    /// Event from host to sandbox
    Event {
        name: String,
        data: serde_json::Value,
    },
    /// Notification (no response expected)
    Notification {
        method: String,
        params: serde_json::Value,
    },

    // ─── Control ────────────────────────────────────────────────────────────
    /// Ping (keep-alive)
    Ping { timestamp: u64 },
    /// Pong (keep-alive response)
    Pong { timestamp: u64 },
    /// Close connection
    Close { code: u16, reason: String },
}

/// Bridge error
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeError {
    /// Error code
    pub code: i32,
    /// Error message
    pub message: String,
    /// Additional data
    pub data: Option<serde_json::Value>,
}

impl BridgeError {
    /// Create parse error
    pub fn parse_error(message: &str) -> Self {
        Self {
            code: -32700,
            message: message.to_string(),
            data: None,
        }
    }

    /// Create invalid request error
    pub fn invalid_request(message: &str) -> Self {
        Self {
            code: -32600,
            message: message.to_string(),
            data: None,
        }
    }

    /// Create method not found error
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method),
            data: None,
        }
    }

    /// Create invalid params error
    pub fn invalid_params(message: &str) -> Self {
        Self {
            code: -32602,
            message: message.to_string(),
            data: None,
        }
    }

    /// Create internal error
    pub fn internal_error(message: &str) -> Self {
        Self {
            code: -32603,
            message: message.to_string(),
            data: None,
        }
    }

    /// Create permission denied error
    pub fn permission_denied(permission: &str) -> Self {
        Self {
            code: -32001,
            message: format!("Permission denied: {}", permission),
            data: None,
        }
    }
}

/// Bridge message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BridgeMessage {
    /// Message ID
    pub id: u64,
    /// Sandbox ID
    pub sandbox_id: String,
    /// Message type
    pub message: BridgeMessageType,
    /// Timestamp
    pub timestamp: u64,
}

impl BridgeMessage {
    /// Create new message
    pub fn new(sandbox_id: SandboxId, message: BridgeMessageType) -> Self {
        Self {
            id: MessageId::new().0,
            sandbox_id: sandbox_id.0.to_string(),
            message,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
        }
    }

    /// Serialize to JSON
    pub fn to_json(&self) -> MiniAppResult<String> {
        serde_json::to_string(self).map_err(|e| MiniAppError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> MiniAppResult<Self> {
        serde_json::from_str(json).map_err(|e| MiniAppError::DeserializationError(e.to_string()))
    }
}

/// Message handler trait
pub trait MessageHandler: Send + Sync {
    /// Handle incoming request
    fn handle_request(
        &self,
        sandbox_id: &SandboxId,
        method: &str,
        params: serde_json::Value,
    ) -> MiniAppResult<serde_json::Value>;

    /// Handle incoming notification
    fn handle_notification(
        &self,
        sandbox_id: &SandboxId,
        method: &str,
        params: serde_json::Value,
    ) -> MiniAppResult<()>;
}

/// Pending request tracking
struct PendingRequest {
    /// Request ID
    id: String,
    /// Request method
    method: String,
    /// Sent timestamp
    sent_at: DateTime<Utc>,
    /// Response sender
    response_tx: tokio::sync::oneshot::Sender<Result<serde_json::Value, BridgeError>>,
}

/// Message bridge for sandbox communication
pub struct MessageBridge {
    /// Sandbox ID
    sandbox_id: SandboxId,
    /// Outbound message sender
    outbound_tx: mpsc::Sender<BridgeMessage>,
    /// Pending requests
    pending: RwLock<HashMap<String, PendingRequest>>,
    /// Message handler
    handler: Arc<dyn MessageHandler>,
    /// Connected flag
    connected: RwLock<bool>,
    /// Last activity
    last_activity: RwLock<DateTime<Utc>>,
}

impl MessageBridge {
    /// Create new bridge
    pub fn new(
        sandbox_id: SandboxId,
        handler: Arc<dyn MessageHandler>,
    ) -> (Self, mpsc::Receiver<BridgeMessage>) {
        let (outbound_tx, outbound_rx) = mpsc::channel(100);

        let bridge = Self {
            sandbox_id,
            outbound_tx,
            pending: RwLock::new(HashMap::new()),
            handler,
            connected: RwLock::new(true),
            last_activity: RwLock::new(Utc::now()),
        };

        (bridge, outbound_rx)
    }

    /// Send request and wait for response
    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> MiniAppResult<serde_json::Value> {
        if !*self.connected.read() {
            return Err(MiniAppError::BridgeNotConnected);
        }

        let id = Uuid::new_v4().to_string();
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();

        // Register pending request
        {
            let pending = PendingRequest {
                id: id.clone(),
                method: method.to_string(),
                sent_at: Utc::now(),
                response_tx,
            };
            self.pending.write().insert(id.clone(), pending);
        }

        // Send request
        let message = BridgeMessage::new(
            self.sandbox_id,
            BridgeMessageType::Request {
                method: method.to_string(),
                params,
                id: id.clone(),
            },
        );

        self.outbound_tx
            .send(message)
            .await
            .map_err(|_| MiniAppError::BridgeMessageFailed("send failed".to_string()))?;

        // Wait for response with timeout
        let timeout = tokio::time::Duration::from_secs(30);
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(Ok(result))) => Ok(result),
            Ok(Ok(Err(error))) => Err(MiniAppError::BridgeMessageFailed(error.message)),
            Ok(Err(_)) => Err(MiniAppError::BridgeMessageFailed(
                "response channel closed".to_string(),
            )),
            Err(_) => {
                // Remove pending request
                self.pending.write().remove(&id);
                Err(MiniAppError::BridgeTimeout)
            }
        }
    }

    /// Send notification (no response expected)
    pub async fn notify(&self, method: &str, params: serde_json::Value) -> MiniAppResult<()> {
        if !*self.connected.read() {
            return Err(MiniAppError::BridgeNotConnected);
        }

        let message = BridgeMessage::new(
            self.sandbox_id,
            BridgeMessageType::Notification {
                method: method.to_string(),
                params,
            },
        );

        self.outbound_tx
            .send(message)
            .await
            .map_err(|_| MiniAppError::BridgeMessageFailed("send failed".to_string()))
    }

    /// Send event to sandbox
    pub async fn emit_event(&self, name: &str, data: serde_json::Value) -> MiniAppResult<()> {
        if !*self.connected.read() {
            return Err(MiniAppError::BridgeNotConnected);
        }

        let message = BridgeMessage::new(
            self.sandbox_id,
            BridgeMessageType::Event {
                name: name.to_string(),
                data,
            },
        );

        self.outbound_tx
            .send(message)
            .await
            .map_err(|_| MiniAppError::BridgeMessageFailed("send failed".to_string()))
    }

    /// Handle incoming message
    pub fn handle_incoming(&self, message: BridgeMessage) -> MiniAppResult<Option<BridgeMessage>> {
        *self.last_activity.write() = Utc::now();

        match message.message {
            BridgeMessageType::Request { method, params, id } => {
                let result = self
                    .handler
                    .handle_request(&self.sandbox_id, &method, params);

                let response = match result {
                    Ok(value) => BridgeMessageType::Response {
                        id,
                        result: Some(value),
                        error: None,
                    },
                    Err(e) => BridgeMessageType::Response {
                        id,
                        result: None,
                        error: Some(BridgeError::internal_error(&e.to_string())),
                    },
                };

                Ok(Some(BridgeMessage::new(self.sandbox_id, response)))
            }

            BridgeMessageType::Response { id, result, error } => {
                // Match to pending request
                if let Some(pending) = self.pending.write().remove(&id) {
                    let response = match (result, error) {
                        (Some(value), _) => Ok(value),
                        (None, Some(err)) => Err(err),
                        (None, None) => Ok(serde_json::Value::Null),
                    };
                    let _ = pending.response_tx.send(response);
                }
                Ok(None)
            }

            BridgeMessageType::Notification { method, params } => {
                self.handler
                    .handle_notification(&self.sandbox_id, &method, params)?;
                Ok(None)
            }

            BridgeMessageType::Ping { timestamp } => Ok(Some(BridgeMessage::new(
                self.sandbox_id,
                BridgeMessageType::Pong { timestamp },
            ))),

            BridgeMessageType::Pong { .. } => Ok(None),

            BridgeMessageType::Close { code, reason } => {
                *self.connected.write() = false;
                tracing::info!("Bridge closed: {} - {}", code, reason);
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    /// Close bridge
    pub async fn close(&self, code: u16, reason: &str) -> MiniAppResult<()> {
        *self.connected.write() = false;

        let message = BridgeMessage::new(
            self.sandbox_id,
            BridgeMessageType::Close {
                code,
                reason: reason.to_string(),
            },
        );

        let _ = self.outbound_tx.send(message).await;
        Ok(())
    }

    /// Check if connected
    pub fn is_connected(&self) -> bool {
        *self.connected.read()
    }

    /// Get last activity time
    pub fn last_activity(&self) -> DateTime<Utc> {
        *self.last_activity.read()
    }

    /// Cleanup timed out requests
    pub fn cleanup_pending(&self, max_age: chrono::Duration) {
        let cutoff = Utc::now() - max_age;
        self.pending.write().retain(|_, pending| {
            if pending.sent_at < cutoff {
                let _ = pending
                    .response_tx
                    .send(Err(BridgeError::internal_error("request timeout")));
                false
            } else {
                true
            }
        });
    }
}

/// Default message handler
pub struct DefaultMessageHandler;

impl MessageHandler for DefaultMessageHandler {
    fn handle_request(
        &self,
        _sandbox_id: &SandboxId,
        method: &str,
        params: serde_json::Value,
    ) -> MiniAppResult<serde_json::Value> {
        match method {
            "echo" => Ok(params),
            "getTime" => Ok(serde_json::json!({
                "timestamp": chrono::Utc::now().timestamp_millis()
            })),
            _ => Err(MiniAppError::InvalidBridgeMessage(format!(
                "unknown method: {}",
                method
            ))),
        }
    }

    fn handle_notification(
        &self,
        sandbox_id: &SandboxId,
        method: &str,
        params: serde_json::Value,
    ) -> MiniAppResult<()> {
        tracing::debug!(
            sandbox_id = %sandbox_id,
            method = %method,
            params = ?params,
            "Received notification"
        );
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_message_id_uniqueness() {
        let id1 = MessageId::new();
        let id2 = MessageId::new();
        assert_ne!(id1.0, id2.0);
    }

    #[test]
    fn test_bridge_error() {
        let error = BridgeError::method_not_found("test");
        assert_eq!(error.code, -32601);
        assert!(error.message.contains("test"));
    }

    #[test]
    fn test_bridge_message_serialization() {
        let sandbox_id = SandboxId::new();
        let message = BridgeMessage::new(
            sandbox_id,
            BridgeMessageType::Request {
                method: "test".to_string(),
                params: serde_json::json!({"key": "value"}),
                id: "1".to_string(),
            },
        );

        let json = message.to_json().unwrap();
        let parsed = BridgeMessage::from_json(&json).unwrap();

        assert_eq!(message.id, parsed.id);
    }

    #[tokio::test]
    async fn test_bridge_notify() {
        let sandbox_id = SandboxId::new();
        let handler = Arc::new(DefaultMessageHandler);
        let (bridge, mut rx) = MessageBridge::new(sandbox_id, handler);

        bridge.notify("test", serde_json::json!({})).await.unwrap();

        let message = rx.recv().await.unwrap();
        match message.message {
            BridgeMessageType::Notification { method, .. } => {
                assert_eq!(method, "test");
            }
            _ => panic!("expected notification"),
        }
    }

    #[tokio::test]
    async fn test_bridge_event() {
        let sandbox_id = SandboxId::new();
        let handler = Arc::new(DefaultMessageHandler);
        let (bridge, mut rx) = MessageBridge::new(sandbox_id, handler);

        bridge
            .emit_event("test_event", serde_json::json!({"data": 123}))
            .await
            .unwrap();

        let message = rx.recv().await.unwrap();
        match message.message {
            BridgeMessageType::Event { name, data } => {
                assert_eq!(name, "test_event");
                assert_eq!(data["data"], 123);
            }
            _ => panic!("expected event"),
        }
    }

    #[test]
    fn test_default_handler() {
        let handler = DefaultMessageHandler;
        let sandbox_id = SandboxId::new();

        // Echo
        let result = handler
            .handle_request(&sandbox_id, "echo", serde_json::json!({"test": true}))
            .unwrap();
        assert_eq!(result["test"], true);

        // Unknown method
        let result = handler.handle_request(&sandbox_id, "unknown", serde_json::json!({}));
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_bridge_close() {
        let sandbox_id = SandboxId::new();
        let handler = Arc::new(DefaultMessageHandler);
        let (bridge, mut rx) = MessageBridge::new(sandbox_id, handler);

        assert!(bridge.is_connected());

        bridge.close(1000, "normal closure").await.unwrap();

        assert!(!bridge.is_connected());

        // Further operations should fail
        let result = bridge.notify("test", serde_json::json!({})).await;
        assert!(result.is_err());
    }

    #[test]
    fn test_ping_pong() {
        let sandbox_id = SandboxId::new();
        let handler = Arc::new(DefaultMessageHandler);
        let (bridge, _rx) = MessageBridge::new(sandbox_id, handler);

        let ping = BridgeMessage::new(sandbox_id, BridgeMessageType::Ping { timestamp: 12345 });

        let response = bridge.handle_incoming(ping).unwrap();
        assert!(response.is_some());

        match response.unwrap().message {
            BridgeMessageType::Pong { timestamp } => {
                assert_eq!(timestamp, 12345);
            }
            _ => panic!("expected pong"),
        }
    }
}
