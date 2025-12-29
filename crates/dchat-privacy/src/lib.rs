// dchat-privacy: Privacy-preserving cryptographic primitives
//
// This crate implements zero-knowledge proofs, blind tokens, and stealth payloads
// for metadata resistance and anonymous operations in dchat.

pub mod blind_tokens;
pub mod ceremony_gen;
pub mod membership_proof;
pub mod stealth;
pub mod zk_proofs;

pub use blind_tokens::{BlindSigner, BlindToken, TokenIssuer};
pub use ceremony_gen::{
    generate_ceremony_artifacts, get_field_modulus, validate_ceremony_keys,
    verify_ceremony_artifacts, verify_ceremony_binding, verify_ceremony_reproducibility,
    CeremonyArtifacts, CeremonyMetadata, CeremonySeedConfig, ValidatedCeremonyKeys,
};
pub use membership_proof::{
    build_merkle_tree, compute_merkle_root, get_merkle_path, MembershipKeys, MembershipProver,
    MembershipVerifier, NullifierSet, ZkMembershipProof, MAX_MERKLE_DEPTH, MIN_MERKLE_DEPTH,
};
pub use stealth::{StealthAddress, StealthPayload};
pub use zk_proofs::{
    ContactProof, Groth16Keys, KeySource, ReputationProof, ZkProof, ZkProver, ZkVerifier,
};
