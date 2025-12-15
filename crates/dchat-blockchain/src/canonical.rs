//! Deterministic serialization and domain-separated hashing utilities.

use crate::block_hierarchy::Hash;
use serde::Serialize;

pub const DOMAIN_SEP_MINIBLOCK_HEADER_HASH_V1: &[u8] = b"dchat/miniblock/header_hash/v1";
pub const DOMAIN_SEP_TX_LEAF_V1: &[u8] = b"dchat/miniblock/tx_leaf/v1";
pub const DOMAIN_SEP_RECEIPT_LEAF_V1: &[u8] = b"dchat/miniblock/receipt_leaf/v1";
pub const DOMAIN_SEP_MERKLE_NODE_V1: &[u8] = b"dchat/merkle/node/v1";
pub const DOMAIN_SEP_DA_CHUNK_LEAF_V1: &[u8] = b"dchat/da/chunk_leaf/v1";
pub const DOMAIN_SEP_DA_SHARD_LEAF_V1: &[u8] = b"dchat/da/shard_leaf/v1";
pub const DOMAIN_SEP_ERROR_V1: &[u8] = b"dchat/error/v1";

/// Canonical bincode serialization.
///
/// We fix endianess and integer encoding so hashing/signing is deterministic across nodes.
pub fn canonical_serialize<T: Serialize>(value: &T) -> Vec<u8> {
    use bincode::Options;

    bincode::DefaultOptions::new()
        .with_fixint_encoding()
        .with_little_endian()
        .serialize(value)
        .expect("canonical bincode serialization should not fail")
}

/// Domain-separated hash: H(tag || 0x00 || data)
pub fn domain_hash(tag: &[u8], data: &[u8]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(tag);
    hasher.update(&[0u8]);
    hasher.update(data);
    Hash::from(*hasher.finalize().as_bytes())
}

/// Domain-separated hash over multiple slices.
pub fn domain_hash_parts(tag: &[u8], parts: &[&[u8]]) -> Hash {
    let mut hasher = blake3::Hasher::new();
    hasher.update(tag);
    hasher.update(&[0u8]);
    for p in parts {
        hasher.update(p);
        hasher.update(&[0u8]);
    }
    Hash::from(*hasher.finalize().as_bytes())
}
