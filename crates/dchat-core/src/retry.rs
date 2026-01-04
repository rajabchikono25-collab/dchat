//! Retry utilities for transient failures in chain operations
//!
//! Provides exponential backoff retry logic for operations that may fail
//! due to network issues, RPC timeouts, or other transient errors.

use std::future::Future;
use std::time::Duration;
use tracing::{debug, warn};

/// Configuration for retry behavior
#[derive(Debug, Clone)]
pub struct RetryConfig {
    /// Maximum number of retry attempts (not including initial attempt)
    pub max_retries: u32,
    /// Initial delay before first retry
    pub initial_delay: Duration,
    /// Maximum delay between retries
    pub max_delay: Duration,
    /// Multiplier for exponential backoff (e.g., 2.0 doubles delay each time)
    pub backoff_multiplier: f64,
    /// Whether to add jitter to prevent thundering herd
    pub jitter: bool,
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_retries: 3,
            initial_delay: Duration::from_millis(100),
            max_delay: Duration::from_secs(10),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }
}

impl RetryConfig {
    /// Configuration for chain operations (more patient)
    pub fn chain_ops() -> Self {
        Self {
            max_retries: 5,
            initial_delay: Duration::from_millis(500),
            max_delay: Duration::from_secs(30),
            backoff_multiplier: 2.0,
            jitter: true,
        }
    }

    /// Configuration for quick operations (fail fast)
    pub fn quick() -> Self {
        Self {
            max_retries: 2,
            initial_delay: Duration::from_millis(50),
            max_delay: Duration::from_secs(1),
            backoff_multiplier: 2.0,
            jitter: false,
        }
    }

    /// Configuration for critical operations (very patient)
    pub fn critical() -> Self {
        Self {
            max_retries: 10,
            initial_delay: Duration::from_secs(1),
            max_delay: Duration::from_secs(60),
            backoff_multiplier: 1.5,
            jitter: true,
        }
    }
}

/// Determines if an error is retryable
pub trait RetryableError {
    /// Returns true if this error is transient and worth retrying
    fn is_retryable(&self) -> bool;
}

/// Error categories that are typically retryable
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetryCategory {
    /// Network timeout or connection error
    NetworkError,
    /// Server temporarily unavailable (503, 429, etc.)
    ServerUnavailable,
    /// Rate limited
    RateLimited,
    /// Transaction pending/not yet confirmed
    TransactionPending,
    /// RPC node temporarily unresponsive
    RpcTimeout,
    /// Not retryable - permanent failure
    PermanentFailure,
}

impl RetryCategory {
    pub fn is_retryable(self) -> bool {
        !matches!(self, RetryCategory::PermanentFailure)
    }
}

/// Categorize common error patterns
pub fn categorize_error(error_msg: &str) -> RetryCategory {
    let lower = error_msg.to_lowercase();

    if lower.contains("timeout")
        || lower.contains("timed out")
        || lower.contains("connection refused")
        || lower.contains("connection reset")
        || lower.contains("network")
        || lower.contains("dns")
    {
        return RetryCategory::NetworkError;
    }

    if lower.contains("503")
        || lower.contains("service unavailable")
        || lower.contains("temporarily unavailable")
        || lower.contains("server busy")
    {
        return RetryCategory::ServerUnavailable;
    }

    if lower.contains("429")
        || lower.contains("rate limit")
        || lower.contains("too many requests")
        || lower.contains("throttle")
    {
        return RetryCategory::RateLimited;
    }

    if lower.contains("pending")
        || lower.contains("not confirmed")
        || lower.contains("waiting for confirmation")
    {
        return RetryCategory::TransactionPending;
    }

    if lower.contains("rpc") && (lower.contains("timeout") || lower.contains("unresponsive")) {
        return RetryCategory::RpcTimeout;
    }

    // Permanent failures
    if lower.contains("invalid")
        || lower.contains("unauthorized")
        || lower.contains("forbidden")
        || lower.contains("not found")
        || lower.contains("insufficient funds")
        || lower.contains("insufficient balance")
        || lower.contains("nonce too low")
        || lower.contains("already exists")
        || lower.contains("duplicate")
    {
        return RetryCategory::PermanentFailure;
    }

    // Default to retryable for unknown errors (conservative approach)
    RetryCategory::NetworkError
}

/// Execute an async operation with exponential backoff retry
///
/// # Example
/// ```ignore
/// use dchat_core::retry::{with_retry, RetryConfig};
///
/// let result = with_retry(RetryConfig::chain_ops(), || async {
///     chain_client.submit_transaction(tx).await
/// }).await;
/// ```
pub async fn with_retry<F, Fut, T, E>(config: RetryConfig, operation: F) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
{
    let mut attempts = 0;
    let mut delay = config.initial_delay;

    loop {
        attempts += 1;

        match operation().await {
            Ok(result) => {
                if attempts > 1 {
                    debug!("Operation succeeded after {} attempts", attempts);
                }
                return Ok(result);
            }
            Err(e) => {
                let category = categorize_error(&e.to_string());

                if !category.is_retryable() || attempts > config.max_retries {
                    if attempts > config.max_retries {
                        warn!(
                            "Operation failed after {} attempts (max retries exhausted): {}",
                            attempts, e
                        );
                    }
                    return Err(e);
                }

                warn!(
                    "Attempt {}/{} failed ({}): {}. Retrying in {:?}...",
                    attempts,
                    config.max_retries + 1,
                    format!("{:?}", category),
                    e,
                    delay
                );

                // Apply jitter if configured
                let actual_delay = if config.jitter {
                    let jitter_factor = 0.5 + rand_jitter() * 0.5; // 0.5x to 1.0x
                    Duration::from_secs_f64(delay.as_secs_f64() * jitter_factor)
                } else {
                    delay
                };

                tokio::time::sleep(actual_delay).await;

                // Exponential backoff
                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_multiplier).min(config.max_delay.as_secs_f64()),
                );
            }
        }
    }
}

/// Execute an async operation with retry, using a custom retryable check
pub async fn with_retry_if<F, Fut, T, E, R>(
    config: RetryConfig,
    operation: F,
    is_retryable: R,
) -> Result<T, E>
where
    F: Fn() -> Fut,
    Fut: Future<Output = Result<T, E>>,
    E: std::fmt::Display,
    R: Fn(&E) -> bool,
{
    let mut attempts = 0;
    let mut delay = config.initial_delay;

    loop {
        attempts += 1;

        match operation().await {
            Ok(result) => {
                if attempts > 1 {
                    debug!("Operation succeeded after {} attempts", attempts);
                }
                return Ok(result);
            }
            Err(e) => {
                if !is_retryable(&e) || attempts > config.max_retries {
                    if attempts > config.max_retries {
                        warn!(
                            "Operation failed after {} attempts (max retries exhausted): {}",
                            attempts, e
                        );
                    }
                    return Err(e);
                }

                warn!(
                    "Attempt {}/{} failed: {}. Retrying in {:?}...",
                    attempts,
                    config.max_retries + 1,
                    e,
                    delay
                );

                let actual_delay = if config.jitter {
                    let jitter_factor = 0.5 + rand_jitter() * 0.5;
                    Duration::from_secs_f64(delay.as_secs_f64() * jitter_factor)
                } else {
                    delay
                };

                tokio::time::sleep(actual_delay).await;

                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_multiplier).min(config.max_delay.as_secs_f64()),
                );
            }
        }
    }
}

/// Simple pseudo-random jitter (0.0 to 1.0) using current time
fn rand_jitter() -> f64 {
    use std::time::SystemTime;
    let nanos = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .subsec_nanos();
    (nanos as f64) / 1_000_000_000.0
}

/// Retry a synchronous operation with blocking sleep
pub fn with_retry_sync<F, T, E>(config: RetryConfig, operation: F) -> Result<T, E>
where
    F: Fn() -> Result<T, E>,
    E: std::fmt::Display,
{
    let mut attempts = 0;
    let mut delay = config.initial_delay;

    loop {
        attempts += 1;

        match operation() {
            Ok(result) => return Ok(result),
            Err(e) => {
                let category = categorize_error(&e.to_string());

                if !category.is_retryable() || attempts > config.max_retries {
                    return Err(e);
                }

                std::thread::sleep(delay);

                delay = Duration::from_secs_f64(
                    (delay.as_secs_f64() * config.backoff_multiplier).min(config.max_delay.as_secs_f64()),
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_categorize_error() {
        assert_eq!(
            categorize_error("connection timeout"),
            RetryCategory::NetworkError
        );
        assert_eq!(
            categorize_error("503 service unavailable"),
            RetryCategory::ServerUnavailable
        );
        assert_eq!(
            categorize_error("429 rate limit exceeded"),
            RetryCategory::RateLimited
        );
        assert_eq!(
            categorize_error("invalid signature"),
            RetryCategory::PermanentFailure
        );
        assert_eq!(
            categorize_error("insufficient funds"),
            RetryCategory::PermanentFailure
        );
    }

    #[tokio::test]
    async fn test_with_retry_success() {
        let config = RetryConfig::quick();
        let result: Result<i32, &str> = with_retry(config, || async { Ok(42) }).await;
        assert_eq!(result.unwrap(), 42);
    }

    #[tokio::test]
    async fn test_with_retry_permanent_failure() {
        let config = RetryConfig::quick();
        let result: Result<i32, String> = with_retry(config, || async {
            Err::<i32, String>("invalid signature".to_string())
        })
        .await;
        assert!(result.is_err());
    }
}
