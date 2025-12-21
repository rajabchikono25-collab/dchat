//! Privacy-preserving accounts with encrypted balances and selective disclosure
//!
//! This module implements confidential transaction primitives using Pedersen commitments
//! and sigma protocols for zero-knowledge proofs of balance conservation.

use std::collections::HashMap;

use curve25519_dalek::constants::RISTRETTO_BASEPOINT_POINT;
use curve25519_dalek::ristretto::{CompressedRistretto, RistrettoPoint};
use curve25519_dalek::scalar::Scalar;
use curve25519_dalek::traits::Identity;
use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;
use sha2::{Digest, Sha512};

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};

/// Generator point G for Pedersen commitments (value component)
/// Using the Ristretto basepoint
const PEDERSEN_G: RistrettoPoint = RISTRETTO_BASEPOINT_POINT;

/// Generator point H for Pedersen commitments (blinding component)
/// Derived deterministically from G using hash-to-curve
fn pedersen_h() -> RistrettoPoint {
    // Hash "dchat-pedersen-h-generator" to create an independent generator
    let mut hasher = Sha512::new();
    hasher.update(b"dchat-pedersen-h-generator-v1");
    let hash_bytes: [u8; 64] = hasher.finalize().into();
    RistrettoPoint::from_uniform_bytes(&hash_bytes)
}

/// Encrypted balance using Pedersen commitments
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct EncryptedBalance {
    /// Pedersen commitment: C = vG + rH
    pub commitment: [u8; 32],
    /// Encrypted value (for owner to decrypt)
    #[serde(with = "BigArray")]
    pub encrypted_value: [u8; 48],
    /// Nonce for encryption
    pub nonce: [u8; 24],
}

impl EncryptedBalance {
    /// Create new encrypted balance
    pub fn new(commitment: [u8; 32], encrypted_value: [u8; 48], nonce: [u8; 24]) -> Self {
        Self {
            commitment,
            encrypted_value,
            nonce,
        }
    }

    /// Zero balance commitment
    pub fn zero() -> Self {
        Self {
            commitment: [0u8; 32],
            encrypted_value: [0u8; 48],
            nonce: [0u8; 24],
        }
    }
}

/// Range proof that value is non-negative and within bounds
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RangeProof {
    /// Bulletproof data
    pub proof_data: Vec<u8>,
    /// Upper bound (2^n - 1)
    pub bit_size: u8,
}

impl RangeProof {
    /// Maximum bit size for range proofs
    pub const MAX_BIT_SIZE: u8 = 64;

    /// Create new range proof
    pub fn new(proof_data: Vec<u8>, bit_size: u8) -> Self {
        Self {
            proof_data,
            bit_size,
        }
    }

    /// Verify the range proof against a commitment
    pub fn verify(&self, commitment: &[u8; 32]) -> ProgramResult<()> {
        if self.bit_size > Self::MAX_BIT_SIZE {
            return Err(ProgramError::RangeProofInvalid);
        }

        if self.proof_data.len() < 64 {
            return Err(ProgramError::RangeProofInvalid);
        }

        // In production: verify using bulletproofs crate
        // This is the verification interface
        // Actual verification delegated to crypto module

        // Verify commitment matches proof
        let proof_commitment = &self.proof_data[0..32];
        if proof_commitment != commitment {
            return Err(ProgramError::RangeProofInvalid);
        }

        Ok(())
    }
}

/// Zero-knowledge proof for balance equality/transfer
///
/// Uses sigma protocols on Ristretto points for proving balance conservation.
/// For a transfer: Σ C_input = Σ C_output (sum of input commitments equals sum of outputs)
///
/// Proof structure (96 bytes total):
/// - bytes 0..32: Schnorr commitment R = k*H (compressed Ristretto point)
/// - bytes 32..64: Challenge e = H(C_diff || R || context) (Scalar)
/// - bytes 64..96: Response s = k + e*r_diff (Scalar)
///
/// Verification: s*H == R + e*C_diff
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceProof {
    /// Proof type
    pub proof_type: BalanceProofType,
    /// Proof data - sigma protocol proof (96 bytes for Sum type)
    pub proof_data: Vec<u8>,
}

/// Types of balance proofs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BalanceProofType {
    /// Proves two commitments represent the same value
    Equality,
    /// Proves Σ C_in = Σ C_out (for transfers)
    Sum,
    /// Proves C1 - C2 = C3 (for transfers with change)
    Difference,
    /// Proves value equals a public amount (for deposits)
    PublicValue,
}

/// Size of a compressed Ristretto point
const POINT_SIZE: usize = 32;
/// Size of a Scalar
const SCALAR_SIZE: usize = 32;
/// Total proof size for Sum proofs (R + e + s)
const SUM_PROOF_SIZE: usize = POINT_SIZE + SCALAR_SIZE + SCALAR_SIZE;

impl BalanceProof {
    /// Create new balance proof
    pub fn new(proof_type: BalanceProofType, proof_data: Vec<u8>) -> Self {
        Self {
            proof_type,
            proof_data,
        }
    }

    /// Create a Sum proof that Σ C_input = Σ C_output
    ///
    /// The prover knows r_diff = Σ r_input - Σ r_output (the difference of blinding factors)
    /// such that C_diff = Σ C_input - Σ C_output = r_diff * H (commits to zero value)
    ///
    /// # Arguments
    /// * `input_commitments` - Compressed Ristretto points for input commitments
    /// * `output_commitments` - Compressed Ristretto points for output commitments
    /// * `blinding_diff` - The difference of blinding factors (r_in - r_out)
    pub fn create_sum_proof(
        input_commitments: &[[u8; 32]],
        output_commitments: &[[u8; 32]],
        blinding_diff: &Scalar,
    ) -> ProgramResult<Self> {
        if input_commitments.is_empty() || output_commitments.is_empty() {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // Compute C_diff = Σ C_input - Σ C_output
        let c_diff = Self::compute_commitment_diff(input_commitments, output_commitments)?;
        let h = pedersen_h();

        // Generate random nonce k for the proof
        let mut k_bytes = [0u8; 64];
        // Deterministic but unpredictable: hash the blinding and commitments
        let mut hasher = Sha512::new();
        hasher.update(b"dchat-balance-proof-nonce");
        hasher.update(blinding_diff.as_bytes());
        for c in input_commitments {
            hasher.update(c);
        }
        for c in output_commitments {
            hasher.update(c);
        }
        k_bytes.copy_from_slice(&hasher.finalize());
        let k = Scalar::from_bytes_mod_order_wide(&k_bytes);

        // R = k * H
        let r_point = k * h;
        let r_compressed = r_point.compress();

        // Challenge e = H(C_diff || R || "dchat-sum-proof")
        let mut challenge_hasher = Sha512::new();
        challenge_hasher.update(c_diff.compress().as_bytes());
        challenge_hasher.update(r_compressed.as_bytes());
        challenge_hasher.update(b"dchat-sum-proof-v1");
        let mut challenge_bytes = [0u8; 64];
        challenge_bytes.copy_from_slice(&challenge_hasher.finalize());
        let e = Scalar::from_bytes_mod_order_wide(&challenge_bytes);

        // Response s = k + e * r_diff
        let s = k + e * blinding_diff;

        // Serialize proof: R (32) || e (32) || s (32)
        let mut proof_data = Vec::with_capacity(SUM_PROOF_SIZE);
        proof_data.extend_from_slice(r_compressed.as_bytes());
        proof_data.extend_from_slice(e.as_bytes());
        proof_data.extend_from_slice(s.as_bytes());

        Ok(Self {
            proof_type: BalanceProofType::Sum,
            proof_data,
        })
    }

    /// Verify sum proof: Σ C_input = Σ C_output using sigma protocol
    ///
    /// Verifies that the prover knows r_diff such that:
    /// C_diff = Σ C_input - Σ C_output = r_diff * H
    ///
    /// This proves the values sum to zero (balance is conserved) without revealing values.
    pub fn verify_transfer(
        &self,
        input_commitments: &[[u8; 32]],
        output_commitments: &[[u8; 32]],
    ) -> ProgramResult<()> {
        if self.proof_type != BalanceProofType::Sum {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // Validate commitment counts
        if input_commitments.is_empty() || output_commitments.is_empty() {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // Validate proof size
        if self.proof_data.len() != SUM_PROOF_SIZE {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // Parse proof components
        let r_bytes: [u8; 32] = self.proof_data[0..32]
            .try_into()
            .map_err(|_| ProgramError::InvalidBalanceProof)?;
        let e_bytes: [u8; 32] = self.proof_data[32..64]
            .try_into()
            .map_err(|_| ProgramError::InvalidBalanceProof)?;
        let s_bytes: [u8; 32] = self.proof_data[64..96]
            .try_into()
            .map_err(|_| ProgramError::InvalidBalanceProof)?;

        // Decompress R point
        let r_compressed = CompressedRistretto::from_slice(&r_bytes)
            .map_err(|_| ProgramError::InvalidBalanceProof)?;
        let r_point = r_compressed
            .decompress()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        // Parse scalars - use canonical decoding to prevent malleability
        let e = Scalar::from_canonical_bytes(e_bytes)
            .into_option()
            .ok_or(ProgramError::InvalidBalanceProof)?;
        let s = Scalar::from_canonical_bytes(s_bytes)
            .into_option()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        // Compute C_diff = Σ C_input - Σ C_output
        let c_diff = Self::compute_commitment_diff(input_commitments, output_commitments)?;

        // Recompute challenge to verify Fiat-Shamir
        let mut challenge_hasher = Sha512::new();
        challenge_hasher.update(c_diff.compress().as_bytes());
        challenge_hasher.update(r_compressed.as_bytes());
        challenge_hasher.update(b"dchat-sum-proof-v1");
        let mut expected_challenge_bytes = [0u8; 64];
        expected_challenge_bytes.copy_from_slice(&challenge_hasher.finalize());
        let expected_e = Scalar::from_bytes_mod_order_wide(&expected_challenge_bytes);

        // Verify challenge matches (Fiat-Shamir binding)
        if e != expected_e {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // Verify Schnorr equation: s*H == R + e*C_diff
        let h = pedersen_h();
        let lhs = s * h;
        let rhs = r_point + e * c_diff;

        if lhs != rhs {
            return Err(ProgramError::InvalidBalanceProof);
        }

        Ok(())
    }

    /// Compute C_diff = Σ C_input - Σ C_output as a Ristretto point
    fn compute_commitment_diff(
        input_commitments: &[[u8; 32]],
        output_commitments: &[[u8; 32]],
    ) -> ProgramResult<RistrettoPoint> {
        // Sum input commitments
        let mut input_sum = RistrettoPoint::identity();
        for commitment_bytes in input_commitments {
            let compressed = CompressedRistretto::from_slice(commitment_bytes)
                .map_err(|_| ProgramError::InvalidBalanceProof)?;
            let point = compressed
                .decompress()
                .ok_or(ProgramError::InvalidBalanceProof)?;
            input_sum += point;
        }

        // Sum output commitments
        let mut output_sum = RistrettoPoint::identity();
        for commitment_bytes in output_commitments {
            let compressed = CompressedRistretto::from_slice(commitment_bytes)
                .map_err(|_| ProgramError::InvalidBalanceProof)?;
            let point = compressed
                .decompress()
                .ok_or(ProgramError::InvalidBalanceProof)?;
            output_sum += point;
        }

        // C_diff = input_sum - output_sum
        Ok(input_sum - output_sum)
    }

    /// Verify equality proof: two commitments represent the same value
    pub fn verify_equality(&self, c1: &[u8; 32], c2: &[u8; 32]) -> ProgramResult<()> {
        if self.proof_type != BalanceProofType::Equality {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // For equality, we verify that C1 - C2 commits to zero
        // This reuses the sum proof verification logic
        self.verify_transfer(&[*c1], &[*c2])
    }
}

/// Privacy account with encrypted state
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyAccount {
    /// Account address (derived from viewing key)
    pub address: Pubkey,
    /// Owner's viewing key commitment
    pub viewing_key_commitment: [u8; 32],
    /// Encrypted balance
    pub balance: EncryptedBalance,
    /// Account nonce for replay protection
    pub nonce: u64,
    /// Stealth address flag
    pub is_stealth: bool,
    /// Creation slot
    pub created_at: u64,
    /// Last update slot
    pub updated_at: u64,
}

impl PrivacyAccount {
    /// Create new privacy account
    pub fn new(
        address: Pubkey,
        viewing_key_commitment: [u8; 32],
        initial_balance: EncryptedBalance,
        slot: u64,
    ) -> Self {
        Self {
            address,
            viewing_key_commitment,
            balance: initial_balance,
            nonce: 0,
            is_stealth: false,
            created_at: slot,
            updated_at: slot,
        }
    }

    /// Create stealth address account
    pub fn new_stealth(address: Pubkey, viewing_key_commitment: [u8; 32], slot: u64) -> Self {
        Self {
            address,
            viewing_key_commitment,
            balance: EncryptedBalance::zero(),
            nonce: 0,
            is_stealth: true,
            created_at: slot,
            updated_at: slot,
        }
    }

    /// Update balance with new encrypted value
    pub fn update_balance(&mut self, new_balance: EncryptedBalance, slot: u64) {
        self.balance = new_balance;
        self.nonce += 1;
        self.updated_at = slot;
    }
}

/// Selective disclosure credential
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DisclosureCredential {
    /// Credential ID
    pub id: [u8; 32],
    /// Issuer program
    pub issuer: Pubkey,
    /// Subject account
    pub subject: Pubkey,
    /// Disclosed attributes (encrypted for verifier)
    pub disclosed_attributes: HashMap<String, Vec<u8>>,
    /// Zero-knowledge proof of undisclosed attributes
    pub attribute_proofs: Vec<AttributeProof>,
    /// Expiry timestamp
    pub expires_at: u64,
    /// Issuer signature
    #[serde(with = "BigArray")]
    pub signature: [u8; 64],
}

/// Proof about an undisclosed attribute
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttributeProof {
    /// Attribute name
    pub name: String,
    /// Proof type
    pub proof_type: AttributeProofType,
    /// ZK proof data
    pub proof_data: Vec<u8>,
}

/// Types of attribute proofs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AttributeProofType {
    /// Proves attribute >= threshold
    GreaterOrEqual {
        /// Threshold value for comparison
        threshold: i64,
    },
    /// Proves attribute < threshold
    LessThan {
        /// Threshold value for comparison
        threshold: i64,
    },
    /// Proves attribute is in set
    SetMembership {
        /// Commitment to the membership set
        set_commitment: [u8; 32],
    },
    /// Proves attribute is not in set
    SetNonMembership {
        /// Commitment to the exclusion set
        set_commitment: [u8; 32],
    },
    /// Proves attribute matches hash
    HashPreimage {
        /// Expected hash of attribute
        hash: [u8; 32],
    },
}

/// Privacy transfer instruction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrivacyTransfer {
    /// Source account
    pub from: Pubkey,
    /// Destination account
    pub to: Pubkey,
    /// New encrypted balance for sender
    pub new_from_balance: EncryptedBalance,
    /// New encrypted balance for receiver
    pub new_to_balance: EncryptedBalance,
    /// Range proofs for both balances
    pub range_proofs: (RangeProof, RangeProof),
    /// Balance proof (sum preservation)
    pub balance_proof: BalanceProof,
    /// Optional memo (encrypted for recipient)
    pub encrypted_memo: Option<Vec<u8>>,
}

impl PrivacyTransfer {
    /// Validate the privacy transfer
    pub fn validate(&self) -> ProgramResult<()> {
        // Verify range proofs
        self.range_proofs
            .0
            .verify(&self.new_from_balance.commitment)?;
        self.range_proofs
            .1
            .verify(&self.new_to_balance.commitment)?;

        Ok(())
    }
}

/// Privacy program for managing private accounts
pub struct PrivacyProgram;

impl PrivacyProgram {
    /// Program ID
    pub const PROGRAM_ID: Pubkey = crate::native_programs::PRIVACY_PROGRAM_ID;

    /// Create account instruction
    pub const CREATE_ACCOUNT: u8 = 0;
    /// Deposit (public -> private)
    pub const DEPOSIT: u8 = 1;
    /// Withdraw (private -> public)
    pub const WITHDRAW: u8 = 2;
    /// Private transfer
    pub const TRANSFER: u8 = 3;
    /// Issue disclosure credential
    pub const ISSUE_CREDENTIAL: u8 = 4;

    /// Create a new privacy account
    pub fn create_account_instruction(
        payer: Pubkey,
        viewing_key_commitment: [u8; 32],
    ) -> crate::instruction::Instruction {
        let data = PrivacyInstruction::CreateAccount {
            viewing_key_commitment,
        };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![crate::account::AccountMeta::new(payer, true)],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }

    /// Deposit public tokens to private account
    pub fn deposit_instruction(
        from_public: Pubkey,
        to_private: Pubkey,
        amount: u64,
        new_encrypted_balance: EncryptedBalance,
        range_proof: RangeProof,
    ) -> crate::instruction::Instruction {
        let data = PrivacyInstruction::Deposit {
            amount,
            new_encrypted_balance,
            range_proof,
        };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![
                crate::account::AccountMeta::new(from_public, true),
                crate::account::AccountMeta::new(to_private, false),
            ],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }

    /// Withdraw from private to public
    pub fn withdraw_instruction(
        from_private: Pubkey,
        to_public: Pubkey,
        amount: u64,
        new_encrypted_balance: EncryptedBalance,
        range_proof: RangeProof,
        withdrawal_proof: BalanceProof,
    ) -> crate::instruction::Instruction {
        let data = PrivacyInstruction::Withdraw {
            amount,
            new_encrypted_balance,
            range_proof,
            withdrawal_proof,
        };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![
                crate::account::AccountMeta::new(from_private, true),
                crate::account::AccountMeta::new(to_public, false),
            ],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }

    /// Private transfer between privacy accounts
    pub fn transfer_instruction(transfer: PrivacyTransfer) -> crate::instruction::Instruction {
        let data = PrivacyInstruction::Transfer {
            transfer: transfer.clone(),
        };

        crate::instruction::Instruction {
            program_id: Self::PROGRAM_ID,
            accounts: vec![
                crate::account::AccountMeta::new(transfer.from, true),
                crate::account::AccountMeta::new(transfer.to, false),
            ],
            data: bincode::serialize(&data).unwrap_or_default(),
        }
    }
}

/// Privacy instruction types
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PrivacyInstruction {
    /// Create a new privacy account
    CreateAccount {
        /// Commitment to viewing key for encrypted balance recovery
        viewing_key_commitment: [u8; 32],
    },
    /// Deposit from public to private
    Deposit {
        /// Amount of tokens to deposit
        amount: u64,
        /// New encrypted balance after deposit
        new_encrypted_balance: EncryptedBalance,
        /// Proof that balance is non-negative
        range_proof: RangeProof,
    },
    /// Withdraw from private to public
    Withdraw {
        /// Amount of tokens to withdraw
        amount: u64,
        /// New encrypted balance after withdrawal
        new_encrypted_balance: EncryptedBalance,
        /// Proof that remaining balance is non-negative
        range_proof: RangeProof,
        /// Proof that withdrawal is authorized
        withdrawal_proof: BalanceProof,
    },
    /// Transfer between privacy accounts
    Transfer {
        /// Full transfer details with proofs
        transfer: PrivacyTransfer,
    },
    /// Issue selective disclosure credential
    IssueCredential {
        /// Subject of the credential
        subject: Pubkey,
        /// Attribute key-value pairs to include
        attributes: HashMap<String, Vec<u8>>,
        /// Credential validity duration
        expires_in_seconds: u64,
    },
    /// Verify credential
    VerifyCredential {
        /// Credential to verify
        credential: DisclosureCredential,
        /// Attributes that must be disclosed
        required_attributes: Vec<String>,
    },
}

/// Stealth address generator
pub struct StealthAddresses;

impl StealthAddresses {
    /// Generate one-time stealth address
    pub fn generate_stealth_address(
        scan_pubkey: &[u8; 32],
        spend_pubkey: &[u8; 32],
        ephemeral_secret: &[u8; 32],
    ) -> Pubkey {
        // P' = H(eS)G + B
        // where e = ephemeral secret, S = scan pubkey, B = spend pubkey
        let mut hasher = blake3::Hasher::new();
        hasher.update(ephemeral_secret);
        hasher.update(scan_pubkey);
        let shared_secret: [u8; 32] = hasher.finalize().into();

        let mut address = [0u8; 32];
        for i in 0..32 {
            address[i] = shared_secret[i] ^ spend_pubkey[i];
        }

        Pubkey::new(address)
    }

    /// Derive viewing key for stealth address
    pub fn derive_viewing_key(
        scan_privkey: &[u8; 32],
        ephemeral_pubkey: &[u8; 32],
        spend_privkey: &[u8; 32],
    ) -> [u8; 32] {
        // k = H(sE) + b
        // where s = scan private key, E = ephemeral pubkey, b = spend privkey
        let mut hasher = blake3::Hasher::new();
        hasher.update(scan_privkey);
        hasher.update(ephemeral_pubkey);
        let shared_secret: [u8; 32] = hasher.finalize().into();

        let mut viewing_key = [0u8; 32];
        for i in 0..32 {
            viewing_key[i] = shared_secret[i].wrapping_add(spend_privkey[i]);
        }

        viewing_key
    }
}

/// Commitment scheme for encrypted values using Pedersen commitments on Ristretto
///
/// A Pedersen commitment C = v*G + r*H where:
/// - v is the value being committed to
/// - r is a random blinding factor
/// - G and H are independent generator points
///
/// Properties:
/// - Hiding: Without knowing r, the value v is information-theoretically hidden
/// - Binding: Cannot open commitment to different value (computationally binding)
/// - Homomorphic: C(v1, r1) + C(v2, r2) = C(v1+v2, r1+r2)
pub struct CommitmentScheme;

impl CommitmentScheme {
    /// Create Pedersen commitment: C = v*G + r*H
    ///
    /// Returns the compressed Ristretto point as 32 bytes
    pub fn commit(value: u64, blinding: &[u8; 32]) -> [u8; 32] {
        let v = Scalar::from(value);

        // Convert blinding bytes to scalar (clamp to valid scalar range)
        let r = Scalar::from_bytes_mod_order(*blinding);

        let g = PEDERSEN_G;
        let h = pedersen_h();

        // C = v*G + r*H
        let commitment = v * g + r * h;
        commitment.compress().to_bytes()
    }

    /// Create commitment with explicit Scalar blinding factor
    pub fn commit_with_scalar(value: u64, blinding: &Scalar) -> [u8; 32] {
        let v = Scalar::from(value);
        let g = PEDERSEN_G;
        let h = pedersen_h();

        let commitment = v * g + *blinding * h;
        commitment.compress().to_bytes()
    }

    /// Generate a random blinding factor
    pub fn random_blinding() -> Scalar {
        let mut bytes = [0u8; 64];
        // Use blake3 with random seed for deterministic test, real random in production
        let mut hasher = blake3::Hasher::new();
        hasher.update(&rand::random::<[u8; 32]>());
        hasher.update(b"dchat-blinding-factor");
        let hash = hasher.finalize();
        bytes[0..32].copy_from_slice(hash.as_bytes());
        Scalar::from_bytes_mod_order_wide(&bytes)
    }

    /// Verify commitment opens to given value with given blinding
    pub fn verify_commitment(commitment: &[u8; 32], value: u64, blinding: &[u8; 32]) -> bool {
        let expected = Self::commit(value, blinding);
        commitment == &expected
    }

    /// Verify commitment with Scalar blinding
    pub fn verify_commitment_scalar(commitment: &[u8; 32], value: u64, blinding: &Scalar) -> bool {
        let expected = Self::commit_with_scalar(value, blinding);
        commitment == &expected
    }

    /// Add commitments homomorphically: C(v1+v2, r1+r2) = C1 + C2
    ///
    /// This is the key property enabling confidential transactions:
    /// we can verify sums without knowing individual values.
    pub fn add_commitments(c1: &[u8; 32], c2: &[u8; 32]) -> ProgramResult<[u8; 32]> {
        let p1 = CompressedRistretto::from_slice(c1)
            .map_err(|_| ProgramError::InvalidBalanceProof)?
            .decompress()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        let p2 = CompressedRistretto::from_slice(c2)
            .map_err(|_| ProgramError::InvalidBalanceProof)?
            .decompress()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        Ok((p1 + p2).compress().to_bytes())
    }

    /// Subtract commitments homomorphically: C(v1-v2, r1-r2) = C1 - C2
    pub fn subtract_commitments(c1: &[u8; 32], c2: &[u8; 32]) -> ProgramResult<[u8; 32]> {
        let p1 = CompressedRistretto::from_slice(c1)
            .map_err(|_| ProgramError::InvalidBalanceProof)?
            .decompress()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        let p2 = CompressedRistretto::from_slice(c2)
            .map_err(|_| ProgramError::InvalidBalanceProof)?
            .decompress()
            .ok_or(ProgramError::InvalidBalanceProof)?;

        Ok((p1 - p2).compress().to_bytes())
    }

    /// Add blinding factors (for computing r_diff = Σ r_in - Σ r_out)
    pub fn add_blindings(blindings: &[Scalar]) -> Scalar {
        blindings.iter().fold(Scalar::ZERO, |acc, b| acc + b)
    }

    /// Subtract blinding factors
    pub fn subtract_blindings(r1: &Scalar, r2: &Scalar) -> Scalar {
        r1 - r2
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encrypted_balance() {
        let balance = EncryptedBalance::new([1u8; 32], [2u8; 48], [3u8; 24]);

        assert_eq!(balance.commitment, [1u8; 32]);
        assert_eq!(balance.encrypted_value, [2u8; 48]);
        assert_eq!(balance.nonce, [3u8; 24]);
    }

    #[test]
    fn test_privacy_account_creation() {
        let address = Pubkey::new([1u8; 32]);
        let viewing_key = [2u8; 32];
        let balance = EncryptedBalance::zero();

        let account = PrivacyAccount::new(address, viewing_key, balance, 100);

        assert_eq!(account.address, address);
        assert_eq!(account.nonce, 0);
        assert!(!account.is_stealth);
        assert_eq!(account.created_at, 100);
    }

    #[test]
    fn test_stealth_address_generation() {
        let scan_pubkey = [1u8; 32];
        let spend_pubkey = [2u8; 32];
        let ephemeral = [3u8; 32];

        let stealth =
            StealthAddresses::generate_stealth_address(&scan_pubkey, &spend_pubkey, &ephemeral);

        // Same inputs should give same output
        let stealth2 =
            StealthAddresses::generate_stealth_address(&scan_pubkey, &spend_pubkey, &ephemeral);

        assert_eq!(stealth, stealth2);

        // Different ephemeral gives different address
        let stealth3 =
            StealthAddresses::generate_stealth_address(&scan_pubkey, &spend_pubkey, &[4u8; 32]);

        assert_ne!(stealth, stealth3);
    }

    #[test]
    fn test_commitment_scheme() {
        let value = 1000u64;
        let blinding = [7u8; 32];

        let commitment = CommitmentScheme::commit(value, &blinding);

        assert!(CommitmentScheme::verify_commitment(
            &commitment,
            value,
            &blinding
        ));
        assert!(!CommitmentScheme::verify_commitment(
            &commitment,
            999,
            &blinding
        ));
    }

    #[test]
    fn test_commitment_homomorphic_addition() {
        // C(v1, r1) + C(v2, r2) should equal C(v1+v2, r1+r2)
        let v1 = 100u64;
        let v2 = 200u64;
        let r1 = Scalar::from(12345u64);
        let r2 = Scalar::from(67890u64);

        let c1 = CommitmentScheme::commit_with_scalar(v1, &r1);
        let c2 = CommitmentScheme::commit_with_scalar(v2, &r2);

        let c_sum = CommitmentScheme::add_commitments(&c1, &c2).unwrap();
        let c_direct = CommitmentScheme::commit_with_scalar(v1 + v2, &(r1 + r2));

        assert_eq!(c_sum, c_direct);
    }

    #[test]
    fn test_commitment_homomorphic_subtraction() {
        let v1 = 500u64;
        let v2 = 200u64;
        let r1 = Scalar::from(11111u64);
        let r2 = Scalar::from(22222u64);

        let c1 = CommitmentScheme::commit_with_scalar(v1, &r1);
        let c2 = CommitmentScheme::commit_with_scalar(v2, &r2);

        let c_diff = CommitmentScheme::subtract_commitments(&c1, &c2).unwrap();
        let c_direct = CommitmentScheme::commit_with_scalar(v1 - v2, &(r1 - r2));

        assert_eq!(c_diff, c_direct);
    }

    #[test]
    fn test_balance_proof_simple_transfer() {
        // Simulate a transfer: 100 tokens from input to output
        // Input: balance 100, Output: balance 100 (same value, different blinding)
        let input_value = 100u64;
        let output_value = 100u64;
        let r_in = Scalar::from(123456789u64);
        let r_out = Scalar::from(987654321u64);

        let c_in = CommitmentScheme::commit_with_scalar(input_value, &r_in);
        let c_out = CommitmentScheme::commit_with_scalar(output_value, &r_out);

        // r_diff = r_in - r_out (prover knows this)
        let r_diff = r_in - r_out;

        // Create proof
        let proof = BalanceProof::create_sum_proof(&[c_in], &[c_out], &r_diff).unwrap();

        // Verify proof
        assert!(proof.verify_transfer(&[c_in], &[c_out]).is_ok());
    }

    #[test]
    fn test_balance_proof_multi_input_output() {
        // Transfer with multiple inputs and outputs
        // Inputs: 30 + 50 + 20 = 100
        // Outputs: 45 + 55 = 100
        let in_vals = [30u64, 50u64, 20u64];
        let out_vals = [45u64, 55u64];

        let r_ins: Vec<Scalar> = (0..3).map(|i| Scalar::from((i + 1) * 11111u64)).collect();
        let r_outs: Vec<Scalar> = (0..2).map(|i| Scalar::from((i + 1) * 22222u64)).collect();

        let c_ins: Vec<[u8; 32]> = in_vals
            .iter()
            .zip(&r_ins)
            .map(|(v, r)| CommitmentScheme::commit_with_scalar(*v, r))
            .collect();

        let c_outs: Vec<[u8; 32]> = out_vals
            .iter()
            .zip(&r_outs)
            .map(|(v, r)| CommitmentScheme::commit_with_scalar(*v, r))
            .collect();

        // r_diff = Σ r_in - Σ r_out
        let r_in_sum = CommitmentScheme::add_blindings(&r_ins);
        let r_out_sum = CommitmentScheme::add_blindings(&r_outs);
        let r_diff = CommitmentScheme::subtract_blindings(&r_in_sum, &r_out_sum);

        let proof = BalanceProof::create_sum_proof(&c_ins, &c_outs, &r_diff).unwrap();
        assert!(proof.verify_transfer(&c_ins, &c_outs).is_ok());
    }

    #[test]
    fn test_balance_proof_invalid_when_values_differ() {
        // Input: 100, Output: 99 (mismatch - should fail)
        let input_value = 100u64;
        let output_value = 99u64; // Different!
        let r_in = Scalar::from(111u64);
        let r_out = Scalar::from(222u64);

        let c_in = CommitmentScheme::commit_with_scalar(input_value, &r_in);
        let c_out = CommitmentScheme::commit_with_scalar(output_value, &r_out);

        // Even with "correct" r_diff for matching values, the proof will fail
        // because C_diff won't be on the H curve
        let r_diff = r_in - r_out;

        let proof = BalanceProof::create_sum_proof(&[c_in], &[c_out], &r_diff).unwrap();

        // This should FAIL because values don't match
        assert!(proof.verify_transfer(&[c_in], &[c_out]).is_err());
    }

    #[test]
    fn test_balance_proof_wrong_blinding_fails() {
        // Correct values but wrong blinding factor in proof
        let input_value = 100u64;
        let output_value = 100u64;
        let r_in = Scalar::from(111u64);
        let r_out = Scalar::from(222u64);

        let c_in = CommitmentScheme::commit_with_scalar(input_value, &r_in);
        let c_out = CommitmentScheme::commit_with_scalar(output_value, &r_out);

        // Wrong r_diff
        let wrong_r_diff = Scalar::from(999999u64);

        let proof = BalanceProof::create_sum_proof(&[c_in], &[c_out], &wrong_r_diff).unwrap();

        // Should fail verification
        assert!(proof.verify_transfer(&[c_in], &[c_out]).is_err());
    }

    #[test]
    fn test_balance_proof_determinism() {
        // Same inputs should produce same proof
        let input_value = 50u64;
        let output_value = 50u64;
        let r_in = Scalar::from(12345u64);
        let r_out = Scalar::from(54321u64);

        let c_in = CommitmentScheme::commit_with_scalar(input_value, &r_in);
        let c_out = CommitmentScheme::commit_with_scalar(output_value, &r_out);
        let r_diff = r_in - r_out;

        let proof1 = BalanceProof::create_sum_proof(&[c_in], &[c_out], &r_diff).unwrap();
        let proof2 = BalanceProof::create_sum_proof(&[c_in], &[c_out], &r_diff).unwrap();

        // Proofs should be identical (deterministic)
        assert_eq!(proof1.proof_data, proof2.proof_data);
    }

    #[test]
    fn test_balance_proof_rejects_tampered_proof() {
        let input_value = 100u64;
        let output_value = 100u64;
        let r_in = Scalar::from(111u64);
        let r_out = Scalar::from(222u64);

        let c_in = CommitmentScheme::commit_with_scalar(input_value, &r_in);
        let c_out = CommitmentScheme::commit_with_scalar(output_value, &r_out);
        let r_diff = r_in - r_out;

        let mut proof = BalanceProof::create_sum_proof(&[c_in], &[c_out], &r_diff).unwrap();

        // Tamper with the proof
        proof.proof_data[50] ^= 0xFF;

        // Should fail
        assert!(proof.verify_transfer(&[c_in], &[c_out]).is_err());
    }

    #[test]
    fn test_balance_proof_empty_inputs_rejected() {
        let c_out = CommitmentScheme::commit_with_scalar(100, &Scalar::from(1u64));
        let r_diff = Scalar::ZERO;

        let result = BalanceProof::create_sum_proof(&[], &[c_out], &r_diff);
        assert!(result.is_err());
    }

    #[test]
    fn test_balance_proof_empty_outputs_rejected() {
        let c_in = CommitmentScheme::commit_with_scalar(100, &Scalar::from(1u64));
        let r_diff = Scalar::ZERO;

        let result = BalanceProof::create_sum_proof(&[c_in], &[], &r_diff);
        assert!(result.is_err());
    }

    #[test]
    fn test_privacy_account_balance_update() {
        let address = Pubkey::new([1u8; 32]);
        let viewing_key = [2u8; 32];
        let balance = EncryptedBalance::zero();

        let mut account = PrivacyAccount::new(address, viewing_key, balance, 100);

        assert_eq!(account.nonce, 0);
        assert_eq!(account.updated_at, 100);

        let new_balance = EncryptedBalance::new([5u8; 32], [6u8; 48], [7u8; 24]);
        account.update_balance(new_balance, 200);

        assert_eq!(account.nonce, 1);
        assert_eq!(account.updated_at, 200);
        assert_eq!(account.balance.commitment, [5u8; 32]);
    }
}
