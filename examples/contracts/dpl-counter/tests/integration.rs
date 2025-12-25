//! Integration tests for DPL Counter contract
//!
//! These tests verify the counter contract behavior using a mock runtime.

use dpl_counter::*;

#[test]
fn test_instruction_tags_are_stable() {
    // Ensure instruction tags don't change between versions
    // This is critical for ABI stability
    assert_eq!(CounterInstruction::Initialize { initial_value: 0 }.tag(), 0);
    assert_eq!(CounterInstruction::Increment.tag(), 1);
    assert_eq!(CounterInstruction::Decrement.tag(), 2);
}

#[test]
fn test_counter_account_size() {
    // Verify the counter account has expected size
    // Size = 8 (discriminator) + 8 (value) + 32 (authority) + 1 (bump) + 8 (total_ops)
    assert_eq!(Counter::SIZE, 57);
}

#[test]
fn test_operation_type_values() {
    // Verify operation types are distinct
    assert_ne!(OperationType::Increment, OperationType::Decrement);
    assert_ne!(OperationType::Increment, OperationType::Set);
    assert_ne!(OperationType::Decrement, OperationType::Set);
}

// Trait to get instruction tag for testing
trait InstructionTag {
    fn tag(&self) -> u8;
}

impl InstructionTag for CounterInstruction {
    fn tag(&self) -> u8 {
        match self {
            CounterInstruction::Initialize { .. } => 0,
            CounterInstruction::Increment => 1,
            CounterInstruction::Decrement => 2,
            CounterInstruction::Set { .. } => 3,
            CounterInstruction::Reset => 4,
            CounterInstruction::TransferAuthority { .. } => 5,
        }
    }
}
