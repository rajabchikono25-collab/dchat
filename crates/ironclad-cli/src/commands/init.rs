//! ironclad init - Initialize a new project

use crate::config::{BuildConfig, IroncladConfig, ProjectConfig};
use crate::error::{IroncladError, IroncladResult};
use colored::*;
use std::fs;
use std::path::Path;

pub async fn run(name: &str, template: &str, here: bool, verbose: bool) -> IroncladResult<()> {
    println!(
        "{} {} {}",
        "🔨".bold(),
        "Initializing new Ironclad project:".cyan().bold(),
        name.green()
    );

    let project_dir = if here {
        std::env::current_dir()?
    } else {
        std::env::current_dir()?.join(name)
    };

    // Check if directory already exists
    if project_dir.exists() && !here {
        return Err(IroncladError::Custom(format!(
            "Directory '{}' already exists",
            project_dir.display()
        )));
    }

    // Create project directory
    fs::create_dir_all(&project_dir)?;

    if verbose {
        println!("  {} Creating project structure...", "→".dimmed());
    }

    // Create directory structure
    fs::create_dir_all(project_dir.join("src"))?;
    fs::create_dir_all(project_dir.join("tests"))?;
    fs::create_dir_all(project_dir.join("idl"))?;

    // Create Cargo.toml
    let cargo_toml = generate_cargo_toml(name);
    fs::write(project_dir.join("Cargo.toml"), cargo_toml)?;

    // Create lib.rs from template
    let lib_rs = get_template(template)?;
    fs::write(project_dir.join("src").join("lib.rs"), lib_rs)?;

    // Create build.rs
    let build_rs = generate_build_rs();
    fs::write(project_dir.join("build.rs"), build_rs)?;

    // Create Ironclad.toml
    let config = IroncladConfig {
        project: ProjectConfig {
            name: name.to_string(),
            version: "0.1.0".to_string(),
            program_id: None,
            authors: vec![],
            description: Some(format!("{} - A dchat smart contract", name)),
        },
        build: BuildConfig {
            target: "wasm32-unknown-unknown".to_string(),
            generate_idl: true,
            ..Default::default()
        },
        ..Default::default()
    };
    config.save(&project_dir.join("Ironclad.toml"))?;

    // Create .gitignore
    let gitignore = generate_gitignore();
    fs::write(project_dir.join(".gitignore"), gitignore)?;

    // Create README.md
    let readme = generate_readme(name);
    fs::write(project_dir.join("README.md"), readme)?;

    // Create basic test file
    let test_file = generate_test_file(name);
    fs::write(project_dir.join("tests").join("integration.rs"), test_file)?;

    println!();
    println!("{}", "✅ Project initialized successfully!".green().bold());
    println!();
    println!("  {}", "Next steps:".cyan().bold());
    println!();
    if !here {
        println!("    cd {}", name);
    }
    println!("    ironclad build");
    println!("    ironclad test");
    println!("    ironclad deploy --network devnet");
    println!();

    Ok(())
}

fn get_template(name: &str) -> IroncladResult<String> {
    match name {
        "counter" => Ok(COUNTER_TEMPLATE.to_string()),
        "escrow" => Ok(ESCROW_TEMPLATE.to_string()),
        "token" => Ok(TOKEN_TEMPLATE.to_string()),
        "custom" | "empty" => Ok(EMPTY_TEMPLATE.to_string()),
        _ => Err(IroncladError::InvalidTemplate(format!(
            "Unknown template '{}'. Available: counter, escrow, token, custom",
            name
        ))),
    }
}

fn generate_cargo_toml(name: &str) -> String {
    format!(
        r#"[package]
name = "{name}"
version = "0.1.0"
edition = "2021"

[lib]
crate-type = ["cdylib", "rlib"]

[dependencies]
# Note: In production, these would be published crates from crates.io
# For now, use local path or git dependency
dchat-dpl = {{ path = "../../dchat/crates/dchat-dpl", features = ["std"] }}

[build-dependencies]
dchat-dpl = {{ path = "../../dchat/crates/dchat-dpl", features = ["build"] }}

[dev-dependencies]
tokio = {{ version = "1", features = ["full"] }}

[features]
default = []
release-verify = []

[profile.release]
opt-level = "z"
lto = true
codegen-units = 1
panic = "abort"
"#,
        name = name
    )
}

fn generate_build_rs() -> String {
    r#"//! Build script for manifest metadata generation

fn main() {
    // Generate DPL manifest metadata
    let config = dchat_dpl::build::BuildConfig::default();
    
    if let Err(e) = dchat_dpl::build::generate_manifest_metadata(&config) {
        println!("cargo:warning=DPL build: {}", e);
    }
    
    println!("cargo:rerun-if-changed=src/lib.rs");
    println!("cargo:rerun-if-env-changed=DPL_SCHEMA_HASH");
}
"#
    .to_string()
}

fn generate_gitignore() -> String {
    r#"/target
Cargo.lock
*.swp
*.swo
.DS_Store
.env
.env.local
*.pem
*.json
!idl/*.json
"#
    .to_string()
}

fn generate_readme(name: &str) -> String {
    format!(
        r#"# {name}

A dchat smart contract built with [Ironclad](https://github.com/dchat/dchat).

## Build

```bash
ironclad build
```

## Test

```bash
ironclad test
```

## Deploy

```bash
# Deploy to devnet
ironclad deploy --network devnet

# Deploy to mainnet
ironclad deploy --network mainnet
```

## Verify

```bash
ironclad verify
```

## Commands

| Command | Description |
|---------|-------------|
| `ironclad build` | Build the smart contract |
| `ironclad test` | Run tests |
| `ironclad verify` | Verify manifest and schema |
| `ironclad deploy` | Deploy to network |
| `ironclad idl extract` | Extract IDL from WASM |
| `ironclad info` | Show program info |
"#,
        name = name
    )
}

fn generate_test_file(name: &str) -> String {
    let mod_name = name.replace('-', "_");
    format!(
        r#"//! Integration tests for {name}

use {mod_name}::*;

#[test]
fn test_instruction_tags() {{
    // Verify instruction tags are stable
    // Add your tests here
}}
"#,
        name = name,
        mod_name = mod_name
    )
}

const COUNTER_TEMPLATE: &str = r#"//! Counter Program
//!
//! A simple counter demonstrating DPL basics.

use dchat_dpl::prelude::*;

#[program]
pub mod counter {
    use super::*;

    /// Initialize a new counter
    pub fn initialize<'a>(mut ctx: Context<'a, Initialize<'a>>, initial_value: u64) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let authority_key = *ctx.accounts.authority.key();
        let counter = &mut ctx.accounts.counter;

        counter.value = initial_value;
        counter.authority = authority_key;
        counter.bump = ctx.bumps.counter;

        emit!(CounterInitialized {
            counter: counter_key,
            authority: authority_key,
            initial_value,
        });

        Ok(())
    }

    /// Increment the counter
    pub fn increment<'a>(mut ctx: Context<'a, Increment<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        counter.value = counter.value.checked_add(1).ok_or(CounterError::Overflow)?;

        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
        });

        Ok(())
    }

    /// Decrement the counter
    pub fn decrement<'a>(mut ctx: Context<'a, Decrement<'a>>) -> Result<()> {
        let counter_key = *ctx.accounts.counter.key();
        let counter = &mut ctx.accounts.counter;
        let old_value = counter.value;

        counter.value = counter.value.checked_sub(1).ok_or(CounterError::Underflow)?;

        emit!(CounterChanged {
            counter: counter_key,
            old_value,
            new_value: counter.value,
        });

        Ok(())
    }
}

#[derive(Instruction)]
pub enum CounterInstruction {
    #[tag = 0]
    Initialize { initial_value: u64 },
    #[tag = 1]
    Increment,
    #[tag = 2]
    Decrement,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, space = Counter::SIZE, seeds = [b"counter", authority.key().as_ref()], bump)]
    pub counter: Account<'info, Counter>,
    #[account(signer)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Increment<'info> {
    #[account(mut)]
    pub counter: Account<'info, Counter>,
}

#[derive(Accounts)]
pub struct Decrement<'info> {
    #[account(mut)]
    pub counter: Account<'info, Counter>,
}

#[account]
pub struct Counter {
    pub value: u64,
    pub authority: Pubkey,
    pub bump: u8,
}

impl Counter {
    pub const SIZE: usize = 8 + 8 + 32 + 1;
}

#[event]
pub struct CounterInitialized {
    pub counter: Pubkey,
    pub authority: Pubkey,
    pub initial_value: u64,
}

#[event]
pub struct CounterChanged {
    pub counter: Pubkey,
    pub old_value: u64,
    pub new_value: u64,
}

#[error_code]
pub enum CounterError {
    #[code = 6000]
    #[msg = "Counter overflow"]
    Overflow,
    #[code = 6001]
    #[msg = "Counter underflow"]
    Underflow,
}
"#;

const ESCROW_TEMPLATE: &str = r#"//! Escrow Program
//!
//! A secure escrow for conditional token transfers.

use dchat_dpl::prelude::*;

#[program]
pub mod escrow {
    use super::*;

    /// Initialize a new escrow
    pub fn initialize<'a>(
        mut ctx: Context<'a, Initialize<'a>>,
        amount: u64,
        unlock_time: u64,
    ) -> Result<()> {
        if amount == 0 {
            return Err(EscrowError::ZeroAmount.into());
        }

        let escrow = &mut ctx.accounts.escrow;
        escrow.depositor = *ctx.accounts.depositor.key();
        escrow.recipient = *ctx.accounts.recipient.key();
        escrow.amount = amount;
        escrow.unlock_time = unlock_time;
        escrow.state = 0; // Active
        escrow.bump = ctx.bumps.escrow;

        Ok(())
    }

    /// Release tokens to recipient
    pub fn release<'a>(ctx: Context<'a, Release<'a>>) -> Result<()> {
        let escrow = &ctx.accounts.escrow;
        
        if escrow.state != 0 {
            return Err(EscrowError::InvalidState.into());
        }

        // In production, transfer tokens here
        Ok(())
    }

    /// Cancel and refund
    pub fn cancel<'a>(mut ctx: Context<'a, Cancel<'a>>) -> Result<()> {
        let escrow = &mut ctx.accounts.escrow;
        escrow.state = 2; // Cancelled
        Ok(())
    }
}

#[derive(Instruction)]
pub enum EscrowInstruction {
    #[tag = 0]
    Initialize { amount: u64, unlock_time: u64 },
    #[tag = 1]
    Release,
    #[tag = 2]
    Cancel,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(init, space = Escrow::SIZE, seeds = [b"escrow", depositor.key().as_ref()], bump)]
    pub escrow: Account<'info, Escrow>,
    #[account(signer)]
    pub depositor: Signer<'info>,
    #[account(signer)]
    pub recipient: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct Release<'info> {
    #[account(mut)]
    pub escrow: Account<'info, Escrow>,
    #[account(signer)]
    pub recipient: Signer<'info>,
}

#[derive(Accounts)]
pub struct Cancel<'info> {
    #[account(mut, has_one = depositor @ EscrowError::Unauthorized)]
    pub escrow: Account<'info, Escrow>,
    #[account(signer)]
    pub depositor: Signer<'info>,
}

#[account]
pub struct Escrow {
    pub depositor: Pubkey,
    pub recipient: Pubkey,
    pub amount: u64,
    pub unlock_time: u64,
    pub state: u8,
    pub bump: u8,
}

impl Escrow {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 8 + 1 + 1;
}

#[error_code]
pub enum EscrowError {
    #[code = 6000]
    #[msg = "Amount must be greater than zero"]
    ZeroAmount,
    #[code = 6001]
    #[msg = "Invalid escrow state"]
    InvalidState,
    #[code = 6002]
    #[msg = "Unauthorized"]
    Unauthorized,
}
"#;

const TOKEN_TEMPLATE: &str = r#"//! Token Program
//!
//! A fungible token implementation.

use dchat_dpl::prelude::*;

#[program]
pub mod token {
    use super::*;

    /// Initialize a new token mint
    pub fn initialize_mint<'a>(
        mut ctx: Context<'a, InitializeMint<'a>>,
        decimals: u8,
        name_len: u8,
    ) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        mint.authority = *ctx.accounts.authority.key();
        mint.supply = 0;
        mint.decimals = decimals;
        mint.bump = ctx.bumps.mint;
        Ok(())
    }

    /// Mint tokens to an account
    pub fn mint_to<'a>(mut ctx: Context<'a, MintTo<'a>>, amount: u64) -> Result<()> {
        let mint = &mut ctx.accounts.mint;
        let account = &mut ctx.accounts.account;
        
        mint.supply = mint.supply.checked_add(amount).ok_or(TokenError::Overflow)?;
        account.amount = account.amount.checked_add(amount).ok_or(TokenError::Overflow)?;
        
        Ok(())
    }

    /// Transfer tokens between accounts
    pub fn transfer<'a>(mut ctx: Context<'a, Transfer<'a>>, amount: u64) -> Result<()> {
        let from = &mut ctx.accounts.from;
        let to = &mut ctx.accounts.to;
        
        from.amount = from.amount.checked_sub(amount).ok_or(TokenError::InsufficientFunds)?;
        to.amount = to.amount.checked_add(amount).ok_or(TokenError::Overflow)?;
        
        Ok(())
    }
}

#[derive(Instruction)]
pub enum TokenInstruction {
    #[tag = 0]
    InitializeMint { decimals: u8, name_len: u8 },
    #[tag = 1]
    MintTo { amount: u64 },
    #[tag = 2]
    Transfer { amount: u64 },
}

#[derive(Accounts)]
pub struct InitializeMint<'info> {
    #[account(init, space = Mint::SIZE, seeds = [b"mint", authority.key().as_ref()], bump)]
    pub mint: Account<'info, Mint>,
    #[account(signer)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}

#[derive(Accounts)]
pub struct MintTo<'info> {
    #[account(mut, has_one = authority @ TokenError::Unauthorized)]
    pub mint: Account<'info, Mint>,
    #[account(mut)]
    pub account: Account<'info, TokenAccount>,
    #[account(signer)]
    pub authority: Signer<'info>,
}

#[derive(Accounts)]
pub struct Transfer<'info> {
    #[account(mut)]
    pub from: Account<'info, TokenAccount>,
    #[account(mut)]
    pub to: Account<'info, TokenAccount>,
    #[account(signer)]
    pub owner: Signer<'info>,
}

#[account]
pub struct Mint {
    pub authority: Pubkey,
    pub supply: u64,
    pub decimals: u8,
    pub bump: u8,
}

impl Mint {
    pub const SIZE: usize = 8 + 32 + 8 + 1 + 1;
}

#[account]
pub struct TokenAccount {
    pub mint: Pubkey,
    pub owner: Pubkey,
    pub amount: u64,
    pub bump: u8,
}

impl TokenAccount {
    pub const SIZE: usize = 8 + 32 + 32 + 8 + 1;
}

#[error_code]
pub enum TokenError {
    #[code = 6000]
    #[msg = "Arithmetic overflow"]
    Overflow,
    #[code = 6001]
    #[msg = "Insufficient funds"]
    InsufficientFunds,
    #[code = 6002]
    #[msg = "Unauthorized"]
    Unauthorized,
}
"#;

const EMPTY_TEMPLATE: &str = r#"//! Custom Program
//!
//! Your dchat smart contract.

use dchat_dpl::prelude::*;

#[program]
pub mod program {
    use super::*;

    /// Your first instruction
    pub fn initialize<'a>(ctx: Context<'a, Initialize<'a>>) -> Result<()> {
        Ok(())
    }
}

#[derive(Instruction)]
pub enum ProgramInstruction {
    #[tag = 0]
    Initialize,
}

#[derive(Accounts)]
pub struct Initialize<'info> {
    #[account(signer)]
    pub authority: Signer<'info>,
    pub system_program: Program<'info, System>,
}
"#;
