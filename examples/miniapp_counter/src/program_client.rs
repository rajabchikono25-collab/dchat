//! Counter Program Client
//!
//! A client library for interacting with the DPL counter program from mini-apps.
//! This demonstrates how mini-apps can call on-chain programs through intents.

use std::collections::VecDeque;

use borsh::{BorshDeserialize, BorshSerialize};
use serde::{Deserialize, Serialize};

/// Program ID for the counter program (derived from program name hash)
pub const COUNTER_PROGRAM_ID: [u8; 32] = {
    // SHA256("counter_program") - first 32 bytes
    let mut id = [0u8; 32];
    id[0] = 0x12;
    id[1] = 0x34;
    id[2] = 0x56;
    id[3] = 0x78;
    // ... rest filled with program address
    id
};

/// Counter account state (matches on-chain structure)
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub struct CounterAccount {
    /// Current counter value (using i64 to match on-chain which uses i64)
    pub value: i64,
    /// Authority who can perform privileged operations
    pub authority: [u8; 32],
    /// PDA bump seed
    pub bump: u8,
    /// Total operations performed
    pub total_operations: u64,
    /// Last updated slot
    pub last_updated: u64,
}

impl CounterAccount {
    /// Size of the account data (excluding discriminator)
    pub const SIZE: usize = 8 + 32 + 1 + 8 + 8; // value + authority + bump + total_ops + last_updated

    /// Account discriminator (first 8 bytes of SHA256("account:Counter"))
    pub const DISCRIMINATOR: [u8; 8] = [0xC0, 0x11, 0x4E, 0x72, 0x00, 0x00, 0x00, 0x00];
}

/// Instruction types for the counter program
#[derive(Debug, Clone, Serialize, Deserialize, BorshSerialize, BorshDeserialize)]
pub enum CounterInstruction {
    /// Initialize a new counter
    Initialize { initial_value: i64 },
    /// Increment the counter by 1
    Increment,
    /// Decrement the counter by 1
    Decrement,
    /// Set the counter to a specific value
    Set { new_value: i64 },
    /// Reset the counter to zero
    Reset,
}

impl CounterInstruction {
    /// Get the instruction tag
    pub fn tag(&self) -> u8 {
        match self {
            CounterInstruction::Initialize { .. } => 0,
            CounterInstruction::Increment => 1,
            CounterInstruction::Decrement => 2,
            CounterInstruction::Set { .. } => 3,
            CounterInstruction::Reset => 4,
        }
    }

    /// Serialize to bytes for on-chain execution
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut data = vec![self.tag()];
        match self {
            CounterInstruction::Initialize { initial_value } => {
                data.extend_from_slice(&initial_value.to_le_bytes());
            }
            CounterInstruction::Set { new_value } => {
                data.extend_from_slice(&new_value.to_le_bytes());
            }
            CounterInstruction::Increment
            | CounterInstruction::Decrement
            | CounterInstruction::Reset => {
                // No additional data
            }
        }
        data
    }
}

/// Events emitted by the counter program
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum CounterEvent {
    /// Counter was initialized
    CounterInitialized {
        initial_value: i64,
        authority: [u8; 32],
    },
    /// Counter value changed
    CounterChanged {
        old_value: i64,
        new_value: i64,
        changer: [u8; 32],
    },
    /// Counter was reset
    CounterReset { old_value: i64, resetter: [u8; 32] },
    /// Authority was transferred
    AuthorityTransferred {
        old_authority: [u8; 32],
        new_authority: [u8; 32],
    },
}

/// Result of a program call
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramCallResult {
    /// Transaction signature/hash
    pub signature: String,
    /// Slot/block number
    pub slot: u64,
    /// Whether the call succeeded
    pub success: bool,
    /// Error message if failed
    pub error: Option<String>,
    /// Events emitted during execution
    pub events: Vec<CounterEvent>,
    /// Compute units consumed
    pub compute_units: u64,
    /// Updated account state
    pub account: Option<CounterAccount>,
}

/// Record of a past transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionRecord {
    pub signature: String,
    pub slot: u64,
    pub success: bool,
    pub instruction: String,
}

/// Transaction to be signed and submitted
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transaction {
    /// Unique transaction ID
    pub id: String,
    /// Program ID to call
    pub program_id: String,
    /// Instruction data
    pub instruction_data: Vec<u8>,
    /// Accounts involved
    pub accounts: Vec<TransactionAccount>,
    /// Recent blockhash
    pub recent_blockhash: String,
}

/// Account reference in a transaction
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransactionAccount {
    /// Account address
    pub pubkey: String,
    /// Is this account a signer?
    pub is_signer: bool,
    /// Is this account writable?
    pub is_writable: bool,
}

/// Counter program client for building and simulating transactions
#[derive(Debug, Clone)]
pub struct CounterProgramClient {
    /// Program ID
    pub program_id: [u8; 32],
    /// User's authority keypair (public key)
    pub authority: [u8; 32],
    /// Counter PDA address
    pub counter_pda: Option<[u8; 32]>,
    /// Simulated on-chain state
    state: Option<CounterAccount>,
    /// Current slot number
    current_slot: u64,
    /// Transaction history
    transaction_history: VecDeque<TransactionRecord>,
}

impl CounterProgramClient {
    /// Create a new client
    pub fn new(authority: [u8; 32]) -> Self {
        Self {
            program_id: COUNTER_PROGRAM_ID,
            authority,
            counter_pda: None,
            state: None,
            current_slot: 1,
            transaction_history: VecDeque::with_capacity(100),
        }
    }

    /// Derive the counter PDA for this authority
    pub fn derive_counter_pda(&mut self) -> [u8; 32] {
        // PDA = hash("counter" || authority || program_id)
        let mut hasher = blake3::Hasher::new();
        hasher.update(b"counter");
        hasher.update(&self.authority);
        hasher.update(&self.program_id);
        let hash = hasher.finalize();
        let mut pda = [0u8; 32];
        pda.copy_from_slice(&hash.as_bytes()[..32]);
        self.counter_pda = Some(pda);
        pda
    }

    /// Get current counter account state
    pub fn get_account(&self) -> Option<CounterAccount> {
        self.state.clone()
    }

    /// Get transaction history
    pub fn get_transaction_history(&self) -> Vec<TransactionRecord> {
        self.transaction_history.iter().cloned().collect()
    }

    /// Build an initialize transaction
    pub fn build_initialize(&self, initial_value: i64) -> Transaction {
        let instruction = CounterInstruction::Initialize { initial_value };
        self.build_transaction(instruction, true)
    }

    /// Build an increment transaction
    pub fn build_increment(&self) -> Transaction {
        let instruction = CounterInstruction::Increment;
        self.build_transaction(instruction, false)
    }

    /// Build a decrement transaction
    pub fn build_decrement(&self) -> Transaction {
        let instruction = CounterInstruction::Decrement;
        self.build_transaction(instruction, false)
    }

    /// Build a set transaction
    pub fn build_set(&self, new_value: i64) -> Transaction {
        let instruction = CounterInstruction::Set { new_value };
        self.build_transaction(instruction, true)
    }

    /// Build a reset transaction
    pub fn build_reset(&self) -> Transaction {
        let instruction = CounterInstruction::Reset;
        self.build_transaction(instruction, true)
    }

    fn build_transaction(
        &self,
        instruction: CounterInstruction,
        requires_authority: bool,
    ) -> Transaction {
        let counter_pda = self.counter_pda.unwrap_or_else(|| {
            let mut hasher = blake3::Hasher::new();
            hasher.update(b"counter");
            hasher.update(&self.authority);
            hasher.update(&self.program_id);
            let hash = hasher.finalize();
            let mut pda = [0u8; 32];
            pda.copy_from_slice(&hash.as_bytes()[..32]);
            pda
        });

        let mut accounts = vec![TransactionAccount {
            pubkey: hex::encode(counter_pda),
            is_signer: false,
            is_writable: true,
        }];

        if requires_authority {
            accounts.push(TransactionAccount {
                pubkey: hex::encode(self.authority),
                is_signer: true,
                is_writable: false,
            });
        }

        // Add system program for initialize
        if matches!(instruction, CounterInstruction::Initialize { .. }) {
            accounts.push(TransactionAccount {
                pubkey: "0000000000000000000000000000000000000000000000000000000000000000"
                    .to_string(),
                is_signer: false,
                is_writable: false,
            });
        }

        Transaction {
            id: uuid::Uuid::new_v4().to_string(),
            program_id: hex::encode(self.program_id),
            instruction_data: instruction.to_bytes(),
            accounts,
            recent_blockhash: hex::encode(blake3::hash(b"blockhash").as_bytes()),
        }
    }

    /// Simulate executing a transaction locally
    pub fn simulate(&mut self, tx: &Transaction) -> ProgramCallResult {
        let instruction_tag = tx.instruction_data.first().copied().unwrap_or(255);

        // Increment slot for each simulation
        self.current_slot += 1;

        let instruction_name = match instruction_tag {
            0 => "Initialize",
            1 => "Increment",
            2 => "Decrement",
            3 => "Set",
            4 => "Reset",
            _ => "Unknown",
        };

        let result = match instruction_tag {
            0 => self.simulate_initialize(tx),
            1 => self.simulate_increment(tx),
            2 => self.simulate_decrement(tx),
            3 => self.simulate_set(tx),
            4 => self.simulate_reset(tx),
            _ => ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: false,
                error: Some("Unknown instruction".to_string()),
                events: vec![],
                compute_units: 0,
                account: None,
            },
        };

        // Record transaction
        self.transaction_history.push_front(TransactionRecord {
            signature: result.signature.clone(),
            slot: result.slot,
            success: result.success,
            instruction: instruction_name.to_string(),
        });

        // Limit history size
        while self.transaction_history.len() > 100 {
            self.transaction_history.pop_back();
        }

        result
    }

    fn simulate_initialize(&mut self, tx: &Transaction) -> ProgramCallResult {
        let initial_value = if tx.instruction_data.len() >= 9 {
            i64::from_le_bytes(tx.instruction_data[1..9].try_into().unwrap_or([0; 8]))
        } else {
            0
        };

        let _counter_pda = self.derive_counter_pda();
        let state = CounterAccount {
            value: initial_value,
            authority: self.authority,
            bump: 255, // Simulated bump
            total_operations: 0,
            last_updated: self.current_slot,
        };
        self.state = Some(state.clone());

        ProgramCallResult {
            signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
            slot: self.current_slot,
            success: true,
            error: None,
            events: vec![CounterEvent::CounterInitialized {
                initial_value,
                authority: self.authority,
            }],
            compute_units: 5000,
            account: Some(state),
        }
    }

    fn simulate_increment(&mut self, tx: &Transaction) -> ProgramCallResult {
        if let Some(ref mut state) = self.state {
            let old_value = state.value;

            if state.value == i64::MAX {
                return ProgramCallResult {
                    signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                    slot: self.current_slot,
                    success: false,
                    error: Some("Counter overflow".to_string()),
                    events: vec![],
                    compute_units: 1000,
                    account: Some(state.clone()),
                };
            }

            state.value += 1;
            state.total_operations += 1;
            state.last_updated = self.current_slot;

            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: true,
                error: None,
                events: vec![CounterEvent::CounterChanged {
                    old_value,
                    new_value: state.value,
                    changer: self.authority,
                }],
                compute_units: 2000,
                account: Some(state.clone()),
            }
        } else {
            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: false,
                error: Some("Counter not initialized".to_string()),
                events: vec![],
                compute_units: 500,
                account: None,
            }
        }
    }

    fn simulate_decrement(&mut self, tx: &Transaction) -> ProgramCallResult {
        if let Some(ref mut state) = self.state {
            let old_value = state.value;

            if state.value == i64::MIN {
                return ProgramCallResult {
                    signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                    slot: self.current_slot,
                    success: false,
                    error: Some("Counter underflow".to_string()),
                    events: vec![],
                    compute_units: 1000,
                    account: Some(state.clone()),
                };
            }

            state.value -= 1;
            state.total_operations += 1;
            state.last_updated = self.current_slot;

            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: true,
                error: None,
                events: vec![CounterEvent::CounterChanged {
                    old_value,
                    new_value: state.value,
                    changer: self.authority,
                }],
                compute_units: 2000,
                account: Some(state.clone()),
            }
        } else {
            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: false,
                error: Some("Counter not initialized".to_string()),
                events: vec![],
                compute_units: 500,
                account: None,
            }
        }
    }

    fn simulate_set(&mut self, tx: &Transaction) -> ProgramCallResult {
        let new_value = if tx.instruction_data.len() >= 9 {
            i64::from_le_bytes(tx.instruction_data[1..9].try_into().unwrap_or([0; 8]))
        } else {
            0
        };

        if let Some(ref mut state) = self.state {
            let old_value = state.value;
            state.value = new_value;
            state.total_operations += 1;
            state.last_updated = self.current_slot;

            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: true,
                error: None,
                events: vec![CounterEvent::CounterChanged {
                    old_value,
                    new_value,
                    changer: self.authority,
                }],
                compute_units: 2500,
                account: Some(state.clone()),
            }
        } else {
            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: false,
                error: Some("Counter not initialized".to_string()),
                events: vec![],
                compute_units: 500,
                account: None,
            }
        }
    }

    fn simulate_reset(&mut self, tx: &Transaction) -> ProgramCallResult {
        if let Some(ref mut state) = self.state {
            let old_value = state.value;
            state.value = 0;
            state.total_operations += 1;
            state.last_updated = self.current_slot;

            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: true,
                error: None,
                events: vec![CounterEvent::CounterReset {
                    old_value,
                    resetter: self.authority,
                }],
                compute_units: 2000,
                account: Some(state.clone()),
            }
        } else {
            ProgramCallResult {
                signature: hex::encode(blake3::hash(tx.id.as_bytes()).as_bytes()),
                slot: self.current_slot,
                success: false,
                error: Some("Counter not initialized".to_string()),
                events: vec![],
                compute_units: 500,
                account: None,
            }
        }
    }

    /// Get current counter state (legacy method name)
    pub fn get_state(&self) -> Option<&CounterAccount> {
        self.state.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_flow() {
        let authority = [1u8; 32];
        let mut client = CounterProgramClient::new(authority);

        // Initialize
        let tx = client.build_initialize(10);
        let result = client.simulate(&tx);
        assert!(result.success);
        assert_eq!(client.get_state().unwrap().value, 10);

        // Increment
        let tx = client.build_increment();
        let result = client.simulate(&tx);
        assert!(result.success);
        assert_eq!(client.get_state().unwrap().value, 11);

        // Decrement
        let tx = client.build_decrement();
        let result = client.simulate(&tx);
        assert!(result.success);
        assert_eq!(client.get_state().unwrap().value, 10);

        // Set
        let tx = client.build_set(100);
        let result = client.simulate(&tx);
        assert!(result.success);
        assert_eq!(client.get_state().unwrap().value, 100);

        // Reset
        let tx = client.build_reset();
        let result = client.simulate(&tx);
        assert!(result.success);
        assert_eq!(client.get_state().unwrap().value, 0);
    }

    #[test]
    fn test_underflow() {
        let authority = [1u8; 32];
        let mut client = CounterProgramClient::new(authority);

        // Initialize with i64::MIN (can't decrement further)
        let tx = client.build_initialize(i64::MIN);
        client.simulate(&tx);

        // Try to decrement - should fail
        let tx = client.build_decrement();
        let result = client.simulate(&tx);
        assert!(!result.success);
        assert!(result.error.as_ref().unwrap().contains("underflow"));
    }

    #[test]
    fn test_transaction_history() {
        let authority = [1u8; 32];
        let mut client = CounterProgramClient::new(authority);

        // Initialize
        let tx = client.build_initialize(0);
        client.simulate(&tx);

        // Do some operations
        for _ in 0..5 {
            let tx = client.build_increment();
            client.simulate(&tx);
        }

        // Check history
        let history = client.get_transaction_history();
        assert_eq!(history.len(), 6); // 1 init + 5 increments
        assert_eq!(history[0].instruction, "Increment"); // Most recent first
        assert_eq!(history[5].instruction, "Initialize");
    }
}
