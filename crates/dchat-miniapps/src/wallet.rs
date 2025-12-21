//! Wallet integration - wallet-invisible flows for mini-apps

use std::collections::HashMap;
use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use ed25519_dalek::{Signature, Signer, SigningKey, Verifier, VerifyingKey};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_with::{serde_as, Bytes};
use uuid::Uuid;

use crate::error::{MiniAppError, MiniAppResult};
use crate::intent::{Intent, IntentPayload, IntentStatus, IntentType};
use crate::permissions::{Permission, PermissionGrant, PermissionManager};
use crate::registry::AppId;
use crate::sandbox::SandboxId;

/// Connection ID
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct ConnectionId(pub Uuid);

impl ConnectionId {
    /// Create new connection ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for ConnectionId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Wallet visibility mode
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum WalletVisibility {
    /// Wallet UI is hidden, only intents are shown
    Hidden,
    /// Wallet balance is visible but signing is automatic
    BalanceOnly,
    /// Full wallet UI (for power users)
    Full,
    /// Completely invisible (all approvals pre-authorized)
    Invisible,
}

impl Default for WalletVisibility {
    fn default() -> Self {
        Self::Hidden
    }
}

/// Wallet connection state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletConnection {
    /// Connection ID
    pub id: ConnectionId,
    /// App ID
    pub app_id: AppId,
    /// Sandbox ID
    pub sandbox_id: SandboxId,
    /// User's public key
    pub user_public_key: [u8; 32],
    /// User's address (derived)
    pub user_address: String,
    /// Connected timestamp
    pub connected_at: DateTime<Utc>,
    /// Last activity
    pub last_activity: DateTime<Utc>,
    /// Visibility mode
    pub visibility: WalletVisibility,
    /// Chain ID
    pub chain_id: String,
    /// Session permissions
    pub session_permissions: Vec<Permission>,
}

impl WalletConnection {
    /// Create new connection
    pub fn new(
        app_id: AppId,
        sandbox_id: SandboxId,
        user_public_key: [u8; 32],
        chain_id: String,
    ) -> Self {
        let user_address = format!("dchat:{}", hex::encode(&user_public_key[..16]));
        let now = Utc::now();

        Self {
            id: ConnectionId::new(),
            app_id,
            sandbox_id,
            user_public_key,
            user_address,
            connected_at: now,
            last_activity: now,
            visibility: WalletVisibility::Hidden,
            chain_id,
            session_permissions: Vec::new(),
        }
    }

    /// Update activity
    pub fn touch(&mut self) {
        self.last_activity = Utc::now();
    }

    /// Set visibility
    pub fn with_visibility(mut self, visibility: WalletVisibility) -> Self {
        self.visibility = visibility;
        self
    }

    /// Add session permission
    pub fn add_permission(&mut self, permission: Permission) {
        if !self.session_permissions.contains(&permission) {
            self.session_permissions.push(permission);
        }
    }

    /// Check if permission is granted for this session
    pub fn has_permission(&self, permission: &Permission) -> bool {
        self.session_permissions.contains(permission)
    }

    /// Check if connection is stale
    pub fn is_stale(&self, max_idle: Duration) -> bool {
        self.last_activity + max_idle < Utc::now()
    }
}

/// Wallet context for mini-app
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WalletContext {
    /// Connection ID
    pub connection_id: ConnectionId,
    /// User address
    pub address: String,
    /// Balance (in base units)
    pub balance: u64,
    /// Token balances
    pub token_balances: HashMap<String, u64>,
    /// Chain ID
    pub chain_id: String,
    /// Visibility mode
    pub visibility: WalletVisibility,
    /// Is connected
    pub is_connected: bool,
}

impl WalletContext {
    /// Create from connection
    pub fn from_connection(connection: &WalletConnection) -> Self {
        Self {
            connection_id: connection.id,
            address: connection.user_address.clone(),
            balance: 0,
            token_balances: HashMap::new(),
            chain_id: connection.chain_id.clone(),
            visibility: connection.visibility,
            is_connected: true,
        }
    }

    /// Update balance
    pub fn with_balance(mut self, balance: u64) -> Self {
        self.balance = balance;
        self
    }

    /// Update token balance
    pub fn with_token_balance(mut self, token: String, balance: u64) -> Self {
        self.token_balances.insert(token, balance);
        self
    }
}

/// Sign request from mini-app
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignRequest {
    /// Request ID
    pub id: String,
    /// Connection ID
    pub connection_id: ConnectionId,
    /// Request type
    pub request_type: SignRequestType,
    /// Message to sign (if applicable)
    pub message: Option<Vec<u8>>,
    /// Intent to sign (if applicable)
    pub intent: Option<IntentPayload>,
    /// Created timestamp
    pub created_at: DateTime<Utc>,
    /// Expires at
    pub expires_at: DateTime<Utc>,
    /// Display message for user
    pub display_message: String,
    /// Requires explicit approval
    pub requires_approval: bool,
}

/// Sign request type
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum SignRequestType {
    /// Sign arbitrary message
    Message,
    /// Sign intent (transaction)
    Intent,
    /// Sign typed data
    TypedData {
        domain: TypedDataDomain,
        types: HashMap<String, Vec<TypedDataField>>,
        primary_type: String,
        message: serde_json::Value,
    },
}

/// Typed data domain
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedDataDomain {
    pub name: String,
    pub version: String,
    pub chain_id: String,
    pub verifying_contract: Option<String>,
}

/// Typed data field
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TypedDataField {
    pub name: String,
    pub field_type: String,
}

impl SignRequest {
    /// Create message sign request
    pub fn message(connection_id: ConnectionId, message: Vec<u8>, display_message: String) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            connection_id,
            request_type: SignRequestType::Message,
            message: Some(message),
            intent: None,
            created_at: now,
            expires_at: now + Duration::minutes(5),
            display_message,
            requires_approval: true,
        }
    }

    /// Create intent sign request
    pub fn intent(connection_id: ConnectionId, intent: IntentPayload) -> Self {
        let now = Utc::now();
        let display = format!("Sign intent: {:?}", intent.intent_type);

        Self {
            id: Uuid::new_v4().to_string(),
            connection_id,
            request_type: SignRequestType::Intent,
            message: None,
            intent: Some(intent),
            created_at: now,
            expires_at: now + Duration::minutes(5),
            display_message: display,
            requires_approval: true,
        }
    }

    /// Set approval requirement
    pub fn with_approval(mut self, requires: bool) -> Self {
        self.requires_approval = requires;
        self
    }

    /// Check if expired
    pub fn is_expired(&self) -> bool {
        self.expires_at < Utc::now()
    }

    /// Get signing payload
    pub fn signing_payload(&self) -> MiniAppResult<Vec<u8>> {
        match &self.request_type {
            SignRequestType::Message => self.message.clone().ok_or(
                MiniAppError::InvalidSignRequest("missing message".to_string()),
            ),
            SignRequestType::Intent => {
                let intent = self
                    .intent
                    .as_ref()
                    .ok_or(MiniAppError::InvalidSignRequest(
                        "missing intent".to_string(),
                    ))?;
                Ok(intent.hash().to_vec())
            }
            SignRequestType::TypedData {
                domain,
                types,
                primary_type,
                message,
            } => {
                // EIP-712 style hashing
                let mut payload = Vec::new();
                payload.extend_from_slice(b"\x19\x01");
                payload.extend_from_slice(&hash_domain(domain));
                payload.extend_from_slice(&hash_struct(primary_type, message, types)?);
                Ok(payload)
            }
        }
    }
}

/// Sign response
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SignResponse {
    /// Request ID
    pub request_id: String,
    /// Success
    pub success: bool,
    /// Signature (if successful)
    pub signature: Option<[u8; 64]>,
    /// Error message (if failed)
    pub error: Option<String>,
    /// Signed at
    pub signed_at: DateTime<Utc>,
}

impl SignResponse {
    /// Create success response
    pub fn success(request_id: String, signature: [u8; 64]) -> Self {
        Self {
            request_id,
            success: true,
            signature: Some(signature),
            error: None,
            signed_at: Utc::now(),
        }
    }

    /// Create failure response
    pub fn failure(request_id: String, error: String) -> Self {
        Self {
            request_id,
            success: false,
            signature: None,
            error: Some(error),
            signed_at: Utc::now(),
        }
    }

    /// Create rejected response
    pub fn rejected(request_id: String) -> Self {
        Self::failure(request_id, "User rejected the request".to_string())
    }
}

/// Pending sign request
struct PendingRequest {
    request: SignRequest,
    response_tx: tokio::sync::oneshot::Sender<SignResponse>,
}

/// Wallet integration for mini-apps
pub struct WalletIntegration {
    /// Active connections
    connections: RwLock<HashMap<ConnectionId, WalletConnection>>,
    /// Sandbox to connection mapping
    sandbox_connections: RwLock<HashMap<SandboxId, ConnectionId>>,
    /// Pending sign requests
    pending_requests: RwLock<HashMap<String, PendingRequest>>,
    /// Permission manager
    permission_manager: Arc<PermissionManager>,
    /// Auto-approve threshold (in base units)
    auto_approve_threshold: RwLock<u64>,
}

impl WalletIntegration {
    /// Create new wallet integration
    pub fn new(permission_manager: Arc<PermissionManager>) -> Self {
        Self {
            connections: RwLock::new(HashMap::new()),
            sandbox_connections: RwLock::new(HashMap::new()),
            pending_requests: RwLock::new(HashMap::new()),
            permission_manager,
            auto_approve_threshold: RwLock::new(1_000_000), // 0.001 tokens default
        }
    }

    /// Connect wallet to sandbox
    pub fn connect(
        &self,
        app_id: AppId,
        sandbox_id: SandboxId,
        user_public_key: [u8; 32],
        chain_id: String,
    ) -> MiniAppResult<WalletConnection> {
        // Check if already connected
        if self.sandbox_connections.read().contains_key(&sandbox_id) {
            return Err(MiniAppError::WalletAlreadyConnected);
        }

        let connection = WalletConnection::new(app_id, sandbox_id, user_public_key, chain_id);
        let connection_id = connection.id;

        self.connections
            .write()
            .insert(connection_id, connection.clone());
        self.sandbox_connections
            .write()
            .insert(sandbox_id, connection_id);

        Ok(connection)
    }

    /// Disconnect wallet
    pub fn disconnect(&self, sandbox_id: &SandboxId) -> MiniAppResult<()> {
        let connection_id = self.sandbox_connections.write().remove(sandbox_id);

        if let Some(id) = connection_id {
            self.connections.write().remove(&id);
            Ok(())
        } else {
            Err(MiniAppError::WalletNotConnected)
        }
    }

    /// Get connection for sandbox
    pub fn get_connection(&self, sandbox_id: &SandboxId) -> Option<WalletConnection> {
        let connection_id = self.sandbox_connections.read().get(sandbox_id).copied()?;
        self.connections.read().get(&connection_id).cloned()
    }

    /// Get wallet context for sandbox
    pub fn get_context(&self, sandbox_id: &SandboxId) -> MiniAppResult<WalletContext> {
        let connection = self
            .get_connection(sandbox_id)
            .ok_or(MiniAppError::WalletNotConnected)?;

        // Update activity
        if let Some(conn) = self.connections.write().get_mut(&connection.id) {
            conn.touch();
        }

        Ok(WalletContext::from_connection(&connection))
    }

    /// Request signature
    pub async fn request_signature(
        &self,
        sandbox_id: &SandboxId,
        request: SignRequest,
    ) -> MiniAppResult<SignResponse> {
        let connection = self
            .get_connection(sandbox_id)
            .ok_or(MiniAppError::WalletNotConnected)?;

        // Verify connection matches
        if request.connection_id != connection.id {
            return Err(MiniAppError::InvalidSignRequest(
                "connection mismatch".to_string(),
            ));
        }

        // Check if auto-approve is possible
        if !request.requires_approval && self.can_auto_approve(&connection, &request) {
            // In production, this would use secure enclave/MPC
            // For now, return error indicating signing key needed
            return Err(MiniAppError::WalletSigningFailed(
                "auto-approve requires signing key".to_string(),
            ));
        }

        // Otherwise, queue for user approval
        let (response_tx, response_rx) = tokio::sync::oneshot::channel();
        let request_id = request.id.clone();

        self.pending_requests.write().insert(
            request_id.clone(),
            PendingRequest {
                request,
                response_tx,
            },
        );

        // Wait for response with timeout
        let timeout = tokio::time::Duration::from_secs(60);
        match tokio::time::timeout(timeout, response_rx).await {
            Ok(Ok(response)) => Ok(response),
            Ok(Err(_)) => {
                self.pending_requests.write().remove(&request_id);
                Err(MiniAppError::WalletSigningFailed(
                    "channel closed".to_string(),
                ))
            }
            Err(_) => {
                self.pending_requests.write().remove(&request_id);
                Err(MiniAppError::WalletSigningFailed(
                    "approval timeout".to_string(),
                ))
            }
        }
    }

    /// Approve pending request (called by user)
    pub fn approve_request(&self, request_id: &str, signing_key: &SigningKey) -> MiniAppResult<()> {
        let pending = self.pending_requests.write().remove(request_id).ok_or(
            MiniAppError::InvalidSignRequest("request not found".to_string()),
        )?;

        // Check expiry
        if pending.request.is_expired() {
            let _ = pending.response_tx.send(SignResponse::failure(
                request_id.to_string(),
                "request expired".to_string(),
            ));
            return Err(MiniAppError::InvalidSignRequest(
                "request expired".to_string(),
            ));
        }

        // Sign the payload
        let payload = pending.request.signing_payload()?;
        let signature = signing_key.sign(&payload);

        let _ = pending.response_tx.send(SignResponse::success(
            request_id.to_string(),
            signature.to_bytes(),
        ));

        Ok(())
    }

    /// Reject pending request (called by user)
    pub fn reject_request(&self, request_id: &str) -> MiniAppResult<()> {
        let pending = self.pending_requests.write().remove(request_id).ok_or(
            MiniAppError::InvalidSignRequest("request not found".to_string()),
        )?;

        let _ = pending
            .response_tx
            .send(SignResponse::rejected(request_id.to_string()));
        Ok(())
    }

    /// Get pending requests for user
    pub fn get_pending_requests(&self) -> Vec<SignRequest> {
        self.pending_requests
            .read()
            .values()
            .map(|p| p.request.clone())
            .collect()
    }

    /// Check if auto-approve is possible
    fn can_auto_approve(&self, connection: &WalletConnection, request: &SignRequest) -> bool {
        // Auto-approve only in invisible mode
        if connection.visibility != WalletVisibility::Invisible {
            return false;
        }

        // Check request type
        match &request.request_type {
            SignRequestType::Message => {
                // Never auto-approve arbitrary messages
                false
            }
            SignRequestType::Intent => {
                // Check intent type and amount
                if let Some(intent) = &request.intent {
                    match &intent.intent_type {
                        IntentType::Transfer { to: _, amount } => {
                            // Auto-approve small transfers
                            *amount <= *self.auto_approve_threshold.read()
                                && connection.has_permission(&Permission::SendTokens)
                        }
                        IntentType::ProgramCall { .. } => {
                            // Never auto-approve program calls
                            false
                        }
                        _ => false,
                    }
                } else {
                    false
                }
            }
            SignRequestType::TypedData { .. } => false,
        }
    }

    /// Set auto-approve threshold
    pub fn set_auto_approve_threshold(&self, threshold: u64) {
        *self.auto_approve_threshold.write() = threshold;
    }

    /// Cleanup stale connections
    pub fn cleanup_stale(&self, max_idle: Duration) {
        let stale: Vec<SandboxId> = self
            .connections
            .read()
            .values()
            .filter(|c| c.is_stale(max_idle))
            .map(|c| c.sandbox_id)
            .collect();

        for sandbox_id in stale {
            let _ = self.disconnect(&sandbox_id);
        }
    }

    /// Cleanup expired pending requests
    pub fn cleanup_expired_requests(&self) {
        let expired: Vec<String> = self
            .pending_requests
            .read()
            .iter()
            .filter(|(_, p)| p.request.is_expired())
            .map(|(id, _)| id.clone())
            .collect();

        for id in expired {
            if let Some(pending) = self.pending_requests.write().remove(&id) {
                let _ = pending
                    .response_tx
                    .send(SignResponse::failure(id, "request expired".to_string()));
            }
        }
    }
}

/// Hash typed data domain (simplified EIP-712)
fn hash_domain(domain: &TypedDataDomain) -> [u8; 32] {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    hasher.update(domain.name.as_bytes());
    hasher.update(domain.version.as_bytes());
    hasher.update(domain.chain_id.as_bytes());
    if let Some(contract) = &domain.verifying_contract {
        hasher.update(contract.as_bytes());
    }
    *hasher.finalize().as_bytes()
}

/// Hash typed data struct (simplified EIP-712)
fn hash_struct(
    _primary_type: &str,
    message: &serde_json::Value,
    _types: &HashMap<String, Vec<TypedDataField>>,
) -> MiniAppResult<[u8; 32]> {
    use blake3::Hasher;
    let mut hasher = Hasher::new();
    let json =
        serde_json::to_vec(message).map_err(|e| MiniAppError::SerializationError(e.to_string()))?;
    hasher.update(&json);
    Ok(*hasher.finalize().as_bytes())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_connection_id() {
        let id1 = ConnectionId::new();
        let id2 = ConnectionId::new();
        assert_ne!(id1.0, id2.0);
    }

    #[test]
    fn test_wallet_connection() {
        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        let connection =
            WalletConnection::new(app_id, sandbox_id, public_key, "dchat-1".to_string());

        assert!(connection.user_address.starts_with("dchat:"));
        assert!(!connection.is_stale(Duration::hours(1)));
    }

    #[test]
    fn test_wallet_context() {
        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        let connection =
            WalletConnection::new(app_id, sandbox_id, public_key, "dchat-1".to_string());
        let context = WalletContext::from_connection(&connection)
            .with_balance(1000000)
            .with_token_balance("USDC".to_string(), 500000);

        assert_eq!(context.balance, 1000000);
        assert_eq!(context.token_balances.get("USDC"), Some(&500000));
    }

    #[test]
    fn test_sign_request_message() {
        let connection_id = ConnectionId::new();
        let request = SignRequest::message(
            connection_id,
            b"Hello World".to_vec(),
            "Sign this message".to_string(),
        );

        assert!(!request.is_expired());
        assert!(request.requires_approval);

        let payload = request.signing_payload().unwrap();
        assert_eq!(payload, b"Hello World".to_vec());
    }

    #[test]
    fn test_sign_response() {
        let response = SignResponse::success("req1".to_string(), [0u8; 64]);
        assert!(response.success);
        assert!(response.signature.is_some());

        let response = SignResponse::rejected("req2".to_string());
        assert!(!response.success);
        assert!(response.error.is_some());
    }

    #[test]
    fn test_wallet_integration() {
        let pm = Arc::new(PermissionManager::new());
        let wallet = WalletIntegration::new(pm);

        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        // Connect
        let connection = wallet
            .connect(app_id, sandbox_id, public_key, "dchat-1".to_string())
            .unwrap();
        assert!(wallet.get_connection(&sandbox_id).is_some());

        // Get context
        let context = wallet.get_context(&sandbox_id).unwrap();
        assert_eq!(context.connection_id, connection.id);

        // Double connect should fail
        let result = wallet.connect(app_id, sandbox_id, public_key, "dchat-1".to_string());
        assert!(result.is_err());

        // Disconnect
        wallet.disconnect(&sandbox_id).unwrap();
        assert!(wallet.get_connection(&sandbox_id).is_none());
    }

    #[test]
    fn test_visibility_modes() {
        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        let connection =
            WalletConnection::new(app_id, sandbox_id, public_key, "dchat-1".to_string())
                .with_visibility(WalletVisibility::Full);

        assert_eq!(connection.visibility, WalletVisibility::Full);
    }

    #[test]
    fn test_session_permissions() {
        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        let mut connection =
            WalletConnection::new(app_id, sandbox_id, public_key, "dchat-1".to_string());

        assert!(!connection.has_permission(&Permission::SendTokens));

        connection.add_permission(Permission::SendTokens);
        assert!(connection.has_permission(&Permission::SendTokens));

        // Adding again should not duplicate
        connection.add_permission(Permission::SendTokens);
        assert_eq!(connection.session_permissions.len(), 1);
    }

    #[test]
    fn test_stale_connection() {
        let app_id = AppId([1u8; 32]);
        let sandbox_id = SandboxId::new();
        let public_key = [2u8; 32];

        let mut connection =
            WalletConnection::new(app_id, sandbox_id, public_key, "dchat-1".to_string());

        // Not stale
        assert!(!connection.is_stale(Duration::hours(1)));

        // Touch updates activity
        connection.touch();
        assert!(!connection.is_stale(Duration::hours(1)));
    }
}
