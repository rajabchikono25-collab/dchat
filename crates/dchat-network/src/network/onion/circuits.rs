//! Circuit Management for Onion Routing
//!
//! Manages multi-hop circuits with automatic timeout and cleanup.
//! Ensures geographic diversity and reputation-based path selection.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use uuid::Uuid;
use thiserror::Error;

/// Minimum number of hops in a circuit
pub const MIN_CIRCUIT_HOPS: usize = 3;

/// Maximum number of hops in a circuit
pub const MAX_CIRCUIT_HOPS: usize = 5;

/// Default circuit lifetime (10 minutes)
pub const CIRCUIT_LIFETIME: Duration = Duration::from_secs(600);

/// Circuit cleanup interval (1 minute)
const CLEANUP_INTERVAL: Duration = Duration::from_secs(60);

/// Unique identifier for a circuit
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CircuitId(Uuid);

impl CircuitId {
    /// Generate a new random circuit ID
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }

    /// Get the UUID representation
    pub fn as_uuid(&self) -> &Uuid {
        &self.0
    }
}

impl Default for CircuitId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for CircuitId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

/// Information about a single hop in the circuit
#[derive(Debug, Clone)]
pub struct CircuitHop {
    /// Peer ID of the relay node
    pub peer_id: String,
    /// Geographic region of the relay
    pub region: String,
    /// Ephemeral public key for this hop
    pub ephemeral_key: Vec<u8>,
    /// Shared secret for encryption
    pub shared_secret: Vec<u8>,
}

/// Status of a circuit
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CircuitStatus {
    /// Circuit is being established
    Building,
    /// Circuit is ready for use
    Ready,
    /// Circuit is being torn down
    Closing,
    /// Circuit has expired
    Expired,
    /// Circuit failed during establishment
    Failed,
}

/// A multi-hop circuit through the network
#[derive(Debug, Clone)]
pub struct Circuit {
    /// Unique circuit identifier
    pub id: CircuitId,
    /// Ordered list of hops (entry -> middle -> exit)
    pub hops: Vec<CircuitHop>,
    /// Current status
    pub status: CircuitStatus,
    /// Creation timestamp
    pub created_at: Instant,
    /// Expiration time
    pub expires_at: Instant,
    /// Total bytes sent through this circuit
    pub bytes_sent: u64,
    /// Total bytes received through this circuit
    pub bytes_received: u64,
}

impl Circuit {
    /// Create a new circuit with the given hops
    pub fn new(hops: Vec<CircuitHop>) -> Result<Self, CircuitError> {
        if hops.len() < MIN_CIRCUIT_HOPS {
            return Err(CircuitError::InsufficientHops {
                required: MIN_CIRCUIT_HOPS,
                provided: hops.len(),
            });
        }

        if hops.len() > MAX_CIRCUIT_HOPS {
            return Err(CircuitError::TooManyHops {
                max: MAX_CIRCUIT_HOPS,
                provided: hops.len(),
            });
        }

        let now = Instant::now();
        Ok(Self {
            id: CircuitId::new(),
            hops,
            status: CircuitStatus::Building,
            created_at: now,
            expires_at: now + CIRCUIT_LIFETIME,
            bytes_sent: 0,
            bytes_received: 0,
        })
    }

    /// Check if the circuit has expired
    pub fn is_expired(&self) -> bool {
        Instant::now() >= self.expires_at
    }

    /// Check if the circuit is ready for use
    pub fn is_ready(&self) -> bool {
        self.status == CircuitStatus::Ready && !self.is_expired()
    }

    /// Mark the circuit as ready
    pub fn mark_ready(&mut self) {
        self.status = CircuitStatus::Ready;
    }

    /// Mark the circuit as failed
    pub fn mark_failed(&mut self) {
        self.status = CircuitStatus::Failed;
    }

    /// Record bytes sent through this circuit
    pub fn record_sent(&mut self, bytes: u64) {
        self.bytes_sent = self.bytes_sent.saturating_add(bytes);
    }

    /// Record bytes received through this circuit
    pub fn record_received(&mut self, bytes: u64) {
        self.bytes_received = self.bytes_received.saturating_add(bytes);
    }

    /// Get the entry hop (first hop)
    pub fn entry_hop(&self) -> Option<&CircuitHop> {
        self.hops.first()
    }

    /// Get the exit hop (last hop)
    pub fn exit_hop(&self) -> Option<&CircuitHop> {
        self.hops.last()
    }

    /// Get all middle hops (excluding entry and exit)
    pub fn middle_hops(&self) -> &[CircuitHop] {
        if self.hops.len() <= 2 {
            &[]
        } else {
            &self.hops[1..self.hops.len() - 1]
        }
    }
}

/// Errors that can occur during circuit operations
#[derive(Debug, Error)]
pub enum CircuitError {
    #[error("Insufficient hops: required {required}, provided {provided}")]
    InsufficientHops { required: usize, provided: usize },

    #[error("Too many hops: max {max}, provided {provided}")]
    TooManyHops { max: usize, provided: usize },

    #[error("Circuit not found: {0}")]
    CircuitNotFound(CircuitId),

    #[error("Circuit not ready: {0}")]
    CircuitNotReady(CircuitId),

    #[error("Circuit expired: {0}")]
    CircuitExpired(CircuitId),

    #[error("Circuit failed: {0}")]
    CircuitFailed(CircuitId),

    #[error("Path selection failed: {0}")]
    PathSelectionFailed(String),

    #[error("Encryption failed: {0}")]
    EncryptionFailed(String),

    #[error("No suitable relays available")]
    NoSuitableRelays,
}

/// Manages multiple circuits
pub struct CircuitManager {
    /// Active circuits indexed by ID
    circuits: Arc<RwLock<HashMap<CircuitId, Circuit>>>,
    /// Cleanup task handle
    cleanup_handle: Arc<RwLock<Option<tokio::task::JoinHandle<()>>>>,
}

impl CircuitManager {
    /// Create a new circuit manager
    pub fn new() -> Self {
        Self {
            circuits: Arc::new(RwLock::new(HashMap::new())),
            cleanup_handle: Arc::new(RwLock::new(None)),
        }
    }

    /// Start the automatic cleanup task
    pub async fn start_cleanup(&self) {
        let circuits = Arc::clone(&self.circuits);
        let handle = tokio::spawn(async move {
            let mut interval = tokio::time::interval(CLEANUP_INTERVAL);
            loop {
                interval.tick().await;
                Self::cleanup_expired_circuits(&circuits).await;
            }
        });

        *self.cleanup_handle.write().await = Some(handle);
    }

    /// Stop the cleanup task
    pub async fn stop_cleanup(&self) {
        if let Some(handle) = self.cleanup_handle.write().await.take() {
            handle.abort();
        }
    }

    /// Clean up expired circuits
    async fn cleanup_expired_circuits(circuits: &Arc<RwLock<HashMap<CircuitId, Circuit>>>) {
        let mut circuits_guard = circuits.write().await;
        let expired_ids: Vec<CircuitId> = circuits_guard
            .iter()
            .filter(|(_, circuit)| circuit.is_expired())
            .map(|(id, _)| *id)
            .collect();

        for id in expired_ids {
            if let Some(mut circuit) = circuits_guard.remove(&id) {
                circuit.status = CircuitStatus::Expired;
                tracing::debug!("Cleaned up expired circuit: {}", id);
            }
        }
    }

    /// Create a new circuit with the given hops
    pub async fn create_circuit(&self, hops: Vec<CircuitHop>) -> Result<CircuitId, CircuitError> {
        let circuit = Circuit::new(hops)?;
        let circuit_id = circuit.id;

        self.circuits.write().await.insert(circuit_id, circuit);
        tracing::info!("Created new circuit: {}", circuit_id);

        Ok(circuit_id)
    }

    /// Get a circuit by ID
    pub async fn get_circuit(&self, id: CircuitId) -> Option<Circuit> {
        self.circuits.read().await.get(&id).cloned()
    }

    /// Mark a circuit as ready
    pub async fn mark_ready(&self, id: CircuitId) -> Result<(), CircuitError> {
        let mut circuits = self.circuits.write().await;
        let circuit = circuits
            .get_mut(&id)
            .ok_or(CircuitError::CircuitNotFound(id))?;

        circuit.mark_ready();
        tracing::info!("Circuit {} is now ready", id);
        Ok(())
    }

    /// Mark a circuit as failed
    pub async fn mark_failed(&self, id: CircuitId) -> Result<(), CircuitError> {
        let mut circuits = self.circuits.write().await;
        let circuit = circuits
            .get_mut(&id)
            .ok_or(CircuitError::CircuitNotFound(id))?;

        circuit.mark_failed();
        tracing::warn!("Circuit {} failed", id);
        Ok(())
    }

    /// Close a circuit
    pub async fn close_circuit(&self, id: CircuitId) -> Result<(), CircuitError> {
        let mut circuits = self.circuits.write().await;
        let mut circuit = circuits
            .remove(&id)
            .ok_or(CircuitError::CircuitNotFound(id))?;

        circuit.status = CircuitStatus::Closing;
        tracing::info!("Closed circuit: {}", id);
        Ok(())
    }

    /// Record bytes sent through a circuit
    pub async fn record_sent(&self, id: CircuitId, bytes: u64) -> Result<(), CircuitError> {
        let mut circuits = self.circuits.write().await;
        let circuit = circuits
            .get_mut(&id)
            .ok_or(CircuitError::CircuitNotFound(id))?;

        circuit.record_sent(bytes);
        Ok(())
    }

    /// Record bytes received through a circuit
    pub async fn record_received(&self, id: CircuitId, bytes: u64) -> Result<(), CircuitError> {
        let mut circuits = self.circuits.write().await;
        let circuit = circuits
            .get_mut(&id)
            .ok_or(CircuitError::CircuitNotFound(id))?;

        circuit.record_received(bytes);
        Ok(())
    }

    /// Get statistics for all circuits
    pub async fn get_stats(&self) -> CircuitStats {
        let circuits = self.circuits.read().await;
        let total = circuits.len();
        let ready = circuits
            .values()
            .filter(|c| c.status == CircuitStatus::Ready)
            .count();
        let building = circuits
            .values()
            .filter(|c| c.status == CircuitStatus::Building)
            .count();
        let failed = circuits
            .values()
            .filter(|c| c.status == CircuitStatus::Failed)
            .count();
        let expired = circuits
            .values()
            .filter(|c| c.is_expired())
            .count();

        CircuitStats {
            total,
            ready,
            building,
            failed,
            expired,
        }
    }

    /// List all active circuit IDs
    pub async fn list_circuits(&self) -> Vec<CircuitId> {
        self.circuits.read().await.keys().copied().collect()
    }
}

impl Default for CircuitManager {
    fn default() -> Self {
        Self::new()
    }
}

/// Circuit statistics
#[derive(Debug, Clone)]
pub struct CircuitStats {
    pub total: usize,
    pub ready: usize,
    pub building: usize,
    pub failed: usize,
    pub expired: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_hop(peer_id: &str, region: &str) -> CircuitHop {
        CircuitHop {
            peer_id: peer_id.to_string(),
            region: region.to_string(),
            ephemeral_key: vec![0u8; 32],
            shared_secret: vec![0u8; 32],
        }
    }

    #[test]
    fn test_circuit_id_generation() {
        let id1 = CircuitId::new();
        let id2 = CircuitId::new();
        assert_ne!(id1, id2);
    }

    #[test]
    fn test_circuit_creation_valid() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let circuit = Circuit::new(hops).unwrap();
        assert_eq!(circuit.hops.len(), 3);
        assert_eq!(circuit.status, CircuitStatus::Building);
        assert!(!circuit.is_expired());
    }

    #[test]
    fn test_circuit_insufficient_hops() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
        ];

        let result = Circuit::new(hops);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CircuitError::InsufficientHops { .. }
        ));
    }

    #[test]
    fn test_circuit_too_many_hops() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
            create_test_hop("peer4", "af-south"),
            create_test_hop("peer5", "sa-east"),
            create_test_hop("peer6", "us-east"),
        ];

        let result = Circuit::new(hops);
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            CircuitError::TooManyHops { .. }
        ));
    }

    #[test]
    fn test_circuit_status_transitions() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let mut circuit = Circuit::new(hops).unwrap();
        assert_eq!(circuit.status, CircuitStatus::Building);

        circuit.mark_ready();
        assert_eq!(circuit.status, CircuitStatus::Ready);
        assert!(circuit.is_ready());

        circuit.mark_failed();
        assert_eq!(circuit.status, CircuitStatus::Failed);
        assert!(!circuit.is_ready());
    }

    #[test]
    fn test_circuit_hop_accessors() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
            create_test_hop("peer4", "af-south"),
        ];

        let circuit = Circuit::new(hops).unwrap();

        assert_eq!(circuit.entry_hop().unwrap().peer_id, "peer1");
        assert_eq!(circuit.exit_hop().unwrap().peer_id, "peer4");
        assert_eq!(circuit.middle_hops().len(), 2);
        assert_eq!(circuit.middle_hops()[0].peer_id, "peer2");
        assert_eq!(circuit.middle_hops()[1].peer_id, "peer3");
    }

    #[test]
    fn test_circuit_traffic_recording() {
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let mut circuit = Circuit::new(hops).unwrap();

        circuit.record_sent(1024);
        circuit.record_sent(2048);
        assert_eq!(circuit.bytes_sent, 3072);

        circuit.record_received(512);
        circuit.record_received(256);
        assert_eq!(circuit.bytes_received, 768);
    }

    #[tokio::test]
    async fn test_circuit_manager_creation() {
        let manager = CircuitManager::new();
        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 0);
    }

    #[tokio::test]
    async fn test_circuit_manager_create_and_get() {
        let manager = CircuitManager::new();
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let circuit_id = manager.create_circuit(hops).await.unwrap();
        let circuit = manager.get_circuit(circuit_id).await.unwrap();

        assert_eq!(circuit.id, circuit_id);
        assert_eq!(circuit.hops.len(), 3);
    }

    #[tokio::test]
    async fn test_circuit_manager_mark_ready() {
        let manager = CircuitManager::new();
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let circuit_id = manager.create_circuit(hops).await.unwrap();
        manager.mark_ready(circuit_id).await.unwrap();

        let circuit = manager.get_circuit(circuit_id).await.unwrap();
        assert_eq!(circuit.status, CircuitStatus::Ready);
    }

    #[tokio::test]
    async fn test_circuit_manager_close_circuit() {
        let manager = CircuitManager::new();
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let circuit_id = manager.create_circuit(hops).await.unwrap();
        manager.close_circuit(circuit_id).await.unwrap();

        let circuit = manager.get_circuit(circuit_id).await;
        assert!(circuit.is_none());
    }

    #[tokio::test]
    async fn test_circuit_manager_stats() {
        let manager = CircuitManager::new();

        let hops1 = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];
        let id1 = manager.create_circuit(hops1).await.unwrap();

        let hops2 = vec![
            create_test_hop("peer4", "af-south"),
            create_test_hop("peer5", "sa-east"),
            create_test_hop("peer6", "us-east"),
        ];
        let id2 = manager.create_circuit(hops2).await.unwrap();

        manager.mark_ready(id1).await.unwrap();

        let stats = manager.get_stats().await;
        assert_eq!(stats.total, 2);
        assert_eq!(stats.ready, 1);
        assert_eq!(stats.building, 1);
    }

    #[tokio::test]
    async fn test_circuit_manager_record_traffic() {
        let manager = CircuitManager::new();
        let hops = vec![
            create_test_hop("peer1", "us-west"),
            create_test_hop("peer2", "eu-central"),
            create_test_hop("peer3", "ap-south"),
        ];

        let circuit_id = manager.create_circuit(hops).await.unwrap();

        manager.record_sent(circuit_id, 1024).await.unwrap();
        manager.record_received(circuit_id, 512).await.unwrap();

        let circuit = manager.get_circuit(circuit_id).await.unwrap();
        assert_eq!(circuit.bytes_sent, 1024);
        assert_eq!(circuit.bytes_received, 512);
    }
}
