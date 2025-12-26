//! Sandbox Host for Counter Mini-App
//!
//! This launches the counter mini-app web UI inside the dchat sandbox environment.
//! It creates a local HTTP server and opens a WebView that connects to the sandbox runtime.
//!
//! Run with: `cargo run --bin sandbox_host`

use std::collections::HashMap;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use axum::{
    extract::{
        ws::{Message, WebSocket},
        Query, State, WebSocketUpgrade,
    },
    http::{header, StatusCode},
    response::{Html, IntoResponse, Response},
    routing::get,
    Router,
};
use futures::{sink::SinkExt, stream::StreamExt};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use tower_http::services::ServeDir;
use tracing::{info, Level};
use tracing_subscriber::FmtSubscriber;

use dchat_miniapps::{
    sandbox::SandboxState, AppId, MiniAppResult, SandboxConfig, SandboxInstance, SandboxMessage,
    SandboxRuntime,
};

/// Application state shared across handlers
struct AppState {
    runtime: Arc<SandboxRuntime>,
    web_dir: PathBuf,
    sandboxes: RwLock<HashMap<String, Arc<SandboxInstance>>>,
}

/// Query params for sandbox initialization
#[derive(Debug, Deserialize)]
struct SandboxQuery {
    app_id: Option<String>,
}

/// WebSocket message from client
#[derive(Debug, Deserialize)]
struct ClientMessage {
    #[serde(rename = "type")]
    msg_type: String,
    payload: serde_json::Value,
    request_id: Option<String>,
}

/// WebSocket message to client
#[derive(Debug, Serialize)]
struct HostMessage {
    #[serde(rename = "type")]
    msg_type: String,
    payload: serde_json::Value,
    #[serde(skip_serializing_if = "Option::is_none")]
    request_id: Option<String>,
}

impl HostMessage {
    fn new(msg_type: &str, payload: serde_json::Value) -> Self {
        Self {
            msg_type: msg_type.to_string(),
            payload,
            request_id: None,
        }
    }

    fn with_request_id(mut self, id: Option<String>) -> Self {
        self.request_id = id;
        self
    }
}

#[tokio::main]
async fn main() -> MiniAppResult<()> {
    // Initialize logging
    let subscriber = FmtSubscriber::builder()
        .with_max_level(Level::INFO)
        .finish();
    tracing::subscriber::set_global_default(subscriber).expect("setting subscriber failed");

    info!("🚀 Starting Counter Mini-App Sandbox Host");
    info!("=========================================");

    // Determine web directory
    let web_dir = std::env::current_dir()?.join("web");
    if !web_dir.exists() {
        info!("Web directory not found at {:?}", web_dir);
        info!("Please run from the miniapp_counter directory");
        return Ok(());
    }

    info!("📁 Serving web files from: {:?}", web_dir);

    // Create sandbox runtime
    let runtime = Arc::new(SandboxRuntime::new(10));

    // Create app state
    let state = Arc::new(AppState {
        runtime,
        web_dir: web_dir.clone(),
        sandboxes: RwLock::new(HashMap::new()),
    });

    // Build router
    let app = Router::new()
        // Sandbox launcher (wraps the app in sandbox context)
        .route("/sandbox", get(sandbox_launcher))
        // WebSocket endpoint for sandbox communication
        .route("/ws", get(websocket_handler))
        // API endpoints
        .route("/api/health", get(health_check))
        // Static files from web directory
        .nest_service("/app", ServeDir::new(&web_dir))
        .with_state(state);

    // Start server
    let addr = SocketAddr::from(([127, 0, 0, 1], 8081));
    info!("🌐 Sandbox host listening on http://{}", addr);
    info!("");
    info!("📱 Open in browser: http://{}/sandbox", addr);
    info!("");
    info!("This runs the mini-app inside the dchat sandbox with:");
    info!("  ✓ Resource limits (memory, CPU, storage)");
    info!("  ✓ Permission enforcement");
    info!("  ✓ Intent signing simulation");
    info!("  ✓ Sandbox storage API");
    info!("");

    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Health check endpoint
async fn health_check() -> impl IntoResponse {
    serde_json::json!({
        "status": "ok",
        "service": "sandbox-host",
        "version": "1.0.0"
    })
    .to_string()
}

/// Sandbox launcher - wraps the mini-app in sandbox context
async fn sandbox_launcher(
    State(state): State<Arc<AppState>>,
    Query(params): Query<SandboxQuery>,
) -> impl IntoResponse {
    let app_id = params.app_id.unwrap_or_else(|| "counter-app".to_string());

    // Create sandbox for this session
    let sandbox_id = create_sandbox(&state, &app_id);

    // Generate the sandbox host page
    let html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>dchat Sandbox - Counter Mini-App</title>
    <style>
        * {{ margin: 0; padding: 0; box-sizing: border-box; }}
        body {{
            font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
            background: #0f172a;
            color: #f8fafc;
            min-height: 100vh;
            display: flex;
            flex-direction: column;
        }}
        .toolbar {{
            background: #1e293b;
            padding: 12px 16px;
            display: flex;
            align-items: center;
            gap: 12px;
            border-bottom: 1px solid #334155;
        }}
        .toolbar-title {{
            font-weight: 600;
            flex: 1;
        }}
        .toolbar-badge {{
            background: #22c55e;
            color: white;
            font-size: 11px;
            padding: 4px 8px;
            border-radius: 999px;
            font-weight: 600;
        }}
        .toolbar-badge.sandboxed {{
            background: #6366f1;
        }}
        .sandbox-frame {{
            flex: 1;
            border: none;
            background: #0f172a;
        }}
        .status-bar {{
            background: #1e293b;
            padding: 8px 16px;
            font-size: 12px;
            color: #94a3b8;
            display: flex;
            gap: 16px;
            border-top: 1px solid #334155;
        }}
        .status-item {{
            display: flex;
            align-items: center;
            gap: 6px;
        }}
        .status-dot {{
            width: 8px;
            height: 8px;
            border-radius: 50%;
            background: #22c55e;
        }}
        .status-dot.warning {{ background: #f59e0b; }}
        .status-dot.error {{ background: #ef4444; }}
    </style>
</head>
<body>
    <div class="toolbar">
        <span class="toolbar-title">🔢 Counter Mini-App</span>
        <span class="toolbar-badge sandboxed">Sandboxed</span>
        <span class="toolbar-badge" id="ws-status">Connecting...</span>
    </div>

    <iframe 
        id="sandbox-frame"
        class="sandbox-frame" 
        src="/app/index.html"
        sandbox="allow-scripts allow-same-origin"
    ></iframe>

    <div class="status-bar">
        <div class="status-item">
            <span class="status-dot" id="sandbox-status"></span>
            <span id="sandbox-state">Initializing...</span>
        </div>
        <div class="status-item">
            Memory: <span id="memory-usage">0 KB</span>
        </div>
        <div class="status-item">
            Storage: <span id="storage-usage">0 KB</span>
        </div>
        <div class="status-item">
            Sandbox ID: <span id="sandbox-id">{sandbox_id}</span>
        </div>
    </div>

    <script>
        const sandboxId = '{sandbox_id}';
        const appId = '{app_id}';
        let ws = null;
        let iframe = null;

        function connect() {{
            ws = new WebSocket(`ws://${{location.host}}/ws?sandbox_id=${{sandboxId}}`);

            ws.onopen = () => {{
                console.log('[Host] WebSocket connected');
                document.getElementById('ws-status').textContent = 'Connected';
                document.getElementById('ws-status').style.background = '#22c55e';

                // Send init to sandbox
                ws.send(JSON.stringify({{
                    type: 'init',
                    payload: {{
                        appId: appId,
                        sandboxId: sandboxId,
                        theme: 'dark',
                        viewport: {{ width: window.innerWidth, height: window.innerHeight }}
                    }}
                }}));
            }};

            ws.onmessage = (event) => {{
                const msg = JSON.parse(event.data);
                console.log('[Host] ←', msg);
                handleHostMessage(msg);
            }};

            ws.onclose = () => {{
                console.log('[Host] WebSocket closed');
                document.getElementById('ws-status').textContent = 'Disconnected';
                document.getElementById('ws-status').style.background = '#ef4444';
                setTimeout(connect, 2000);
            }};

            ws.onerror = (err) => {{
                console.error('[Host] WebSocket error:', err);
            }};
        }}

        function handleHostMessage(msg) {{
            switch (msg.type) {{
                case 'dchat:init':
                    // Forward init data to iframe
                    iframe.contentWindow.postMessage(msg, '*');
                    document.getElementById('sandbox-state').textContent = 'Running';
                    break;

                case 'dchat:intent:response':
                case 'dchat:wallet:response':
                case 'dchat:storage:response':
                    // Forward responses to iframe
                    iframe.contentWindow.postMessage(msg, '*');
                    break;

                case 'resource_update':
                    updateResourceDisplay(msg.payload);
                    break;
            }}
        }}

        function updateResourceDisplay(resources) {{
            if (resources.memory_used !== undefined) {{
                document.getElementById('memory-usage').textContent = 
                    formatBytes(resources.memory_used);
            }}
            if (resources.storage_used !== undefined) {{
                document.getElementById('storage-usage').textContent = 
                    formatBytes(resources.storage_used);
            }}
        }}

        function formatBytes(bytes) {{
            if (bytes < 1024) return bytes + ' B';
            if (bytes < 1024 * 1024) return (bytes / 1024).toFixed(1) + ' KB';
            return (bytes / (1024 * 1024)).toFixed(1) + ' MB';
        }}

        // Listen for messages from iframe
        window.addEventListener('message', (event) => {{
            // Only accept messages from our iframe
            if (event.source !== iframe.contentWindow) return;

            const msg = event.data;
            if (!msg || !msg.type || !msg.type.startsWith('dchat:')) return;

            console.log('[Host] → (from iframe)', msg);

            // Forward to sandbox backend
            if (ws && ws.readyState === WebSocket.OPEN) {{
                ws.send(JSON.stringify({{
                    type: msg.type.replace('dchat:', ''),
                    payload: msg.payload,
                    requestId: msg.payload?.requestId
                }}));
            }}
        }});

        // Initialize
        document.addEventListener('DOMContentLoaded', () => {{
            iframe = document.getElementById('sandbox-frame');
            connect();
        }});
    </script>
</body>
</html>"#,
        sandbox_id = sandbox_id,
        app_id = app_id
    );

    Html(html)
}

fn create_sandbox(state: &AppState, app_id: &str) -> String {
    // Create app ID from string
    let mut app_id_bytes = [0u8; 32];
    let hash = blake3::hash(app_id.as_bytes());
    app_id_bytes.copy_from_slice(hash.as_bytes());
    let app_id = AppId(app_id_bytes);

    // Create user ID (simulated)
    let user_id = [1u8; 32];

    // Create sandbox config
    let config = SandboxConfig {
        max_memory: 64 * 1024 * 1024, // 64 MB
        max_cpu_time_ms: 5000,
        max_storage: 5 * 1024 * 1024, // 5 MB
        max_network_requests: 30,
        allow_network: true,
        allowed_domains: vec!["localhost".to_string()],
        debug_mode: true,
        ..Default::default()
    };

    // Create sandbox
    match state.runtime.create_sandbox(app_id, user_id, config) {
        Ok(sandbox) => {
            let id = sandbox.id.to_string();
            sandbox.start().ok();
            state.sandboxes.write().insert(id.clone(), sandbox);
            id
        }
        Err(e) => {
            tracing::error!("Failed to create sandbox: {}", e);
            "error".to_string()
        }
    }
}

/// WebSocket handler for sandbox communication
async fn websocket_handler(
    State(state): State<Arc<AppState>>,
    ws: WebSocketUpgrade,
    Query(params): Query<HashMap<String, String>>,
) -> impl IntoResponse {
    let sandbox_id = params.get("sandbox_id").cloned().unwrap_or_default();

    ws.on_upgrade(move |socket| handle_websocket(socket, state, sandbox_id))
}

async fn handle_websocket(socket: WebSocket, state: Arc<AppState>, sandbox_id: String) {
    let (mut sender, mut receiver) = socket.split();

    info!("WebSocket connected for sandbox: {}", sandbox_id);

    // Get sandbox instance
    let sandbox = state.sandboxes.read().get(&sandbox_id).cloned();

    // Create channel for sending messages to client
    let (tx, mut rx) = mpsc::channel::<HostMessage>(100);

    // Spawn task to forward messages to client
    let send_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if let Ok(json) = serde_json::to_string(&msg) {
                if sender.send(Message::Text(json.into())).await.is_err() {
                    break;
                }
            }
        }
    });

    // Send initial state
    if let Some(ref sandbox) = sandbox {
        let init_msg = HostMessage::new("dchat:init", sandbox.context.to_init_data());
        let _ = tx.send(init_msg).await;
    }

    // Process incoming messages
    while let Some(Ok(msg)) = receiver.next().await {
        if let Message::Text(text) = msg {
            if let Ok(client_msg) = serde_json::from_str::<ClientMessage>(&text) {
                let response =
                    handle_client_message(&state, &sandbox_id, &client_msg, sandbox.as_ref());

                if let Some(resp) = response {
                    let _ = tx.send(resp).await;
                }

                // Send resource update
                if let Some(ref sandbox) = sandbox {
                    let resources = sandbox.resources.snapshot();
                    let update = HostMessage::new(
                        "resource_update",
                        serde_json::to_value(resources).unwrap(),
                    );
                    let _ = tx.send(update).await;
                }
            }
        }
    }

    info!("WebSocket disconnected for sandbox: {}", sandbox_id);
    send_task.abort();
}

fn handle_client_message(
    _state: &AppState,
    sandbox_id: &str,
    msg: &ClientMessage,
    sandbox: Option<&Arc<SandboxInstance>>,
) -> Option<HostMessage> {
    match msg.msg_type.as_str() {
        "ready" => {
            info!("Sandbox {} ready", sandbox_id);
            None
        }

        "intent:request" => {
            // Simulate intent signing
            info!("Intent request: {:?}", msg.payload);

            let intent_id = uuid::Uuid::new_v4().to_string();
            let response = serde_json::json!({
                "requestId": msg.request_id,
                "intent": {
                    "id": intent_id,
                    "status": "signed",
                    "signature": "simulated_signature_abc123"
                }
            });

            Some(
                HostMessage::new("dchat:intent:response", response)
                    .with_request_id(msg.request_id.clone()),
            )
        }

        "wallet:request" => {
            info!("Wallet request: {:?}", msg.payload);

            let action = msg
                .payload
                .get("action")
                .and_then(|v| v.as_str())
                .unwrap_or("unknown");

            let response = match action {
                "getAddress" => serde_json::json!({
                    "requestId": msg.request_id,
                    "address": "dchat1qwertyuiop1234567890"
                }),
                _ => serde_json::json!({
                    "requestId": msg.request_id,
                    "error": format!("Unknown wallet action: {}", action)
                }),
            };

            Some(
                HostMessage::new("dchat:wallet:response", response)
                    .with_request_id(msg.request_id.clone()),
            )
        }

        "storage:get" | "storage:set" => {
            info!("Storage request: {:?}", msg.payload);

            if let Some(sandbox) = sandbox {
                let method = if msg.msg_type == "storage:get" {
                    "getItem"
                } else {
                    "setItem"
                };
                let result = sandbox.invoke_method_public(method, &msg.payload);

                let response = match result {
                    Ok(value) => serde_json::json!({
                        "requestId": msg.request_id,
                        "value": value
                    }),
                    Err(e) => serde_json::json!({
                        "requestId": msg.request_id,
                        "error": e.to_string()
                    }),
                };

                Some(
                    HostMessage::new("dchat:storage:response", response)
                        .with_request_id(msg.request_id.clone()),
                )
            } else {
                Some(
                    HostMessage::new(
                        "dchat:storage:response",
                        serde_json::json!({
                            "requestId": msg.request_id,
                            "error": "Sandbox not found"
                        }),
                    )
                    .with_request_id(msg.request_id.clone()),
                )
            }
        }

        "ui:haptic" => {
            info!("Haptic feedback: {:?}", msg.payload);
            None
        }

        "analytics:event" => {
            info!("Analytics event: {:?}", msg.payload);
            None
        }

        _ => {
            info!("Unknown message type: {}", msg.msg_type);
            None
        }
    }
}

// Add a public method to invoke sandbox methods
trait SandboxInstanceExt {
    fn invoke_method_public(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> MiniAppResult<serde_json::Value>;
}

impl SandboxInstanceExt for SandboxInstance {
    fn invoke_method_public(
        &self,
        method: &str,
        params: &serde_json::Value,
    ) -> MiniAppResult<serde_json::Value> {
        // Create an InvokeMethod message and handle it
        let msg = SandboxMessage::InvokeMethod {
            method: method.to_string(),
            params: params.clone(),
            id: "internal".to_string(),
        };

        match self.handle_message(msg)? {
            Some(SandboxMessage::MethodResult { result, error, .. }) => {
                if let Some(err) = error {
                    Err(dchat_miniapps::MiniAppError::SandboxExecutionFailed(err))
                } else {
                    Ok(result)
                }
            }
            _ => Ok(serde_json::Value::Null),
        }
    }
}
