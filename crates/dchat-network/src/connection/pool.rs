/// Connection Pool - Manages active peer connections with capacity limits
///
/// Features:
/// - LRU eviction when at capacity
/// - Reputation-based priority scoring
/// - Last activity tracking
/// - Connection state management
///
/// See ARCHITECTURE.md Section 12.3: Connection Management
use dchat_core::Result;
use libp2p::PeerId;
use std::collections::{HashMap, VecDeque};
use std::time::{Duration, Instant};

/// Connection pool with capacity management
pub struct ConnectionPool {
    max_connections: usize,
    target_connections: usize,
    connections: HashMap<PeerId, ConnectionInfo>,
    lru_queue: VecDeque<PeerId>,
}

impl ConnectionPool {
    /// Create new connection pool
    pub fn new(max_connections: usize, target_connections: usize) -> Self {
        Self {
            max_connections,
            target_connections,
            connections: HashMap::new(),
            lru_queue: VecDeque::new(),
        }
    }

    /// Add connection to pool
    pub async fn add_connection(&mut self, peer_id: PeerId) -> Result<()> {
        if self.connections.len() >= self.max_connections {
            return Err(dchat_core::Error::network("Connection pool full"));
        }

        let info = ConnectionInfo {
            peer_id,
            state: ConnectionState::Connected,
            connected_at: Instant::now(),
            last_activity: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
            latency: Duration::from_millis(0),
            reputation: 0.5, // Neutral starting reputation
        };

        self.connections.insert(peer_id, info);
        self.lru_queue.push_back(peer_id);

        Ok(())
    }

    /// Remove connection from pool
    pub async fn remove_connection(&mut self, peer_id: &PeerId) -> Result<()> {
        self.connections.remove(peer_id);
        self.lru_queue.retain(|p| p != peer_id);
        Ok(())
    }

    /// Update connection activity
    pub fn update_activity(&mut self, peer_id: &PeerId) {
        if let Some(info) = self.connections.get_mut(peer_id) {
            info.last_activity = Instant::now();

            // Move to back of LRU queue (most recently used)
            self.lru_queue.retain(|p| p != peer_id);
            self.lru_queue.push_back(*peer_id);
        }
    }

    /// Update connection stats
    pub fn update_stats(
        &mut self,
        peer_id: &PeerId,
        bytes_sent: u64,
        bytes_received: u64,
        latency: Duration,
    ) {
        if let Some(info) = self.connections.get_mut(peer_id) {
            info.bytes_sent += bytes_sent;
            info.bytes_received += bytes_received;
            info.latency = latency;
            info.last_activity = Instant::now();
        }
    }

    /// Update peer reputation
    pub fn update_reputation(&mut self, peer_id: &PeerId, reputation: f64) {
        if let Some(info) = self.connections.get_mut(peer_id) {
            info.reputation = reputation.clamp(0.0, 1.0);
        }
    }

    /// Get connection info
    pub async fn get_connection_info(&self, peer_id: &PeerId) -> Option<ConnectionInfo> {
        self.connections.get(peer_id).cloned()
    }

    /// Get all connections
    pub fn get_all_connections(&self) -> Vec<PeerId> {
        self.connections.keys().copied().collect()
    }

    /// Get pruning candidates (lowest priority connections)
    pub async fn get_pruning_candidates(&self) -> Result<Vec<PeerId>> {
        let mut candidates: Vec<_> = self.connections.iter().collect();

        // Sort by priority score (lower = more likely to prune)
        candidates.sort_by(|a, b| {
            let score_a = self.calculate_priority_score(a.1);
            let score_b = self.calculate_priority_score(b.1);
            score_a.partial_cmp(&score_b).unwrap()
        });

        // Return bottom 10% as candidates
        let candidate_count = (self.connections.len() / 10).max(1);
        Ok(candidates
            .iter()
            .take(candidate_count)
            .map(|(id, _)| **id)
            .collect())
    }

    /// Calculate priority score for connection
    fn calculate_priority_score(&self, info: &ConnectionInfo) -> f64 {
        let now = Instant::now();
        let age = now.duration_since(info.connected_at).as_secs_f64();
        let idle_time = now.duration_since(info.last_activity).as_secs_f64();

        // Score components:
        // 1. Reputation (0.0-1.0) - weight 40%
        // 2. Recent activity (inverse of idle time) - weight 30%
        // 3. Connection age - weight 20%
        // 4. Latency (inverse) - weight 10%

        let reputation_score = info.reputation * 0.4;

        let activity_score = if idle_time > 0.0 {
            (1.0 / (1.0 + idle_time / 60.0)) * 0.3 // Normalize by minutes
        } else {
            0.3
        };

        let age_score = (age / 3600.0).min(1.0) * 0.2; // Normalize by hours

        let latency_score = if info.latency.as_millis() > 0 {
            (1.0 / (1.0 + info.latency.as_secs_f64())) * 0.1
        } else {
            0.1
        };

        reputation_score + activity_score + age_score + latency_score
    }

    /// Get idle connections (inactive for specified duration)
    pub async fn get_idle_connections(&self, idle_timeout: Duration) -> Result<Vec<PeerId>> {
        let now = Instant::now();

        Ok(self
            .connections
            .iter()
            .filter(|(_, info)| now.duration_since(info.last_activity) > idle_timeout)
            .map(|(id, _)| *id)
            .collect())
    }

    /// Get connection count
    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }

    /// Check if at capacity
    pub fn is_at_capacity(&self) -> bool {
        self.connections.len() >= self.max_connections
    }

    /// Check if below target
    pub fn is_below_target(&self) -> bool {
        self.connections.len() < self.target_connections
    }

    /// Get least recently used connection
    pub fn get_lru_connection(&self) -> Option<PeerId> {
        self.lru_queue.front().copied()
    }
}

/// Connection information
#[derive(Debug, Clone)]
pub struct ConnectionInfo {
    pub peer_id: PeerId,
    pub state: ConnectionState,
    pub connected_at: Instant,
    pub last_activity: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
    pub latency: Duration,
    pub reputation: f64,
}

impl ConnectionInfo {
    /// Get connection duration
    pub fn connection_duration(&self) -> Duration {
        Instant::now().duration_since(self.connected_at)
    }

    /// Get idle duration
    pub fn idle_duration(&self) -> Duration {
        Instant::now().duration_since(self.last_activity)
    }

    /// Check if connection is idle
    pub fn is_idle(&self, idle_timeout: Duration) -> bool {
        self.idle_duration() > idle_timeout
    }
}

/// Connection state
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    Connected,
    Disconnected,
    Reconnecting,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_peer() -> PeerId {
        PeerId::random()
    }

    #[tokio::test]
    async fn test_pool_creation() {
        let pool = ConnectionPool::new(50, 30);
        assert_eq!(pool.connection_count(), 0);
        assert!(!pool.is_at_capacity());
        assert!(pool.is_below_target());
    }

    #[tokio::test]
    async fn test_add_connection() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer = create_test_peer();

        let result = pool.add_connection(peer).await;
        assert!(result.is_ok());
        assert_eq!(pool.connection_count(), 1);
    }

    #[tokio::test]
    async fn test_remove_connection() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer = create_test_peer();

        pool.add_connection(peer).await.unwrap();
        assert_eq!(pool.connection_count(), 1);

        pool.remove_connection(&peer).await.unwrap();
        assert_eq!(pool.connection_count(), 0);
    }

    #[tokio::test]
    async fn test_capacity_limit() {
        let mut pool = ConnectionPool::new(2, 1);

        let peer1 = create_test_peer();
        let peer2 = create_test_peer();
        let peer3 = create_test_peer();

        assert!(pool.add_connection(peer1).await.is_ok());
        assert!(pool.add_connection(peer2).await.is_ok());
        assert!(pool.is_at_capacity());

        // Should fail at capacity
        assert!(pool.add_connection(peer3).await.is_err());
    }

    #[tokio::test]
    async fn test_update_activity() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer = create_test_peer();

        pool.add_connection(peer).await.unwrap();

        let info_before = pool.get_connection_info(&peer).await.unwrap();

        tokio::time::sleep(Duration::from_millis(10)).await;
        pool.update_activity(&peer);

        let info_after = pool.get_connection_info(&peer).await.unwrap();
        assert!(info_after.last_activity > info_before.last_activity);
    }

    #[tokio::test]
    async fn test_update_reputation() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer = create_test_peer();

        pool.add_connection(peer).await.unwrap();
        pool.update_reputation(&peer, 0.8);

        let info = pool.get_connection_info(&peer).await.unwrap();
        assert_eq!(info.reputation, 0.8);
    }

    #[tokio::test]
    async fn test_lru_queue() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer1 = create_test_peer();
        let peer2 = create_test_peer();

        pool.add_connection(peer1).await.unwrap();
        pool.add_connection(peer2).await.unwrap();

        // peer1 should be LRU
        assert_eq!(pool.get_lru_connection(), Some(peer1));

        // Update peer1 activity
        pool.update_activity(&peer1);

        // Now peer2 should be LRU
        assert_eq!(pool.get_lru_connection(), Some(peer2));
    }

    #[tokio::test]
    async fn test_idle_connections() {
        let mut pool = ConnectionPool::new(50, 30);
        let peer = create_test_peer();

        pool.add_connection(peer).await.unwrap();

        let idle = pool
            .get_idle_connections(Duration::from_secs(1))
            .await
            .unwrap();
        assert!(idle.is_empty()); // Just added, not idle yet

        tokio::time::sleep(Duration::from_millis(1100)).await;

        let idle = pool
            .get_idle_connections(Duration::from_secs(1))
            .await
            .unwrap();
        assert_eq!(idle.len(), 1);
        assert_eq!(idle[0], peer);
    }
}
