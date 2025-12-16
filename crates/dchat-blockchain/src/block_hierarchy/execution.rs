//! Transaction Execution Engine
//!
//! Handles transaction execution within miniblocks with proper state tracking,
//! receipts generation, and deterministic state root computation.

use super::core_types::{
    Address, ExecutionTransaction, Hash, LaneId, Miniblock, MiniblockBody, TxReceipt,
};
use super::lane_sharding::BodyCommitments;
use super::BlockError;
use crate::canonical;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, RwLock};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

/// Maximum gas per miniblock
pub const MAX_GAS_PER_MINIBLOCK: u64 = 15_000_000;
/// Base gas cost per transaction (re-export from ExecutionTransaction)
pub const BASE_TX_GAS: u64 = ExecutionTransaction::BASE_GAS;
/// Gas cost per byte of tx payload (re-export from ExecutionTransaction)
pub const GAS_PER_BYTE: u64 = ExecutionTransaction::GAS_PER_BYTE;

// ─────────────────────────────────────────────────────────────────────────────
// Account State
// ─────────────────────────────────────────────────────────────────────────────

/// Account state in world state
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct AccountState {
    /// Account balance
    pub balance: u64,
    /// Account nonce (for replay protection)
    pub nonce: u64,
    /// Optional code hash (for smart accounts)
    pub code_hash: Option<Hash>,
    /// Storage root (for accounts with storage)
    pub storage_root: Option<Hash>,
}

impl AccountState {
    /// Create new empty account
    pub fn new() -> Self {
        Self::default()
    }

    /// Create account with balance
    pub fn with_balance(balance: u64) -> Self {
        Self {
            balance,
            ..Default::default()
        }
    }

    /// Compute hash of account state
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/account_state/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// World State
// ─────────────────────────────────────────────────────────────────────────────

/// World state representing all accounts
#[derive(Debug, Clone)]
pub struct WorldState {
    /// Account states indexed by address
    accounts: BTreeMap<Address, AccountState>,
    /// State root (cached)
    state_root: Option<Hash>,
    /// Dirty accounts (modified since last root computation)
    dirty: bool,
}

impl WorldState {
    /// Create new empty world state
    pub fn new() -> Self {
        Self {
            accounts: BTreeMap::new(),
            state_root: None,
            dirty: false,
        }
    }

    /// Create from existing accounts
    pub fn from_accounts(accounts: BTreeMap<Address, AccountState>) -> Self {
        Self {
            accounts,
            state_root: None,
            dirty: true,
        }
    }

    /// Get account state
    pub fn get_account(&self, address: &Address) -> AccountState {
        self.accounts.get(address).cloned().unwrap_or_default()
    }

    /// Set account state
    pub fn set_account(&mut self, address: Address, state: AccountState) {
        self.accounts.insert(address, state);
        self.dirty = true;
        self.state_root = None;
    }

    /// Get mutable account (creates if missing)
    pub fn get_account_mut(&mut self, address: &Address) -> &mut AccountState {
        self.dirty = true;
        self.state_root = None;
        self.accounts.entry(*address).or_default()
    }

    /// Compute state root (Merkle root of all accounts)
    pub fn compute_state_root(&mut self) -> Hash {
        if !self.dirty {
            if let Some(root) = self.state_root {
                return root;
            }
        }

        // Build sorted Merkle tree of account hashes
        let mut leaves: Vec<Hash> = self
            .accounts
            .iter()
            .map(|(addr, state)| {
                let mut data = addr.to_vec();
                data.extend_from_slice(state.hash().as_bytes());
                canonical::domain_hash(b"dchat/account_leaf/v1", &data)
            })
            .collect();

        let root = compute_merkle_root(&mut leaves);
        self.state_root = Some(root);
        self.dirty = false;
        root
    }

    /// Get current state root (may be stale)
    pub fn state_root(&self) -> Option<Hash> {
        self.state_root
    }

    /// Account count
    pub fn account_count(&self) -> usize {
        self.accounts.len()
    }

    /// Create snapshot for parallel execution
    pub fn snapshot(&self) -> WorldStateSnapshot {
        WorldStateSnapshot {
            accounts: self.accounts.clone(),
        }
    }

    /// Apply changes from snapshot
    pub fn apply_changes(&mut self, changes: &[(Address, AccountState)]) {
        for (addr, state) in changes {
            self.set_account(*addr, state.clone());
        }
    }
}

impl Default for WorldState {
    fn default() -> Self {
        Self::new()
    }
}

/// Immutable snapshot for parallel reads
#[derive(Debug, Clone)]
pub struct WorldStateSnapshot {
    accounts: BTreeMap<Address, AccountState>,
}

impl WorldStateSnapshot {
    pub fn get_account(&self, address: &Address) -> AccountState {
        self.accounts.get(address).cloned().unwrap_or_default()
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Transaction Type Alias
// ─────────────────────────────────────────────────────────────────────────────

/// Transaction type for execution (alias to core_types::ExecutionTransaction)
pub type Transaction = ExecutionTransaction;

// ─────────────────────────────────────────────────────────────────────────────
// Execution Context
// ─────────────────────────────────────────────────────────────────────────────

/// Context for executing transactions
pub struct ExecutionContext {
    /// Block height
    pub block_height: u64,
    /// Block timestamp
    pub timestamp: u64,
    /// Gas limit for this execution
    pub gas_limit: u64,
    /// Gas used so far
    pub gas_used: u64,
    /// Fees collected
    pub fees_collected: u64,
}

impl ExecutionContext {
    pub fn new(block_height: u64, timestamp: u64, gas_limit: u64) -> Self {
        Self {
            block_height,
            timestamp,
            gas_limit,
            gas_used: 0,
            fees_collected: 0,
        }
    }

    pub fn remaining_gas(&self) -> u64 {
        self.gas_limit.saturating_sub(self.gas_used)
    }

    pub fn use_gas(&mut self, gas: u64) -> bool {
        if self.gas_used + gas > self.gas_limit {
            return false;
        }
        self.gas_used += gas;
        true
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Execution Engine
// ─────────────────────────────────────────────────────────────────────────────

/// Transaction execution engine
pub struct ExecutionEngine {
    /// Whether to execute in parallel
    parallel: bool,
}

impl ExecutionEngine {
    pub fn new(parallel: bool) -> Self {
        Self { parallel }
    }

    /// Execute a miniblock
    pub fn execute_miniblock(
        &self,
        miniblock: &Miniblock,
        state: &mut WorldState,
        ctx: &mut ExecutionContext,
    ) -> Result<Vec<TxReceipt>, BlockError> {
        // Validate miniblock bounds
        miniblock.header.validate_bounds()?;

        // Get body, return empty receipts if body not present
        let body = match &miniblock.body {
            Some(b) => b,
            None => return Ok(Vec::new()),
        };

        // Get transactions from body
        let txs = self.extract_transactions(body)?;

        // Execute based on mode
        if self.parallel {
            self.execute_parallel(&txs, state, ctx)
        } else {
            self.execute_sequential(&txs, state, ctx)
        }
    }

    /// Extract transactions from body
    fn extract_transactions(&self, body: &MiniblockBody) -> Result<Vec<Transaction>, BlockError> {
        // MiniblockBody.transactions contains dchat_chain::Transaction (high-level envelope)
        // Convert to ExecutionTransaction using the from_chain_transaction method
        body.transactions
            .iter()
            .enumerate()
            .map(|(idx, chain_tx)| {
                ExecutionTransaction::from_chain_transaction(chain_tx).ok_or_else(|| {
                    BlockError::InvalidTransaction(format!(
                        "failed to parse execution data from chain transaction at index {}",
                        idx
                    ))
                })
            })
            .collect()
    }

    /// Execute transactions sequentially
    fn execute_sequential(
        &self,
        txs: &[Transaction],
        state: &mut WorldState,
        ctx: &mut ExecutionContext,
    ) -> Result<Vec<TxReceipt>, BlockError> {
        let mut receipts = Vec::with_capacity(txs.len());

        for (idx, tx) in txs.iter().enumerate() {
            let receipt = self.execute_single_tx(tx, state, ctx, idx)?;
            receipts.push(receipt);
        }

        Ok(receipts)
    }

    /// Execute transactions in parallel by lane
    fn execute_parallel(
        &self,
        txs: &[Transaction],
        state: &mut WorldState,
        ctx: &mut ExecutionContext,
    ) -> Result<Vec<TxReceipt>, BlockError> {
        // Group by lane
        let mut lanes: HashMap<LaneId, Vec<(usize, &Transaction)>> = HashMap::new();
        for (idx, tx) in txs.iter().enumerate() {
            lanes.entry(tx.lane()).or_default().push((idx, tx));
        }

        // Take snapshot
        let snapshot = state.snapshot();
        let results = Arc::new(RwLock::new(vec![None; txs.len()]));

        // Execute each lane in parallel
        let lane_changes: Vec<_> = lanes
            .par_iter()
            .map(|(_lane, lane_txs)| {
                let mut lane_changes: Vec<(Address, AccountState)> = Vec::new();

                for &(idx, tx) in lane_txs {
                    // Simple state update for this tx
                    let sender = tx.sender;
                    let mut sender_state = snapshot.get_account(&sender);

                    // Check nonce
                    if sender_state.nonce != tx.nonce {
                        let receipt = TxReceipt::failure(tx.tx_id, idx as u32, "nonce mismatch");
                        results.write().unwrap()[idx] = Some(receipt);
                        continue;
                    }

                    // Check balance
                    let total_cost = tx.value + tx.gas_limit * tx.gas_price;
                    if sender_state.balance < total_cost {
                        let receipt =
                            TxReceipt::failure(tx.tx_id, idx as u32, "insufficient balance");
                        results.write().unwrap()[idx] = Some(receipt);
                        continue;
                    }

                    // Deduct from sender
                    let gas_cost = tx.gas_cost();
                    let fee = gas_cost * tx.gas_price;
                    sender_state.balance -= tx.value + fee;
                    sender_state.nonce += 1;
                    lane_changes.push((sender, sender_state));

                    // Credit recipient
                    if let Some(recipient) = tx.recipient {
                        let mut recipient_state = snapshot.get_account(&recipient);
                        recipient_state.balance += tx.value;
                        lane_changes.push((recipient, recipient_state));
                    }

                    // Success receipt with post-state root (computed per-lane)
                    let receipt = TxReceipt::success(
                        tx.tx_id,
                        idx as u32,
                        gas_cost,
                        Hash::ZERO, // Post-state root computed after all lanes merge
                    );
                    results.write().unwrap()[idx] = Some(receipt);
                }

                lane_changes
            })
            .collect();

        // Apply all lane changes
        for changes in lane_changes {
            state.apply_changes(&changes);
        }

        // Collect receipts in order
        let receipts: Vec<TxReceipt> = results
            .read()
            .unwrap()
            .iter()
            .filter_map(|r| r.clone())
            .collect();

        // Update context
        ctx.gas_used += receipts.iter().map(|r| r.gas_used).sum::<u64>();

        Ok(receipts)
    }

    /// Execute single transaction
    fn execute_single_tx(
        &self,
        tx: &Transaction,
        state: &mut WorldState,
        ctx: &mut ExecutionContext,
        idx: usize,
    ) -> Result<TxReceipt, BlockError> {
        let gas_cost = tx.gas_cost();

        // Check gas limit
        if !ctx.use_gas(gas_cost) {
            return Ok(TxReceipt::failure(tx.tx_id, idx as u32, "out of gas"));
        }

        // Get sender account
        let sender_account = state.get_account_mut(&tx.sender);

        // Verify nonce
        if sender_account.nonce != tx.nonce {
            return Ok(TxReceipt::failure(tx.tx_id, idx as u32, "nonce mismatch"));
        }

        // Calculate total cost
        let fee = gas_cost * tx.gas_price;
        let total_cost = tx.value + fee;

        // Check balance
        if sender_account.balance < total_cost {
            return Ok(TxReceipt::failure(
                tx.tx_id,
                idx as u32,
                "insufficient balance",
            ));
        }

        // Deduct from sender
        sender_account.balance -= total_cost;
        sender_account.nonce += 1;

        // Credit recipient
        if let Some(recipient) = tx.recipient {
            let recipient_account = state.get_account_mut(&recipient);
            recipient_account.balance += tx.value;
        }

        // Collect fee
        ctx.fees_collected += fee;

        // Compute post-state root after this transaction
        let post_state_root = state.compute_state_root();

        Ok(TxReceipt::success(
            tx.tx_id,
            idx as u32,
            gas_cost,
            post_state_root,
        ))
    }
}

impl Default for ExecutionEngine {
    fn default() -> Self {
        Self::new(true)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// State Transition
// ─────────────────────────────────────────────────────────────────────────────

/// State transition result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StateTransition {
    /// Pre-state root
    pub pre_state_root: Hash,
    /// Post-state root
    pub post_state_root: Hash,
    /// Transactions root
    pub tx_root: Hash,
    /// Receipts root
    pub receipts_root: Hash,
    /// Total gas used
    pub gas_used: u64,
    /// Fees collected
    pub fees_collected: u64,
}

impl StateTransition {
    /// Compute from execution
    pub fn compute(
        pre_root: Hash,
        post_root: Hash,
        commitments: &BodyCommitments,
        ctx: &ExecutionContext,
    ) -> Self {
        Self {
            pre_state_root: pre_root,
            post_state_root: post_root,
            tx_root: commitments.tx_root,
            receipts_root: commitments.receipts_root,
            gas_used: ctx.gas_used,
            fees_collected: ctx.fees_collected,
        }
    }

    /// Hash of state transition
    pub fn hash(&self) -> Hash {
        let bytes = canonical::canonical_serialize(self);
        canonical::domain_hash(b"dchat/state_transition/v1", &bytes)
    }
}

// ─────────────────────────────────────────────────────────────────────────────
// Helper Functions
// ─────────────────────────────────────────────────────────────────────────────

/// Compute Merkle root from leaves
fn compute_merkle_root(leaves: &mut Vec<Hash>) -> Hash {
    if leaves.is_empty() {
        return Hash::ZERO;
    }

    // Pad to power of 2
    while leaves.len() & (leaves.len() - 1) != 0 {
        leaves.push(Hash::ZERO);
    }

    while leaves.len() > 1 {
        let mut next = Vec::with_capacity(leaves.len() / 2);
        for chunk in leaves.chunks(2) {
            let mut combined = Vec::with_capacity(64);
            combined.extend_from_slice(chunk[0].as_bytes());
            combined.extend_from_slice(chunk.get(1).unwrap_or(&Hash::ZERO).as_bytes());
            next.push(canonical::domain_hash(b"dchat/merkle_node/v1", &combined));
        }
        *leaves = next;
    }

    leaves[0]
}

// ─────────────────────────────────────────────────────────────────────────────
// Tests
// ─────────────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_account_state() {
        let state = AccountState::with_balance(1000);
        assert_eq!(state.balance, 1000);
        assert_eq!(state.nonce, 0);

        let hash = state.hash();
        assert_ne!(hash, Hash::ZERO);
    }

    #[test]
    fn test_world_state() {
        let mut state = WorldState::new();
        let addr = [1u8; 32];

        state.set_account(addr, AccountState::with_balance(1000));
        let account = state.get_account(&addr);
        assert_eq!(account.balance, 1000);

        let root = state.compute_state_root();
        assert_ne!(root, Hash::ZERO);
    }

    #[test]
    fn test_transaction() {
        let tx = Transaction {
            tx_id: uuid::Uuid::new_v4(),
            hash: Hash::ZERO,
            sender: [1u8; 32],
            recipient: Some([2u8; 32]),
            value: 100,
            payload: vec![0; 100],
            nonce: 0,
            gas_limit: 100_000,
            gas_price: 1,
            signature: vec![],
        };

        let hash = tx.compute_hash();
        assert_ne!(hash, Hash::ZERO);

        let gas = tx.gas_cost();
        assert_eq!(gas, BASE_TX_GAS + 100 * GAS_PER_BYTE);
    }

    #[test]
    fn test_execution_context() {
        let mut ctx = ExecutionContext::new(1, 0, 100_000);

        assert!(ctx.use_gas(50_000));
        assert_eq!(ctx.remaining_gas(), 50_000);

        assert!(ctx.use_gas(50_000));
        assert_eq!(ctx.remaining_gas(), 0);

        assert!(!ctx.use_gas(1));
    }

    #[test]
    fn test_merkle_root() {
        let mut leaves = vec![
            Hash::from([1u8; 32]),
            Hash::from([2u8; 32]),
            Hash::from([3u8; 32]),
            Hash::from([4u8; 32]),
        ];

        let root = compute_merkle_root(&mut leaves);
        assert_ne!(root, Hash::ZERO);
    }
}
