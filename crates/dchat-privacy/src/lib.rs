// dchat-privacy: Privacy-preserving cryptographic primitives
//
// This crate implements zero-knowledge proofs, blind tokens, and stealth payloads
// for metadata resistance and anonymous operations in dchat.

pub mod blind_tokens;
pub mod stealth;
pub mod zk_proofs;

pub use blind_tokens::{BlindSigner, BlindToken, TokenIssuer};
pub use stealth::{StealthAddress, StealthPayload};
pub use zk_proofs::{ContactProof, Groth16Keys, ReputationProof, ZkProof, ZkProver, ZkVerifier};
