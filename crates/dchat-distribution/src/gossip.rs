use crate::package::{DownloadSource, PackageMetadata};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use uuid::Uuid;

/// Gossip message for announcing new versions
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VersionAnnouncement {
    /// Message ID
    pub id: Uuid,

    /// Announcing node ID
    pub node_id: String,

    /// Version being announced
    pub version: String,

    /// Package metadata
    pub metadata: PackageMetadata,

    /// Download sources for this version
    pub sources: Vec<DownloadSource>,

    /// Timestamp of announcement
    pub timestamp: DateTime<Utc>,

    /// Time-to-live (number of hops)
    pub ttl: u8,
}

/// Update discovery via peer gossip protocol
pub struct GossipDiscovery {
    /// Known version announcements
    announcements: HashMap<String, VersionAnnouncement>,

    /// Recently seen announcement IDs (for deduplication)
    seen_announcements: HashMap<Uuid, DateTime<Utc>>,

    /// Maximum TTL for announcements
    max_ttl: u8,
}

impl GossipDiscovery {
    pub fn new(max_ttl: u8) -> Self {
        Self {
            announcements: HashMap::new(),
            seen_announcements: HashMap::new(),
            max_ttl,
        }
    }

    /// Process a received version announcement
    pub fn handle_announcement(&mut self, mut announcement: VersionAnnouncement) -> bool {
        // Check if we've seen this announcement before
        if self.seen_announcements.contains_key(&announcement.id) {
            return false; // Already processed
        }

        // Record as seen
        self.seen_announcements.insert(announcement.id, Utc::now());

        // Check TTL
        if announcement.ttl == 0 {
            return false; // Don't propagate further
        }

        // Store announcement
        self.announcements
            .insert(announcement.version.clone(), announcement.clone());

        // Decrement TTL for propagation
        announcement.ttl -= 1;

        true // Should propagate to other peers
    }

    /// Get all known versions
    pub fn get_known_versions(&self) -> Vec<String> {
        self.announcements.keys().cloned().collect()
    }

    /// Get announcement for a specific version
    pub fn get_announcement(&self, version: &str) -> Option<&VersionAnnouncement> {
        self.announcements.get(version)
    }

    /// Create a new version announcement to broadcast
    pub fn create_announcement(
        &self,
        node_id: String,
        version: String,
        metadata: PackageMetadata,
        sources: Vec<DownloadSource>,
    ) -> VersionAnnouncement {
        VersionAnnouncement {
            id: Uuid::new_v4(),
            node_id,
            version,
            metadata,
            sources,
            timestamp: Utc::now(),
            ttl: self.max_ttl,
        }
    }

    /// Prune old announcements
    pub fn prune_old_announcements(&mut self, max_age_hours: i64) {
        let cutoff = Utc::now() - chrono::Duration::hours(max_age_hours);

        self.announcements.retain(|_, ann| ann.timestamp > cutoff);
        self.seen_announcements.retain(|_, ts| *ts > cutoff);
    }
}

/// Update scheduler for automatic version checking
pub struct UpdateScheduler {
    /// Last check timestamp
    last_check: Option<DateTime<Utc>>,

    /// Check interval in hours
    check_interval_hours: u64,

    /// Pending download version
    pending_download: Option<String>,
}

impl UpdateScheduler {
    pub fn new(check_interval_hours: u64) -> Self {
        Self {
            last_check: None,
            check_interval_hours,
            pending_download: None,
        }
    }

    /// Check if it's time to check for updates
    pub fn should_check_now(&self) -> bool {
        match self.last_check {
            None => true, // Never checked
            Some(last) => {
                let elapsed = Utc::now() - last;
                elapsed.num_hours() >= self.check_interval_hours as i64
            }
        }
    }

    /// Mark that we just checked
    pub fn mark_checked(&mut self) {
        self.last_check = Some(Utc::now());
    }

    /// Set pending download
    pub fn set_pending_download(&mut self, version: String) {
        self.pending_download = Some(version);
    }

    /// Get pending download
    pub fn get_pending_download(&self) -> Option<&String> {
        self.pending_download.as_ref()
    }

    /// Clear pending download
    pub fn clear_pending_download(&mut self) {
        self.pending_download = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::package::PackageType;

    #[test]
    fn test_gossip_discovery_deduplication() {
        let mut discovery = GossipDiscovery::new(5);

        let announcement = VersionAnnouncement {
            id: Uuid::new_v4(),
            node_id: "node1".to_string(),
            version: "1.2.3".to_string(),
            metadata: PackageMetadata {
                version: "1.2.3".to_string(),
                release_date: Utc::now(),
                package_type: PackageType::Binary,
                platform: "linux-x64".to_string(),
                sha256: "abc".to_string(),
                blake3: "def".to_string(),
                size_bytes: 1024,
                signature: vec![],
                signer_pubkey: vec![],
                release_notes_url: None,
                min_compatible_version: None,
            },
            sources: vec![],
            timestamp: Utc::now(),
            ttl: 5,
        };

        // First time should be accepted
        assert!(discovery.handle_announcement(announcement.clone()));

        // Second time should be rejected (duplicate)
        assert!(!discovery.handle_announcement(announcement.clone()));
    }

    #[test]
    fn test_gossip_ttl_decrement() {
        let mut discovery = GossipDiscovery::new(5);

        let mut announcement = VersionAnnouncement {
            id: Uuid::new_v4(),
            node_id: "node1".to_string(),
            version: "1.2.3".to_string(),
            metadata: PackageMetadata {
                version: "1.2.3".to_string(),
                release_date: Utc::now(),
                package_type: PackageType::Binary,
                platform: "linux-x64".to_string(),
                sha256: "abc".to_string(),
                blake3: "def".to_string(),
                size_bytes: 1024,
                signature: vec![],
                signer_pubkey: vec![],
                release_notes_url: None,
                min_compatible_version: None,
            },
            sources: vec![],
            timestamp: Utc::now(),
            ttl: 1,
        };

        // Should propagate (TTL = 1 -> 0)
        assert!(discovery.handle_announcement(announcement.clone()));

        // TTL = 0 should not propagate
        announcement.id = Uuid::new_v4(); // Different message
        announcement.ttl = 0;
        assert!(!discovery.handle_announcement(announcement));
    }

    #[test]
    fn test_update_scheduler() {
        let mut scheduler = UpdateScheduler::new(24);

        // Should check initially
        assert!(scheduler.should_check_now());

        scheduler.mark_checked();

        // Should not check immediately after
        assert!(!scheduler.should_check_now());
    }

    #[test]
    fn test_pending_download() {
        let mut scheduler = UpdateScheduler::new(24);

        assert!(scheduler.get_pending_download().is_none());

        scheduler.set_pending_download("1.2.3".to_string());
        assert_eq!(scheduler.get_pending_download(), Some(&"1.2.3".to_string()));

        scheduler.clear_pending_download();
        assert!(scheduler.get_pending_download().is_none());
    }
}
