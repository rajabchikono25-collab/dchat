// CLI Command Handler Module
//
// This module provides dedicated handler functions for CLI commands,
// consolidating logic from main.rs inline match arms for better maintainability.
//
// Architecture:
// - Each submodule handles a command category (e.g., staking, rewards, governance)
// - Handlers receive parsed CLI arguments and return Result<()>
// - Common utilities (format_tokens, init clients) are encapsulated per module
//
// Adding a new command handler:
// 1. Create new file: cli_handlers/<command>.rs
// 2. Add `pub mod <command>;` below
// 3. Implement `handle_*` functions for each subcommand
// 4. Update main.rs to call the handlers

pub mod chaos;
pub mod governance;
pub mod miniapp;
pub mod network;
pub mod program;
pub mod rewards;
pub mod staking;
pub mod wallet;
