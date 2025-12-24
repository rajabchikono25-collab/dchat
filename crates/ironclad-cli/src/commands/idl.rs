//! ironclad idl - IDL management

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;
use sha2::{Digest, Sha256};

pub async fn run_extract(program: Option<&str>, output: Option<&str>) -> IroncladResult<()> {
    let project = Project::find()?;

    println!("{} {}", "📄".bold(), "Extracting IDL...".cyan().bold());

    let wasm_path = if let Some(p) = program {
        std::path::PathBuf::from(p)
    } else {
        project.wasm_path(true)
    };

    if !wasm_path.exists() {
        return Err(IroncladError::WasmError(format!(
            "WASM file not found: {}",
            wasm_path.display()
        )));
    }

    let idl = extract_idl_from_wasm(&wasm_path)?;

    let output_path = if let Some(o) = output {
        std::path::PathBuf::from(o)
    } else {
        project.idl_path()
    };

    std::fs::create_dir_all(output_path.parent().unwrap())?;
    std::fs::write(&output_path, &idl)?;

    println!(
        "  {} IDL written to: {}",
        "✓".green(),
        output_path.display()
    );

    Ok(())
}

pub async fn run_hash(idl_file: Option<&str>) -> IroncladResult<()> {
    let project = Project::find()?;

    let idl_path = if let Some(f) = idl_file {
        std::path::PathBuf::from(f)
    } else {
        project.idl_path()
    };

    if !idl_path.exists() {
        return Err(IroncladError::IdlError(format!(
            "IDL file not found: {}. Run 'ironclad idl extract' first.",
            idl_path.display()
        )));
    }

    let idl_content = std::fs::read(&idl_path)?;
    let mut hasher = Sha256::new();
    hasher.update(&idl_content);
    let hash = hasher.finalize();

    println!("{} {}", "🔐".bold(), "IDL Hash".cyan().bold());
    println!();
    println!("  File: {}", idl_path.display().to_string().dimmed());
    println!("  SHA256: {}", hex::encode(hash).green());

    Ok(())
}

pub async fn run_validate(idl_file: Option<&str>) -> IroncladResult<()> {
    let project = Project::find()?;

    println!("{} {}", "✅".bold(), "Validating IDL...".cyan().bold());

    let idl_path = if let Some(f) = idl_file {
        std::path::PathBuf::from(f)
    } else {
        project.idl_path()
    };

    if !idl_path.exists() {
        return Err(IroncladError::IdlError(format!(
            "IDL file not found: {}",
            idl_path.display()
        )));
    }

    let idl_content = std::fs::read_to_string(&idl_path)?;

    // Parse as JSON
    let idl: serde_json::Value = serde_json::from_str(&idl_content)
        .map_err(|e| IroncladError::IdlError(format!("Invalid JSON: {}", e)))?;

    // Validate required fields
    let required = ["version", "name"];
    for field in required {
        if idl.get(field).is_none() {
            return Err(IroncladError::IdlError(format!(
                "Missing required field: {}",
                field
            )));
        }
    }

    println!("  {} IDL is valid", "✓".green());
    println!(
        "    Version: {}",
        idl["version"].as_str().unwrap_or("unknown")
    );
    println!("    Name: {}", idl["name"].as_str().unwrap_or("unknown"));

    if let Some(instructions) = idl.get("instructions").and_then(|v| v.as_array()) {
        println!("    Instructions: {}", instructions.len());
    }

    if let Some(accounts) = idl.get("accounts").and_then(|v| v.as_array()) {
        println!("    Accounts: {}", accounts.len());
    }

    if let Some(events) = idl.get("events").and_then(|v| v.as_array()) {
        println!("    Events: {}", events.len());
    }

    if let Some(errors) = idl.get("errors").and_then(|v| v.as_array()) {
        println!("    Errors: {}", errors.len());
    }

    Ok(())
}

pub async fn run_generate(
    idl_file: Option<&str>,
    language: &str,
    output: Option<&str>,
) -> IroncladResult<()> {
    println!(
        "{} {} client...",
        "🔧".bold(),
        format!("Generating {}", language).cyan().bold()
    );

    let project = Project::find()?;

    let idl_path = if let Some(f) = idl_file {
        std::path::PathBuf::from(f)
    } else {
        project.idl_path()
    };

    if !idl_path.exists() {
        return Err(IroncladError::IdlError(format!(
            "IDL file not found: {}",
            idl_path.display()
        )));
    }

    let _idl_content = std::fs::read_to_string(&idl_path)?;

    let output_file = match language {
        "typescript" | "ts" => {
            let out = output
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("generated/{}.ts", project.config.project.name));
            // In production, would generate actual TypeScript client
            let ts_code = generate_typescript_client(&project.config.project.name);
            std::fs::create_dir_all(std::path::Path::new(&out).parent().unwrap())?;
            std::fs::write(&out, ts_code)?;
            out
        }
        "rust" | "rs" => {
            let out = output
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("generated/{}_client.rs", project.config.project.name));
            // In production, would generate actual Rust client
            let rs_code = generate_rust_client(&project.config.project.name);
            std::fs::create_dir_all(std::path::Path::new(&out).parent().unwrap())?;
            std::fs::write(&out, rs_code)?;
            out
        }
        "python" | "py" => {
            let out = output
                .map(|s| s.to_string())
                .unwrap_or_else(|| format!("generated/{}_client.py", project.config.project.name));
            let py_code = generate_python_client(&project.config.project.name);
            std::fs::create_dir_all(std::path::Path::new(&out).parent().unwrap())?;
            std::fs::write(&out, py_code)?;
            out
        }
        _ => {
            return Err(IroncladError::IdlError(format!(
                "Unknown language: {}. Supported: typescript, rust, python",
                language
            )));
        }
    };

    println!("  {} Generated: {}", "✓".green(), output_file);

    Ok(())
}

fn extract_idl_from_wasm(wasm_path: &std::path::Path) -> IroncladResult<String> {
    let wasm_bytes = std::fs::read(wasm_path)?;

    // Look for DPLM section
    for (i, window) in wasm_bytes.windows(4).enumerate() {
        if window == b"DPLM" {
            if i + 7 > wasm_bytes.len() {
                return Err(IroncladError::IdlError("Truncated manifest".to_string()));
            }

            let version = wasm_bytes[i + 4];
            let len_bytes: [u8; 2] = [wasm_bytes[i + 5], wasm_bytes[i + 6]];
            let len = u16::from_le_bytes(len_bytes) as usize;

            if i + 7 + len > wasm_bytes.len() {
                return Err(IroncladError::IdlError(
                    "Invalid manifest length".to_string(),
                ));
            }

            let _manifest_data = &wasm_bytes[i + 7..i + 7 + len];

            // Generate IDL from manifest
            let idl = serde_json::json!({
                "version": "0.1.0",
                "name": "program",
                "metadata": {
                    "manifest_version": version,
                    "source": wasm_path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                },
                "instructions": [],
                "accounts": [],
                "types": [],
                "events": [],
                "errors": []
            });

            return serde_json::to_string_pretty(&idl)
                .map_err(|e| IroncladError::IdlError(format!("Failed to serialize IDL: {}", e)));
        }
    }

    Err(IroncladError::IdlError(
        "No DPLM manifest found in WASM".to_string(),
    ))
}

fn generate_typescript_client(name: &str) -> String {
    format!(
        r#"// Auto-generated TypeScript client for {name}
// Generated by ironclad idl generate

import {{ PublicKey, Connection, Transaction }} from '@dchat/web3.js';

export class {class_name}Client {{
    constructor(
        public readonly programId: PublicKey,
        public readonly connection: Connection,
    ) {{}}

    // TODO: Add instruction methods from IDL
}}

export const PROGRAM_ID = new PublicKey('TODO_PROGRAM_ID');
"#,
        name = name,
        class_name = to_pascal_case(name)
    )
}

fn generate_rust_client(name: &str) -> String {
    format!(
        r#"//! Auto-generated Rust client for {name}
//! Generated by ironclad idl generate

use dchat_dpl::prelude::*;

// TODO: Add instruction methods from IDL

pub const PROGRAM_ID: &str = "TODO_PROGRAM_ID";
"#,
        name = name
    )
}

fn generate_python_client(name: &str) -> String {
    format!(
        r#"# Auto-generated Python client for {name}
# Generated by ironclad idl generate

from dchat.rpc import Client

class {class_name}Client:
    def __init__(self, program_id: str, client: Client):
        self.program_id = program_id
        self.client = client
    
    # TODO: Add instruction methods from IDL

PROGRAM_ID = "TODO_PROGRAM_ID"
"#,
        name = name,
        class_name = to_pascal_case(name)
    )
}

fn to_pascal_case(s: &str) -> String {
    s.split(&['-', '_'][..])
        .map(|word| {
            let mut chars = word.chars();
            match chars.next() {
                None => String::new(),
                Some(first) => first.to_uppercase().chain(chars).collect(),
            }
        })
        .collect()
}
