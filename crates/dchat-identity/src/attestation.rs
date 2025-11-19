use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use chrono::{DateTime, Utc};
use crate::verification::{VerifiedBadge, VerificationProof, ProofType};
use dchat_core::types::Signature;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    pub verified: bool,
    pub reason: Option<String>,
}

/// Verify a device attestation payload. For now this accepts the simulated
/// attestation string produced by `src/onboarding/keyless/enclave.rs` and
/// returns a successful verification. In production, this should validate
/// certificate chains, signatures, and platform-specific proofs.
pub fn verify_device_attestation(attestation: &str) -> Result<AttestationResult> {
    // Quick simulated verifier
    if attestation == "dchat-enclave-attestation-v1" {
        Ok(AttestationResult {
            verified: true,
            reason: None,
        })
    } else {
        Err(Error::unauthenticated("Attestation verification failed"))
    }
}

/// Create a `VerifiedBadge` from a successful attestation.
/// This is a development helper that packages attestation into a badge.
pub fn badge_from_attestation(issuer: String, attestation: &str) -> Result<VerifiedBadge> {
    // Verify attestation first
    let _ = verify_device_attestation(attestation)?;

    let now: DateTime<Utc> = Utc::now();

    let proof = VerificationProof {
        proof_type: ProofType::SelfSigned,
        signature: Signature::new(vec![0u8; 64]),
        metadata: {
            let mut m = std::collections::HashMap::new();
            m.insert("attestation".to_string(), attestation.to_string());
            m
        },
    };

    let badge = VerifiedBadge {
        badge_type: crate::verification::BadgeType::Verified,
        issued_at: now,
        expires_at: None,
        issuer,
        proof,
    };

    Ok(badge)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_verify_simulated_attestation() {
        let res = verify_device_attestation("dchat-enclave-attestation-v1").unwrap();
        assert!(res.verified);
    }

    #[test]
    fn test_verify_bad_attestation() {
        let res = verify_device_attestation("invalid");
        assert!(res.is_err());
    }

    #[test]
    fn test_badge_from_attestation() {
        let badge = badge_from_attestation("system".to_string(), "dchat-enclave-attestation-v1").unwrap();
        assert_eq!(badge.issuer, "system");
        assert!(badge.is_valid());
    }
}
