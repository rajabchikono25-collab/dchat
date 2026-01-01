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

    // Parse the WASM binary to find dpl_manifest custom section
    // WASM binary starts with magic (\0asm) and version
    if wasm_bytes.len() < 8 || &wasm_bytes[0..4] != b"\0asm" {
        return Err(IroncladError::IdlError("Invalid WASM file".to_string()));
    }

    let mut offset = 8; // Skip magic and version
    let mut manifest_data: Option<&[u8]> = None;
    let program_name = wasm_path
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "program".to_string());

    // Parse sections
    while offset < wasm_bytes.len() {
        if offset >= wasm_bytes.len() {
            break;
        }

        let section_id = wasm_bytes[offset];
        offset += 1;

        // Read section size (LEB128)
        let (section_size, bytes_read) = read_leb128(&wasm_bytes[offset..])?;
        offset += bytes_read;

        if section_id == 0 {
            // Custom section - check if it's dpl_manifest
            let section_start = offset;
            let (name_len, name_bytes_read) = read_leb128(&wasm_bytes[offset..])?;
            offset += name_bytes_read;

            if offset + name_len > wasm_bytes.len() {
                break;
            }

            let name = &wasm_bytes[offset..offset + name_len];
            offset += name_len;

            if name == b"dpl_manifest" {
                let data_len = section_size - name_bytes_read - name_len;
                if offset + data_len <= wasm_bytes.len() {
                    manifest_data = Some(&wasm_bytes[offset..offset + data_len]);
                }
            }

            // Move to end of section
            offset = section_start + section_size;
        } else {
            // Skip non-custom sections
            offset += section_size;
        }
    }

    // Parse manifest if found
    match manifest_data {
        Some(data) if data.len() >= 64 => {
            // Verify magic bytes
            if &data[0..4] != b"DPLM" {
                return Err(IroncladError::IdlError(
                    "Invalid manifest magic bytes".to_string(),
                ));
            }

            // Parse manifest fields
            let sdk_major = u16::from_le_bytes([data[4], data[5]]);
            let sdk_minor = u16::from_le_bytes([data[6], data[7]]);
            let sdk_patch = u16::from_le_bytes([data[8], data[9]]);
            let edition = u16::from_le_bytes([data[10], data[11]]);
            let abi_version = data[12];
            let import_profile = match data[13] {
                0 => "legacy",
                1 => "wasi",
                2 => "hybrid",
                _ => "unknown",
            };

            // Extract schema hash
            let mut schema_hash = [0u8; 32];
            schema_hash.copy_from_slice(&data[16..48]);

            // Parse capabilities
            let cap_bits = u64::from_le_bytes([
                data[48], data[49], data[50], data[51], data[52], data[53], data[54], data[55],
            ]);

            let mut capabilities = Vec::new();
            if cap_bits & (1 << 0) != 0 {
                capabilities.push("emits_events");
            }
            if cap_bits & (1 << 1) != 0 {
                capabilities.push("uses_cpi");
            }
            if cap_bits & (1 << 2) != 0 {
                capabilities.push("uses_pdas");
            }
            if cap_bits & (1 << 3) != 0 {
                capabilities.push("requires_signers");
            }
            if cap_bits & (1 << 4) != 0 {
                capabilities.push("uses_tokens");
            }
            if cap_bits & (1 << 5) != 0 {
                capabilities.push("uses_privacy");
            }
            if cap_bits & (1 << 6) != 0 {
                capabilities.push("uses_capabilities");
            }
            if cap_bits & (1 << 7) != 0 {
                capabilities.push("upgradeable");
            }

            // Generate IDL from manifest
            let idl = serde_json::json!({
                "version": "0.1.0",
                "name": program_name,
                "metadata": {
                    "sdk_version": format!("{}.{}.{}", sdk_major, sdk_minor, sdk_patch),
                    "edition": edition,
                    "abi_version": abi_version,
                    "import_profile": import_profile,
                    "schema_hash": hex::encode(schema_hash),
                    "capabilities": capabilities,
                    "source": wasm_path.file_name().map(|s| s.to_string_lossy()).unwrap_or_default()
                },
                "instructions": [],
                "accounts": [],
                "types": [],
                "events": [],
                "errors": []
            });

            serde_json::to_string_pretty(&idl)
                .map_err(|e| IroncladError::IdlError(format!("Failed to serialize IDL: {}", e)))
        }
        Some(_) => Err(IroncladError::IdlError(
            "Manifest section too small".to_string(),
        )),
        None => Err(IroncladError::IdlError(
            "No dpl_manifest section found in WASM".to_string(),
        )),
    }
}

/// Read a LEB128 unsigned integer from bytes
fn read_leb128(bytes: &[u8]) -> IroncladResult<(usize, usize)> {
    let mut result: usize = 0;
    let mut shift = 0;
    let mut bytes_read = 0;

    for &byte in bytes {
        bytes_read += 1;
        result |= ((byte & 0x7F) as usize) << shift;
        if byte & 0x80 == 0 {
            return Ok((result, bytes_read));
        }
        shift += 7;
        if shift >= 64 {
            return Err(IroncladError::IdlError("LEB128 overflow".to_string()));
        }
    }

    Err(IroncladError::IdlError(
        "Truncated LEB128 encoding".to_string(),
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
