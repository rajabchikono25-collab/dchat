//! ironclad build - Build the smart contract

use crate::error::{IroncladError, IroncladResult};
use crate::project::Project;
use colored::*;
use indicatif::{ProgressBar, ProgressStyle};
use std::process::Command;
use std::time::Instant;

pub async fn run(release: bool, verify: bool, verbose: bool) -> IroncladResult<()> {
    let start = Instant::now();
    let project = Project::find()?;

    println!(
        "{} {} {}",
        "🔨".bold(),
        "Building".cyan().bold(),
        project.config.project.name.green()
    );

    // Create progress spinner
    let pb = ProgressBar::new_spinner();
    pb.set_style(
        ProgressStyle::default_spinner()
            .template("{spinner:.green} {msg}")
            .unwrap(),
    );

    // Step 1: Compile to WASM
    pb.set_message("Compiling to WASM...");
    pb.enable_steady_tick(std::time::Duration::from_millis(100));

    let mut cmd = Command::new("cargo");
    cmd.arg("build")
        .arg("--target")
        .arg(&project.config.build.target)
        .current_dir(&project.root);

    if release {
        cmd.arg("--release");
    }

    // Add rustflags if configured
    if !project.config.build.rustflags.is_empty() {
        cmd.env("RUSTFLAGS", project.config.build.rustflags.join(" "));
    }

    if verbose {
        cmd.arg("-v");
    }

    let output = cmd
        .output()
        .map_err(|e| IroncladError::BuildFailed(format!("Failed to run cargo: {}", e)))?;

    if !output.status.success() {
        pb.finish_and_clear();
        let stderr = String::from_utf8_lossy(&output.stderr);
        eprintln!("{}", stderr);
        return Err(IroncladError::BuildFailed("Cargo build failed".to_string()));
    }

    pb.set_message("Locating WASM artifact...");

    // Find WASM file
    let wasm_path = project.wasm_path(release);
    if !wasm_path.exists() {
        pb.finish_and_clear();
        return Err(IroncladError::BuildFailed(format!(
            "WASM file not found at {}",
            wasm_path.display()
        )));
    }

    let wasm_size = std::fs::metadata(&wasm_path)?.len();

    pb.finish_and_clear();
    println!(
        "  {} WASM compiled: {} ({})",
        "✓".green(),
        wasm_path.display().to_string().dimmed(),
        format_size(wasm_size)
    );

    // Step 2: Extract IDL if configured
    if project.config.build.generate_idl {
        println!("  {} Extracting IDL from source...", "→".dimmed());

        match extract_idl(&project) {
            Ok(idl) => {
                let idl_path = project.idl_path();
                std::fs::create_dir_all(idl_path.parent().unwrap())?;
                std::fs::write(&idl_path, idl)?;
                println!(
                    "  {} IDL generated: {}",
                    "✓".green(),
                    idl_path.display().to_string().dimmed()
                );
            }
            Err(e) => {
                println!(
                    "  {} IDL extraction failed: {}",
                    "⚠".yellow(),
                    e.to_string().dimmed()
                );
            }
        }
    }

    // Step 3: Verify manifest if requested
    if verify {
        println!("  {} Verifying manifest...", "→".dimmed());

        let verify_output = Command::new("dchat")
            .arg("program")
            .arg("validate")
            .arg("--wasm")
            .arg(&wasm_path)
            .output();

        match verify_output {
            Ok(output) if output.status.success() => {
                println!("  {} Manifest verified", "✓".green());
            }
            Ok(output) => {
                let stderr = String::from_utf8_lossy(&output.stderr);
                println!(
                    "  {} Manifest verification failed: {}",
                    "✗".red(),
                    stderr.trim()
                );
            }
            Err(e) => {
                println!(
                    "  {} Could not verify manifest: {}",
                    "⚠".yellow(),
                    e.to_string().dimmed()
                );
            }
        }
    }

    // Print summary
    let elapsed = start.elapsed();
    println!();
    println!(
        "{}",
        format!("✅ Built in {:.2}s", elapsed.as_secs_f64())
            .green()
            .bold()
    );

    if verbose {
        println!();
        println!("  {}", "Build artifacts:".cyan());
        println!("    WASM: {}", wasm_path.display());
        if project.config.build.generate_idl {
            println!("    IDL:  {}", project.idl_path().display());
        }
    }

    Ok(())
}

/// Extract IDL by parsing the Rust source files in the project
fn extract_idl(project: &Project) -> IroncladResult<String> {
    let src_dir = project.root.join("src");
    let lib_rs = src_dir.join("lib.rs");

    if !lib_rs.exists() {
        return Err(IroncladError::IdlError("src/lib.rs not found".to_string()));
    }

    // Parse the source file
    let source = std::fs::read_to_string(&lib_rs)?;
    let idl = parse_source_to_idl(&source, &project.config.project.name)?;

    serde_json::to_string_pretty(&idl)
        .map_err(|e| IroncladError::IdlError(format!("Failed to serialize IDL: {}", e)))
}

/// Parse Rust source code to extract IDL components
fn parse_source_to_idl(source: &str, program_name: &str) -> IroncladResult<serde_json::Value> {
    let syntax = syn::parse_file(source)
        .map_err(|e| IroncladError::IdlError(format!("Failed to parse Rust source: {}", e)))?;

    let mut instructions = Vec::new();
    let mut accounts = Vec::new();
    let mut types = Vec::new();
    let mut events = Vec::new();
    let mut errors = Vec::new();
    let mut program_module_name = program_name.to_string();

    // First pass: find the #[program] module to get instruction definitions
    for item in &syntax.items {
        if let syn::Item::Mod(item_mod) = item {
            if has_program_attribute(&item_mod.attrs) {
                program_module_name = item_mod.ident.to_string();

                // Extract instructions from public functions in the module
                if let Some((_, items)) = &item_mod.content {
                    for inner_item in items {
                        if let syn::Item::Fn(func) = inner_item {
                            if matches!(func.vis, syn::Visibility::Public(_)) {
                                instructions.push(extract_instruction_from_fn(func)?);
                            }
                        }
                    }
                }
            }
        }
    }

    // Second pass: find #[derive(Accounts)] structs, #[account] structs, #[event], #[error_code]
    for item in &syntax.items {
        match item {
            syn::Item::Struct(item_struct) => {
                // Check for #[derive(Accounts)]
                if has_derive_accounts(&item_struct.attrs) {
                    accounts.push(extract_accounts_struct(item_struct)?);
                }
                // Check for #[account]
                else if has_account_attribute(&item_struct.attrs) {
                    types.push(extract_account_type(item_struct)?);
                }
                // Check for #[event]
                else if has_event_attribute(&item_struct.attrs) {
                    events.push(extract_event_struct(item_struct)?);
                }
            }
            syn::Item::Enum(item_enum) => {
                // Check for #[error_code]
                if has_error_code_attribute(&item_enum.attrs) {
                    errors.extend(extract_error_codes(item_enum)?);
                }
                // Check for types used in instructions
                else if !has_derive_instruction(&item_enum.attrs) {
                    // Regular enum that might be used as a type
                    if is_borsh_serializable(&item_enum.attrs) {
                        types.push(extract_enum_type(item_enum)?);
                    }
                }
            }
            _ => {}
        }
    }

    Ok(serde_json::json!({
        "version": "0.1.0",
        "name": program_module_name,
        "metadata": {
            "sdk": "dchat-dpl",
            "sdk_version": "0.1.0"
        },
        "instructions": instructions,
        "accounts": accounts,
        "types": types,
        "events": events,
        "errors": errors
    }))
}

/// Check if an item has #[program] attribute
fn has_program_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("program"))
}

/// Check if an item has #[derive(Accounts)]
fn has_derive_accounts(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                return tokens.contains("Accounts");
            }
        }
        false
    })
}

/// Check if an item has #[derive(Instruction)]
fn has_derive_instruction(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                return tokens.contains("Instruction");
            }
        }
        false
    })
}

/// Check if an item has #[account] attribute
fn has_account_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("account"))
}

/// Check if an item has #[event] attribute
fn has_event_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("event"))
}

/// Check if an item has #[error_code] attribute
fn has_error_code_attribute(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| attr.path().is_ident("error_code"))
}

/// Check if an item is borsh serializable
fn is_borsh_serializable(attrs: &[syn::Attribute]) -> bool {
    attrs.iter().any(|attr| {
        if attr.path().is_ident("derive") {
            if let syn::Meta::List(meta_list) = &attr.meta {
                let tokens = meta_list.tokens.to_string();
                return tokens.contains("BorshSerialize") || tokens.contains("borsh");
            }
        }
        false
    })
}

/// Extract instruction info from a function
fn extract_instruction_from_fn(func: &syn::ItemFn) -> IroncladResult<serde_json::Value> {
    let name = func.sig.ident.to_string();
    let discriminator = compute_discriminator(&format!("global:{}", name));

    // Extract accounts from Context<T> parameter
    let mut accounts_type = String::new();
    let mut args = Vec::new();

    for (idx, input) in func.sig.inputs.iter().enumerate() {
        if let syn::FnArg::Typed(pat_type) = input {
            if idx == 0 {
                // First arg should be Context<T>
                accounts_type = extract_context_type(&pat_type.ty);
            } else {
                // Other args are instruction arguments
                if let syn::Pat::Ident(pat_ident) = &*pat_type.pat {
                    let arg_name = pat_ident.ident.to_string();
                    let arg_type = type_to_idl_type(&pat_type.ty);
                    args.push(serde_json::json!({
                        "name": arg_name,
                        "type": arg_type
                    }));
                }
            }
        }
    }

    Ok(serde_json::json!({
        "name": name,
        "discriminator": format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            discriminator[0], discriminator[1], discriminator[2], discriminator[3],
            discriminator[4], discriminator[5], discriminator[6], discriminator[7]),
        "accounts": accounts_type,
        "args": args
    }))
}

/// Extract the T from Context<'a, T>
fn extract_context_type(ty: &syn::Type) -> String {
    if let syn::Type::Path(type_path) = ty {
        for segment in &type_path.path.segments {
            if segment.ident == "Context" {
                if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                    for arg in &args.args {
                        if let syn::GenericArgument::Type(inner_ty) = arg {
                            if let syn::Type::Path(inner_path) = inner_ty {
                                if let Some(last_seg) = inner_path.path.segments.last() {
                                    return last_seg.ident.to_string();
                                }
                            }
                        }
                    }
                }
            }
        }
    }
    String::new()
}

/// Extract accounts structure info
fn extract_accounts_struct(item: &syn::ItemStruct) -> IroncladResult<serde_json::Value> {
    let name = item.ident.to_string();
    let mut fields = Vec::new();

    if let syn::Fields::Named(named) = &item.fields {
        for field in &named.named {
            if let Some(ident) = &field.ident {
                let field_name = ident.to_string();
                let field_type = extract_account_field_type(&field.ty);
                let constraints = extract_account_constraints(&field.attrs);

                fields.push(serde_json::json!({
                    "name": field_name,
                    "type": field_type,
                    "isMut": constraints.is_mut,
                    "isSigner": constraints.is_signer,
                    "pda": constraints.pda_seeds
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "name": name,
        "fields": fields
    }))
}

#[derive(Default)]
struct AccountConstraints {
    is_mut: bool,
    is_signer: bool,
    pda_seeds: Option<Vec<String>>,
}

/// Extract account constraints from #[account(...)] attributes
fn extract_account_constraints(attrs: &[syn::Attribute]) -> AccountConstraints {
    let mut constraints = AccountConstraints::default();

    for attr in attrs {
        if attr.path().is_ident("account") {
            let tokens = attr.meta.to_token_stream().to_string();
            constraints.is_mut = tokens.contains("mut") || tokens.contains("init");
            constraints.is_signer = tokens.contains("signer");

            // Extract seeds if present
            if tokens.contains("seeds") {
                // Simplified: just note that it's a PDA
                constraints.pda_seeds = Some(vec!["<pda>".to_string()]);
            }
        }
    }

    constraints
}

/// Extract the inner type from Account<'info, T>, Signer<'info>, etc.
fn extract_account_field_type(ty: &syn::Type) -> String {
    if let syn::Type::Path(type_path) = ty {
        if let Some(segment) = type_path.path.segments.last() {
            let ident = segment.ident.to_string();
            match ident.as_str() {
                "Account" | "Signer" | "Program" | "SystemAccount" => {
                    if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                        for arg in &args.args {
                            if let syn::GenericArgument::Type(inner_ty) = arg {
                                if let syn::Type::Path(inner_path) = inner_ty {
                                    if let Some(last) = inner_path.path.segments.last() {
                                        if last.ident != "info" {
                                            return format!("{}:{}", ident, last.ident);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    return ident;
                }
                _ => return ident,
            }
        }
    }
    "unknown".to_string()
}

/// Extract account type definition from #[account] struct
fn extract_account_type(item: &syn::ItemStruct) -> IroncladResult<serde_json::Value> {
    let name = item.ident.to_string();
    let discriminator = compute_discriminator(&format!("account:{}", name));
    let mut fields = Vec::new();

    if let syn::Fields::Named(named) = &item.fields {
        for field in &named.named {
            if let Some(ident) = &field.ident {
                fields.push(serde_json::json!({
                    "name": ident.to_string(),
                    "type": type_to_idl_type(&field.ty)
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "name": name,
        "type": {
            "kind": "struct",
            "fields": fields
        },
        "discriminator": format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            discriminator[0], discriminator[1], discriminator[2], discriminator[3],
            discriminator[4], discriminator[5], discriminator[6], discriminator[7])
    }))
}

/// Extract event struct
fn extract_event_struct(item: &syn::ItemStruct) -> IroncladResult<serde_json::Value> {
    let name = item.ident.to_string();
    let discriminator = compute_discriminator(&format!("event:{}", name));
    let mut fields = Vec::new();

    if let syn::Fields::Named(named) = &item.fields {
        for field in &named.named {
            if let Some(ident) = &field.ident {
                fields.push(serde_json::json!({
                    "name": ident.to_string(),
                    "type": type_to_idl_type(&field.ty)
                }));
            }
        }
    }

    Ok(serde_json::json!({
        "name": name,
        "fields": fields,
        "discriminator": format!("{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}{:02x}",
            discriminator[0], discriminator[1], discriminator[2], discriminator[3],
            discriminator[4], discriminator[5], discriminator[6], discriminator[7])
    }))
}

/// Extract error codes from #[error_code] enum
fn extract_error_codes(item: &syn::ItemEnum) -> IroncladResult<Vec<serde_json::Value>> {
    let mut errors = Vec::new();

    for variant in &item.variants {
        let name = variant.ident.to_string();
        let mut code: u32 = 6000; // Default starting code
        let mut msg = String::new();

        for attr in &variant.attrs {
            let tokens = attr.meta.to_token_stream().to_string();
            if tokens.contains("code") {
                // Extract code = NNNN
                if let Some(idx) = tokens.find("code") {
                    let rest = &tokens[idx..];
                    if let Some(eq_idx) = rest.find('=') {
                        let num_part = rest[eq_idx + 1..].trim();
                        if let Some(end) = num_part.find(|c: char| !c.is_ascii_digit()) {
                            code = num_part[..end].trim().parse().unwrap_or(6000);
                        } else {
                            code = num_part
                                .trim_end_matches(']')
                                .trim()
                                .parse()
                                .unwrap_or(6000);
                        }
                    }
                }
            }
            if tokens.contains("msg") {
                // Extract msg = "..."
                if let Some(start) = tokens.find('"') {
                    if let Some(end) = tokens[start + 1..].find('"') {
                        msg = tokens[start + 1..start + 1 + end].to_string();
                    }
                }
            }
        }

        errors.push(serde_json::json!({
            "code": code,
            "name": name,
            "msg": msg
        }));
    }

    Ok(errors)
}

/// Extract enum type definition
fn extract_enum_type(item: &syn::ItemEnum) -> IroncladResult<serde_json::Value> {
    let name = item.ident.to_string();
    let variants: Vec<_> = item
        .variants
        .iter()
        .map(|v| {
            serde_json::json!({
                "name": v.ident.to_string()
            })
        })
        .collect();

    Ok(serde_json::json!({
        "name": name,
        "type": {
            "kind": "enum",
            "variants": variants
        }
    }))
}

/// Convert a Rust type to IDL type string
fn type_to_idl_type(ty: &syn::Type) -> String {
    match ty {
        syn::Type::Path(type_path) => {
            if let Some(segment) = type_path.path.segments.last() {
                let ident = segment.ident.to_string();
                match ident.as_str() {
                    "u8" => "u8".to_string(),
                    "u16" => "u16".to_string(),
                    "u32" => "u32".to_string(),
                    "u64" => "u64".to_string(),
                    "u128" => "u128".to_string(),
                    "i8" => "i8".to_string(),
                    "i16" => "i16".to_string(),
                    "i32" => "i32".to_string(),
                    "i64" => "i64".to_string(),
                    "i128" => "i128".to_string(),
                    "bool" => "bool".to_string(),
                    "String" => "string".to_string(),
                    "Pubkey" => "pubkey".to_string(),
                    "Vec" => {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                                return format!("vec<{}>", type_to_idl_type(inner));
                            }
                        }
                        "vec<unknown>".to_string()
                    }
                    "Option" => {
                        if let syn::PathArguments::AngleBracketed(args) = &segment.arguments {
                            if let Some(syn::GenericArgument::Type(inner)) = args.args.first() {
                                return format!("option<{}>", type_to_idl_type(inner));
                            }
                        }
                        "option<unknown>".to_string()
                    }
                    _ => ident, // Custom type reference
                }
            } else {
                "unknown".to_string()
            }
        }
        syn::Type::Array(arr) => {
            let inner = type_to_idl_type(&arr.elem);
            if let syn::Expr::Lit(syn::ExprLit {
                lit: syn::Lit::Int(lit_int),
                ..
            }) = &arr.len
            {
                return format!("array<{}, {}>", inner, lit_int.base10_digits());
            }
            format!("array<{}>", inner)
        }
        _ => "unknown".to_string(),
    }
}

/// Compute instruction/account/event discriminator using FNV-1a hash
/// (Matches the compile-time discriminator in the macro)
fn compute_discriminator(input: &str) -> [u8; 8] {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    let mut hash = FNV_OFFSET;
    for byte in input.as_bytes() {
        hash ^= *byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash.to_le_bytes()
}

use quote::ToTokens;

fn format_size(bytes: u64) -> String {
    if bytes < 1024 {
        format!("{} B", bytes)
    } else if bytes < 1024 * 1024 {
        format!("{:.1} KB", bytes as f64 / 1024.0)
    } else {
        format!("{:.2} MB", bytes as f64 / (1024.0 * 1024.0))
    }
}
