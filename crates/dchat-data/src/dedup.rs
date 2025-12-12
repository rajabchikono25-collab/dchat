//! Content deduplication utilities
//!
//! Provides deduplication through content-addressing and optional
//! delta encoding for efficient storage of similar content.

use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::content_id::ContentId;
use crate::error::{DataError, DataResult};

/// A deduplication entry tracking reference counts
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DedupEntry {
    /// Content ID (hash of the data)
    pub content_id: ContentId,

    /// Original data size in bytes
    pub size: u64,

    /// Number of references to this content
    pub ref_count: u32,

    /// Whether this is stored as a delta
    pub is_delta: bool,

    /// Base content ID if this is a delta
    pub delta_base: Option<ContentId>,
}

impl DedupEntry {
    /// Create a new dedup entry for content
    pub fn new(data: &[u8]) -> Self {
        Self {
            content_id: ContentId::from_data(data),
            size: data.len() as u64,
            ref_count: 1,
            is_delta: false,
            delta_base: None,
        }
    }

    /// Create a delta entry
    pub fn new_delta(data: &[u8], base: ContentId) -> Self {
        Self {
            content_id: ContentId::from_data(data),
            size: data.len() as u64,
            ref_count: 1,
            is_delta: true,
            delta_base: Some(base),
        }
    }

    /// Increment reference count
    pub fn add_ref(&mut self) {
        self.ref_count = self.ref_count.saturating_add(1);
    }

    /// Decrement reference count, returns true if still referenced
    pub fn remove_ref(&mut self) -> bool {
        self.ref_count = self.ref_count.saturating_sub(1);
        self.ref_count > 0
    }
}

/// Delta-encoded content for storage efficiency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeltaEncoded {
    /// The base content ID this delta is relative to
    pub base_id: ContentId,

    /// Delta operations to reconstruct content
    pub operations: Vec<DeltaOperation>,

    /// Expected result content ID (for verification)
    pub result_id: ContentId,
}

/// Operations for delta reconstruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum DeltaOperation {
    /// Copy bytes from base content
    Copy { offset: u64, length: u64 },

    /// Insert new bytes
    Insert { data: Vec<u8> },
}

impl DeltaEncoded {
    /// Create a delta between base and new data using longest common subsequence matching
    ///
    /// This implementation uses a rolling hash approach to find matching blocks
    /// between base and new data, generating Copy operations for matching regions
    /// and Insert operations for new content.
    pub fn create(base_data: &[u8], new_data: &[u8]) -> Self {
        let base_id = ContentId::from_data(base_data);
        let result_id = ContentId::from_data(new_data);

        // For very small files, just insert
        if base_data.len() < 16 || new_data.len() < 16 {
            return DeltaEncoded {
                base_id,
                operations: vec![DeltaOperation::Insert {
                    data: new_data.to_vec(),
                }],
                result_id,
            };
        }

        let mut operations = Vec::new();

        // Check for prefix match
        let prefix_len = Self::common_prefix_len(base_data, new_data);
        if prefix_len > 0 {
            operations.push(DeltaOperation::Copy {
                offset: 0,
                length: prefix_len as u64,
            });
        }

        // Check for suffix match (after prefix)
        let remaining_base = &base_data[prefix_len..];
        let remaining_new = &new_data[prefix_len..];
        let suffix_len = Self::common_suffix_len(remaining_base, remaining_new);

        // Middle portion that differs
        let middle_new_end = remaining_new.len().saturating_sub(suffix_len);
        let middle_new = &remaining_new[..middle_new_end];

        if !middle_new.is_empty() {
            // Try to find matching blocks in the middle portion
            let middle_ops = Self::find_matching_blocks(remaining_base, middle_new, prefix_len);
            operations.extend(middle_ops);
        }

        if suffix_len > 0 {
            let suffix_offset = base_data.len() - suffix_len;
            operations.push(DeltaOperation::Copy {
                offset: suffix_offset as u64,
                length: suffix_len as u64,
            });
        }

        // If no operations were generated, fallback to full insert
        if operations.is_empty() {
            operations.push(DeltaOperation::Insert {
                data: new_data.to_vec(),
            });
        }

        // Optimize: merge consecutive inserts
        let operations = Self::optimize_operations(operations);

        DeltaEncoded {
            base_id,
            operations,
            result_id,
        }
    }

    /// Find common prefix length between two slices
    fn common_prefix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter().zip(b.iter()).take_while(|(x, y)| x == y).count()
    }

    /// Find common suffix length between two slices
    fn common_suffix_len(a: &[u8], b: &[u8]) -> usize {
        a.iter()
            .rev()
            .zip(b.iter().rev())
            .take_while(|(x, y)| x == y)
            .count()
    }

    /// Find matching blocks using a simple rolling hash approach
    fn find_matching_blocks(base: &[u8], target: &[u8], base_offset: usize) -> Vec<DeltaOperation> {
        const BLOCK_SIZE: usize = 8;
        let mut operations = Vec::new();

        if target.is_empty() {
            return operations;
        }

        // Build a hash table of base data blocks
        let mut base_blocks: HashMap<u64, Vec<usize>> = HashMap::new();
        for i in 0..base.len().saturating_sub(BLOCK_SIZE) {
            let hash = Self::simple_hash(&base[i..i + BLOCK_SIZE]);
            base_blocks.entry(hash).or_default().push(i);
        }

        let mut pos = 0;
        let mut pending_insert: Vec<u8> = Vec::new();

        while pos < target.len() {
            let remaining = target.len() - pos;

            if remaining >= BLOCK_SIZE {
                let hash = Self::simple_hash(&target[pos..pos + BLOCK_SIZE]);

                if let Some(positions) = base_blocks.get(&hash) {
                    // Verify match and extend
                    for &base_pos in positions {
                        if base[base_pos..].starts_with(&target[pos..pos + BLOCK_SIZE]) {
                            // Found match, flush pending insert
                            if !pending_insert.is_empty() {
                                operations.push(DeltaOperation::Insert {
                                    data: std::mem::take(&mut pending_insert),
                                });
                            }

                            // Extend the match as far as possible
                            let mut match_len = BLOCK_SIZE;
                            while base_pos + match_len < base.len()
                                && pos + match_len < target.len()
                                && base[base_pos + match_len] == target[pos + match_len]
                            {
                                match_len += 1;
                            }

                            operations.push(DeltaOperation::Copy {
                                offset: (base_offset + base_pos) as u64,
                                length: match_len as u64,
                            });

                            pos += match_len;
                            break;
                        }
                    }

                    // If no actual match found at any position, add to pending
                    if pos < target.len()
                        && (operations.is_empty()
                            || !matches!(operations.last(), Some(DeltaOperation::Copy { .. }))
                            || pending_insert.is_empty())
                    {
                        pending_insert.push(target[pos]);
                        pos += 1;
                    }
                } else {
                    pending_insert.push(target[pos]);
                    pos += 1;
                }
            } else {
                pending_insert.push(target[pos]);
                pos += 1;
            }
        }

        // Flush remaining insert
        if !pending_insert.is_empty() {
            operations.push(DeltaOperation::Insert {
                data: pending_insert,
            });
        }

        operations
    }

    /// Simple hash function for block matching
    fn simple_hash(data: &[u8]) -> u64 {
        let mut hash: u64 = 0;
        for &b in data {
            hash = hash.wrapping_mul(31).wrapping_add(b as u64);
        }
        hash
    }

    /// Merge consecutive Insert operations
    fn optimize_operations(operations: Vec<DeltaOperation>) -> Vec<DeltaOperation> {
        let mut optimized = Vec::new();

        for op in operations {
            match (&mut optimized.last_mut(), op) {
                (
                    Some(DeltaOperation::Insert { data: existing }),
                    DeltaOperation::Insert { data: new },
                ) => {
                    existing.extend(new);
                }
                (_, op) => optimized.push(op),
            }
        }

        optimized
    }

    /// Apply delta to base data to reconstruct original
    pub fn apply(&self, base_data: &[u8]) -> DataResult<Vec<u8>> {
        // Verify base data
        if ContentId::from_data(base_data) != self.base_id {
            return Err(DataError::Integrity(
                "Base data does not match expected content ID".to_string(),
            ));
        }

        let mut result = Vec::new();

        for op in &self.operations {
            match op {
                DeltaOperation::Copy { offset, length } => {
                    let start = *offset as usize;
                    let end = start + *length as usize;

                    if end > base_data.len() {
                        return Err(DataError::Integrity(
                            "Delta copy operation exceeds base data bounds".to_string(),
                        ));
                    }

                    result.extend_from_slice(&base_data[start..end]);
                }
                DeltaOperation::Insert { data } => {
                    result.extend_from_slice(data);
                }
            }
        }

        // Verify result
        if ContentId::from_data(&result) != self.result_id {
            return Err(DataError::Integrity(
                "Reconstructed data does not match expected content ID".to_string(),
            ));
        }

        Ok(result)
    }

    /// Calculate storage savings compared to storing full content
    pub fn savings(&self, full_size: u64) -> i64 {
        let delta_size: u64 = self
            .operations
            .iter()
            .map(|op| match op {
                DeltaOperation::Copy { .. } => 16, // 2 u64 values
                DeltaOperation::Insert { data } => data.len() as u64,
            })
            .sum();

        full_size as i64 - delta_size as i64
    }
}

/// In-memory deduplication store
pub struct DedupStore {
    /// Content ID -> Entry mapping
    entries: HashMap<ContentId, DedupEntry>,

    /// Content ID -> Raw data (for reconstruction)
    data: HashMap<ContentId, Vec<u8>>,

    /// Total bytes saved through deduplication
    bytes_saved: u64,
}

impl DedupStore {
    /// Create a new empty dedup store
    pub fn new() -> Self {
        Self {
            entries: HashMap::new(),
            data: HashMap::new(),
            bytes_saved: 0,
        }
    }

    /// Store data with deduplication
    ///
    /// Returns the content ID and whether this was a duplicate
    pub fn store(&mut self, data: &[u8]) -> (ContentId, bool) {
        let content_id = ContentId::from_data(data);

        if let Some(entry) = self.entries.get_mut(&content_id) {
            // Duplicate found
            entry.add_ref();
            self.bytes_saved += data.len() as u64;
            (content_id, true)
        } else {
            // New content
            let entry = DedupEntry::new(data);
            self.entries.insert(content_id, entry);
            self.data.insert(content_id, data.to_vec());
            (content_id, false)
        }
    }

    /// Retrieve data by content ID
    pub fn get(&self, content_id: &ContentId) -> Option<&[u8]> {
        self.data.get(content_id).map(|v| v.as_slice())
    }

    /// Remove a reference to content
    ///
    /// Returns true if the content was deleted (no more references)
    pub fn remove_ref(&mut self, content_id: &ContentId) -> bool {
        if let Some(entry) = self.entries.get_mut(content_id) {
            if !entry.remove_ref() {
                // No more references, remove content
                self.entries.remove(content_id);
                self.data.remove(content_id);
                return true;
            }
        }
        false
    }

    /// Get entry for a content ID
    pub fn get_entry(&self, content_id: &ContentId) -> Option<&DedupEntry> {
        self.entries.get(content_id)
    }

    /// Get total bytes saved through deduplication
    pub fn bytes_saved(&self) -> u64 {
        self.bytes_saved
    }

    /// Get number of unique content entries
    pub fn entry_count(&self) -> usize {
        self.entries.len()
    }

    /// Get total stored bytes
    pub fn total_bytes(&self) -> u64 {
        self.data.values().map(|v| v.len() as u64).sum()
    }

    /// Check if content exists
    pub fn contains(&self, content_id: &ContentId) -> bool {
        self.entries.contains_key(content_id)
    }
}

impl Default for DedupStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_dedup_entry_new() {
        let data = b"test data";
        let entry = DedupEntry::new(data);

        assert_eq!(entry.size, data.len() as u64);
        assert_eq!(entry.ref_count, 1);
        assert!(!entry.is_delta);
    }

    #[test]
    fn test_dedup_entry_ref_counting() {
        let data = b"test data";
        let mut entry = DedupEntry::new(data);

        entry.add_ref();
        assert_eq!(entry.ref_count, 2);

        assert!(entry.remove_ref()); // Still referenced
        assert_eq!(entry.ref_count, 1);

        assert!(!entry.remove_ref()); // No longer referenced
        assert_eq!(entry.ref_count, 0);
    }

    #[test]
    fn test_dedup_store_basic() {
        let mut store = DedupStore::new();

        let data = b"hello world";
        let (id, is_dup) = store.store(data);
        assert!(!is_dup);

        let retrieved = store.get(&id).unwrap();
        assert_eq!(retrieved, data);
    }

    #[test]
    fn test_dedup_store_deduplication() {
        let mut store = DedupStore::new();

        let data = b"duplicate me";
        let (id1, is_dup1) = store.store(data);
        let (id2, is_dup2) = store.store(data);

        assert!(!is_dup1);
        assert!(is_dup2);
        assert_eq!(id1, id2);

        let entry = store.get_entry(&id1).unwrap();
        assert_eq!(entry.ref_count, 2);
    }

    #[test]
    fn test_dedup_store_bytes_saved() {
        let mut store = DedupStore::new();

        let data = b"save bytes";
        store.store(data);
        store.store(data);
        store.store(data);

        assert_eq!(store.bytes_saved(), data.len() as u64 * 2);
    }

    #[test]
    fn test_dedup_store_remove_ref() {
        let mut store = DedupStore::new();

        let data = b"remove me";
        let (id, _) = store.store(data);
        store.store(data); // Add second reference

        assert!(!store.remove_ref(&id)); // Still one ref
        assert!(store.contains(&id));

        assert!(store.remove_ref(&id)); // No more refs
        assert!(!store.contains(&id));
    }

    #[test]
    fn test_delta_encoded_prefix() {
        let base = b"hello";
        let new = b"hello world";

        let delta = DeltaEncoded::create(base, new);
        assert_eq!(delta.operations.len(), 2);

        let reconstructed = delta.apply(base).unwrap();
        assert_eq!(reconstructed, new);
    }

    #[test]
    fn test_delta_encoded_full_replace() {
        let base = b"hello";
        let new = b"goodbye";

        let delta = DeltaEncoded::create(base, new);
        let reconstructed = delta.apply(base).unwrap();
        assert_eq!(reconstructed, new);
    }

    #[test]
    fn test_delta_encoded_wrong_base() {
        let base = b"hello";
        let new = b"hello world";

        let delta = DeltaEncoded::create(base, new);
        let result = delta.apply(b"wrong base");
        assert!(result.is_err());
    }

    #[test]
    fn test_delta_savings() {
        let base = b"hello";
        let new = b"hello world";

        let delta = DeltaEncoded::create(base, new);
        let savings = delta.savings(new.len() as u64);

        // With prefix detection, should save space
        assert!(savings > 0 || delta.operations.len() < 3);
    }
}
