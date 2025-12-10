/// Reconnect Manager - Handles automatic reconnection with exponential backoff
///
/// Features:
/// - Exponential backoff (1s, 2s, 4s, 8s, 16s)
/// - Max 5 reconnection attempts
/// - Circuit breaker pattern
/// - Configurable policies
///
/// See ARCHITECTURE.md Section 12.3: Connection Management
use dchat_core::Result;
use libp2p::PeerId;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// Reconnect manager
pub struct ReconnectManager {
    policy: ReconnectPolicy,
    pending: HashMap<PeerId, ReconnectState>,
}

impl ReconnectManager {
    /// Create new reconnect manager
    pub fn new(policy: ReconnectPolicy) -> Self {
        Self {
            policy,
            pending: HashMap::new(),
        }
    }

    /// Check if should reconnect to peer
    pub fn should_reconnect(&self, peer_id: &PeerId) -> bool {
        match self.pending.get(peer_id) {
            Some(state) => state.attempt < self.policy.max_attempts,
            None => true, // First failure, should try
        }
    }

    /// Schedule reconnection
    pub async fn schedule_reconnection(&mut self, peer_id: PeerId) -> Result<()> {
        // Extract backoff parameters before mutable borrow
        let backoff_strategy = self.policy.backoff_strategy.clone();

        let state = self
            .pending
            .entry(peer_id)
            .or_insert_with(|| ReconnectState {
                attempt: 0,
                next_attempt: Instant::now(),
                total_failures: 0,
                last_failure: Instant::now(),
            });

        // Increment attempt - scheduling counts as an attempt
        let attempt = state.attempt + 1;

        // Calculate backoff using new attempt count
        let backoff = Self::calculate_backoff_static(&backoff_strategy, attempt);

        state.attempt = attempt;
        state.total_failures += 1;
        state.last_failure = Instant::now();
        state.next_attempt = Instant::now() + backoff;

        Ok(())
    }

    /// Calculate backoff duration (static method to avoid borrow issues)
    fn calculate_backoff_static(backoff_strategy: &BackoffStrategy, attempt: u32) -> Duration {
        match backoff_strategy {
            BackoffStrategy::Exponential {
                base_delay,
                max_delay,
            } => {
                let delay_ms = base_delay.as_millis() as u64 * 2u64.pow(attempt.saturating_sub(1));
                Duration::from_millis(delay_ms.min(max_delay.as_millis() as u64))
            }
            BackoffStrategy::Linear { delay } => *delay * attempt,
            BackoffStrategy::Constant { delay } => *delay,
        }
    }

    /// Calculate backoff duration
    pub fn calculate_backoff(&self, attempt: u32) -> Duration {
        Self::calculate_backoff_static(&self.policy.backoff_strategy, attempt)
    }

    /// Get reconnections due now
    pub async fn get_due_reconnections(&self) -> Result<Vec<PeerId>> {
        let now = Instant::now();

        Ok(self
            .pending
            .iter()
            .filter(|(_, state)| {
                state.attempt <= self.policy.max_attempts && state.next_attempt <= now
            })
            .map(|(peer_id, _)| *peer_id)
            .collect())
    }

    /// Mark reconnection as successful
    pub async fn mark_reconnection_success(&mut self, peer_id: &PeerId) -> Result<()> {
        self.pending.remove(peer_id);
        Ok(())
    }

    pub async fn mark_reconnection_failure(&mut self, peer_id: &PeerId) -> Result<()> {
        // Extract parameters before mutable borrow
        let backoff_strategy = self.policy.backoff_strategy.clone();
        let max_attempts = self.policy.max_attempts;

        let should_remove = if let Some(state) = self.pending.get_mut(peer_id) {
            let attempt = state.attempt + 1;

            if attempt > max_attempts {
                true // Circuit breaker - exceeded max attempts
            } else {
                // Calculate backoff using extracted strategy
                let backoff = Self::calculate_backoff_static(&backoff_strategy, attempt);

                state.attempt = attempt;
                state.total_failures += 1;
                state.last_failure = Instant::now();
                state.next_attempt = Instant::now() + backoff;
                false
            }
        } else {
            false
        };

        if should_remove {
            self.pending.remove(peer_id);
        }

        Ok(())
    }

    /// Cancel reconnection
    pub async fn cancel_reconnection(&mut self, peer_id: &PeerId) -> Result<()> {
        self.pending.remove(peer_id);
        Ok(())
    }

    /// Get pending reconnection count
    pub fn pending_count(&self) -> usize {
        self.pending.len()
    }

    /// Get reconnection state
    pub fn get_reconnect_state(&self, peer_id: &PeerId) -> Option<ReconnectInfo> {
        self.pending.get(peer_id).map(|state| ReconnectInfo {
            attempt: state.attempt,
            next_attempt: state.next_attempt,
            total_failures: state.total_failures,
            last_failure: state.last_failure,
            max_attempts: self.policy.max_attempts,
        })
    }

    /// Clean up old reconnection states
    pub async fn cleanup_old_states(&mut self, max_age: Duration) -> Result<()> {
        let now = Instant::now();

        self.pending
            .retain(|_, state| now.duration_since(state.last_failure) < max_age);

        Ok(())
    }
}

/// Reconnection state for a peer
struct ReconnectState {
    attempt: u32,
    next_attempt: Instant,
    total_failures: u64,
    last_failure: Instant,
}

/// Reconnection policy
#[derive(Debug, Clone)]
pub struct ReconnectPolicy {
    /// Maximum reconnection attempts before giving up
    pub max_attempts: u32,

    /// Backoff strategy
    pub backoff_strategy: BackoffStrategy,

    /// Enable circuit breaker
    pub circuit_breaker: bool,
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self {
            max_attempts: 5,
            backoff_strategy: BackoffStrategy::Exponential {
                base_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(16),
            },
            circuit_breaker: true,
        }
    }
}

/// Backoff strategy for reconnection
#[derive(Debug, Clone)]
pub enum BackoffStrategy {
    /// Exponential backoff (1s, 2s, 4s, 8s, 16s)
    Exponential {
        base_delay: Duration,
        max_delay: Duration,
    },

    /// Linear backoff (1s, 2s, 3s, 4s, 5s)
    Linear { delay: Duration },

    /// Constant backoff (same delay each time)
    Constant { delay: Duration },
}

/// Reconnection information
#[derive(Debug, Clone)]
pub struct ReconnectInfo {
    pub attempt: u32,
    pub next_attempt: Instant,
    pub total_failures: u64,
    pub last_failure: Instant,
    pub max_attempts: u32,
}

impl ReconnectInfo {
    /// Check if reconnection is exhausted
    pub fn is_exhausted(&self) -> bool {
        self.attempt >= self.max_attempts
    }

    /// Get time until next attempt
    pub fn time_until_next_attempt(&self) -> Duration {
        let now = Instant::now();
        if self.next_attempt > now {
            self.next_attempt.duration_since(now)
        } else {
            Duration::from_secs(0)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer() -> PeerId {
        PeerId::random()
    }

    #[test]
    fn test_reconnect_policy_default() {
        let policy = ReconnectPolicy::default();
        assert_eq!(policy.max_attempts, 5);
        assert!(policy.circuit_breaker);
    }

    #[tokio::test]
    async fn test_reconnect_manager_creation() {
        let policy = ReconnectPolicy::default();
        let manager = ReconnectManager::new(policy);
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_should_reconnect() {
        let policy = ReconnectPolicy::default();
        let manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        assert!(manager.should_reconnect(&peer)); // First failure
    }

    #[tokio::test]
    async fn test_schedule_reconnection() {
        let policy = ReconnectPolicy::default();
        let mut manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        let result = manager.schedule_reconnection(peer).await;
        assert!(result.is_ok());
        assert_eq!(manager.pending_count(), 1);
    }

    #[tokio::test]
    async fn test_exponential_backoff() {
        let policy = ReconnectPolicy {
            max_attempts: 5,
            backoff_strategy: BackoffStrategy::Exponential {
                base_delay: Duration::from_secs(1),
                max_delay: Duration::from_secs(16),
            },
            circuit_breaker: true,
        };

        let manager = ReconnectManager::new(policy);

        // Test backoff calculation
        assert_eq!(manager.calculate_backoff(1), Duration::from_secs(1));
        assert_eq!(manager.calculate_backoff(2), Duration::from_secs(2));
        assert_eq!(manager.calculate_backoff(3), Duration::from_secs(4));
        assert_eq!(manager.calculate_backoff(4), Duration::from_secs(8));
        assert_eq!(manager.calculate_backoff(5), Duration::from_secs(16));
        assert_eq!(manager.calculate_backoff(6), Duration::from_secs(16)); // Capped at max
    }

    #[tokio::test]
    async fn test_linear_backoff() {
        let policy = ReconnectPolicy {
            max_attempts: 5,
            backoff_strategy: BackoffStrategy::Linear {
                delay: Duration::from_secs(1),
            },
            circuit_breaker: true,
        };

        let manager = ReconnectManager::new(policy);

        assert_eq!(manager.calculate_backoff(1), Duration::from_secs(1));
        assert_eq!(manager.calculate_backoff(2), Duration::from_secs(2));
        assert_eq!(manager.calculate_backoff(3), Duration::from_secs(3));
    }

    #[tokio::test]
    async fn test_mark_success() {
        let policy = ReconnectPolicy::default();
        let mut manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        manager.schedule_reconnection(peer).await.unwrap();
        assert_eq!(manager.pending_count(), 1);

        manager.mark_reconnection_success(&peer).await.unwrap();
        assert_eq!(manager.pending_count(), 0);
    }

    #[tokio::test]
    async fn test_mark_failure() {
        let policy = ReconnectPolicy::default();
        let mut manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        manager.schedule_reconnection(peer).await.unwrap();

        let state = manager.get_reconnect_state(&peer).unwrap();
        assert_eq!(state.attempt, 1);

        manager.mark_reconnection_failure(&peer).await.unwrap();

        let state = manager.get_reconnect_state(&peer).unwrap();
        assert_eq!(state.attempt, 2);
    }

    #[tokio::test]
    async fn test_max_attempts_circuit_breaker() {
        let policy = ReconnectPolicy {
            max_attempts: 2,
            backoff_strategy: BackoffStrategy::Constant {
                delay: Duration::from_millis(100),
            },
            circuit_breaker: true,
        };

        let mut manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        manager.schedule_reconnection(peer).await.unwrap();
        assert_eq!(manager.pending_count(), 1);

        manager.mark_reconnection_failure(&peer).await.unwrap();
        assert_eq!(manager.pending_count(), 1);

        // Second failure should trigger circuit breaker
        manager.mark_reconnection_failure(&peer).await.unwrap();
        assert_eq!(manager.pending_count(), 0); // Circuit breaker removes from pending
    }

    #[tokio::test]
    async fn test_cancel_reconnection() {
        let policy = ReconnectPolicy::default();
        let mut manager = ReconnectManager::new(policy);
        let peer = create_test_peer();

        manager.schedule_reconnection(peer).await.unwrap();
        assert_eq!(manager.pending_count(), 1);

        manager.cancel_reconnection(&peer).await.unwrap();
        assert_eq!(manager.pending_count(), 0);
    }
}
