//! Instruction types for the program runtime

use serde::{Deserialize, Serialize};

use crate::account::{AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::{MAX_ACCOUNTS_PER_INSTRUCTION, MAX_INSTRUCTION_DATA_SIZE};

/// Raw instruction data bytes
pub type InstructionData = Vec<u8>;

/// Instruction account reference with metadata
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct InstructionAccount {
    /// Index into the account keys array
    pub index: u16,
    /// Is this account a signer?
    pub is_signer: bool,
    /// Is this account writable?
    pub is_writable: bool,
}

impl InstructionAccount {
    /// Create new instruction account reference
    pub fn new(index: u16, is_signer: bool, is_writable: bool) -> Self {
        Self {
            index,
            is_signer,
            is_writable,
        }
    }
}

/// High-level instruction (before compilation)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Instruction {
    /// Program to invoke
    pub program_id: Pubkey,
    /// Accounts to pass to the program
    pub accounts: Vec<AccountMeta>,
    /// Instruction data
    pub data: InstructionData,
}

impl Instruction {
    /// Create a new instruction
    pub fn new(program_id: Pubkey, accounts: Vec<AccountMeta>, data: InstructionData) -> Self {
        Self {
            program_id,
            accounts,
            data,
        }
    }

    /// Create instruction with program ID and data only (no accounts)
    pub fn new_with_data(program_id: Pubkey, data: InstructionData) -> Self {
        Self {
            program_id,
            accounts: Vec::new(),
            data,
        }
    }

    /// Validate instruction constraints
    pub fn validate(&self) -> ProgramResult<()> {
        if self.accounts.len() > MAX_ACCOUNTS_PER_INSTRUCTION {
            return Err(ProgramError::MaxAccountsExceeded);
        }

        if self.data.len() > MAX_INSTRUCTION_DATA_SIZE {
            return Err(ProgramError::MaxInstructionDataExceeded);
        }

        Ok(())
    }

    /// Get the hash of this instruction (for receipts and intents)
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();
        hasher.update(self.program_id.as_bytes());

        for account in &self.accounts {
            hasher.update(account.pubkey.as_bytes());
            hasher.update(&[account.is_signer as u8, account.is_writable as u8]);
        }

        hasher.update(&self.data);
        *hasher.finalize().as_bytes()
    }
}

/// Compiled instruction (after deduplication of account keys)
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CompiledInstruction {
    /// Index of the program in the account keys array
    pub program_id_index: u8,
    /// Indices of accounts in the account keys array
    pub accounts: Vec<u8>,
    /// Instruction data
    pub data: InstructionData,
}

impl CompiledInstruction {
    /// Create a new compiled instruction
    pub fn new(program_id_index: u8, accounts: Vec<u8>, data: InstructionData) -> Self {
        Self {
            program_id_index,
            accounts,
            data,
        }
    }

    /// Deserialize instruction data to a specific type
    pub fn deserialize_data<T: for<'de> Deserialize<'de>>(&self) -> ProgramResult<T> {
        bincode::deserialize(&self.data).map_err(|_| ProgramError::InvalidInstructionData)
    }
}

/// Instruction builder for ergonomic construction
pub struct InstructionBuilder {
    program_id: Pubkey,
    accounts: Vec<AccountMeta>,
    data: Vec<u8>,
}

impl InstructionBuilder {
    /// Start building an instruction
    pub fn new(program_id: Pubkey) -> Self {
        Self {
            program_id,
            accounts: Vec::new(),
            data: Vec::new(),
        }
    }

    /// Add a signer-writable account
    pub fn signer_writable(mut self, pubkey: Pubkey) -> Self {
        self.accounts.push(AccountMeta::signer_writable(pubkey));
        self
    }

    /// Add a signer-readonly account
    pub fn signer_readonly(mut self, pubkey: Pubkey) -> Self {
        self.accounts.push(AccountMeta::signer_readonly(pubkey));
        self
    }

    /// Add a writable account (not signer)
    pub fn writable(mut self, pubkey: Pubkey) -> Self {
        self.accounts.push(AccountMeta::writable(pubkey));
        self
    }

    /// Add a readonly account
    pub fn readonly(mut self, pubkey: Pubkey) -> Self {
        self.accounts.push(AccountMeta::readonly(pubkey));
        self
    }

    /// Add account with custom flags
    pub fn account(mut self, account: AccountMeta) -> Self {
        self.accounts.push(account);
        self
    }

    /// Set raw data
    pub fn data(mut self, data: Vec<u8>) -> Self {
        self.data = data;
        self
    }

    /// Set data from serializable type
    pub fn data_from<T: Serialize>(mut self, value: &T) -> ProgramResult<Self> {
        self.data = bincode::serialize(value).map_err(|_| ProgramError::InvalidInstructionData)?;
        Ok(self)
    }

    /// Build the instruction
    pub fn build(self) -> Instruction {
        Instruction {
            program_id: self.program_id,
            accounts: self.accounts,
            data: self.data,
        }
    }
}

/// Batch of instructions forming a transaction
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct InstructionBatch {
    /// All unique account keys referenced
    pub account_keys: Vec<Pubkey>,
    /// Account key indices that are signers
    pub signer_indices: Vec<u8>,
    /// Account key indices that are writable
    pub writable_indices: Vec<u8>,
    /// Compiled instructions
    pub instructions: Vec<CompiledInstruction>,
    /// Recent blockhash for replay protection
    pub recent_blockhash: [u8; 32],
    /// Intent ID for cross-chain operations (optional)
    pub intent_id: Option<[u8; 32]>,
}

impl InstructionBatch {
    /// Compile instructions into a batch
    pub fn compile(
        instructions: Vec<Instruction>,
        payer: Pubkey,
        recent_blockhash: [u8; 32],
    ) -> ProgramResult<Self> {
        let mut account_keys = Vec::new();
        let mut signer_indices = Vec::new();
        let mut writable_indices = Vec::new();

        // Add payer first (always signer and writable)
        account_keys.push(payer);
        signer_indices.push(0);
        writable_indices.push(0);

        // Collect all unique accounts
        for ix in &instructions {
            // Add program ID
            if !account_keys.contains(&ix.program_id) {
                account_keys.push(ix.program_id);
            }

            // Add accounts
            for meta in &ix.accounts {
                let idx = if let Some(pos) = account_keys.iter().position(|k| *k == meta.pubkey) {
                    pos as u8
                } else {
                    let pos = account_keys.len() as u8;
                    account_keys.push(meta.pubkey);
                    pos
                };

                if meta.is_signer && !signer_indices.contains(&idx) {
                    signer_indices.push(idx);
                }
                if meta.is_writable && !writable_indices.contains(&idx) {
                    writable_indices.push(idx);
                }
            }
        }

        // Compile instructions
        let mut compiled = Vec::with_capacity(instructions.len());
        for ix in instructions {
            ix.validate()?;

            let program_id_index = account_keys
                .iter()
                .position(|k| *k == ix.program_id)
                .ok_or(ProgramError::InternalError(
                    "program_id not found".to_string(),
                ))? as u8;

            let accounts: Vec<u8> = ix
                .accounts
                .iter()
                .map(|meta| {
                    account_keys
                        .iter()
                        .position(|k| *k == meta.pubkey)
                        .map(|p| p as u8)
                        .ok_or(ProgramError::InternalError("account not found".to_string()))
                })
                .collect::<ProgramResult<_>>()?;

            compiled.push(CompiledInstruction {
                program_id_index,
                accounts,
                data: ix.data,
            });
        }

        Ok(Self {
            account_keys,
            signer_indices,
            writable_indices,
            instructions: compiled,
            recent_blockhash,
            intent_id: None,
        })
    }

    /// Set intent ID for cross-chain operations
    pub fn with_intent(mut self, intent_id: [u8; 32]) -> Self {
        self.intent_id = Some(intent_id);
        self
    }

    /// Get total number of accounts
    pub fn account_count(&self) -> usize {
        self.account_keys.len()
    }

    /// Get hash of this batch
    pub fn hash(&self) -> [u8; 32] {
        let mut hasher = blake3::Hasher::new();

        for key in &self.account_keys {
            hasher.update(key.as_bytes());
        }

        for ix in &self.instructions {
            hasher.update(&[ix.program_id_index]);
            hasher.update(&ix.accounts);
            hasher.update(&ix.data);
        }

        hasher.update(&self.recent_blockhash);

        if let Some(intent_id) = &self.intent_id {
            hasher.update(intent_id);
        }

        *hasher.finalize().as_bytes()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_instruction_creation() {
        let program_id = Pubkey::new([1u8; 32]);
        let account = Pubkey::new([2u8; 32]);

        let ix = InstructionBuilder::new(program_id)
            .signer_writable(account)
            .data(vec![1, 2, 3])
            .build();

        assert_eq!(ix.program_id, program_id);
        assert_eq!(ix.accounts.len(), 1);
        assert_eq!(ix.data, vec![1, 2, 3]);
    }

    #[test]
    fn test_instruction_validation() {
        let program_id = Pubkey::new([1u8; 32]);

        // Valid instruction
        let ix = Instruction::new(program_id, vec![], vec![1, 2, 3]);
        assert!(ix.validate().is_ok());

        // Too many accounts
        let many_accounts: Vec<AccountMeta> = (0..100)
            .map(|i| AccountMeta::readonly(Pubkey::new([i as u8; 32])))
            .collect();
        let ix_many = Instruction::new(program_id, many_accounts, vec![]);
        assert!(matches!(
            ix_many.validate(),
            Err(ProgramError::MaxAccountsExceeded)
        ));
    }

    #[test]
    fn test_instruction_batch_compile() {
        let program_id = Pubkey::new([1u8; 32]);
        let payer = Pubkey::new([0u8; 32]);
        let account1 = Pubkey::new([2u8; 32]);
        let account2 = Pubkey::new([3u8; 32]);

        let ix1 = InstructionBuilder::new(program_id)
            .writable(account1)
            .data(vec![1])
            .build();

        let ix2 = InstructionBuilder::new(program_id)
            .readonly(account2)
            .data(vec![2])
            .build();

        let batch = InstructionBatch::compile(vec![ix1, ix2], payer, [0u8; 32]).unwrap();

        assert_eq!(batch.instructions.len(), 2);
        // Payer + program + 2 accounts = 4 unique keys
        assert_eq!(batch.account_keys.len(), 4);
    }

    #[test]
    fn test_instruction_hash_determinism() {
        let program_id = Pubkey::new([1u8; 32]);
        let account = Pubkey::new([2u8; 32]);

        let ix1 = InstructionBuilder::new(program_id)
            .signer_writable(account)
            .data(vec![1, 2, 3])
            .build();

        let ix2 = InstructionBuilder::new(program_id)
            .signer_writable(account)
            .data(vec![1, 2, 3])
            .build();

        assert_eq!(ix1.hash(), ix2.hash());
    }
}
