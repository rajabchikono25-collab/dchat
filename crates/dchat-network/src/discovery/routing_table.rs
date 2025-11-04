// Kademlia routing table implementation

use super::peer_info::PeerInfo;
use libp2p::PeerId;
use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// K-bucket based routing table for Kademlia DHT
#[allow(dead_code)]
pub struct RoutingTable {
    local_id: PeerId,
    buckets: Vec<KBucket>,
    k: usize, // Bucket size (typically 20)
}

impl RoutingTable {
    /// Create a new routing table for the given peer
    pub fn new(local_id: PeerId, k: usize) -> Self {
        // Create 256 buckets (one for each bit position in PeerId)
        let buckets = (0..256).map(|_| KBucket::new(k)).collect();

        Self {
            local_id,
            buckets,
            k,
        }
    }

    /// Add a peer to the routing table
    pub fn add_peer(&mut self, peer: PeerInfo) -> Result<(), String> {
        if peer.peer_id == self.local_id {
            return Err("Cannot add self to routing table".to_string());
        }

        let bucket_index = self.bucket_index(&peer.peer_id);
        self.buckets[bucket_index].add_peer(peer)
    }

    /// Remove a peer from the routing table
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        let bucket_index = self.bucket_index(peer_id);
        self.buckets[bucket_index].remove_peer(peer_id);
    }

    /// Find the k closest peers to a target
    pub fn find_closest(&self, target: &PeerId, count: usize) -> Vec<PeerInfo> {
        let mut peers = Vec::new();

        // Start from the bucket closest to target
        let bucket_index = self.bucket_index(target);

        // Collect from closest bucket first
        peers.extend(self.buckets[bucket_index].peers());

        // Then spiral outward
        let mut offset = 1;
        while peers.len() < count && offset < 256 {
            if bucket_index >= offset {
                peers.extend(self.buckets[bucket_index - offset].peers());
            }
            if bucket_index + offset < 256 {
                peers.extend(self.buckets[bucket_index + offset].peers());
            }
            offset += 1;
        }

        // Sort by XOR distance and take closest
        peers.sort_by_key(|p| xor_distance(&p.peer_id, target));
        peers.truncate(count);

        peers
    }

    /// Update last-seen timestamp for a peer
    pub fn update_peer(&mut self, peer_id: &PeerId) -> Result<(), String> {
        let bucket_index = self.bucket_index(peer_id);
        self.buckets[bucket_index].update_peer(peer_id)
    }

    /// Get all peers in the routing table
    pub fn all_peers(&self) -> Vec<PeerInfo> {
        self.buckets
            .iter()
            .flat_map(|bucket| bucket.peers())
            .collect()
    }

    /// Get total number of peers
    pub fn peer_count(&self) -> usize {
        self.buckets.iter().map(|b| b.len()).sum()
    }

    /// Remove stale peers that haven't been seen recently
    pub fn remove_stale_peers(&mut self, timeout: Duration) {
        for bucket in &mut self.buckets {
            bucket.remove_stale(timeout);
        }
    }

    /// Get bucket statistics
    pub fn bucket_stats(&self) -> Vec<(usize, usize)> {
        self.buckets
            .iter()
            .enumerate()
            .map(|(i, b)| (i, b.len()))
            .filter(|(_, len)| *len > 0)
            .collect()
    }

    /// Calculate which bucket a peer should go in
    fn bucket_index(&self, peer_id: &PeerId) -> usize {
        let distance = xor_distance(&self.local_id, peer_id);

        // Find the most significant bit position
        // This gives us the bucket index (0-255)
        for i in (0..256).rev() {
            let bit_mask = U256::from(1u128) << i;
            if distance & bit_mask != U256::zero() {
                return 255 - i;
            }
        }

        0 // Default to bucket 0 if all bits are zero
    }
}

/// A single k-bucket in the routing table
pub struct KBucket {
    peers: VecDeque<PeerInfo>,
    max_size: usize,
    last_updated: Instant,
}

impl KBucket {
    /// Create a new k-bucket
    pub fn new(max_size: usize) -> Self {
        Self {
            peers: VecDeque::new(),
            max_size,
            last_updated: Instant::now(),
        }
    }

    /// Add a peer to the bucket
    pub fn add_peer(&mut self, peer: PeerInfo) -> Result<(), String> {
        // Check if peer already exists
        if let Some(pos) = self.peers.iter().position(|p| p.peer_id == peer.peer_id) {
            // Move to end (most recently seen)
            let mut existing = self.peers.remove(pos).unwrap();
            existing.touch();
            self.peers.push_back(existing);
            self.last_updated = Instant::now();
            return Ok(());
        }

        // If bucket is full, remove least recently seen
        if self.peers.len() >= self.max_size {
            self.peers.pop_front();
        }

        self.peers.push_back(peer);
        self.last_updated = Instant::now();
        Ok(())
    }

    /// Remove a peer from the bucket
    pub fn remove_peer(&mut self, peer_id: &PeerId) {
        if let Some(pos) = self.peers.iter().position(|p| p.peer_id == *peer_id) {
            self.peers.remove(pos);
            self.last_updated = Instant::now();
        }
    }

    /// Update last-seen for a peer
    pub fn update_peer(&mut self, peer_id: &PeerId) -> Result<(), String> {
        if let Some(pos) = self.peers.iter().position(|p| p.peer_id == *peer_id) {
            let mut peer = self.peers.remove(pos).unwrap();
            peer.touch();
            self.peers.push_back(peer);
            self.last_updated = Instant::now();
            Ok(())
        } else {
            Err("Peer not found in bucket".to_string())
        }
    }

    /// Get all peers in the bucket
    pub fn peers(&self) -> Vec<PeerInfo> {
        self.peers.iter().cloned().collect()
    }

    /// Get number of peers in bucket
    pub fn len(&self) -> usize {
        self.peers.len()
    }

    /// Check if bucket is empty
    pub fn is_empty(&self) -> bool {
        self.peers.is_empty()
    }

    /// Remove stale peers
    pub fn remove_stale(&mut self, timeout: Duration) {
        self.peers.retain(|p| !p.is_stale(timeout));
        if !self.peers.is_empty() {
            self.last_updated = Instant::now();
        }
    }
}

/// Simplified U256 for XOR distance calculation
/// In production, use a proper big integer library like uint
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct U256([u64; 4]);

impl U256 {
    fn zero() -> Self {
        U256([0; 4])
    }

    fn from(val: u128) -> Self {
        U256([val as u64, (val >> 64) as u64, 0, 0])
    }
}

impl std::ops::BitAnd for U256 {
    type Output = Self;

    fn bitand(self, rhs: Self) -> Self::Output {
        U256([
            self.0[0] & rhs.0[0],
            self.0[1] & rhs.0[1],
            self.0[2] & rhs.0[2],
            self.0[3] & rhs.0[3],
        ])
    }
}

impl std::ops::Shl<usize> for U256 {
    type Output = Self;

    fn shl(self, _rhs: usize) -> Self::Output {
        // Simplified shift for demonstration
        // In production, implement proper 256-bit shift
        self
    }
}

/// Calculate XOR distance between two peer IDs
fn xor_distance(a: &PeerId, b: &PeerId) -> U256 {
    // Convert PeerIds to bytes and XOR them
    let a_bytes = a.to_bytes();
    let b_bytes = b.to_bytes();

    let mut result = [0u64; 4];

    // XOR the bytes (simplified - assumes 32-byte peer IDs)
    for i in 0..4 {
        let mut a_part = 0u64;
        let mut b_part = 0u64;

        for j in 0..8 {
            let idx = i * 8 + j;
            if idx < a_bytes.len() {
                a_part |= (a_bytes[idx] as u64) << (j * 8);
            }
            if idx < b_bytes.len() {
                b_part |= (b_bytes[idx] as u64) << (j * 8);
            }
        }

        result[i] = a_part ^ b_part;
    }

    U256(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use libp2p::Multiaddr;

    #[test]
    fn test_routing_table_creation() {
        let local_id = PeerId::random();
        let table = RoutingTable::new(local_id, 20);

        assert_eq!(table.peer_count(), 0);
        assert_eq!(table.buckets.len(), 256);
    }

    #[test]
    fn test_add_peer() {
        let local_id = PeerId::random();
        let mut table = RoutingTable::new(local_id, 20);

        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let peer_info = PeerInfo::new(peer_id, vec![addr]);

        assert!(table.add_peer(peer_info).is_ok());
        assert_eq!(table.peer_count(), 1);
    }

    #[test]
    fn test_cannot_add_self() {
        let local_id = PeerId::random();
        let mut table = RoutingTable::new(local_id, 20);

        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let peer_info = PeerInfo::new(local_id, vec![addr]);

        assert!(table.add_peer(peer_info).is_err());
    }

    #[test]
    fn test_remove_peer() {
        let local_id = PeerId::random();
        let mut table = RoutingTable::new(local_id, 20);

        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
        let peer_info = PeerInfo::new(peer_id, vec![addr]);

        table.add_peer(peer_info).unwrap();
        assert_eq!(table.peer_count(), 1);

        table.remove_peer(&peer_id);
        assert_eq!(table.peer_count(), 0);
    }

    #[test]
    fn test_find_closest() {
        let local_id = PeerId::random();
        let mut table = RoutingTable::new(local_id, 20);

        // Add some peers
        for _ in 0..10 {
            let peer_id = PeerId::random();
            let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9000".parse().unwrap();
            let peer_info = PeerInfo::new(peer_id, vec![addr]);
            table.add_peer(peer_info).unwrap();
        }

        let target = PeerId::random();
        let closest = table.find_closest(&target, 5);

        assert!(closest.len() <= 5);
    }

    #[test]
    fn test_k_bucket() {
        let mut bucket = KBucket::new(3);

        assert_eq!(bucket.len(), 0);
        assert!(bucket.is_empty());

        // Add peers
        for i in 0..3 {
            let peer_id = PeerId::random();
            let addr: Multiaddr = format!("/ip4/127.0.0.1/tcp/{}", 9000 + i).parse().unwrap();
            let peer_info = PeerInfo::new(peer_id, vec![addr]);
            bucket.add_peer(peer_info).unwrap();
        }

        assert_eq!(bucket.len(), 3);

        // Adding 4th peer should evict oldest
        let peer_id = PeerId::random();
        let addr: Multiaddr = "/ip4/127.0.0.1/tcp/9003".parse().unwrap();
        let peer_info = PeerInfo::new(peer_id, vec![addr]);
        bucket.add_peer(peer_info).unwrap();

        assert_eq!(bucket.len(), 3);
    }
}
