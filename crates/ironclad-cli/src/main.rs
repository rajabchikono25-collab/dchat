//! Ironclad CLI - DPL Smart Contract Development Framework
//!
//! Ironclad is to dchat what Anchor is to Solana. It provides:
//!
//! - Project scaffolding (`ironclad init`)
//! - Build pipeline (`ironclad build`)
//! - Testing framework (`ironclad test`)
//! - Deployment tools (`ironclad deploy`)
//! - Manifest verification (`ironclad verify`)
//! - IDL generation (`ironclad idl`)
//!
//! # Quick Start
//!
//! ```bash
//! # Create a new project
//! ironclad init my-program
//!
//! # Build for deployment
//! ironclad build
//!
//! # Run tests
//! ironclad test
//!
//! # Verify manifest
//! ironclad verify
//!
//! # Deploy to network
//! ironclad deploy --network devnet
//! ```

use clap::{Parser, Subcommand};
use colored::*;
use std::path::PathBuf;
use std::process::ExitCode;

mod commands;
mod config;
mod error;
mod project;

use commands::*;
use error::IroncladResult;

/// Ironclad - DPL Smart Contract Development Framework
///
/// Build, test, and deploy dchat smart contracts with ease.
/// Similar to Anchor for Solana, but for the dchat ecosystem.
#[derive(Parser)]
#[command(name = "ironclad")]
#[command(author = "dchat team")]
#[command(version)]
#[command(about = "DPL smart contract development framework", long_about = None)]
#[command(propagate_version = true)]
struct Cli {
    /// Verbose output
    #[arg(short, long, global = true)]
    verbose: bool,

    /// Config file path
    #[arg(short, long, global = true, default_value = "Ironclad.toml")]
    config: PathBuf,

    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    /// Initialize a new Ironclad project
    Init {
        /// Project name
        name: String,

        /// Template to use (counter, escrow, token, custom)
        #[arg(short, long, default_value = "counter")]
        template: String,

        /// Initialize in current directory
        #[arg(long)]
        here: bool,
    },

    /// Build the smart contract
    Build {
        /// Build in release mode
        #[arg(short, long)]
        release: bool,

        /// Target directory
        #[arg(short, long)]
        target_dir: Option<PathBuf>,

        /// Generate IDL
        #[arg(long)]
        idl: bool,

        /// Verify schema hash after build
        #[arg(long)]
        verify: bool,

        /// Expected schema hash (for CI)
        #[arg(long)]
        expected_hash: Option<String>,
    },

    /// Run tests
    Test {
        /// Run specific test
        #[arg(short, long)]
        test: Option<String>,

        /// Skip build before testing
        #[arg(long)]
        skip_build: bool,

        /// Run with verbose output
        #[arg(long)]
        nocapture: bool,
    },

    /// Verify program manifest and schema
    Verify {
        /// Path to WASM file (defaults to build output)
        #[arg(short, long)]
        wasm: Option<PathBuf>,

        /// Expected schema hash
        #[arg(long)]
        expected_hash: Option<String>,

        /// Path to IDL file to compare against
        #[arg(long)]
        idl: Option<PathBuf>,

        /// Strict mode: reject zero/placeholder hashes
        #[arg(long)]
        strict: bool,
    },

    /// Generate or extract IDL
    Idl {
        #[command(subcommand)]
        action: IdlAction,
    },

    /// Deploy program to network
    Deploy {
        /// Network to deploy to (localnet, devnet, testnet, mainnet)
        #[arg(short, long, default_value = "localnet")]
        network: String,

        /// Keypair file for signing
        #[arg(short, long)]
        keypair: Option<PathBuf>,

        /// RPC URL (overrides network default)
        #[arg(long)]
        rpc_url: Option<String>,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,

        /// Path to WASM file (defaults to build output)
        #[arg(long)]
        wasm: Option<PathBuf>,
    },

    /// Upgrade an existing program
    Upgrade {
        /// Program ID to upgrade
        #[arg(long)]
        program_id: String,

        /// Network (localnet, devnet, testnet, mainnet)
        #[arg(short, long, default_value = "localnet")]
        network: String,

        /// Authority keypair
        #[arg(short, long)]
        keypair: Option<PathBuf>,

        /// Skip confirmation prompt
        #[arg(short, long)]
        yes: bool,
    },

    /// Show program info
    Info {
        /// Program ID or path to WASM (optional - shows local project if omitted)
        #[arg(short, long)]
        program: Option<String>,

        /// Network for deployed programs
        #[arg(short, long)]
        network: Option<String>,

        /// Output format (text, json, yaml)
        #[arg(long, default_value = "text")]
        format: String,
    },

    /// Manage project configuration
    Config {
        #[command(subcommand)]
        action: ConfigAction,
    },

    /// Clean build artifacts
    Clean {
        /// Also remove IDL files
        #[arg(long)]
        idl: bool,

        /// Remove all generated files
        #[arg(long)]
        all: bool,
    },

    /// Generate shell completions
    Completions {
        /// Shell to generate completions for
        shell: clap_complete::Shell,
    },
}

#[derive(Subcommand)]
enum IdlAction {
    /// Extract IDL from built WASM
    Extract {
        /// Path to WASM file
        #[arg(short, long)]
        wasm: Option<PathBuf>,

        /// Output path (defaults to idl/<program>.json)
        #[arg(short, long)]
        output: Option<PathBuf>,
    },

    /// Show IDL schema hash
    Hash {
        /// Path to IDL file
        path: PathBuf,
    },

    /// Validate IDL file
    Validate {
        /// Path to IDL file
        path: PathBuf,
    },

    /// Generate TypeScript client from IDL
    Generate {
        /// Path to IDL file
        idl: PathBuf,

        /// Output directory
        #[arg(short, long, default_value = "ts-client")]
        output: PathBuf,
    },
}

#[derive(Subcommand)]
enum ConfigAction {
    /// Show current configuration
    Show,

    /// Set a configuration value
    Set {
        /// Key to set (e.g., network.devnet.url)
        key: String,

        /// Value to set
        value: String,
    },

    /// Get a configuration value
    Get {
        /// Key to get
        key: String,
    },

    /// Initialize default configuration
    Init,
}

#[tokio::main]
async fn main() -> ExitCode {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("ironclad=info".parse().unwrap()),
        )
        .init();

    let cli = Cli::parse();

    if let Err(e) = run(cli).await {
        eprintln!("{} {}", "error:".red().bold(), e);
        ExitCode::FAILURE
    } else {
        ExitCode::SUCCESS
    }
}

async fn run(cli: Cli) -> IroncladResult<()> {
    match cli.command {
        Commands::Init {
            name,
            template,
            here,
        } => init::run(&name, &template, here, cli.verbose).await,

        Commands::Build {
            release,
            target_dir: _,
            idl: _,
            verify,
            expected_hash: _,
        } => build::run(release, verify, cli.verbose).await,

        Commands::Test {
            test,
            skip_build: _,
            nocapture,
        } => test::run(test.as_deref(), nocapture || cli.verbose).await,

        Commands::Verify {
            wasm,
            expected_hash,
            idl: _,
            strict: _,
        } => {
            verify::run(
                wasm.as_ref().map(|p| p.to_str().unwrap()),
                expected_hash.as_deref(),
                cli.verbose,
            )
            .await
        }

        Commands::Idl { action } => match action {
            IdlAction::Extract { wasm, output } => {
                idl::run_extract(
                    wasm.as_ref().map(|p| p.to_str().unwrap()),
                    output.as_ref().map(|p| p.to_str().unwrap()),
                )
                .await
            }
            IdlAction::Hash { path } => idl::run_hash(Some(path.to_str().unwrap())).await,
            IdlAction::Validate { path } => idl::run_validate(Some(path.to_str().unwrap())).await,
            IdlAction::Generate { idl, output } => {
                idl::run_generate(
                    Some(idl.to_str().unwrap()),
                    "typescript",
                    Some(output.to_str().unwrap()),
                )
                .await
            }
        },

        Commands::Deploy {
            network,
            keypair,
            rpc_url: _,
            yes: _,
            wasm,
        } => {
            deploy::run(
                &network,
                wasm.as_ref().map(|p| p.to_str().unwrap()),
                keypair.as_ref().map(|p| p.to_str().unwrap()),
                false,
                cli.verbose,
            )
            .await
        }

        Commands::Upgrade {
            program_id,
            network,
            keypair: _,
            yes: _,
        } => upgrade::run(&program_id, &network, None, false, cli.verbose).await,

        Commands::Info {
            program,
            network,
            format: _,
        } => info::run(program.as_deref(), network.as_deref(), cli.verbose).await,

        Commands::Config { action } => match action {
            ConfigAction::Show => config_cmd::run_show().await,
            ConfigAction::Set { key, value } => config_cmd::run_set(&key, &value).await,
            ConfigAction::Get { key } => config_cmd::run_get(&key).await,
            ConfigAction::Init => config_cmd::run_init().await,
        },

        Commands::Clean { idl: _, all: _ } => clean::run(cli.verbose).await,

        Commands::Completions { shell } => {
            use clap::CommandFactory;
            let mut cmd = Cli::command();
            clap_complete::generate(shell, &mut cmd, "ironclad", &mut std::io::stdout());
            Ok(())
        }
    }
}
