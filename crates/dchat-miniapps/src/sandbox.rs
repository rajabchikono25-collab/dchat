//! Sandbox runtime - secure isolation for mini-app execution

use std::collections::HashMap;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use chrono::{DateTime, Utc};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::manifest::AppManifest;
use crate::permissions::{Permission, PermissionManager, PermissionSet};
use crate::registry::AppId;

/// Sandbox instance ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct SandboxId(pub Uuid);

impl SandboxId {
    /// Generate new ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for SandboxId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for SandboxId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Sandbox configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SandboxConfig {
    /// Maximum memory in bytes
    pub max_memory: u64,
    /// Maximum CPU time per operation in milliseconds
    pub max_cpu_time_ms: u64,
    /// Maximum storage in bytes
    pub max_storage: u64,
    /// Maximum network requests per minute
    pub max_network_requests: u32,
    /// Allow external network access
    pub allow_network: bool,
    /// Allowed domains for network access
    pub allowed_domains: Vec<String>,
    /// Enable debugging
    pub debug_mode: bool,
    /// Timeout for sandbox operations
    pub operation_timeout: Duration,
    /// Maximum concurrent operations
    pub max_concurrent_ops: u32,
}

impl Default for SandboxConfig {
    fn default() -> Self {
        Self {
            max_memory: 128 * 1024 * 1024, // 128 MB
            max_cpu_time_ms: 5000,         // 5 seconds
            max_storage: 10 * 1024 * 1024, // 10 MB
            max_network_requests: 60,
            allow_network: true,
            allowed_domains: Vec::new(),
            debug_mode: false,
            operation_timeout: Duration::from_secs(30),
            max_concurrent_ops: 10,
        }
    }
}

impl SandboxConfig {
    /// Create config from manifest
    pub fn from_manifest(manifest: &AppManifest) -> Self {
        Self {
            max_memory: manifest.runtime.max_memory_mb as u64 * 1024 * 1024,
            max_cpu_time_ms: manifest.runtime.max_cpu_time_ms as u64,
            allow_network: manifest.permissions.contains(&Permission::Network),
            allowed_domains: manifest.resources.allowed_domains.clone(),
            ..Default::default()
        }
    }
}

/// Resource usage tracking
#[derive(Debug, Default)]
pub struct ResourceUsage {
    /// Memory used in bytes
    pub memory_used: AtomicU64,
    /// CPU time used in milliseconds
    pub cpu_time_used: AtomicU64,
    /// Storage used in bytes
    pub storage_used: AtomicU64,
    /// Network requests made
    pub network_requests: AtomicU64,
}

impl ResourceUsage {
    /// Create new usage tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if memory limit exceeded
    pub fn check_memory(&self, limit: u64) -> MiniAppResult<()> {
        let used = self.memory_used.load(Ordering::Relaxed);
        if used > limit {
            return Err(MiniAppError::SandboxResourceLimitExceeded {
                resource: "memory".to_string(),
                used,
                limit,
            });
        }
        Ok(())
    }

    /// Check if CPU time limit exceeded
    pub fn check_cpu_time(&self, limit: u64) -> MiniAppResult<()> {
        let used = self.cpu_time_used.load(Ordering::Relaxed);
        if used > limit {
            return Err(MiniAppError::SandboxResourceLimitExceeded {
                resource: "cpu_time".to_string(),
                used,
                limit,
            });
        }
        Ok(())
    }

    /// Record memory usage
    pub fn record_memory(&self, bytes: u64) {
        self.memory_used.fetch_add(bytes, Ordering::Relaxed);
    }

    /// Record CPU time
    pub fn record_cpu_time(&self, ms: u64) {
        self.cpu_time_used.fetch_add(ms, Ordering::Relaxed);
    }

    /// Record network request
    pub fn record_network_request(&self) {
        self.network_requests.fetch_add(1, Ordering::Relaxed);
    }

    /// Snapshot current usage
    pub fn snapshot(&self) -> ResourceSnapshot {
        ResourceSnapshot {
            memory_used: self.memory_used.load(Ordering::Relaxed),
            cpu_time_used: self.cpu_time_used.load(Ordering::Relaxed),
            storage_used: self.storage_used.load(Ordering::Relaxed),
            network_requests: self.network_requests.load(Ordering::Relaxed),
        }
    }
}

/// Snapshot of resource usage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResourceSnapshot {
    /// Memory used in bytes
    pub memory_used: u64,
    /// CPU time used in milliseconds
    pub cpu_time_used: u64,
    /// Storage used in bytes
    pub storage_used: u64,
    /// Network requests made
    pub network_requests: u64,
}

/// Sandbox state
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SandboxState {
    /// Initializing
    Initializing,
    /// Ready to execute
    Ready,
    /// Currently executing
    Running,
    /// Paused
    Paused,
    /// Terminated normally
    Terminated,
    /// Crashed with error
    Crashed,
}

/// Message type for sandbox communication
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", content = "data")]
pub enum SandboxMessage {
    // ─── Lifecycle ──────────────────────────────────────────────────────────
    /// Initialize sandbox
    Init {
        /// Application ID
        app_id: String,
        /// Configuration parameters
        config: serde_json::Value,
    },
    /// Sandbox ready
    Ready,
    /// Terminate sandbox
    Terminate {
        /// Reason for termination
        reason: String,
    },
    /// Sandbox terminated
    Terminated {
        /// Exit code
        code: i32,
    },

    // ─── User Interface ─────────────────────────────────────────────────────
    /// Update viewport size
    ViewportChange {
        /// Viewport width in pixels
        width: u32,
        /// Viewport height in pixels
        height: u32,
    },
    /// Theme changed
    ThemeChange {
        /// New theme name (light, dark, etc.)
        theme: String,
    },
    /// Back button pressed
    BackButton,
    /// Settings button pressed  
    SettingsButton,
    /// Main button clicked
    MainButtonClick,

    // ─── Data ───────────────────────────────────────────────────────────────
    /// Send data to sandbox
    SendData {
        /// Data payload
        payload: serde_json::Value,
    },
    /// Receive data from sandbox
    ReceiveData {
        /// Received data payload
        payload: serde_json::Value,
    },

    // ─── Requests ───────────────────────────────────────────────────────────
    /// Permission request
    PermissionRequest {
        /// Requested permission names
        permissions: Vec<String>,
    },
    /// Permission response
    PermissionResponse {
        /// Granted permissions
        granted: Vec<String>,
        /// Denied permissions
        denied: Vec<String>,
    },
    /// Invoke method
    InvokeMethod {
        /// Method name
        method: String,
        /// Method parameters
        params: serde_json::Value,
        /// Request ID for correlation
        id: String,
    },
    /// Method result
    MethodResult {
        /// Request ID this result correlates to
        id: String,
        /// Result value
        result: serde_json::Value,
        /// Error message if failed
        error: Option<String>,
    },

    // ─── Events ─────────────────────────────────────────────────────────────
    /// Custom event
    Event {
        /// Event name
        name: String,
        /// Event data
        data: serde_json::Value,
    },
    /// Error occurred
    Error {
        /// Error code
        code: u32,
        /// Error message
        message: String,
    },
}

impl SandboxMessage {
    /// Serialize to JSON
    pub fn to_json(&self) -> MiniAppResult<String> {
        serde_json::to_string(self).map_err(|e| MiniAppError::SerializationError(e.to_string()))
    }

    /// Deserialize from JSON
    pub fn from_json(json: &str) -> MiniAppResult<Self> {
        serde_json::from_str(json).map_err(|e| MiniAppError::DeserializationError(e.to_string()))
    }

    /// Get message type name
    pub fn type_name(&self) -> &'static str {
        match self {
            Self::Init { .. } => "init",
            Self::Ready => "ready",
            Self::Terminate { .. } => "terminate",
            Self::Terminated { .. } => "terminated",
            Self::ViewportChange { .. } => "viewport_change",
            Self::ThemeChange { .. } => "theme_change",
            Self::BackButton => "back_button",
            Self::SettingsButton => "settings_button",
            Self::MainButtonClick => "main_button_click",
            Self::SendData { .. } => "send_data",
            Self::ReceiveData { .. } => "receive_data",
            Self::PermissionRequest { .. } => "permission_request",
            Self::PermissionResponse { .. } => "permission_response",
            Self::InvokeMethod { .. } => "invoke_method",
            Self::MethodResult { .. } => "method_result",
            Self::Event { .. } => "event",
            Self::Error { .. } => "error",
        }
    }
}

/// Sandbox execution context
#[derive(Debug, Clone)]
pub struct SandboxContext {
    /// Sandbox ID
    pub sandbox_id: SandboxId,
    /// App ID
    pub app_id: AppId,
    /// User ID
    pub user_id: [u8; 32],
    /// Session ID
    pub session_id: Uuid,
    /// Start timestamp
    pub started_at: DateTime<Utc>,
    /// Theme (light/dark)
    pub theme: String,
    /// Viewport dimensions
    pub viewport: (u32, u32),
    /// Language
    pub language: String,
    /// Platform
    pub platform: String,
}

impl SandboxContext {
    /// Create new context
    pub fn new(app_id: AppId, user_id: [u8; 32]) -> Self {
        Self {
            sandbox_id: SandboxId::new(),
            app_id,
            user_id,
            session_id: Uuid::new_v4(),
            started_at: Utc::now(),
            theme: "light".to_string(),
            viewport: (375, 667), // Default mobile size
            language: "en".to_string(),
            platform: "dchat".to_string(),
        }
    }

    /// Convert to init data for WebApp
    pub fn to_init_data(&self) -> serde_json::Value {
        serde_json::json!({
            "sandbox_id": self.sandbox_id.0.to_string(),
            "app_id": hex::encode(self.app_id.0),
            "session_id": self.session_id.to_string(),
            "theme": self.theme,
            "viewport": {
                "width": self.viewport.0,
                "height": self.viewport.1
            },
            "language": self.language,
            "platform": self.platform
        })
    }
}

/// Sandbox instance
pub struct SandboxInstance {
    /// Sandbox ID
    pub id: SandboxId,
    /// App ID
    pub app_id: AppId,
    /// Configuration
    pub config: SandboxConfig,
    /// Current state
    state: RwLock<SandboxState>,
    /// Resource usage
    pub resources: Arc<ResourceUsage>,
    /// Context
    pub context: SandboxContext,
    /// Message queue (outbound)
    outbound: RwLock<Vec<SandboxMessage>>,
    /// Created timestamp
    pub created_at: Instant,
    /// Local storage
    storage: RwLock<HashMap<String, Vec<u8>>>,
}

impl SandboxInstance {
    /// Create new sandbox instance
    pub fn new(app_id: AppId, user_id: [u8; 32], config: SandboxConfig) -> Self {
        let context = SandboxContext::new(app_id, user_id);
        let id = context.sandbox_id;

        Self {
            id,
            app_id,
            config,
            state: RwLock::new(SandboxState::Initializing),
            resources: Arc::new(ResourceUsage::new()),
            context,
            outbound: RwLock::new(Vec::new()),
            created_at: Instant::now(),
            storage: RwLock::new(HashMap::new()),
        }
    }

    /// Get current state
    pub fn state(&self) -> SandboxState {
        *self.state.read()
    }

    /// Transition to new state
    pub fn set_state(&self, new_state: SandboxState) {
        *self.state.write() = new_state;
    }

    /// Initialize sandbox
    pub fn initialize(&self) -> MiniAppResult<()> {
        let current = self.state();
        if current != SandboxState::Initializing {
            return Err(MiniAppError::SandboxExecutionFailed(format!(
                "cannot initialize from state {:?}",
                current
            )));
        }

        self.set_state(SandboxState::Ready);
        Ok(())
    }

    /// Start execution
    pub fn start(&self) -> MiniAppResult<()> {
        let current = self.state();
        if current != SandboxState::Ready && current != SandboxState::Paused {
            return Err(MiniAppError::SandboxExecutionFailed(format!(
                "cannot start from state {:?}",
                current
            )));
        }

        self.set_state(SandboxState::Running);
        Ok(())
    }

    /// Pause execution
    pub fn pause(&self) -> MiniAppResult<()> {
        let current = self.state();
        if current != SandboxState::Running {
            return Err(MiniAppError::SandboxExecutionFailed(format!(
                "cannot pause from state {:?}",
                current
            )));
        }

        self.set_state(SandboxState::Paused);
        Ok(())
    }

    /// Terminate sandbox
    pub fn terminate(&self, reason: &str) -> MiniAppResult<()> {
        self.set_state(SandboxState::Terminated);
        self.queue_message(SandboxMessage::Terminate {
            reason: reason.to_string(),
        });
        Ok(())
    }

    /// Queue outbound message
    pub fn queue_message(&self, message: SandboxMessage) {
        self.outbound.write().push(message);
    }

    /// Drain outbound messages
    pub fn drain_messages(&self) -> Vec<SandboxMessage> {
        std::mem::take(&mut *self.outbound.write())
    }

    /// Handle incoming message
    pub fn handle_message(&self, message: SandboxMessage) -> MiniAppResult<Option<SandboxMessage>> {
        // Check resource limits
        self.resources.check_memory(self.config.max_memory)?;
        self.resources.check_cpu_time(self.config.max_cpu_time_ms)?;

        match message {
            SandboxMessage::Ready => {
                self.set_state(SandboxState::Ready);
                Ok(None)
            }

            SandboxMessage::InvokeMethod { method, params, id } => {
                // Record CPU time for method invocation
                let start = Instant::now();

                let result = self.invoke_method(&method, &params);

                let elapsed_ms = start.elapsed().as_millis() as u64;
                self.resources.record_cpu_time(elapsed_ms);

                let (result_value, error_msg) = match result {
                    Ok(val) => (val, None),
                    Err(e) => (serde_json::Value::Null, Some(e.to_string())),
                };

                Ok(Some(SandboxMessage::MethodResult {
                    id,
                    result: result_value,
                    error: error_msg,
                }))
            }

            SandboxMessage::ReceiveData { payload } => {
                // App sent data - record for processing
                self.resources
                    .record_memory(payload.to_string().len() as u64);
                Ok(None)
            }

            SandboxMessage::Error { code, message } => {
                tracing::error!("Sandbox error {}: {}", code, message);
                self.set_state(SandboxState::Crashed);
                Ok(None)
            }

            _ => Ok(None),
        }
    }

    /// Invoke sandbox method
    fn invoke_method(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> MiniAppResult<serde_json::Value> {
        match method {
            "getInitData" => Ok(self.context.to_init_data()),

            "setItem" => {
                let key = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    MiniAppError::InvalidSandboxMessage("missing key".to_string())
                })?;
                let value = params
                    .get("value")
                    .and_then(|v| v.as_str())
                    .ok_or_else(|| {
                        MiniAppError::InvalidSandboxMessage("missing value".to_string())
                    })?;

                let value_bytes = value.as_bytes().to_vec();

                // Check storage limit
                let current_storage = self.resources.storage_used.load(Ordering::Relaxed);
                let new_total = current_storage + value_bytes.len() as u64;
                if new_total > self.config.max_storage {
                    return Err(MiniAppError::SandboxResourceLimitExceeded {
                        resource: "storage".to_string(),
                        used: new_total,
                        limit: self.config.max_storage,
                    });
                }

                self.resources
                    .storage_used
                    .fetch_add(value_bytes.len() as u64, Ordering::Relaxed);
                self.storage.write().insert(key.to_string(), value_bytes);

                Ok(serde_json::json!(true))
            }

            "getItem" => {
                let key = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    MiniAppError::InvalidSandboxMessage("missing key".to_string())
                })?;

                let value = self
                    .storage
                    .read()
                    .get(key)
                    .map(|v| String::from_utf8_lossy(v).to_string());

                Ok(serde_json::json!(value))
            }

            "removeItem" => {
                let key = params.get("key").and_then(|v| v.as_str()).ok_or_else(|| {
                    MiniAppError::InvalidSandboxMessage("missing key".to_string())
                })?;

                self.storage.write().remove(key);
                Ok(serde_json::json!(true))
            }

            "getResourceUsage" => {
                let snapshot = self.resources.snapshot();
                Ok(serde_json::to_value(snapshot)
                    .map_err(|e| MiniAppError::SerializationError(e.to_string()))?)
            }

            _ => Err(MiniAppError::InvalidSandboxMessage(format!(
                "unknown method: {}",
                method
            ))),
        }
    }

    /// Check if sandbox is healthy
    pub fn is_healthy(&self) -> bool {
        let state = self.state();
        matches!(
            state,
            SandboxState::Ready | SandboxState::Running | SandboxState::Paused
        )
    }

    /// Get runtime duration
    pub fn runtime(&self) -> Duration {
        self.created_at.elapsed()
    }
}

/// Sandbox runtime (manages multiple sandbox instances)
pub struct SandboxRuntime {
    /// Active sandboxes
    sandboxes: RwLock<HashMap<SandboxId, Arc<SandboxInstance>>>,
    /// Permission manager
    permissions: Arc<RwLock<PermissionManager>>,
    /// Maximum concurrent sandboxes
    max_sandboxes: usize,
}

impl SandboxRuntime {
    /// Create new runtime
    pub fn new(max_sandboxes: usize) -> Self {
        Self {
            sandboxes: RwLock::new(HashMap::new()),
            permissions: Arc::new(RwLock::new(PermissionManager::new())),
            max_sandboxes,
        }
    }

    /// Create and register new sandbox
    pub fn create_sandbox(
        &self,
        app_id: AppId,
        user_id: [u8; 32],
        config: SandboxConfig,
    ) -> MiniAppResult<Arc<SandboxInstance>> {
        let mut sandboxes = self.sandboxes.write();

        // Check limit
        if sandboxes.len() >= self.max_sandboxes {
            // Try to clean up terminated sandboxes first
            sandboxes.retain(|_, s| s.is_healthy());

            if sandboxes.len() >= self.max_sandboxes {
                return Err(MiniAppError::SandboxCreationFailed(
                    "maximum sandboxes reached".to_string(),
                ));
            }
        }

        let sandbox = Arc::new(SandboxInstance::new(app_id, user_id, config));
        sandbox.initialize()?;

        let id = sandbox.id;
        sandboxes.insert(id, sandbox.clone());

        Ok(sandbox)
    }

    /// Get sandbox by ID
    pub fn get_sandbox(&self, id: &SandboxId) -> Option<Arc<SandboxInstance>> {
        self.sandboxes.read().get(id).cloned()
    }

    /// Terminate sandbox
    pub fn terminate_sandbox(&self, id: &SandboxId, reason: &str) -> MiniAppResult<()> {
        let sandbox =
            self.sandboxes.read().get(id).cloned().ok_or_else(|| {
                MiniAppError::SandboxExecutionFailed("sandbox not found".to_string())
            })?;

        sandbox.terminate(reason)?;
        self.sandboxes.write().remove(id);
        Ok(())
    }

    /// Terminate all sandboxes for an app
    pub fn terminate_app_sandboxes(&self, app_id: &AppId) {
        let mut sandboxes = self.sandboxes.write();
        let to_remove: Vec<SandboxId> = sandboxes
            .iter()
            .filter(|(_, s)| &s.app_id == app_id)
            .map(|(id, _)| *id)
            .collect();

        for id in to_remove {
            if let Some(sandbox) = sandboxes.get(&id) {
                let _ = sandbox.terminate("app terminated");
            }
            sandboxes.remove(&id);
        }
    }

    /// Get active sandbox count
    pub fn active_count(&self) -> usize {
        self.sandboxes.read().len()
    }

    /// Clean up terminated sandboxes
    pub fn cleanup(&self) {
        self.sandboxes.write().retain(|_, s| s.is_healthy());
    }

    /// Get permission manager
    pub fn permissions(&self) -> Arc<RwLock<PermissionManager>> {
        self.permissions.clone()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sandbox_id() {
        let id1 = SandboxId::new();
        let id2 = SandboxId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_sandbox_creation() {
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];
        let config = SandboxConfig::default();

        let sandbox = SandboxInstance::new(app_id, user_id, config);
        assert_eq!(sandbox.state(), SandboxState::Initializing);

        sandbox.initialize().unwrap();
        assert_eq!(sandbox.state(), SandboxState::Ready);
    }

    #[test]
    fn test_sandbox_state_transitions() {
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];
        let sandbox = SandboxInstance::new(app_id, user_id, SandboxConfig::default());

        sandbox.initialize().unwrap();
        assert_eq!(sandbox.state(), SandboxState::Ready);

        sandbox.start().unwrap();
        assert_eq!(sandbox.state(), SandboxState::Running);

        sandbox.pause().unwrap();
        assert_eq!(sandbox.state(), SandboxState::Paused);

        sandbox.start().unwrap();
        assert_eq!(sandbox.state(), SandboxState::Running);

        sandbox.terminate("test").unwrap();
        assert_eq!(sandbox.state(), SandboxState::Terminated);
    }

    #[test]
    fn test_sandbox_storage() {
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];
        let sandbox = SandboxInstance::new(app_id, user_id, SandboxConfig::default());
        sandbox.initialize().unwrap();

        // Set item
        let params = serde_json::json!({"key": "test", "value": "hello"});
        let result = sandbox.invoke_method("setItem", &params);
        assert!(result.is_ok());

        // Get item
        let params = serde_json::json!({"key": "test"});
        let result = sandbox.invoke_method("getItem", &params).unwrap();
        assert_eq!(result, serde_json::json!("hello"));
    }

    #[test]
    fn test_resource_usage() {
        let resources = ResourceUsage::new();

        resources.record_memory(1000);
        resources.record_cpu_time(100);
        resources.record_network_request();

        let snapshot = resources.snapshot();
        assert_eq!(snapshot.memory_used, 1000);
        assert_eq!(snapshot.cpu_time_used, 100);
        assert_eq!(snapshot.network_requests, 1);
    }

    #[test]
    fn test_sandbox_runtime() {
        let runtime = SandboxRuntime::new(10);
        let app_id = AppId([1u8; 32]);
        let user_id = [2u8; 32];

        let sandbox = runtime
            .create_sandbox(app_id, user_id, SandboxConfig::default())
            .unwrap();
        assert_eq!(runtime.active_count(), 1);

        let id = sandbox.id;
        runtime.terminate_sandbox(&id, "test").unwrap();
        assert_eq!(runtime.active_count(), 0);
    }

    #[test]
    fn test_sandbox_message_serialization() {
        let msg = SandboxMessage::SendData {
            payload: serde_json::json!({"test": "data"}),
        };

        let json = msg.to_json().unwrap();
        let parsed = SandboxMessage::from_json(&json).unwrap();

        assert_eq!(msg.type_name(), parsed.type_name());
    }
}
