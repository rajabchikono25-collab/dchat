// Mini-App CLI Command Handlers
//
// Dedicated handler functions for miniapp subcommands.
// Provides mini-app lifecycle management: launch, register, validate.
// Extracted from main.rs for better maintainability.

use dchat_core::error::{Error, Result};
use dchat_miniapps::{
    manifest::AppManifest,
    registry::{AppId, Developer, DeveloperId},
    sandbox::{SandboxConfig, SandboxId, SandboxMessage},
};
use std::path::PathBuf;

/// Handle `dchat miniapp launch` command
pub async fn handle_launch(
    manifest: Option<PathBuf>,
    app_id: Option<String>,
    user_id: Option<String>,
    theme: String,
    width: u32,
    height: u32,
    debug: bool,
) -> Result<()> {
    println!("\n🚀 DCHAT MINI-APP LAUNCHER");
    println!("══════════════════════════════════════════════════════════════════");

    // Load manifest from file or fetch from registry
    let app_manifest = if let Some(manifest_path) = manifest {
        let manifest_file = if manifest_path.is_dir() {
            manifest_path.join("manifest.json")
        } else {
            manifest_path.clone()
        };

        if !manifest_file.exists() {
            return Err(Error::validation(format!(
                "Manifest file not found: {:?}",
                manifest_file
            )));
        }

        println!("📦 Loading manifest from: {:?}", manifest_file);
        let manifest_json = std::fs::read_to_string(&manifest_file)
            .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;

        serde_json::from_str::<AppManifest>(&manifest_json)
            .map_err(|e| Error::validation(format!("Invalid manifest JSON: {}", e)))?
    } else if let Some(app_id_hex) = app_id {
        println!("📦 Fetching app {} from registry...", app_id_hex);
        return Err(Error::validation(
            "Registry lookup not yet implemented. Use --manifest to launch local apps.".to_string(),
        ));
    } else {
        return Err(Error::validation(
            "Either --manifest or --app-id must be provided".to_string(),
        ));
    };

    // Validate manifest
    println!("🔍 Validating manifest...");
    app_manifest
        .validate()
        .map_err(|e| Error::validation(format!("Manifest validation failed: {:?}", e)))?;
    println!("   ✅ Manifest valid");

    // Display app info
    println!();
    println!("📱 App Information:");
    println!("   Name:        {}", app_manifest.metadata.name);
    println!("   Version:     {}", app_manifest.version);
    println!(
        "   Description: {}",
        app_manifest.metadata.short_description
    );
    println!("   Category:    {:?}", app_manifest.metadata.category);
    println!("   Entry Point: {}", app_manifest.resources.entry_point);

    // Display requested permissions
    if !app_manifest.permissions.is_empty() {
        println!();
        println!("🔐 Requested Permissions:");
        for perm in app_manifest.permissions.iter() {
            println!("   • {:?}", perm);
        }
    }

    // Create sandbox configuration
    let mut sandbox_config = SandboxConfig::from_manifest(&app_manifest);
    sandbox_config.debug_mode = debug;

    println!();
    println!("🔧 Sandbox Configuration:");
    println!(
        "   Max Memory:  {} MB",
        sandbox_config.max_memory / 1024 / 1024
    );
    println!("   CPU Timeout: {} ms", sandbox_config.max_cpu_time_ms);
    println!("   Network:     {}", sandbox_config.allow_network);
    println!("   Debug Mode:  {}", sandbox_config.debug_mode);

    // Generate session context
    let sandbox_id = SandboxId::new();
    let session_user_id = if let Some(uid) = user_id {
        let bytes =
            hex::decode(&uid).map_err(|_| Error::validation("Invalid user_id hex".to_string()))?;
        if bytes.len() != 32 {
            return Err(Error::validation("user_id must be 32 bytes".to_string()));
        }
        let mut arr = [0u8; 32];
        arr.copy_from_slice(&bytes);
        arr
    } else {
        // Generate temporary user ID
        let mut arr = [0u8; 32];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut arr);
        arr
    };

    // Derive app ID from manifest
    let developer_id = DeveloperId::from_bytes([0u8; 32]);
    let derived_app_id = AppId::derive(&developer_id, &app_manifest.metadata.name);

    println!();
    println!("🆔 Session Details:");
    println!("   Sandbox ID:  {}", sandbox_id);
    println!("   App ID:      {}", derived_app_id);
    println!(
        "   User ID:     0x{}...",
        hex::encode(&session_user_id[..8])
    );
    println!("   Viewport:    {}x{}", width, height);
    println!("   Theme:       {}", theme);

    // Initialize sandbox messages
    let init_message = SandboxMessage::Init {
        app_id: derived_app_id.to_string(),
        config: serde_json::json!({
            "theme": theme,
            "viewport": { "width": width, "height": height },
            "debug": debug,
        }),
    };

    println!();
    println!("══════════════════════════════════════════════════════════════════");
    println!("✅ MINI-APP READY TO LAUNCH");
    println!("══════════════════════════════════════════════════════════════════");
    println!();
    println!("Entry point: {}", app_manifest.resources.entry_point);
    println!(
        "Init message: {}",
        init_message.to_json().unwrap_or_default()
    );
    println!();
    println!("💡 In a full client, this would open a WebView/iframe sandbox.");
    println!("   Use the dchat-miniapps crate to integrate into your application.");

    Ok(())
}

/// Handle `dchat miniapp register-developer` command
pub async fn handle_register_developer(
    name: String,
    keypair: PathBuf,
    website: Option<String>,
    email: Option<String>,
) -> Result<()> {
    println!("\n👤 REGISTER MINI-APP DEVELOPER");
    println!("══════════════════════════════════════════════════════════════════");

    if !keypair.exists() {
        return Err(Error::validation(format!(
            "Keypair file not found: {:?}",
            keypair
        )));
    }

    // Load keypair
    let keypair_json = std::fs::read_to_string(&keypair)
        .map_err(|e| Error::storage(format!("Failed to read keypair: {}", e)))?;
    let keypair_data: serde_json::Value = serde_json::from_str(&keypair_json)
        .map_err(|e| Error::validation(format!("Invalid keypair JSON: {}", e)))?;

    let pubkey_hex = keypair_data
        .get("public_key")
        .and_then(|v| v.as_str())
        .ok_or_else(|| Error::validation("Missing public_key in keypair file"))?;

    let pubkey_bytes = hex::decode(pubkey_hex)
        .map_err(|_| Error::validation("Invalid public key hex".to_string()))?;

    if pubkey_bytes.len() != 32 {
        return Err(Error::validation("Public key must be 32 bytes".to_string()));
    }

    let mut pubkey_arr = [0u8; 32];
    pubkey_arr.copy_from_slice(&pubkey_bytes);

    // Create developer registration
    let mut developer = Developer::new(name.clone(), pubkey_arr);
    developer.website = website;
    developer.email = email;

    println!("📝 Developer Registration:");
    println!("   Name:      {}", developer.name);
    println!("   ID:        {}", developer.id);
    println!("   Public Key: 0x{}...", &pubkey_hex[..16]);
    if let Some(ref w) = developer.website {
        println!("   Website:   {}", w);
    }
    if let Some(ref e) = developer.email {
        println!("   Email:     {}", e);
    }
    println!("   Status:    {:?}", developer.status);
    println!();
    println!("💡 In production, this would submit a registration transaction");
    println!("   to the chat chain for verification.");

    Ok(())
}

/// Handle `dchat miniapp register` command
pub async fn handle_register(
    manifest: PathBuf,
    keypair: PathBuf,
    bundle: Option<PathBuf>,
) -> Result<()> {
    println!("\n📦 REGISTER MINI-APP");
    println!("══════════════════════════════════════════════════════════════════");

    if !manifest.exists() {
        return Err(Error::validation(format!(
            "Manifest file not found: {:?}",
            manifest
        )));
    }

    if !keypair.exists() {
        return Err(Error::validation(format!(
            "Keypair file not found: {:?}",
            keypair
        )));
    }

    // Load and validate manifest
    let manifest_json = std::fs::read_to_string(&manifest)
        .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;
    let app_manifest: AppManifest = serde_json::from_str(&manifest_json)
        .map_err(|e| Error::validation(format!("Invalid manifest: {}", e)))?;

    app_manifest
        .validate()
        .map_err(|e| Error::validation(format!("Manifest validation failed: {:?}", e)))?;

    println!("📱 App: {}", app_manifest.metadata.name);
    println!("   Version: {}", app_manifest.version);

    if let Some(bundle_path) = bundle {
        if !bundle_path.exists() {
            return Err(Error::validation(format!(
                "Bundle not found: {:?}",
                bundle_path
            )));
        }
        let bundle_size = std::fs::metadata(&bundle_path)
            .map(|m| m.len())
            .unwrap_or(0);
        println!("   Bundle: {:?} ({} KB)", bundle_path, bundle_size / 1024);
    }

    println!();
    println!("💡 In production, this would:");
    println!("   1. Upload bundle to IPFS/decentralized storage");
    println!("   2. Submit registration transaction to chat chain");
    println!("   3. Await verification from network");

    Ok(())
}

/// Handle `dchat miniapp info` command
pub async fn handle_info(app_id: String) -> Result<()> {
    println!("\n📱 MINI-APP INFORMATION");
    println!("══════════════════════════════════════════════════════════════════");
    println!("App ID: {}", app_id);
    println!();
    println!("💡 In production, this would query the on-chain registry");
    println!("   for app metadata, developer info, and download stats.");

    Ok(())
}

/// Handle `dchat miniapp list` command
pub async fn handle_list(category: Option<String>, installed: bool) -> Result<()> {
    println!("\n📋 MINI-APP LIST");
    println!("══════════════════════════════════════════════════════════════════");

    if let Some(cat) = category {
        println!("Filter: category = {}", cat);
    }
    if installed {
        println!("Filter: installed only");
    }

    println!();
    println!("💡 In production, this would query the on-chain registry");
    println!("   and list available/installed mini-apps.");

    Ok(())
}

/// Handle `dchat miniapp init` command
pub async fn handle_init(name: String, path: PathBuf, category: String) -> Result<()> {
    println!("\n🆕 CREATE NEW MINI-APP PROJECT");
    println!("══════════════════════════════════════════════════════════════════");

    let project_dir = path.join(&name);
    std::fs::create_dir_all(&project_dir)
        .map_err(|e| Error::storage(format!("Failed to create directory: {}", e)))?;

    // Create manifest.json
    let manifest = serde_json::json!({
        "manifest_version": { "major": 1, "minor": 0 },
        "version": "1.0.0",
        "metadata": {
            "name": name,
            "short_description": format!("A {} mini-app", category),
            "description": format!("A {} mini-app built for dchat", category),
            "icon": "icon.png",
            "category": category,
            "tags": [category],
            "languages": ["en"],
            "age_rating": "everyone"
        },
        "permissions": [],
        "resources": {
            "entry_point": "index.html",
            "allowed_domains": [],
            "preload": ["app.js", "style.css"]
        },
        "runtime": {
            "max_memory_mb": 128,
            "max_cpu_time_ms": 5000
        }
    });

    let manifest_path = project_dir.join("manifest.json");
    std::fs::write(
        &manifest_path,
        serde_json::to_string_pretty(&manifest).unwrap(),
    )
    .map_err(|e| Error::storage(format!("Failed to write manifest: {}", e)))?;

    // Create index.html
    let index_html = format!(
        r#"<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>{}</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="app">
        <h1>Welcome to {}</h1>
        <p>Your dchat mini-app is ready!</p>
        <button id="main-btn">Click Me</button>
    </div>
    <script src="app.js"></script>
</body>
</html>
"#,
        name, name
    );
    std::fs::write(project_dir.join("index.html"), index_html)
        .map_err(|e| Error::storage(format!("Failed to write index.html: {}", e)))?;

    // Create app.js
    let app_js = r#"// dchat Mini-App JavaScript
console.log('Mini-app loaded!');

// Listen for messages from the dchat sandbox
window.addEventListener('message', (event) => {
    const message = event.data;
    console.log('Received message:', message);
    
    switch (message.type) {
        case 'init':
            console.log('App initialized with config:', message.data.config);
            break;
        case 'theme_change':
            document.body.className = message.data.theme;
            break;
    }
});

// Send ready message to sandbox
window.parent.postMessage({ type: 'ready' }, '*');

// Main button click handler
document.getElementById('main-btn')?.addEventListener('click', () => {
    window.parent.postMessage({
        type: 'permission_request',
        data: { permissions: ['view_balance'] }
    }, '*');
});
"#;
    std::fs::write(project_dir.join("app.js"), app_js)
        .map_err(|e| Error::storage(format!("Failed to write app.js: {}", e)))?;

    // Create style.css
    let style_css = r#"/* dchat Mini-App Styles */
* {
    box-sizing: border-box;
    margin: 0;
    padding: 0;
}

body {
    font-family: -apple-system, BlinkMacSystemFont, 'Segoe UI', Roboto, sans-serif;
    background: linear-gradient(135deg, #667eea 0%, #764ba2 100%);
    min-height: 100vh;
    display: flex;
    justify-content: center;
    align-items: center;
    color: white;
}

body.dark {
    background: linear-gradient(135deg, #1a1a2e 0%, #16213e 100%);
}

#app {
    text-align: center;
    padding: 2rem;
}

h1 {
    font-size: 2rem;
    margin-bottom: 1rem;
}

p {
    font-size: 1.1rem;
    opacity: 0.9;
    margin-bottom: 2rem;
}

button {
    background: white;
    color: #667eea;
    border: none;
    padding: 1rem 2rem;
    font-size: 1rem;
    border-radius: 8px;
    cursor: pointer;
    transition: transform 0.2s, box-shadow 0.2s;
}

button:hover {
    transform: translateY(-2px);
    box-shadow: 0 4px 12px rgba(0,0,0,0.2);
}
"#;
    std::fs::write(project_dir.join("style.css"), style_css)
        .map_err(|e| Error::storage(format!("Failed to write style.css: {}", e)))?;

    println!("✅ Created mini-app project: {:?}", project_dir);
    println!();
    println!("📁 Project structure:");
    println!("   {:?}/", project_dir);
    println!("   ├── manifest.json");
    println!("   ├── index.html");
    println!("   ├── app.js");
    println!("   └── style.css");
    println!();
    println!("💡 Next steps:");
    println!("   1. Edit the files to build your app");
    println!(
        "   2. Validate: dchat miniapp validate --manifest {:?}",
        manifest_path
    );
    println!(
        "   3. Launch:   dchat miniapp launch --manifest {:?}",
        project_dir
    );

    Ok(())
}

/// Handle `dchat miniapp validate` command
pub async fn handle_validate(manifest: PathBuf, verbose: bool) -> Result<()> {
    println!("\n🔍 VALIDATE MINI-APP MANIFEST");
    println!("══════════════════════════════════════════════════════════════════");

    if !manifest.exists() {
        return Err(Error::validation(format!(
            "Manifest file not found: {:?}",
            manifest
        )));
    }

    let manifest_json = std::fs::read_to_string(&manifest)
        .map_err(|e| Error::storage(format!("Failed to read manifest: {}", e)))?;

    println!("📄 File: {:?}", manifest);
    println!("   Size: {} bytes", manifest_json.len());
    println!();

    // Parse JSON
    let app_manifest: AppManifest = match serde_json::from_str(&manifest_json) {
        Ok(m) => m,
        Err(e) => {
            println!("❌ JSON PARSE ERROR");
            println!("   {}", e);
            return Err(Error::validation(format!("JSON parse error: {}", e)));
        }
    };

    // Validate manifest
    match app_manifest.validate() {
        Ok(()) => {
            println!("✅ MANIFEST VALID");
            println!();
            println!("📱 App: {}", app_manifest.metadata.name);
            println!("   Version:     {}", app_manifest.version);
            println!("   Category:    {:?}", app_manifest.metadata.category);
            println!("   Entry Point: {}", app_manifest.resources.entry_point);
            println!(
                "   Permissions: {} requested",
                app_manifest.permissions.len()
            );

            if verbose {
                println!();
                println!("📋 Full Manifest:");
                println!(
                    "{}",
                    serde_json::to_string_pretty(&app_manifest).unwrap_or_default()
                );
            }
        }
        Err(e) => {
            println!("❌ VALIDATION FAILED");
            println!("   {:?}", e);
            return Err(Error::validation(format!("Validation failed: {:?}", e)));
        }
    }

    Ok(())
}
