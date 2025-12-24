//! End-to-End Dispatcher Tests
//!
//! These tests wire up serialized AccountsBlob and instruction_data to drive
//! the dispatcher in unit tests, simulating the full execution path from
//! serialized inputs to deserialized outputs.

use dchat_programs::abi::{AccountsBlob, IxEnvelope, SerializableAccount};
use dchat_programs::account::{Pubkey, MOTES_PER_DCHAT};
use dchat_programs::error::ProgramError;

// Simple account structure for testing
#[derive(Debug, Clone)]
struct TestAccount {
    pub pubkey: Pubkey,
    pub owner: Pubkey,
    pub motes: u64,
    pub data: Vec<u8>,
    pub is_signer: bool,
    pub is_writable: bool,
}

impl TestAccount {
    fn to_serializable(&self) -> SerializableAccount {
        SerializableAccount {
            pubkey: self.pubkey,
            owner: self.owner,
            motes: self.motes,
            data: self.data.clone(),
            is_signer: self.is_signer,
            is_writable: self.is_writable,
            executable: false,
            rent_epoch: 0,
        }
    }
}

// Mock instruction tags
#[repr(u16)]
enum MockInstruction {
    Initialize = 0,
    Transfer = 1,
    Increment = 2,
}

fn execute_mock_instruction(
    tag: u16,
    payload: &[u8],
    accounts: &mut [TestAccount],
) -> Result<Vec<u8>, ProgramError> {
    match tag {
        0 => execute_initialize(payload, accounts),
        1 => execute_transfer(payload, accounts),
        2 => execute_increment(payload, accounts),
        _ => Err(ProgramError::InvalidInstructionData),
    }
}

fn execute_initialize(
    payload: &[u8],
    accounts: &mut [TestAccount],
) -> Result<Vec<u8>, ProgramError> {
    if payload.len() != 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let initial_value = u64::from_le_bytes(payload[0..8].try_into().unwrap());
    if accounts.len() < 2 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[1].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !accounts[0].is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    let authority_pubkey = accounts[1].pubkey;
    let mut data = vec![0u8; 40];
    data[0..8].copy_from_slice(&initial_value.to_le_bytes());
    data[8..40].copy_from_slice(authority_pubkey.as_bytes());
    accounts[0].data = data;
    Ok(vec![])
}

fn execute_transfer(payload: &[u8], accounts: &mut [TestAccount]) -> Result<Vec<u8>, ProgramError> {
    if payload.len() != 8 {
        return Err(ProgramError::InvalidInstructionData);
    }
    let amount = u64::from_le_bytes(payload[0..8].try_into().unwrap());
    if accounts.len() < 3 {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    if !accounts[2].is_signer {
        return Err(ProgramError::MissingRequiredSignature);
    }
    if !accounts[0].is_writable || !accounts[1].is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if accounts[0].motes < amount {
        return Err(ProgramError::InsufficientFunds);
    }
    accounts[0].motes = accounts[0]
        .motes
        .checked_sub(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    accounts[1].motes = accounts[1]
        .motes
        .checked_add(amount)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    Ok(vec![])
}

fn execute_increment(
    _payload: &[u8],
    accounts: &mut [TestAccount],
) -> Result<Vec<u8>, ProgramError> {
    if accounts.is_empty() {
        return Err(ProgramError::NotEnoughAccountKeys);
    }
    let counter = &mut accounts[0];
    if !counter.is_writable {
        return Err(ProgramError::InvalidAccountData);
    }
    if counter.data.len() < 8 {
        return Err(ProgramError::InvalidAccountData);
    }
    let current = u64::from_le_bytes(counter.data[0..8].try_into().unwrap());
    let new_value = current
        .checked_add(1)
        .ok_or(ProgramError::ArithmeticOverflow)?;
    counter.data[0..8].copy_from_slice(&new_value.to_le_bytes());
    Ok(new_value.to_le_bytes().to_vec())
}

#[test]
fn test_e2e_initialize_instruction() {
    let counter_key = Pubkey::new([1u8; 32]);
    let authority_key = Pubkey::new([2u8; 32]);
    let program_key = Pubkey::new([100u8; 32]);
    let mut accounts = vec![
        TestAccount {
            pubkey: counter_key,
            owner: program_key,
            motes: MOTES_PER_DCHAT,
            data: vec![0u8; 40],
            is_signer: false,
            is_writable: true,
        },
        TestAccount {
            pubkey: authority_key,
            owner: program_key,
            motes: MOTES_PER_DCHAT * 10,
            data: vec![],
            is_signer: true,
            is_writable: false,
        },
    ];
    let serializable: Vec<_> = accounts.iter().map(|a| a.to_serializable()).collect();
    let accounts_blob = AccountsBlob::new(&serializable).expect("Failed");
    let accounts_bytes = accounts_blob.encode();
    let initial_value = 42u64;
    let ix_envelope = IxEnvelope::new(
        MockInstruction::Initialize as u16,
        initial_value.to_le_bytes().to_vec(),
    )
    .expect("Failed");
    let ix_bytes = ix_envelope.encode();
    let decoded_accounts = AccountsBlob::decode(&accounts_bytes).expect("Failed");
    let decoded_ix = IxEnvelope::decode(&ix_bytes).expect("Failed");
    assert_eq!(decoded_accounts.entries.len(), 2);
    assert_eq!(decoded_ix.tag, MockInstruction::Initialize as u16);
    let result = execute_mock_instruction(decoded_ix.tag, &decoded_ix.payload, &mut accounts);
    assert!(result.is_ok());
    let stored_value = u64::from_le_bytes(accounts[0].data[0..8].try_into().unwrap());
    assert_eq!(stored_value, initial_value);
}

#[test]
fn test_e2e_multiple_instructions_sequence() {
    let counter_key = Pubkey::new([1u8; 32]);
    let authority_key = Pubkey::new([2u8; 32]);
    let program_key = Pubkey::new([100u8; 32]);
    let mut accounts = vec![
        TestAccount {
            pubkey: counter_key,
            owner: program_key,
            motes: MOTES_PER_DCHAT,
            data: vec![0u8; 40],
            is_signer: false,
            is_writable: true,
        },
        TestAccount {
            pubkey: authority_key,
            owner: program_key,
            motes: MOTES_PER_DCHAT,
            data: vec![],
            is_signer: true,
            is_writable: false,
        },
    ];
    execute_mock_instruction(0, &100u64.to_le_bytes(), &mut accounts).expect("Init failed");
    execute_mock_instruction(2, &[], &mut accounts).expect("Inc1 failed");
    execute_mock_instruction(2, &[], &mut accounts).expect("Inc2 failed");
    let final_value = u64::from_le_bytes(accounts[0].data[0..8].try_into().unwrap());
    assert_eq!(final_value, 102);
}
