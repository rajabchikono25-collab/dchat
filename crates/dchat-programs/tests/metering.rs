//! Metering correctness tests
//!
//! Verifies that compute unit consumption is:
//! - Correctly tracked across all operations
//! - Properly charges for crypto syscalls
//! - Enforces budget limits
//! - Reserves fees upfront

use std::sync::Arc;

use dchat_programs::error::ProgramError;
use dchat_programs::metering::{
    ComputeBudget, ComputeMeter, ComputeUnitCosts, CryptoOpCosts, MemoryCosts, StorageCosts,
};
use dchat_programs::{DEFAULT_COMPUTE_UNITS, MAX_COMPUTE_UNITS};

#[test]
fn test_compute_meter_creation() {
    let budget = ComputeBudget::default();
    let meter = ComputeMeter::new(budget);

    assert_eq!(meter.remaining(), DEFAULT_COMPUTE_UNITS);
    assert_eq!(meter.consumed(), 0);
}

#[test]
fn test_compute_meter_consume_success() {
    let budget = ComputeBudget::new(1000);
    let meter = ComputeMeter::new(budget);

    // Consume some units
    assert!(meter.consume(500).is_ok());
    assert_eq!(meter.remaining(), 500);
    assert_eq!(meter.consumed(), 500);

    // Consume more
    assert!(meter.consume(300).is_ok());
    assert_eq!(meter.remaining(), 200);
    assert_eq!(meter.consumed(), 800);
}

#[test]
fn test_compute_meter_budget_exceeded() {
    let budget = ComputeBudget::new(100);
    let meter = ComputeMeter::new(budget);

    // Try to consume more than budget
    let result = meter.consume(150);
    assert!(matches!(result, Err(ProgramError::ComputeBudgetExceeded)));

    // Budget should be unchanged
    assert_eq!(meter.remaining(), 100);
    assert_eq!(meter.consumed(), 0);
}

#[test]
fn test_compute_meter_exact_budget() {
    let budget = ComputeBudget::new(100);
    let meter = ComputeMeter::new(budget);

    // Consume exactly the budget
    assert!(meter.consume(100).is_ok());
    assert_eq!(meter.remaining(), 0);
    assert_eq!(meter.consumed(), 100);

    // Any further consumption should fail
    let result = meter.consume(1);
    assert!(matches!(result, Err(ProgramError::ComputeBudgetExceeded)));
}

#[test]
fn test_instruction_cost_calculation() {
    let budget = ComputeBudget::new(10000);
    let meter = ComputeMeter::new(budget);

    // Consume instruction base cost
    let data_len = 100;
    let account_count = 5;

    let expected_cost = meter.budget().unit_costs.instruction_base
        + (data_len as u64 * meter.budget().unit_costs.instruction_data_byte)
        + (account_count as u64 * meter.budget().unit_costs.instruction_account);

    assert!(meter.consume_instruction(data_len, account_count).is_ok());
    assert_eq!(meter.consumed(), expected_cost);
}

#[test]
fn test_crypto_syscall_costs() {
    let costs = CryptoOpCosts::default();

    // SHA256 cost: base + per_byte
    let sha256_cost = costs.sha256_base + (1000 * costs.sha256_per_byte);
    assert!(sha256_cost > costs.sha256_base);

    // BLAKE3 cost
    let blake3_cost = costs.blake3_base + (1000 * costs.blake3_per_byte);
    assert!(blake3_cost > costs.blake3_base);

    // Ed25519 verify is expensive
    assert!(costs.ed25519_verify > costs.sha256_base);

    // Secp256k1 recover is most expensive
    assert!(costs.secp256k1_recover > costs.ed25519_verify);
}

#[test]
fn test_sha256_metering() {
    let budget = ComputeBudget::new(100000);
    let meter = ComputeMeter::new(budget);

    let data_len = 1024;
    assert!(meter.consume_sha256(data_len).is_ok());

    let expected = meter.budget().crypto_costs.sha256_base
        + (data_len as u64 * meter.budget().crypto_costs.sha256_per_byte);
    assert_eq!(meter.consumed(), expected);
}

#[test]
fn test_blake3_metering() {
    let budget = ComputeBudget::new(100000);
    let meter = ComputeMeter::new(budget);

    let data_len = 2048;
    assert!(meter.consume_blake3(data_len).is_ok());

    let expected = meter.budget().crypto_costs.blake3_base
        + (data_len as u64 * meter.budget().crypto_costs.blake3_per_byte);
    assert_eq!(meter.consumed(), expected);
}

#[test]
fn test_ed25519_metering() {
    let budget = ComputeBudget::new(100000);
    let meter = ComputeMeter::new(budget);

    assert!(meter.consume_ed25519_verify().is_ok());
    assert_eq!(meter.consumed(), meter.budget().crypto_costs.ed25519_verify);
}

#[test]
fn test_storage_costs() {
    let costs = StorageCosts::default();

    // Write is more expensive than read
    assert!(costs.write_per_byte > costs.read_per_byte);

    // Account creation has base cost
    assert!(costs.account_creation_base > 0);

    // Rent exemption threshold
    assert!(costs.rent_exemption_threshold > 0.0);
}

#[test]
fn test_memory_costs() {
    let costs = MemoryCosts::default();

    assert!(costs.max_heap_bytes > 0);
    assert!(costs.max_stack_bytes > 0);
    assert!(costs.heap_alloc_cost > 0);
}

#[test]
fn test_compute_fee_calculation() {
    let budget = ComputeBudget::new(200_000);
    let lamports_per_cu = 100; // 100 lamports per CU

    let fee = budget.compute_fee(lamports_per_cu);
    assert_eq!(fee, 200_000 * 100);
}

#[test]
fn test_max_budget() {
    let budget = ComputeBudget::max();
    assert_eq!(budget.max_units, MAX_COMPUTE_UNITS);
}

#[test]
fn test_concurrent_metering() {
    use std::thread;

    let budget = ComputeBudget::new(1_000_000);
    let meter = Arc::new(ComputeMeter::new(budget));

    // Spawn multiple threads consuming from same meter
    let handles: Vec<_> = (0..10)
        .map(|_| {
            let m = Arc::clone(&meter);
            thread::spawn(move || {
                for _ in 0..100 {
                    let _ = m.consume(100);
                }
            })
        })
        .collect();

    for handle in handles {
        handle.join().expect("thread join");
    }

    // Should have consumed up to 100,000 (10 * 100 * 100)
    // But might be less if budget exceeded
    assert!(meter.consumed() <= 1_000_000);
}

#[test]
fn test_stack_depth_tracking() {
    let budget = ComputeBudget::new(100_000);
    let meter = ComputeMeter::new(budget);

    // Push stack frames
    assert!(meter.push_stack().is_ok());
    assert_eq!(meter.stack_depth(), 1);

    assert!(meter.push_stack().is_ok());
    assert_eq!(meter.stack_depth(), 2);

    // Pop stack frames
    meter.pop_stack();
    assert_eq!(meter.stack_depth(), 1);

    meter.pop_stack();
    assert_eq!(meter.stack_depth(), 0);
}

#[test]
fn test_heap_tracking() {
    let budget = ComputeBudget::new(100_000);
    let meter = ComputeMeter::new(budget);

    // Allocate heap
    assert!(meter.allocate_heap(1024).is_ok());
    assert_eq!(meter.heap_used(), 1024);

    assert!(meter.allocate_heap(2048).is_ok());
    assert_eq!(meter.heap_used(), 3072);
}

#[test]
fn test_heap_limit_exceeded() {
    let mut budget = ComputeBudget::default();
    budget.heap_size = 1024;
    let meter = ComputeMeter::new(budget);

    // Try to allocate more than limit
    let result = meter.allocate_heap(2048);
    assert!(matches!(result, Err(ProgramError::MemoryLimitExceeded)));
}

#[test]
fn test_meter_snapshot_restore() {
    let budget = ComputeBudget::new(10000);
    let meter = Arc::new(ComputeMeter::new(budget));

    // Consume some
    assert!(meter.consume(1000).is_ok());
    assert_eq!(meter.consumed(), 1000);

    // Take snapshot
    let snapshot = meter.snapshot();

    // Consume more
    assert!(meter.consume(500).is_ok());
    assert_eq!(meter.consumed(), 1500);

    // Restore snapshot
    meter.restore(&snapshot);
    assert_eq!(meter.consumed(), 1000);
    assert_eq!(meter.remaining(), 9000);
}

#[test]
fn test_upfront_fee_reservation() {
    let budget = ComputeBudget::new(100_000);
    let lamports_per_cu = 1;

    // Calculate fee upfront
    let max_fee = budget.compute_fee(lamports_per_cu);
    assert_eq!(max_fee, 100_000);

    // Simulate execution with less consumption
    let meter = ComputeMeter::new(budget);
    assert!(meter.consume(50_000).is_ok());

    // Actual fee would be refunded partially
    let actual_fee = meter.consumed() * lamports_per_cu;
    let refund = max_fee - actual_fee;
    assert_eq!(refund, 50_000);
}

/// Property-based metering tests
#[cfg(feature = "proptest")]
mod proptest_tests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn prop_consumption_monotonic(consumptions in prop::collection::vec(1u64..1000, 1..50)) {
            let total: u64 = consumptions.iter().sum();
            let budget = ComputeBudget::new(total + 1000);
            let meter = ComputeMeter::new(budget);

            let mut running = 0u64;
            for c in consumptions {
                let before = meter.consumed();
                if meter.consume(c).is_ok() {
                    running += c;
                    prop_assert_eq!(meter.consumed(), running);
                    prop_assert!(meter.consumed() >= before);
                }
            }
        }

        #[test]
        fn prop_remaining_plus_consumed_equals_budget(consume in 0u64..10000) {
            let budget_units = 10000u64;
            let budget = ComputeBudget::new(budget_units);
            let meter = ComputeMeter::new(budget);

            if meter.consume(consume).is_ok() {
                prop_assert_eq!(meter.remaining() + meter.consumed(), budget_units);
            }
        }
    }
}
