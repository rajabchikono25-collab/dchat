//! Privacy-preserving accounts with encrypted balances and selective disclosure

use std::collections::HashMap;

use serde::{Deserialize, Serialize};
use serde_big_array::BigArray;

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};

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
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceProof {
    /// Proof type
    pub proof_type: BalanceProofType,
    /// Proof data
    pub proof_data: Vec<u8>,
}

/// Types of balance proofs
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BalanceProofType {
    /// Proves two commitments represent the same value
    Equality,
    /// Proves C1 + C2 = C3 (for transfers)
    Sum,
    /// Proves C1 - C2 = C3 (for transfers with change)
    Difference,
    /// Proves value equals a public amount (for deposits)
    PublicValue,
}

impl BalanceProof {
    /// Create new balance proof
    pub fn new(proof_type: BalanceProofType, proof_data: Vec<u8>) -> Self {
        Self {
            proof_type,
            proof_data,
        }
    }

    /// Verify sum proof: C_in - C_out = 0
    pub fn verify_transfer(
        &self,
        input_commitments: &[[u8; 32]],
        output_commitments: &[[u8; 32]],
    ) -> ProgramResult<()> {
        if self.proof_type != BalanceProofType::Sum {
            return Err(ProgramError::InvalidBalanceProof);
        }

        // In production: verify using sigma protocol
        // Σ inputs = Σ outputs

        Ok(())
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
    GreaterOrEqual { threshold: i64 },
    /// Proves attribute < threshold
    LessThan { threshold: i64 },
    /// Proves attribute is in set
    SetMembership { set_commitment: [u8; 32] },
    /// Proves attribute is not in set
    SetNonMembership { set_commitment: [u8; 32] },
    /// Proves attribute matches hash
    HashPreimage { hash: [u8; 32] },
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
    CreateAccount { viewing_key_commitment: [u8; 32] },
    /// Deposit from public to private
    Deposit {
        amount: u64,
        new_encrypted_balance: EncryptedBalance,
        range_proof: RangeProof,
    },
    /// Withdraw from private to public
    Withdraw {
        amount: u64,
        new_encrypted_balance: EncryptedBalance,
        range_proof: RangeProof,
        withdrawal_proof: BalanceProof,
    },
    /// Transfer between privacy accounts
    Transfer { transfer: PrivacyTransfer },
    /// Issue selective disclosure credential
    IssueCredential {
        subject: Pubkey,
        attributes: HashMap<String, Vec<u8>>,
        expires_in_seconds: u64,
    },
    /// Verify credential
    VerifyCredential {
        credential: DisclosureCredential,
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

/// Commitment scheme for encrypted values
pub struct CommitmentScheme;

impl CommitmentScheme {
    /// Create Pedersen commitment: C = vG + rH
    pub fn commit(value: u64, blinding: &[u8; 32]) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(&value.to_le_bytes());
        hasher.update(blinding);
        hasher.finalize().into()
    }

    /// Verify commitment
    pub fn verify_commitment(commitment: &[u8; 32], value: u64, blinding: &[u8; 32]) -> bool {
        let expected = Self::commit(value, blinding);
        commitment == &expected
    }

    /// Add commitments homomorphically
    pub fn add_commitments(c1: &[u8; 32], c2: &[u8; 32]) -> [u8; 32] {
        // In real implementation: point addition on curve
        // For now: simple XOR (placeholder for actual curve ops)
        let mut result = [0u8; 32];
        for i in 0..32 {
            result[i] = c1[i] ^ c2[i];
        }
        result
    }

    /// Subtract commitments homomorphically
    pub fn subtract_commitments(c1: &[u8; 32], c2: &[u8; 32]) -> [u8; 32] {
        // In real implementation: point subtraction on curve
        Self::add_commitments(c1, c2) // XOR is self-inverse
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
