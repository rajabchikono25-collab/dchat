//! CLI Error Handling
//!
//! This module provides a standardized error type for CLI operations with:
//! - Proper exit codes for different error categories
//! - Consistent error formatting for user-facing output
//! - Integration with the core Error type
//!
//! ## Exit Codes
//!
//! | Code | Category      | Description                                    |
//! |------|---------------|------------------------------------------------|
//! | 0    | Success       | Command completed successfully                 |
//! | 1    | User Error    | Invalid input, validation failure              |
//! | 2    | Config Error  | Configuration missing or invalid               |
//! | 3    | Network Error | RPC connection failed, timeout                 |
//! | 4    | Chain Error   | Blockchain operation failed                    |
//! | 5    | Storage Error | Database or file system error                  |
//! | 6    | Auth Error    | Authentication or authorization failure        |
//! | 10   | Internal      | Unexpected internal error                      |
//!
//! ## Usage
//!
//! ```rust,ignore
//! use dchat::cli_error::{CliError, CliResult};
//!
//! fn some_command() -> CliResult<()> {
//!     if !valid_input {
//!         return Err(CliError::validation("Invalid user ID format"));
//!     }
//!     
//!     let chain = connect_chain()
//!         .map_err(|e| CliError::network(e, "Failed to connect to chain"))?;
//!     
//!     Ok(())
//! }
//! ```

use dchat_core::error::Error as CoreError;
use std::fmt;
use std::process::ExitCode;

/// Exit codes for CLI operations
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum ExitCodeKind {
    /// Command completed successfully
    Success = 0,
    /// User input error (validation failure, invalid arguments)
    UserError = 1,
    /// Configuration error (missing or invalid config)
    ConfigError = 2,
    /// Network error (RPC connection failed, timeout)
    NetworkError = 3,
    /// Blockchain/chain operation error
    ChainError = 4,
    /// Storage error (database, file system)
    StorageError = 5,
    /// Authentication/authorization error
    AuthError = 6,
    /// Unexpected internal error
    InternalError = 10,
}

impl From<ExitCodeKind> for ExitCode {
    fn from(kind: ExitCodeKind) -> Self {
        ExitCode::from(kind as u8)
    }
}

/// CLI-specific error type with exit codes and user-friendly formatting
#[derive(Debug)]
pub struct CliError {
    /// The error category (determines exit code)
    pub kind: ExitCodeKind,
    /// User-friendly error message
    pub message: String,
    /// Additional context or hint for the user
    pub hint: Option<String>,
    /// Underlying error (if any)
    pub source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl CliError {
    /// Create a new CLI error with the given kind and message
    pub fn new(kind: ExitCodeKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            hint: None,
            source: None,
        }
    }

    /// Add a hint for the user
    pub fn with_hint(mut self, hint: impl Into<String>) -> Self {
        self.hint = Some(hint.into());
        self
    }

    /// Add an underlying error source
    pub fn with_source(mut self, source: impl std::error::Error + Send + Sync + 'static) -> Self {
        self.source = Some(Box::new(source));
        self
    }

    /// Create a validation/user input error
    pub fn validation(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::UserError, message)
    }

    /// Create a configuration error
    pub fn config(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::ConfigError, message)
    }

    /// Create a network error
    pub fn network(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::NetworkError, message)
    }

    /// Create a chain/blockchain error
    pub fn chain(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::ChainError, message)
    }

    /// Create a storage error
    pub fn storage(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::StorageError, message)
    }

    /// Create an authentication error
    pub fn auth(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::AuthError, message)
    }

    /// Create an internal error
    pub fn internal(message: impl Into<String>) -> Self {
        Self::new(ExitCodeKind::InternalError, message)
    }

    /// Get the exit code for this error
    pub fn exit_code(&self) -> ExitCode {
        self.kind.into()
    }

    /// Format the error for display to the user
    pub fn format_for_display(&self) -> String {
        let mut output = String::new();

        // Error icon based on category
        let icon = match self.kind {
            ExitCodeKind::Success => "✅",
            ExitCodeKind::UserError => "❌",
            ExitCodeKind::ConfigError => "⚙️",
            ExitCodeKind::NetworkError => "🌐",
            ExitCodeKind::ChainError => "⛓️",
            ExitCodeKind::StorageError => "💾",
            ExitCodeKind::AuthError => "🔐",
            ExitCodeKind::InternalError => "💥",
        };

        // Category name
        let category = match self.kind {
            ExitCodeKind::Success => "Success",
            ExitCodeKind::UserError => "Input Error",
            ExitCodeKind::ConfigError => "Configuration Error",
            ExitCodeKind::NetworkError => "Network Error",
            ExitCodeKind::ChainError => "Blockchain Error",
            ExitCodeKind::StorageError => "Storage Error",
            ExitCodeKind::AuthError => "Authentication Error",
            ExitCodeKind::InternalError => "Internal Error",
        };

        output.push_str(&format!("{} {}: {}\n", icon, category, self.message));

        // Add hint if available
        if let Some(hint) = &self.hint {
            output.push_str(&format!("💡 Hint: {}\n", hint));
        }

        // Add source error if available (for debugging)
        if let Some(source) = &self.source {
            output.push_str(&format!("   Caused by: {}\n", source));
        }

        output
    }

    /// Print the error to stderr and return the exit code
    pub fn print_and_exit_code(&self) -> ExitCode {
        eprintln!("{}", self.format_for_display());
        self.exit_code()
    }
}

impl fmt::Display for CliError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(hint) = &self.hint {
            write!(f, " (hint: {})", hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for CliError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source
            .as_ref()
            .map(|e| e.as_ref() as &(dyn std::error::Error + 'static))
    }
}

/// Convert from dchat_core::error::Error to CliError
impl From<CoreError> for CliError {
    fn from(err: CoreError) -> Self {
        // Map core error types to CLI error categories
        let message = err.to_string();

        // Attempt to categorize based on error message content
        // (since CoreError might not expose variants directly)
        if message.contains("config") || message.contains("Config") {
            CliError::config(message)
        } else if message.contains("network")
            || message.contains("connection")
            || message.contains("timeout")
        {
            CliError::network(message)
        } else if message.contains("chain")
            || message.contains("blockchain")
            || message.contains("transaction")
        {
            CliError::chain(message)
        } else if message.contains("storage")
            || message.contains("database")
            || message.contains("file")
        {
            CliError::storage(message)
        } else if message.contains("auth")
            || message.contains("permission")
            || message.contains("unauthorized")
        {
            CliError::auth(message)
        } else if message.contains("validation")
            || message.contains("invalid")
            || message.contains("Invalid")
        {
            CliError::validation(message)
        } else {
            // Default to internal error for unrecognized patterns
            CliError::internal(message)
        }
    }
}

/// Result type for CLI operations
pub type CliResult<T> = Result<T, CliError>;

/// Extension trait for adding context to Results
pub trait CliResultExt<T> {
    /// Add a user-friendly hint to the error
    fn with_hint(self, hint: impl Into<String>) -> CliResult<T>;

    /// Map the error to a specific CLI error kind
    fn map_cli_err(self, kind: ExitCodeKind, message: impl Into<String>) -> CliResult<T>;

    /// Convert to a validation error with custom message
    fn validation_err(self, message: impl Into<String>) -> CliResult<T>;

    /// Convert to a network error with custom message
    fn network_err(self, message: impl Into<String>) -> CliResult<T>;

    /// Convert to a chain error with custom message
    fn chain_err(self, message: impl Into<String>) -> CliResult<T>;
}

impl<T, E: std::error::Error + Send + Sync + 'static> CliResultExt<T> for Result<T, E> {
    fn with_hint(self, hint: impl Into<String>) -> CliResult<T> {
        self.map_err(|e| {
            let mut cli_err = CliError::internal(e.to_string());
            cli_err.hint = Some(hint.into());
            cli_err.source = Some(Box::new(e));
            cli_err
        })
    }

    fn map_cli_err(self, kind: ExitCodeKind, message: impl Into<String>) -> CliResult<T> {
        self.map_err(|e| {
            let mut cli_err = CliError::new(kind, message);
            cli_err.source = Some(Box::new(e));
            cli_err
        })
    }

    fn validation_err(self, message: impl Into<String>) -> CliResult<T> {
        self.map_cli_err(ExitCodeKind::UserError, message)
    }

    fn network_err(self, message: impl Into<String>) -> CliResult<T> {
        self.map_cli_err(ExitCodeKind::NetworkError, message)
    }

    fn chain_err(self, message: impl Into<String>) -> CliResult<T> {
        self.map_cli_err(ExitCodeKind::ChainError, message)
    }
}

/// Handle CLI errors at the top level of main()
///
/// This function should be called from main() to handle errors consistently:
///
/// ```rust,ignore
/// fn main() -> ExitCode {
///     match run() {
///         Ok(()) => ExitCode::SUCCESS,
///         Err(e) => handle_cli_error(e),
///     }
/// }
/// ```
pub fn handle_cli_error(error: CliError) -> ExitCode {
    error.print_and_exit_code()
}

/// Convenience macro for early return with a validation error
#[macro_export]
macro_rules! bail_validation {
    ($($arg:tt)*) => {
        return Err($crate::cli_error::CliError::validation(format!($($arg)*)))
    };
}

/// Convenience macro for early return with a config error
#[macro_export]
macro_rules! bail_config {
    ($($arg:tt)*) => {
        return Err($crate::cli_error::CliError::config(format!($($arg)*)))
    };
}

/// Convenience macro for early return with a network error
#[macro_export]
macro_rules! bail_network {
    ($($arg:tt)*) => {
        return Err($crate::cli_error::CliError::network(format!($($arg)*)))
    };
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exit_codes() {
        assert_eq!(ExitCodeKind::Success as u8, 0);
        assert_eq!(ExitCodeKind::UserError as u8, 1);
        assert_eq!(ExitCodeKind::ConfigError as u8, 2);
        assert_eq!(ExitCodeKind::NetworkError as u8, 3);
        assert_eq!(ExitCodeKind::ChainError as u8, 4);
        assert_eq!(ExitCodeKind::StorageError as u8, 5);
        assert_eq!(ExitCodeKind::AuthError as u8, 6);
        assert_eq!(ExitCodeKind::InternalError as u8, 10);
    }

    #[test]
    fn test_cli_error_creation() {
        let err = CliError::validation("Invalid user ID").with_hint("User IDs must be valid UUIDs");

        assert_eq!(err.kind, ExitCodeKind::UserError);
        assert!(err.message.contains("Invalid user ID"));
        assert!(err.hint.is_some());
    }

    #[test]
    fn test_error_display() {
        let err = CliError::network("Connection refused");
        let display = err.format_for_display();

        assert!(display.contains("Network Error"));
        assert!(display.contains("Connection refused"));
    }

    #[test]
    fn test_core_error_conversion() {
        // Test that core errors are properly categorized
        let config_err = CoreError::Config("missing value".to_string());
        let cli_err: CliError = config_err.into();
        assert_eq!(cli_err.kind, ExitCodeKind::ConfigError);
    }
}
