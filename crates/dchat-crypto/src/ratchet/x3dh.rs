//! X3DH (Extended Triple Diffie-Hellman) Key Agreement
//!
//! X3DH establishes the initial shared secret between two parties,
//! which is then used to initialize the Double Ratchet.

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use x25519_dalek::{PublicKey, SharedSecret, StaticSecret};
use zeroize::{Zeroize, ZeroizeOnDrop};

use crate::kdf::Hkdf;
use crate::signatures::{Signature, SigningKey, VerifyingKey};

/// X3DH Identity Key (long-term key pair)
pub struct X3dhIdentityKey {
    /// Ed25519 signing key for identity
    signing_key: SigningKey,
    /// X25519 key derived from identity for DH
    dh_secret: StaticSecret,
    /// Public DH key
    dh_public: PublicKey,
    /// Verifying key (cached)
    verifying_key: VerifyingKey,
}

impl X3dhIdentityKey {
    /// Generate a new identity key pair
    pub fn generate() -> Self {
        let mut rng = rand::thread_rng();
        let signing_key = SigningKey::generate(&mut rng);
        let verifying_key = signing_key.verifying_key();

        // Derive X25519 key from Ed25519 key (using hash of verifying key as seed)
        let seed = crate::hash(verifying_key.as_bytes());
        let dh_secret = StaticSecret::from(seed);
        let dh_public = PublicKey::from(&dh_secret);

        Self {
            signing_key,
            dh_secret,
            dh_public,
            verifying_key,
        }
    }

    /// Get the identity public key for signing verification
    pub fn verifying_key(&self) -> VerifyingKey {
        self.verifying_key.clone()
    }

    /// Get the DH public key
    pub fn dh_public_key(&self) -> &PublicKey {
        &self.dh_public
    }

    /// Perform DH with a peer's public key
    pub fn dh(&self, peer_public: &PublicKey) -> SharedSecret {
        self.dh_secret.diffie_hellman(peer_public)
    }

    /// Sign data with the identity key
    pub fn sign(&self, data: &[u8]) -> Signature {
        self.signing_key.sign(data)
    }
}

/// X3DH Ephemeral Key (one-time use)
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct X3dhEphemeralKey {
    #[zeroize(skip)]
    public: PublicKey,
    secret: [u8; 32],
}

impl X3dhEphemeralKey {
    /// Generate a new ephemeral key pair
    pub fn generate() -> Self {
        use rand::RngCore;
        let mut secret_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);

        Self {
            public,
            secret: secret_bytes,
        }
    }

    /// Get the public key
    pub fn public_key(&self) -> &PublicKey {
        &self.public
    }

    /// Perform DH
    pub fn dh(&self, peer_public: &PublicKey) -> SharedSecret {
        let secret = StaticSecret::from(self.secret);
        secret.diffie_hellman(peer_public)
    }
}

/// X3DH Signed Pre-Key (medium-term, signed by identity)
#[derive(Clone, Serialize, Deserialize)]
pub struct X3dhSignedPreKey {
    pub public: [u8; 32],
    #[serde(with = "signature_bytes")]
    pub signature: [u8; 64],
    pub timestamp: u64,
}

/// Custom serde module for [u8; 64] signature
mod signature_bytes {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};

    pub fn serialize<S>(bytes: &[u8; 64], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        bytes.to_vec().serialize(serializer)
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 64], D::Error>
    where
        D: Deserializer<'de>,
    {
        let vec: Vec<u8> = Vec::deserialize(deserializer)?;
        vec.try_into()
            .map_err(|_| serde::de::Error::custom("Invalid signature length"))
    }
}

impl X3dhSignedPreKey {
    /// Generate a new signed pre-key
    pub fn generate(identity: &X3dhIdentityKey) -> (Self, StaticSecret) {
        use rand::RngCore;
        let mut secret_bytes = [0u8; 32];
        rand::thread_rng().fill_bytes(&mut secret_bytes);
        let secret = StaticSecret::from(secret_bytes);
        let public = PublicKey::from(&secret);

        let timestamp = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        // Sign the public key
        let mut sign_data = Vec::new();
        sign_data.extend_from_slice(public.as_bytes());
        sign_data.extend_from_slice(&timestamp.to_le_bytes());
        let signature = identity.sign(&sign_data);

        let signed = Self {
            public: *public.as_bytes(),
            signature: signature.to_bytes(),
            timestamp,
        };

        (signed, secret)
    }

    /// Verify the signature
    pub fn verify(&self, identity_key: &VerifyingKey) -> Result<()> {
        let mut sign_data = Vec::new();
        sign_data.extend_from_slice(&self.public);
        sign_data.extend_from_slice(&self.timestamp.to_le_bytes());

        let signature = Signature::from_bytes(self.signature);

        identity_key.verify(&sign_data, &signature)
    }

    /// Get the public key
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from(self.public)
    }
}

/// X3DH One-Time Pre-Key (single use)
#[derive(Clone, Serialize, Deserialize)]
pub struct X3dhOneTimePreKey {
    pub id: u32,
    pub public: [u8; 32],
}

impl X3dhOneTimePreKey {
    /// Generate a batch of one-time pre-keys
    pub fn generate_batch(start_id: u32, count: u32) -> (Vec<Self>, Vec<StaticSecret>) {
        use rand::RngCore;

        let mut public_keys = Vec::with_capacity(count as usize);
        let mut secrets = Vec::with_capacity(count as usize);

        for i in 0..count {
            let mut secret_bytes = [0u8; 32];
            rand::thread_rng().fill_bytes(&mut secret_bytes);
            let secret = StaticSecret::from(secret_bytes);
            let public = PublicKey::from(&secret);

            public_keys.push(Self {
                id: start_id + i,
                public: *public.as_bytes(),
            });
            secrets.push(secret);
        }

        (public_keys, secrets)
    }

    /// Get the public key
    pub fn public_key(&self) -> PublicKey {
        PublicKey::from(self.public)
    }
}

/// Pre-key bundle published by Bob for Alice to initiate X3DH
#[derive(Clone, Serialize, Deserialize)]
pub struct X3dhPreKeyBundle {
    /// Bob's identity public key (for DH)
    pub identity_key: [u8; 32],
    /// Bob's identity verifying key (for signature verification)
    pub identity_verifying_key: [u8; 32],
    /// Bob's signed pre-key
    pub signed_pre_key: X3dhSignedPreKey,
    /// Optional one-time pre-key
    pub one_time_pre_key: Option<X3dhOneTimePreKey>,
}

impl X3dhPreKeyBundle {
    /// Verify the bundle's signatures
    pub fn verify(&self) -> Result<()> {
        let verifying_key = VerifyingKey::from_bytes(&self.identity_verifying_key)
            .map_err(|e| Error::crypto(format!("Invalid identity key: {}", e)))?;

        self.signed_pre_key.verify(&verifying_key)?;

        // Check signed pre-key isn't too old (30 days max)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs())
            .unwrap_or(0);

        const MAX_AGE_SECS: u64 = 30 * 24 * 60 * 60; // 30 days
        if now.saturating_sub(self.signed_pre_key.timestamp) > MAX_AGE_SECS {
            return Err(Error::crypto("Signed pre-key is too old".to_string()));
        }

        Ok(())
    }

    /// Get the identity DH public key
    pub fn identity_dh_key(&self) -> PublicKey {
        PublicKey::from(self.identity_key)
    }
}

/// Result of X3DH key agreement
#[derive(Zeroize, ZeroizeOnDrop)]
pub struct X3dhResult {
    /// The shared secret (used to initialize Double Ratchet)
    pub shared_secret: [u8; 32],
    /// The ephemeral public key (sent to Bob in first message)
    #[zeroize(skip)]
    pub ephemeral_public: [u8; 32],
    /// The one-time pre-key ID used (if any)
    #[zeroize(skip)]
    pub one_time_key_id: Option<u32>,
}

/// Bundle of keys for X3DH initiator (Alice)
pub struct X3dhKeyBundle;

impl X3dhKeyBundle {
    /// Initiate X3DH as sender (Alice)
    ///
    /// Computes: SK = HKDF(DH1 || DH2 || DH3 || [DH4])
    /// - DH1 = DH(IKa, SPKb)
    /// - DH2 = DH(EKa, IKb)
    /// - DH3 = DH(EKa, SPKb)
    /// - DH4 = DH(EKa, OPKb) [if one-time pre-key available]
    pub fn initiate(
        alice_identity: &X3dhIdentityKey,
        bob_bundle: &X3dhPreKeyBundle,
    ) -> Result<X3dhResult> {
        // Verify Bob's bundle
        bob_bundle.verify()?;

        // Generate ephemeral key
        let ephemeral = X3dhEphemeralKey::generate();

        // DH1: Alice identity with Bob's signed pre-key
        let dh1 = alice_identity.dh(&bob_bundle.signed_pre_key.public_key());

        // DH2: Alice ephemeral with Bob's identity
        let dh2 = ephemeral.dh(&bob_bundle.identity_dh_key());

        // DH3: Alice ephemeral with Bob's signed pre-key
        let dh3 = ephemeral.dh(&bob_bundle.signed_pre_key.public_key());

        // Concatenate DH outputs
        let mut dh_concat = Vec::with_capacity(32 * 4);
        dh_concat.extend_from_slice(dh1.as_bytes());
        dh_concat.extend_from_slice(dh2.as_bytes());
        dh_concat.extend_from_slice(dh3.as_bytes());

        // DH4 (optional): Alice ephemeral with Bob's one-time pre-key
        let one_time_key_id = if let Some(ref otpk) = bob_bundle.one_time_pre_key {
            let dh4 = ephemeral.dh(&otpk.public_key());
            dh_concat.extend_from_slice(dh4.as_bytes());
            Some(otpk.id)
        } else {
            None
        };

        // Derive shared secret
        let info = b"X3DH-dchat-v1";
        let sk = Hkdf::derive(None, &dh_concat, info, 32)?;

        let mut shared_secret = [0u8; 32];
        shared_secret.copy_from_slice(&sk);

        Ok(X3dhResult {
            shared_secret,
            ephemeral_public: *ephemeral.public_key().as_bytes(),
            one_time_key_id,
        })
    }

    /// Complete X3DH as receiver (Bob)
    ///
    /// Computes the same SK using Bob's private keys
    pub fn respond(
        bob_identity: &X3dhIdentityKey,
        bob_signed_pre_key_secret: &StaticSecret,
        bob_one_time_pre_key_secret: Option<&StaticSecret>,
        alice_identity_dh_key: &PublicKey,
        alice_ephemeral_key: &PublicKey,
    ) -> Result<[u8; 32]> {
        // DH1: Bob's signed pre-key with Alice's identity
        let dh1 = bob_signed_pre_key_secret.diffie_hellman(alice_identity_dh_key);

        // DH2: Bob's identity with Alice's ephemeral
        let dh2 = bob_identity.dh(alice_ephemeral_key);

        // DH3: Bob's signed pre-key with Alice's ephemeral
        let dh3 = bob_signed_pre_key_secret.diffie_hellman(alice_ephemeral_key);

        // Concatenate DH outputs
        let mut dh_concat = Vec::with_capacity(32 * 4);
        dh_concat.extend_from_slice(dh1.as_bytes());
        dh_concat.extend_from_slice(dh2.as_bytes());
        dh_concat.extend_from_slice(dh3.as_bytes());

        // DH4 (optional)
        if let Some(otpk_secret) = bob_one_time_pre_key_secret {
            let dh4 = otpk_secret.diffie_hellman(alice_ephemeral_key);
            dh_concat.extend_from_slice(dh4.as_bytes());
        }

        // Derive shared secret
        let info = b"X3DH-dchat-v1";
        let sk = Hkdf::derive(None, &dh_concat, info, 32)?;

        let mut shared_secret = [0u8; 32];
        shared_secret.copy_from_slice(&sk);

        Ok(shared_secret)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_x3dh_key_agreement() {
        // Alice's identity
        let alice_identity = X3dhIdentityKey::generate();

        // Bob's identity and pre-keys
        let bob_identity = X3dhIdentityKey::generate();
        let (signed_pre_key, signed_pre_key_secret) = X3dhSignedPreKey::generate(&bob_identity);
        let (one_time_keys, one_time_secrets) = X3dhOneTimePreKey::generate_batch(0, 10);

        // Bob publishes his pre-key bundle
        let bob_bundle = X3dhPreKeyBundle {
            identity_key: *bob_identity.dh_public_key().as_bytes(),
            identity_verifying_key: bob_identity.verifying_key().to_bytes(),
            signed_pre_key,
            one_time_pre_key: Some(one_time_keys[0].clone()),
        };

        // Alice initiates X3DH
        let alice_result = X3dhKeyBundle::initiate(&alice_identity, &bob_bundle).unwrap();
        assert!(alice_result.one_time_key_id.is_some());

        // Bob responds to complete X3DH
        let alice_ephemeral = PublicKey::from(alice_result.ephemeral_public);
        let bob_shared_secret = X3dhKeyBundle::respond(
            &bob_identity,
            &signed_pre_key_secret,
            Some(&one_time_secrets[0]),
            alice_identity.dh_public_key(),
            &alice_ephemeral,
        )
        .unwrap();

        // Both should derive the same shared secret
        assert_eq!(alice_result.shared_secret, bob_shared_secret);
    }

    #[test]
    fn test_x3dh_without_one_time_key() {
        let alice_identity = X3dhIdentityKey::generate();
        let bob_identity = X3dhIdentityKey::generate();
        let (signed_pre_key, signed_pre_key_secret) = X3dhSignedPreKey::generate(&bob_identity);

        let bob_bundle = X3dhPreKeyBundle {
            identity_key: *bob_identity.dh_public_key().as_bytes(),
            identity_verifying_key: bob_identity.verifying_key().to_bytes(),
            signed_pre_key,
            one_time_pre_key: None, // No one-time key
        };

        let alice_result = X3dhKeyBundle::initiate(&alice_identity, &bob_bundle).unwrap();
        assert!(alice_result.one_time_key_id.is_none());

        let alice_ephemeral = PublicKey::from(alice_result.ephemeral_public);
        let bob_shared_secret = X3dhKeyBundle::respond(
            &bob_identity,
            &signed_pre_key_secret,
            None,
            alice_identity.dh_public_key(),
            &alice_ephemeral,
        )
        .unwrap();

        assert_eq!(alice_result.shared_secret, bob_shared_secret);
    }

    #[test]
    fn test_signed_prekey_verification() {
        let identity = X3dhIdentityKey::generate();
        let (signed_pre_key, _) = X3dhSignedPreKey::generate(&identity);

        // Should verify with correct identity
        assert!(signed_pre_key.verify(&identity.verifying_key()).is_ok());

        // Should fail with wrong identity
        let other_identity = X3dhIdentityKey::generate();
        assert!(signed_pre_key
            .verify(&other_identity.verifying_key())
            .is_err());
    }
}
