//! Solana Transaction Building
//!
//! Transaction construction, serialization, and signing for Solana.

use dchat_core::error::{Error, Result};
use ed25519_dalek::{Signer, SigningKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

use crate::wallet::solana_compat::SolanaAddress;

/// Maximum transaction size in bytes
pub const MAX_TRANSACTION_SIZE: usize = 1232;

/// Transaction version
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TransactionVersion {
    Legacy,
    V0,
}

/// A Solana transaction
#[derive(Debug, Clone)]
pub struct SolanaTransaction {
    /// Transaction message
    pub message: TransactionMessage,
    /// Signatures (one per required signer)
    pub signatures: Vec<[u8; 64]>,
    /// Transaction version
    pub version: TransactionVersion,
}

impl SolanaTransaction {
    /// Create a new unsigned transaction
    pub fn new(message: TransactionMessage) -> Self {
        let num_signers = message.header.num_required_signatures as usize;
        Self {
            message,
            signatures: vec![[0u8; 64]; num_signers],
            version: TransactionVersion::Legacy,
        }
    }

    /// Create a V0 transaction
    pub fn new_v0(message: TransactionMessage) -> Self {
        let num_signers = message.header.num_required_signatures as usize;
        Self {
            message,
            signatures: vec![[0u8; 64]; num_signers],
            version: TransactionVersion::V0,
        }
    }

    /// Sign the transaction with a keypair
    pub fn sign(&mut self, keypair: &SigningKey, signer_index: usize) -> Result<()> {
        if signer_index >= self.signatures.len() {
            return Err(Error::validation("Invalid signer index"));
        }

        let message_bytes = self.message.serialize()?;
        let signature = keypair.sign(&message_bytes);
        self.signatures[signer_index] = signature.to_bytes();

        Ok(())
    }

    /// Sign with multiple keypairs
    pub fn sign_all(&mut self, keypairs: &[&SigningKey]) -> Result<()> {
        for (i, keypair) in keypairs.iter().enumerate() {
            self.sign(keypair, i)?;
        }
        Ok(())
    }

    /// Check if transaction is fully signed
    pub fn is_signed(&self) -> bool {
        self.signatures.iter().all(|s| s != &[0u8; 64])
    }

    /// Get the transaction signature (first signature)
    pub fn signature(&self) -> Option<&[u8; 64]> {
        self.signatures.first()
    }

    /// Get the transaction signature as base58
    pub fn signature_base58(&self) -> Option<String> {
        self.signature().map(|s| bs58::encode(s).into_string())
    }

    /// Serialize the transaction for sending
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        // Compact array of signatures
        data.push(self.signatures.len() as u8);
        for sig in &self.signatures {
            data.extend_from_slice(sig);
        }

        // Message
        let message_bytes = self.message.serialize()?;
        data.extend_from_slice(&message_bytes);

        if data.len() > MAX_TRANSACTION_SIZE {
            return Err(Error::validation(format!(
                "Transaction too large: {} bytes (max {})",
                data.len(),
                MAX_TRANSACTION_SIZE
            )));
        }

        Ok(data)
    }

    /// Serialize to base64
    pub fn to_base64(&self) -> Result<String> {
        use base64::{engine::general_purpose::STANDARD, Engine};
        let bytes = self.serialize()?;
        Ok(STANDARD.encode(bytes))
    }

    /// Get transaction fee estimate (based on number of signatures)
    pub fn fee_estimate(&self, lamports_per_signature: u64) -> u64 {
        self.signatures.len() as u64 * lamports_per_signature
    }
}

/// Transaction message header
#[derive(Debug, Clone, Copy, Default)]
pub struct MessageHeader {
    /// Number of required signatures
    pub num_required_signatures: u8,
    /// Number of read-only signed accounts
    pub num_readonly_signed_accounts: u8,
    /// Number of read-only unsigned accounts
    pub num_readonly_unsigned_accounts: u8,
}

impl MessageHeader {
    pub fn serialize(&self) -> [u8; 3] {
        [
            self.num_required_signatures,
            self.num_readonly_signed_accounts,
            self.num_readonly_unsigned_accounts,
        ]
    }
}

/// Transaction message
#[derive(Debug, Clone)]
pub struct TransactionMessage {
    /// Message header
    pub header: MessageHeader,
    /// Account keys
    pub account_keys: Vec<SolanaAddress>,
    /// Recent blockhash
    pub recent_blockhash: [u8; 32],
    /// Instructions
    pub instructions: Vec<CompiledInstruction>,
}

impl TransactionMessage {
    /// Create a new message
    pub fn new(
        instructions: Vec<Instruction>,
        payer: SolanaAddress,
        recent_blockhash: [u8; 32],
    ) -> Result<Self> {
        // Collect all unique accounts
        let mut accounts: Vec<AccountMeta> = Vec::new();
        let mut seen: HashMap<String, usize> = HashMap::new();

        // Payer is always first and writable signer
        accounts.push(AccountMeta {
            pubkey: payer.clone(),
            is_signer: true,
            is_writable: true,
        });
        seen.insert(payer.to_base58(), 0);

        // Add accounts from all instructions
        for instruction in &instructions {
            for account in &instruction.accounts {
                let key = account.pubkey.to_base58();
                if let Some(&idx) = seen.get(&key) {
                    // Upgrade permissions if needed
                    if account.is_signer {
                        accounts[idx].is_signer = true;
                    }
                    if account.is_writable {
                        accounts[idx].is_writable = true;
                    }
                } else {
                    seen.insert(key, accounts.len());
                    accounts.push(account.clone());
                }
            }

            // Add program ID
            let program_key = instruction.program_id.to_base58();
            if !seen.contains_key(&program_key) {
                seen.insert(program_key, accounts.len());
                accounts.push(AccountMeta {
                    pubkey: instruction.program_id.clone(),
                    is_signer: false,
                    is_writable: false,
                });
            }
        }

        // Sort accounts: signers first, then non-signers
        // Within each group: writable first, then readonly
        accounts.sort_by(|a, b| {
            match (a.is_signer, b.is_signer) {
                (true, false) => std::cmp::Ordering::Less,
                (false, true) => std::cmp::Ordering::Greater,
                _ => match (a.is_writable, b.is_writable) {
                    (true, false) => std::cmp::Ordering::Less,
                    (false, true) => std::cmp::Ordering::Greater,
                    _ => std::cmp::Ordering::Equal,
                }
            }
        });

        // Rebuild the index map after sorting
        seen.clear();
        for (i, account) in accounts.iter().enumerate() {
            seen.insert(account.pubkey.to_base58(), i);
        }

        // Calculate header
        let num_required_signatures = accounts.iter().filter(|a| a.is_signer).count() as u8;
        let num_readonly_signed = accounts.iter()
            .filter(|a| a.is_signer && !a.is_writable)
            .count() as u8;
        let num_readonly_unsigned = accounts.iter()
            .filter(|a| !a.is_signer && !a.is_writable)
            .count() as u8;

        let header = MessageHeader {
            num_required_signatures,
            num_readonly_signed_accounts: num_readonly_signed,
            num_readonly_unsigned_accounts: num_readonly_unsigned,
        };

        // Compile instructions
        let compiled_instructions: Vec<CompiledInstruction> = instructions
            .iter()
            .map(|ix| {
                let program_id_index = *seen.get(&ix.program_id.to_base58())
                    .expect("Program ID should be in accounts") as u8;
                
                let account_indices: Vec<u8> = ix.accounts
                    .iter()
                    .map(|a| *seen.get(&a.pubkey.to_base58())
                        .expect("Account should be in accounts") as u8)
                    .collect();

                CompiledInstruction {
                    program_id_index,
                    accounts: account_indices,
                    data: ix.data.clone(),
                }
            })
            .collect();

        let account_keys: Vec<SolanaAddress> = accounts
            .into_iter()
            .map(|a| a.pubkey)
            .collect();

        Ok(Self {
            header,
            account_keys,
            recent_blockhash,
            instructions: compiled_instructions,
        })
    }

    /// Serialize the message
    pub fn serialize(&self) -> Result<Vec<u8>> {
        let mut data = Vec::new();

        // Header
        data.extend_from_slice(&self.header.serialize());

        // Account keys (compact array)
        data.push(self.account_keys.len() as u8);
        for key in &self.account_keys {
            data.extend_from_slice(key.as_bytes());
        }

        // Recent blockhash
        data.extend_from_slice(&self.recent_blockhash);

        // Instructions (compact array)
        data.push(self.instructions.len() as u8);
        for ix in &self.instructions {
            data.push(ix.program_id_index);
            
            // Account indices (compact array)
            data.push(ix.accounts.len() as u8);
            data.extend_from_slice(&ix.accounts);
            
            // Data (compact array)
            encode_compact_u16(&mut data, ix.data.len() as u16);
            data.extend_from_slice(&ix.data);
        }

        Ok(data)
    }
}

/// Encode a u16 as a compact array length
fn encode_compact_u16(buf: &mut Vec<u8>, val: u16) {
    if val < 128 {
        buf.push(val as u8);
    } else if val < 16384 {
        buf.push(((val & 0x7F) | 0x80) as u8);
        buf.push((val >> 7) as u8);
    } else {
        buf.push(((val & 0x7F) | 0x80) as u8);
        buf.push((((val >> 7) & 0x7F) | 0x80) as u8);
        buf.push((val >> 14) as u8);
    }
}

/// Account metadata for instruction
#[derive(Debug, Clone)]
pub struct AccountMeta {
    /// Account public key
    pub pubkey: SolanaAddress,
    /// Is this a signer?
    pub is_signer: bool,
    /// Is this writable?
    pub is_writable: bool,
}

impl AccountMeta {
    /// Create a writable signer
    pub fn signer_writable(pubkey: SolanaAddress) -> Self {
        Self {
            pubkey,
            is_signer: true,
            is_writable: true,
        }
    }

    /// Create a read-only signer
    pub fn signer_readonly(pubkey: SolanaAddress) -> Self {
        Self {
            pubkey,
            is_signer: true,
            is_writable: false,
        }
    }

    /// Create a writable non-signer
    pub fn writable(pubkey: SolanaAddress) -> Self {
        Self {
            pubkey,
            is_signer: false,
            is_writable: true,
        }
    }

    /// Create a read-only non-signer
    pub fn readonly(pubkey: SolanaAddress) -> Self {
        Self {
            pubkey,
            is_signer: false,
            is_writable: false,
        }
    }
}

/// High-level instruction (before compilation)
#[derive(Debug, Clone)]
pub struct Instruction {
    /// Program ID to invoke
    pub program_id: SolanaAddress,
    /// Accounts required by the instruction
    pub accounts: Vec<AccountMeta>,
    /// Instruction data
    pub data: Vec<u8>,
}

impl Instruction {
    /// Create a new instruction
    pub fn new(program_id: SolanaAddress, accounts: Vec<AccountMeta>, data: Vec<u8>) -> Self {
        Self {
            program_id,
            accounts,
            data,
        }
    }
}

/// Compiled instruction (with account indices)
#[derive(Debug, Clone)]
pub struct CompiledInstruction {
    /// Index of the program ID in account keys
    pub program_id_index: u8,
    /// Indices of accounts in the account keys array
    pub accounts: Vec<u8>,
    /// Instruction data
    pub data: Vec<u8>,
}

/// Transaction builder for easy construction
pub struct TransactionBuilder {
    instructions: Vec<Instruction>,
    payer: Option<SolanaAddress>,
    signers: Vec<SigningKey>,
}

impl TransactionBuilder {
    /// Create a new transaction builder
    pub fn new() -> Self {
        Self {
            instructions: Vec::new(),
            payer: None,
            signers: Vec::new(),
        }
    }

    /// Set the fee payer
    pub fn payer(mut self, payer: SolanaAddress) -> Self {
        self.payer = Some(payer);
        self
    }

    /// Add an instruction
    pub fn instruction(mut self, instruction: Instruction) -> Self {
        self.instructions.push(instruction);
        self
    }

    /// Add multiple instructions
    pub fn instructions(mut self, instructions: Vec<Instruction>) -> Self {
        self.instructions.extend(instructions);
        self
    }

    /// Add a signer
    pub fn signer(mut self, signer: SigningKey) -> Self {
        self.signers.push(signer);
        self
    }

    /// Build the transaction
    pub fn build(self, recent_blockhash: [u8; 32]) -> Result<SolanaTransaction> {
        let payer = self.payer.ok_or_else(|| Error::validation("Payer not set"))?;

        if self.instructions.is_empty() {
            return Err(Error::validation("No instructions provided"));
        }

        let message = TransactionMessage::new(self.instructions, payer, recent_blockhash)?;
        let mut transaction = SolanaTransaction::new(message);

        // Sign with all provided signers
        let signers: Vec<&SigningKey> = self.signers.iter().collect();
        if !signers.is_empty() {
            transaction.sign_all(&signers)?;
        }

        Ok(transaction)
    }
}

impl Default for TransactionBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Transaction status
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum TransactionStatus {
    /// Transaction is being processed
    Processing,
    /// Transaction confirmed
    Confirmed,
    /// Transaction finalized
    Finalized,
    /// Transaction failed
    Failed,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let header = MessageHeader {
            num_required_signatures: 2,
            num_readonly_signed_accounts: 0,
            num_readonly_unsigned_accounts: 1,
        };
        assert_eq!(header.serialize(), [2, 0, 1]);
    }

    #[test]
    fn test_compact_u16_encoding() {
        let mut buf = Vec::new();
        encode_compact_u16(&mut buf, 0);
        assert_eq!(buf, vec![0]);

        buf.clear();
        encode_compact_u16(&mut buf, 127);
        assert_eq!(buf, vec![127]);

        buf.clear();
        encode_compact_u16(&mut buf, 128);
        assert_eq!(buf, vec![0x80, 0x01]);
    }

    #[test]
    fn test_account_meta_constructors() {
        let addr = SolanaAddress::from_bytes(&[1u8; 32]).unwrap();
        
        let signer = AccountMeta::signer_writable(addr.clone());
        assert!(signer.is_signer);
        assert!(signer.is_writable);

        let readonly = AccountMeta::readonly(addr);
        assert!(!readonly.is_signer);
        assert!(!readonly.is_writable);
    }
}
