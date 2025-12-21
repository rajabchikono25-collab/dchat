//! Dchat Program Language v1 - Host Copy-Out Commit
//!
//! This module implements the host-side logic for the copy-in/copy-out (A) approach:
//! 1. Before VM execution: serialize accounts to AccountsBlob and copy to guest memory
//! 2. After VM execution: read back mutated AccountsBlob and validate/commit changes
//!
//! # Invariants Enforced
//! - Account count must match input
//! - Account order must match input
//! - Account keys must match input
//! - Readonly accounts cannot change lamports/data/owner/executable
//! - Writable accounts may change lamports/data but NOT owner/executable in v1
//! - Lamports conservation across all accounts (sum before == sum after)
//!   - Exception: Native mint/burn paths must be explicitly allowed
//! - Data cannot be resized in v1 (fixed-size accounts)
//!
//! # Receipt Generation
//! After successful commit, generates receipt-friendly hashes for verifiability.

use std::collections::HashMap;

use crate::abi::{
    AbiError, AccountTocEntry, AccountsBlob, SerializableAccount, ABI_VERSION,
    ACCOUNTS_BLOB_HEADER_SIZE, TOC_ENTRY_SIZE,
};
use crate::account::{Account, AccountMeta, Pubkey};
use crate::error::{ProgramError, ProgramResult};
use crate::metering::ComputeMeter;

// ═══════════════════════════════════════════════════════════════════════════════
// COPY-IN PREPARATION
// ═══════════════════════════════════════════════════════════════════════════════

/// Copy-in context prepared before VM execution
#[derive(Debug, Clone)]
pub struct CopyInContext {
    /// Account snapshots before execution
    pub snapshots: Vec<AccountSnapshot>,
    /// Total lamports before (for conservation check)
    pub total_lamports_before: u64,
    /// Serialized accounts blob
    pub accounts_blob: Vec<u8>,
    /// Account indices by pubkey for fast lookup
    pub index_by_key: HashMap<Pubkey, usize>,
}

/// Snapshot of account state before execution
#[derive(Debug, Clone)]
pub struct AccountSnapshot {
    /// Public key
    pub pubkey: Pubkey,
    /// Owner
    pub owner: Pubkey,
    /// Lamports before
    pub lamports: u64,
    /// Data hash before
    pub data_hash: [u8; 32],
    /// Data length
    pub data_len: usize,
    /// Is signer
    pub is_signer: bool,
    /// Is writable
    pub is_writable: bool,
    /// Is executable
    pub executable: bool,
    /// Rent epoch
    pub rent_epoch: u64,
    /// Original data (for delta computation)
    pub data: Vec<u8>,
}

impl AccountSnapshot {
    /// Create snapshot from account
    pub fn from_account(account: &Account, meta: &AccountMeta) -> Self {
        Self {
            pubkey: account.key,
            owner: account.owner,
            lamports: account.lamports,
            data_hash: *blake3::hash(account.data.as_slice()).as_bytes(),
            data_len: account.data.len(),
            is_signer: meta.is_signer,
            is_writable: meta.is_writable,
            executable: account.executable,
            rent_epoch: account.rent_epoch,
            data: account.data.to_vec(),
        }
    }
}

/// Prepare accounts for copy-in to guest memory
pub fn prepare_copy_in(
    accounts: &[Account],
    metas: &[AccountMeta],
    compute_meter: &ComputeMeter,
) -> ProgramResult<CopyInContext> {
    if accounts.len() != metas.len() {
        return Err(ProgramError::Custom(AbiError::AccountCountMismatch as u32));
    }

    let mut snapshots = Vec::with_capacity(accounts.len());
    let mut total_lamports: u64 = 0;
    let mut index_by_key = HashMap::with_capacity(accounts.len());

    // Create serializable accounts
    let serializable_accounts: Vec<SerializableAccount> = accounts
        .iter()
        .zip(metas.iter())
        .enumerate()
        .map(|(i, (acc, meta))| {
            let snapshot = AccountSnapshot::from_account(acc, meta);
            total_lamports = total_lamports.saturating_add(acc.lamports);
            index_by_key.insert(acc.key, i);
            snapshots.push(snapshot);

            SerializableAccount {
                pubkey: acc.key,
                owner: acc.owner,
                lamports: acc.lamports,
                data: acc.data.to_vec(),
                is_signer: meta.is_signer,
                is_writable: meta.is_writable,
                executable: acc.executable,
                rent_epoch: acc.rent_epoch,
            }
        })
        .collect();

    // Create blob
    let blob = AccountsBlob::new(&serializable_accounts)?;
    let encoded = blob.encode();

    // Meter copy-in cost
    let copy_in_cost = encoded.len() as u64;
    compute_meter.consume(copy_in_cost)?;

    Ok(CopyInContext {
        snapshots,
        total_lamports_before: total_lamports,
        accounts_blob: encoded,
        index_by_key,
    })
}

// ═══════════════════════════════════════════════════════════════════════════════
// COPY-OUT COMMIT
// ═══════════════════════════════════════════════════════════════════════════════

/// Result of copy-out validation and commit
#[derive(Debug, Clone)]
pub struct CopyOutResult {
    /// Account deltas (what changed)
    pub deltas: Vec<AccountDelta>,
    /// Total compute cost of copy-out
    pub copy_out_cost: u64,
    /// Receipt hash for verification
    pub receipt_hash: [u8; 32],
    /// Pre-state hash
    pub pre_state_hash: [u8; 32],
    /// Post-state hash
    pub post_state_hash: [u8; 32],
}

/// Delta for a single account
#[derive(Debug, Clone)]
pub struct AccountDelta {
    /// Account public key
    pub pubkey: Pubkey,
    /// Lamports before
    pub lamports_before: u64,
    /// Lamports after
    pub lamports_after: u64,
    /// Data changed
    pub data_changed: bool,
    /// New data (if changed)
    pub new_data: Option<Vec<u8>>,
    /// Pre-data hash
    pub pre_data_hash: [u8; 32],
    /// Post-data hash
    pub post_data_hash: [u8; 32],
}

/// Configuration for copy-out commit
#[derive(Debug, Clone)]
pub struct CopyOutConfig {
    /// Allow native mint (lamports creation)
    pub allow_native_mint: bool,
    /// Allow native burn (lamports destruction)
    pub allow_native_burn: bool,
    /// Native mint/burn authority (if allowed)
    pub mint_burn_authority: Option<Pubkey>,
    /// Maximum lamports that can be minted/burned per instruction
    pub max_mint_burn_amount: u64,
}

impl Default for CopyOutConfig {
    fn default() -> Self {
        Self {
            allow_native_mint: false,
            allow_native_burn: false,
            mint_burn_authority: None,
            max_mint_burn_amount: 0,
        }
    }
}

/// Validate and commit changes from guest memory back to host accounts
///
/// This is the critical function that enforces all invariants.
pub fn validate_and_commit(
    copy_in: &CopyInContext,
    mutated_blob: &[u8],
    accounts: &mut [Account],
    config: &CopyOutConfig,
    compute_meter: &ComputeMeter,
) -> ProgramResult<CopyOutResult> {
    // Meter copy-out cost
    let copy_out_cost = mutated_blob.len() as u64;
    compute_meter.consume(copy_out_cost)?;

    // Parse mutated blob
    let blob = AccountsBlob::decode(mutated_blob)?;

    // ─── Invariant 1: Account count must match ─────────────────────────────────
    if blob.entries.len() != copy_in.snapshots.len() {
        return Err(ProgramError::Custom(AbiError::AccountCountMismatch as u32));
    }

    // ─── Invariant 2: ABI version must match ───────────────────────────────────
    if blob.version != ABI_VERSION {
        return Err(ProgramError::Custom(AbiError::UnsupportedVersion as u32));
    }

    // Calculate pre-state hash
    let pre_state_hash = compute_state_hash(&copy_in.snapshots);

    let mut deltas = Vec::with_capacity(blob.entries.len());
    let mut total_lamports_after: u64 = 0;
    let mut post_state_data = Vec::new();

    for (i, (entry, snapshot)) in blob
        .entries
        .iter()
        .zip(copy_in.snapshots.iter())
        .enumerate()
    {
        // ─── Invariant 3: Account order/keys must match ────────────────────────
        if entry.pubkey != snapshot.pubkey {
            return Err(ProgramError::Custom(AbiError::AccountKeyMismatch as u32));
        }

        // ─── Invariant 4: Owner cannot change in v1 ────────────────────────────
        if entry.owner != snapshot.owner {
            return Err(ProgramError::Custom(
                AbiError::OwnerModificationDenied as u32,
            ));
        }

        // ─── Invariant 5: Executable flag cannot change ────────────────────────
        if entry.executable != snapshot.executable {
            return Err(ProgramError::Custom(
                AbiError::ExecutableModificationDenied as u32,
            ));
        }

        // ─── Invariant 6: Data cannot be resized in v1 ─────────────────────────
        if entry.data_len as usize != snapshot.data_len {
            return Err(ProgramError::Custom(AbiError::DataResizeDenied as u32));
        }

        // Get new data from blob
        let new_data = blob
            .get_account_data(i)
            .ok_or(ProgramError::InvalidAccountData)?;
        let new_data_hash = *blake3::hash(new_data).as_bytes();

        // ─── Invariant 7: Readonly accounts cannot change ──────────────────────
        if !snapshot.is_writable {
            // Check lamports unchanged
            if entry.lamports != snapshot.lamports {
                return Err(ProgramError::Custom(AbiError::ReadonlyModified as u32));
            }

            // Check data unchanged
            if new_data_hash != snapshot.data_hash {
                return Err(ProgramError::Custom(AbiError::ReadonlyModified as u32));
            }
        }

        // Track total lamports
        total_lamports_after = total_lamports_after.saturating_add(entry.lamports);

        // Collect delta
        let data_changed = new_data_hash != snapshot.data_hash;
        deltas.push(AccountDelta {
            pubkey: entry.pubkey,
            lamports_before: snapshot.lamports,
            lamports_after: entry.lamports,
            data_changed,
            new_data: if data_changed {
                Some(new_data.to_vec())
            } else {
                None
            },
            pre_data_hash: snapshot.data_hash,
            post_data_hash: new_data_hash,
        });

        // Collect post-state data for hash
        post_state_data.extend_from_slice(&entry.pubkey.0);
        post_state_data.extend_from_slice(&entry.lamports.to_le_bytes());
        post_state_data.extend_from_slice(&new_data_hash);
    }

    // ─── Invariant 8: Lamports conservation ────────────────────────────────────
    let lamports_delta = if total_lamports_after >= copy_in.total_lamports_before {
        total_lamports_after - copy_in.total_lamports_before
    } else {
        copy_in.total_lamports_before - total_lamports_after
    };

    if total_lamports_after != copy_in.total_lamports_before {
        // Check if mint/burn is allowed
        let is_increase = total_lamports_after > copy_in.total_lamports_before;

        if is_increase {
            if !config.allow_native_mint || lamports_delta > config.max_mint_burn_amount {
                return Err(ProgramError::Custom(AbiError::LamportsConservation as u32));
            }
        } else {
            if !config.allow_native_burn || lamports_delta > config.max_mint_burn_amount {
                return Err(ProgramError::Custom(AbiError::LamportsConservation as u32));
            }
        }
    }

    // ─── Apply Changes to Host Accounts ────────────────────────────────────────
    for (i, delta) in deltas.iter().enumerate() {
        if delta.lamports_before != delta.lamports_after {
            accounts[i].lamports = delta.lamports_after;
        }

        if let Some(ref new_data) = delta.new_data {
            accounts[i].data.as_mut_slice().copy_from_slice(new_data);
        }
    }

    // Calculate post-state hash
    let post_state_hash = *blake3::hash(&post_state_data).as_bytes();

    // Calculate receipt hash
    let receipt_hash = compute_receipt_hash(&pre_state_hash, &post_state_hash, &deltas);

    Ok(CopyOutResult {
        deltas,
        copy_out_cost,
        receipt_hash,
        pre_state_hash,
        post_state_hash,
    })
}

/// Compute deterministic hash of pre-execution state
fn compute_state_hash(snapshots: &[AccountSnapshot]) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    for snapshot in snapshots {
        hasher.update(&snapshot.pubkey.0);
        hasher.update(&snapshot.lamports.to_le_bytes());
        hasher.update(&snapshot.data_hash);
    }
    *hasher.finalize().as_bytes()
}

/// Compute receipt hash from state changes
fn compute_receipt_hash(
    pre_state: &[u8; 32],
    post_state: &[u8; 32],
    deltas: &[AccountDelta],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(pre_state);
    hasher.update(post_state);

    for delta in deltas {
        hasher.update(&delta.pubkey.0);
        hasher.update(&delta.lamports_before.to_le_bytes());
        hasher.update(&delta.lamports_after.to_le_bytes());
        hasher.update(&[delta.data_changed as u8]);
        hasher.update(&delta.pre_data_hash);
        hasher.update(&delta.post_data_hash);
    }

    *hasher.finalize().as_bytes()
}

// ═══════════════════════════════════════════════════════════════════════════════
// METERING FOR COPY OPERATIONS
// ═══════════════════════════════════════════════════════════════════════════════

/// Metering costs for copy-in/copy-out operations
#[derive(Debug, Clone)]
pub struct CopyMeteringCosts {
    /// Cost per byte of copy-in (accounts → guest memory)
    pub copy_in_per_byte: u64,
    /// Cost per byte of copy-out (guest memory → accounts)
    pub copy_out_per_byte: u64,
    /// Base cost for copy-in operation
    pub copy_in_base: u64,
    /// Base cost for copy-out operation
    pub copy_out_base: u64,
    /// Cost per account for validation
    pub validation_per_account: u64,
    /// Cost for hash computation
    pub hash_cost: u64,
}

impl Default for CopyMeteringCosts {
    fn default() -> Self {
        Self {
            copy_in_per_byte: 1,
            copy_out_per_byte: 1,
            copy_in_base: 100,
            copy_out_base: 100,
            validation_per_account: 50,
            hash_cost: 100,
        }
    }
}

/// Calculate total copy-in cost
pub fn calculate_copy_in_cost(blob_size: usize, costs: &CopyMeteringCosts) -> u64 {
    costs.copy_in_base + (blob_size as u64 * costs.copy_in_per_byte)
}

/// Calculate total copy-out cost
pub fn calculate_copy_out_cost(
    blob_size: usize,
    account_count: usize,
    costs: &CopyMeteringCosts,
) -> u64 {
    costs.copy_out_base
        + (blob_size as u64 * costs.copy_out_per_byte)
        + (account_count as u64 * costs.validation_per_account)
        + costs.hash_cost * 2 // pre and post state hashes
}

// ═══════════════════════════════════════════════════════════════════════════════
// LOG/EVENT/RETURN DATA SIZE METERING
// ═══════════════════════════════════════════════════════════════════════════════

/// Metering context for tracking emitted data sizes
#[derive(Debug, Clone, Default)]
pub struct EmissionMetering {
    /// Total bytes of log messages
    pub logs_bytes: u64,
    /// Log message count
    pub logs_count: u64,
    /// Total bytes of events
    pub events_bytes: u64,
    /// Event count
    pub events_count: u64,
    /// Return data bytes
    pub return_data_bytes: u64,
}

impl EmissionMetering {
    /// Create new metering context
    pub fn new() -> Self {
        Self::default()
    }

    /// Record a log message
    pub fn record_log(&mut self, size: usize) {
        self.logs_bytes = self.logs_bytes.saturating_add(size as u64);
        self.logs_count = self.logs_count.saturating_add(1);
    }

    /// Record an event
    pub fn record_event(&mut self, size: usize) {
        self.events_bytes = self.events_bytes.saturating_add(size as u64);
        self.events_count = self.events_count.saturating_add(1);
    }

    /// Record return data
    pub fn record_return_data(&mut self, size: usize) {
        self.return_data_bytes = size as u64;
    }

    /// Calculate total emission cost
    pub fn total_cost(&self, cost_per_byte: u64) -> u64 {
        (self.logs_bytes + self.events_bytes + self.return_data_bytes) * cost_per_byte
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// INTEGRATED EXECUTION WITH COPY-IN/OUT
// ═══════════════════════════════════════════════════════════════════════════════

/// Complete execution context with copy-in/out semantics
#[derive(Debug)]
pub struct CopyExecutionContext<'a> {
    /// Copy-in context
    pub copy_in: CopyInContext,
    /// Accounts (mutable for commit)
    pub accounts: &'a mut [Account],
    /// Account metas
    pub metas: Vec<AccountMeta>,
    /// Copy-out config
    pub config: CopyOutConfig,
    /// Emission metering
    pub emission_metering: EmissionMetering,
}

impl<'a> CopyExecutionContext<'a> {
    /// Create new execution context
    pub fn new(
        accounts: &'a mut [Account],
        metas: Vec<AccountMeta>,
        config: CopyOutConfig,
        compute_meter: &ComputeMeter,
    ) -> ProgramResult<Self> {
        let copy_in = prepare_copy_in(accounts, &metas, compute_meter)?;

        Ok(Self {
            copy_in,
            accounts,
            metas,
            config,
            emission_metering: EmissionMetering::new(),
        })
    }

    /// Get accounts blob for guest memory
    pub fn accounts_blob(&self) -> &[u8] {
        &self.copy_in.accounts_blob
    }

    /// Commit changes from guest memory
    pub fn commit(
        &mut self,
        mutated_blob: &[u8],
        compute_meter: &ComputeMeter,
    ) -> ProgramResult<CopyOutResult> {
        validate_and_commit(
            &self.copy_in,
            mutated_blob,
            self.accounts,
            &self.config,
            compute_meter,
        )
    }
}

// ═══════════════════════════════════════════════════════════════════════════════
// TESTS
// ═══════════════════════════════════════════════════════════════════════════════

#[cfg(test)]
mod tests {
    use super::*;
    use crate::account::AccountData;
    use crate::metering::ComputeBudget;
    use std::sync::Arc;

    fn create_test_account(key: [u8; 32], lamports: u64, data: Vec<u8>) -> Account {
        Account {
            key: Pubkey::new(key),
            lamports,
            data: AccountData::new(data),
            owner: Pubkey::new([10u8; 32]),
            executable: false,
            rent_epoch: 0,
            state: crate::account::AccountState::Initialized,
        }
    }

    fn create_test_meter() -> Arc<ComputeMeter> {
        Arc::new(ComputeMeter::new(ComputeBudget::new(1_000_000)))
    }

    #[test]
    fn test_copy_in_preparation() {
        let mut accounts = vec![
            create_test_account([1u8; 32], 1000, vec![1, 2, 3]),
            create_test_account([2u8; 32], 2000, vec![4, 5]),
        ];

        let metas = vec![
            AccountMeta::signer_writable(Pubkey::new([1u8; 32])),
            AccountMeta::readonly(Pubkey::new([2u8; 32])),
        ];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        assert_eq!(copy_in.snapshots.len(), 2);
        assert_eq!(copy_in.total_lamports_before, 3000);
        assert!(!copy_in.accounts_blob.is_empty());
    }

    #[test]
    fn test_copy_out_no_changes() {
        let mut accounts = vec![
            create_test_account([1u8; 32], 1000, vec![1, 2, 3]),
            create_test_account([2u8; 32], 2000, vec![4, 5]),
        ];

        let metas = vec![
            AccountMeta::signer_writable(Pubkey::new([1u8; 32])),
            AccountMeta::writable(Pubkey::new([2u8; 32])),
        ];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Use the same blob (no changes)
        let result = validate_and_commit(
            &copy_in,
            &copy_in.accounts_blob,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        )
        .unwrap();

        assert!(result.deltas.iter().all(|d| !d.data_changed));
        assert_eq!(accounts[0].lamports, 1000);
        assert_eq!(accounts[1].lamports, 2000);
    }

    #[test]
    fn test_copy_out_lamports_transfer() {
        let mut accounts = vec![
            create_test_account([1u8; 32], 1000, vec![1, 2, 3]),
            create_test_account([2u8; 32], 2000, vec![4, 5]),
        ];

        let metas = vec![
            AccountMeta::signer_writable(Pubkey::new([1u8; 32])),
            AccountMeta::writable(Pubkey::new([2u8; 32])),
        ];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Simulate lamports transfer: -500 from acc1, +500 to acc2
        let mut mutated_blob = copy_in.accounts_blob.clone();

        // Parse and modify
        let mut blob = AccountsBlob::decode(&mutated_blob).unwrap();

        // Modify lamports in TOC entries
        blob.entries[0].lamports = 500; // Was 1000
        blob.entries[1].lamports = 2500; // Was 2000

        let mutated = blob.encode();

        let result = validate_and_commit(
            &copy_in,
            &mutated,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        )
        .unwrap();

        assert_eq!(result.deltas[0].lamports_before, 1000);
        assert_eq!(result.deltas[0].lamports_after, 500);
        assert_eq!(result.deltas[1].lamports_before, 2000);
        assert_eq!(result.deltas[1].lamports_after, 2500);

        // Verify accounts were updated
        assert_eq!(accounts[0].lamports, 500);
        assert_eq!(accounts[1].lamports, 2500);
    }

    #[test]
    fn test_copy_out_rejects_readonly_modification() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::readonly(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Try to modify lamports on readonly account
        let mut blob = AccountsBlob::decode(&copy_in.accounts_blob).unwrap();
        blob.entries[0].lamports = 2000; // Try to change

        let mutated = blob.encode();

        let result = validate_and_commit(
            &copy_in,
            &mutated,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        );

        assert!(matches!(
            result,
            Err(ProgramError::Custom(code)) if code == AbiError::ReadonlyModified as u32
        ));
    }

    #[test]
    fn test_copy_out_rejects_lamports_conservation_violation() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Try to create lamports out of thin air
        let mut blob = AccountsBlob::decode(&copy_in.accounts_blob).unwrap();
        blob.entries[0].lamports = 2000; // Try to mint

        let mutated = blob.encode();

        let result = validate_and_commit(
            &copy_in,
            &mutated,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        );

        assert!(matches!(
            result,
            Err(ProgramError::Custom(code)) if code == AbiError::LamportsConservation as u32
        ));
    }

    #[test]
    fn test_copy_out_allows_mint_when_configured() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Mint 500 lamports
        let mut blob = AccountsBlob::decode(&copy_in.accounts_blob).unwrap();
        blob.entries[0].lamports = 1500;

        let mutated = blob.encode();

        let config = CopyOutConfig {
            allow_native_mint: true,
            max_mint_burn_amount: 1000,
            ..Default::default()
        };

        let result =
            validate_and_commit(&copy_in, &mutated, &mut accounts, &config, &meter).unwrap();

        assert_eq!(accounts[0].lamports, 1500);
    }

    #[test]
    fn test_copy_out_rejects_account_count_mismatch() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Create a blob with 2 accounts instead of 1
        let fake_accounts = vec![
            SerializableAccount {
                pubkey: Pubkey::new([1u8; 32]),
                owner: Pubkey::new([10u8; 32]),
                lamports: 1000,
                data: vec![1, 2, 3],
                is_signer: true,
                is_writable: true,
                executable: false,
                rent_epoch: 0,
            },
            SerializableAccount {
                pubkey: Pubkey::new([99u8; 32]),
                owner: Pubkey::new([10u8; 32]),
                lamports: 0,
                data: vec![],
                is_signer: false,
                is_writable: false,
                executable: false,
                rent_epoch: 0,
            },
        ];
        let fake_blob = AccountsBlob::new(&fake_accounts).unwrap().encode();

        let result = validate_and_commit(
            &copy_in,
            &fake_blob,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        );

        assert!(matches!(
            result,
            Err(ProgramError::Custom(code)) if code == AbiError::AccountCountMismatch as u32
        ));
    }

    #[test]
    fn test_copy_out_rejects_key_mismatch() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Create a blob with different key
        let fake_accounts = vec![SerializableAccount {
            pubkey: Pubkey::new([99u8; 32]), // Wrong key!
            owner: Pubkey::new([10u8; 32]),
            lamports: 1000,
            data: vec![1, 2, 3],
            is_signer: true,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        }];
        let fake_blob = AccountsBlob::new(&fake_accounts).unwrap().encode();

        let result = validate_and_commit(
            &copy_in,
            &fake_blob,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        );

        assert!(matches!(
            result,
            Err(ProgramError::Custom(code)) if code == AbiError::AccountKeyMismatch as u32
        ));
    }

    #[test]
    fn test_copy_out_rejects_owner_change() {
        let mut accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        // Try to change owner
        let fake_accounts = vec![SerializableAccount {
            pubkey: Pubkey::new([1u8; 32]),
            owner: Pubkey::new([99u8; 32]), // Different owner!
            lamports: 1000,
            data: vec![1, 2, 3],
            is_signer: true,
            is_writable: true,
            executable: false,
            rent_epoch: 0,
        }];
        let fake_blob = AccountsBlob::new(&fake_accounts).unwrap().encode();

        let result = validate_and_commit(
            &copy_in,
            &fake_blob,
            &mut accounts,
            &CopyOutConfig::default(),
            &meter,
        );

        assert!(matches!(
            result,
            Err(ProgramError::Custom(code)) if code == AbiError::OwnerModificationDenied as u32
        ));
    }

    #[test]
    fn test_emission_metering() {
        let mut metering = EmissionMetering::new();

        metering.record_log(100);
        metering.record_log(50);
        metering.record_event(200);
        metering.record_return_data(32);

        assert_eq!(metering.logs_bytes, 150);
        assert_eq!(metering.logs_count, 2);
        assert_eq!(metering.events_bytes, 200);
        assert_eq!(metering.events_count, 1);
        assert_eq!(metering.return_data_bytes, 32);

        let cost = metering.total_cost(2);
        assert_eq!(cost, (150 + 200 + 32) * 2);
    }

    #[test]
    fn test_receipt_hash_determinism() {
        let accounts = vec![create_test_account([1u8; 32], 1000, vec![1, 2, 3])];

        let metas = vec![AccountMeta::signer_writable(Pubkey::new([1u8; 32]))];

        let meter = create_test_meter();
        let copy_in1 = prepare_copy_in(&accounts, &metas, &meter).unwrap();
        let copy_in2 = prepare_copy_in(&accounts, &metas, &meter).unwrap();

        let mut accounts1 = accounts.clone();
        let mut accounts2 = accounts.clone();

        let result1 = validate_and_commit(
            &copy_in1,
            &copy_in1.accounts_blob,
            &mut accounts1,
            &CopyOutConfig::default(),
            &meter,
        )
        .unwrap();

        let result2 = validate_and_commit(
            &copy_in2,
            &copy_in2.accounts_blob,
            &mut accounts2,
            &CopyOutConfig::default(),
            &meter,
        )
        .unwrap();

        assert_eq!(result1.pre_state_hash, result2.pre_state_hash);
        assert_eq!(result1.post_state_hash, result2.post_state_hash);
        assert_eq!(result1.receipt_hash, result2.receipt_hash);
    }
}
