//! Solana Integration Module for dchat
//!
//! Complete Solana blockchain integration including:
//! - RPC client for Solana JSON-RPC API
//! - Transaction building and signing
//! - SPL Token operations (wDCHAT)
//! - Bridge program integration
//! - Account management
//! - High-level client API

pub mod accounts;
pub mod client;
pub mod program;
pub mod rpc;
pub mod spl_token;
pub mod transaction;

pub use accounts::{AccountInfo, Rent, SolanaAccount, SystemProgram};
pub use client::{create_shared_client, SharedSolanaClient, SolanaClient};
pub use program::{
    BridgeInstruction, BridgeProgram, BridgeState, DepositRecord, LockAccounts, UnlockAccounts,
    WithdrawRecord,
};
pub use rpc::{Commitment, RpcError, SolanaRpcClient, SolanaRpcConfig};
pub use spl_token::{
    AssociatedTokenAccount, MintInfo, SplToken, TokenAccount, TokenInstruction, TokenTransfer,
};
pub use transaction::AccountMeta;
pub use transaction::{
    CompiledInstruction, Instruction, MessageHeader, SolanaTransaction, TransactionBuilder,
    TransactionMessage, TransactionStatus as SolanaTxStatus,
};

/// Solana network cluster
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Cluster {
    /// Mainnet-beta
    Mainnet,
    /// Devnet (development)
    Devnet,
    /// Testnet
    Testnet,
    /// Local validator
    Localnet,
    /// Custom RPC endpoint
    Custom,
}

impl Cluster {
    /// Get RPC URL for cluster
    pub fn rpc_url(&self) -> &'static str {
        match self {
            Cluster::Mainnet => "https://api.mainnet-beta.solana.com",
            Cluster::Devnet => "https://api.devnet.solana.com",
            Cluster::Testnet => "https://api.testnet.solana.com",
            Cluster::Localnet => "http://127.0.0.1:8899",
            Cluster::Custom => "",
        }
    }

    /// Get WebSocket URL for cluster
    pub fn ws_url(&self) -> &'static str {
        match self {
            Cluster::Mainnet => "wss://api.mainnet-beta.solana.com",
            Cluster::Devnet => "wss://api.devnet.solana.com",
            Cluster::Testnet => "wss://api.testnet.solana.com",
            Cluster::Localnet => "ws://127.0.0.1:8900",
            Cluster::Custom => "",
        }
    }
}

/// Lamports per SOL
pub const LAMPORTS_PER_SOL: u64 = 1_000_000_000;

/// Token program ID
pub const TOKEN_PROGRAM_ID: &str = "TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA";

/// Associated Token program ID
pub const ASSOCIATED_TOKEN_PROGRAM_ID: &str = "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL";

/// System program ID
pub const SYSTEM_PROGRAM_ID: &str = "11111111111111111111111111111111";

/// Rent sysvar
pub const RENT_SYSVAR_ID: &str = "SysvarRent111111111111111111111111111111111";

/// Recent blockhashes sysvar
pub const RECENT_BLOCKHASHES_ID: &str = "SysvarRecentB1telephones1111111111111111111";

/// Convert lamports to SOL
pub fn lamports_to_sol(lamports: u64) -> f64 {
    lamports as f64 / LAMPORTS_PER_SOL as f64
}

/// Convert SOL to lamports
pub fn sol_to_lamports(sol: f64) -> u64 {
    (sol * LAMPORTS_PER_SOL as f64) as u64
}
