//! Account model for the program runtime
//!
//! Implements Solana-style accounts with strict "touch only passed accounts" capability.

use std::cell::{Ref, RefCell, RefMut};
use std::fmt;
use std::rc::Rc;

use serde::{Deserialize, Serialize};
use zeroize::Zeroize;

use crate::error::{ProgramError, ProgramResult};

/// Public key / address (32 bytes)
#[derive(Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub struct Pubkey(pub [u8; 32]);

impl Pubkey {
    /// Create a new pubkey from bytes
    pub const fn new(bytes: [u8; 32]) -> Self {
        Pubkey(bytes)
    }

    /// Create a zero pubkey
    pub const fn zero() -> Self {
        Pubkey([0u8; 32])
    }

    /// Check if this is the zero pubkey
    pub fn is_zero(&self) -> bool {
        self.0 == [0u8; 32]
    }

    /// Get the bytes
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Create from a slice (must be exactly 32 bytes)
    pub fn from_slice(slice: &[u8]) -> ProgramResult<Self> {
        if slice.len() != 32 {
            return Err(ProgramError::InvalidAccountData);
        }
        let mut bytes = [0u8; 32];
        bytes.copy_from_slice(slice);
        Ok(Pubkey(bytes))
    }

    /// Create from hex string
    pub fn from_hex(s: &str) -> ProgramResult<Self> {
        let bytes = hex::decode(s).map_err(|_| ProgramError::InvalidAccountData)?;
        Self::from_slice(&bytes)
    }

    /// Convert to hex string
    pub fn to_hex(&self) -> String {
        hex::encode(self.0)
    }

    /// Create from base58 string
    pub fn from_base58(s: &str) -> ProgramResult<Self> {
        let bytes = bs58::decode(s)
            .into_vec()
            .map_err(|_| ProgramError::InvalidAccountData)?;
        Self::from_slice(&bytes)
    }

    /// Convert to base58 string
    pub fn to_base58(&self) -> String {
        bs58::encode(&self.0).into_string()
    }
}

impl fmt::Debug for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Pubkey({})", self.to_base58())
    }
}

impl fmt::Display for Pubkey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_base58())
    }
}

/// Account balance in lamports (smallest unit)
pub type Lamports = u64;

/// Rent epoch for account
pub type RentEpoch = u64;

/// Account state indicator
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AccountState {
    /// Account is uninitialized (zero balance, no data)
    Uninitialized,
    /// Account is initialized and active
    Initialized,
    /// Account is frozen (cannot be modified)
    Frozen,
}

impl Default for AccountState {
    fn default() -> Self {
        AccountState::Uninitialized
    }
}

/// Account data container with interior mutability for safe access
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountData {
    /// Raw data bytes
    data: Vec<u8>,
    /// Original length (for detecting unauthorized resizing)
    original_len: usize,
}

impl AccountData {
    /// Create new account data
    pub fn new(data: Vec<u8>) -> Self {
        let original_len = data.len();
        Self { data, original_len }
    }

    /// Create empty account data with given size
    pub fn with_size(size: usize) -> Self {
        Self {
            data: vec![0u8; size],
            original_len: size,
        }
    }

    /// Get data length
    pub fn len(&self) -> usize {
        self.data.len()
    }

    /// Check if empty
    pub fn is_empty(&self) -> bool {
        self.data.is_empty()
    }

    /// Get immutable reference to data
    pub fn as_slice(&self) -> &[u8] {
        &self.data
    }

    /// Get mutable reference to data
    pub fn as_mut_slice(&mut self) -> &mut [u8] {
        &mut self.data
    }

    /// Check if data size was changed (unauthorized resize)
    pub fn was_resized(&self) -> bool {
        self.data.len() != self.original_len
    }

    /// Reset original length tracking after authorized resize
    pub fn commit_resize(&mut self) {
        self.original_len = self.data.len();
    }

    /// Resize data (must be authorized)
    pub fn resize(&mut self, new_size: usize, value: u8) {
        self.data.resize(new_size, value);
    }

    /// Set data from bytes (takes ownership)
    pub fn set_from_bytes(&mut self, bytes: Vec<u8>) {
        self.data = bytes;
        self.original_len = self.data.len();
    }

    /// Set data from slice (copies data)
    pub fn set_from_slice(&mut self, bytes: &[u8]) {
        self.data = bytes.to_vec();
        self.original_len = self.data.len();
    }

    /// Clear data (reset to empty)
    pub fn clear(&mut self) {
        self.data.zeroize();
        self.data.clear();
        self.original_len = 0;
    }

    /// Get inner data as Vec<u8> for serialization
    pub fn to_vec(&self) -> Vec<u8> {
        self.data.clone()
    }
}

impl Drop for AccountData {
    fn drop(&mut self) {
        // Zeroize sensitive data on drop
        self.data.zeroize();
    }
}

/// Core account structure
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Account {
    /// Account public key (address)
    pub key: Pubkey,
    /// Lamports balance
    pub lamports: Lamports,
    /// Account data
    pub data: AccountData,
    /// Program that owns this account
    pub owner: Pubkey,
    /// Is this account executable (a program)?
    pub executable: bool,
    /// Rent epoch when this account was created
    pub rent_epoch: RentEpoch,
    /// Account state
    pub state: AccountState,
}

impl Account {
    /// Create a new uninitialized account
    pub fn new(key: Pubkey) -> Self {
        Self {
            key,
            lamports: 0,
            data: AccountData::new(vec![]),
            owner: Pubkey::zero(),
            executable: false,
            rent_epoch: 0,
            state: AccountState::Uninitialized,
        }
    }

    /// Create a new account with data
    pub fn new_with_data(key: Pubkey, lamports: Lamports, data: Vec<u8>, owner: Pubkey) -> Self {
        Self {
            key,
            lamports,
            data: AccountData::new(data),
            owner,
            executable: false,
            rent_epoch: 0,
            state: AccountState::Initialized,
        }
    }

    /// Create a program account
    pub fn new_program(key: Pubkey, bytecode: Vec<u8>, owner: Pubkey) -> Self {
        Self {
            key,
            lamports: 0,
            data: AccountData::new(bytecode),
            owner,
            executable: true,
            rent_epoch: 0,
            state: AccountState::Initialized,
        }
    }

    /// Check if account is rent exempt at current lamports level
    pub fn is_rent_exempt(&self, rent_per_byte_year: u64, rent_exemption_threshold: f64) -> bool {
        let min_balance =
            ((self.data.len() as u64 + 128) * rent_per_byte_year) as f64 * rent_exemption_threshold;
        self.lamports >= min_balance as u64
    }

    /// Calculate minimum balance for rent exemption
    pub fn minimum_balance(
        data_len: usize,
        rent_per_byte_year: u64,
        rent_exemption_threshold: f64,
    ) -> u64 {
        (((data_len as u64 + 128) * rent_per_byte_year) as f64 * rent_exemption_threshold) as u64
    }
}

/// Account metadata for instruction invocation
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccountMeta {
    /// Account public key
    pub pubkey: Pubkey,
    /// Is this account a signer?
    pub is_signer: bool,
    /// Is this account writable?
    pub is_writable: bool,
}

impl AccountMeta {
    /// Create a new writable signer account meta
    pub fn new(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: true,
        }
    }

    /// Create a new read-only account meta
    pub fn new_readonly(pubkey: Pubkey, is_signer: bool) -> Self {
        Self {
            pubkey,
            is_signer,
            is_writable: false,
        }
    }

    /// Signer and writable
    pub fn signer_writable(pubkey: Pubkey) -> Self {
        Self {
            pubkey,
            is_signer: true,
            is_writable: true,
        }
    }

    /// Signer but read-only
    pub fn signer_readonly(pubkey: Pubkey) -> Self {
        Self {
            pubkey,
            is_signer: true,
            is_writable: false,
        }
    }

    /// Not a signer but writable
    pub fn writable(pubkey: Pubkey) -> Self {
        Self {
            pubkey,
            is_signer: false,
            is_writable: true,
        }
    }

    /// Not a signer and read-only
    pub fn readonly(pubkey: Pubkey) -> Self {
        Self {
            pubkey,
            is_signer: false,
            is_writable: false,
        }
    }
}

/// Account info wrapper with interior mutability for safe program access
///
/// Programs receive AccountInfo references and can only access accounts
/// that were explicitly passed to the instruction (strict capability model).
#[derive(Debug)]
pub struct AccountInfo<'a> {
    /// Account key
    pub key: &'a Pubkey,
    /// Is signer
    pub is_signer: bool,
    /// Is writable
    pub is_writable: bool,
    /// Lamports (interior mutable)
    pub lamports: Rc<RefCell<&'a mut u64>>,
    /// Data (interior mutable)
    pub data: Rc<RefCell<&'a mut [u8]>>,
    /// Owner
    pub owner: &'a Pubkey,
    /// Is executable
    pub executable: bool,
    /// Rent epoch
    pub rent_epoch: RentEpoch,
}

impl<'a> AccountInfo<'a> {
    /// Try to borrow lamports immutably
    pub fn try_borrow_lamports(&self) -> ProgramResult<Ref<'_, &'a mut u64>> {
        self.lamports
            .try_borrow()
            .map_err(|_| ProgramError::BorrowsOverlap)
    }

    /// Try to borrow lamports mutably
    pub fn try_borrow_mut_lamports(&self) -> ProgramResult<RefMut<'_, &'a mut u64>> {
        if !self.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }
        self.lamports
            .try_borrow_mut()
            .map_err(|_| ProgramError::BorrowsOverlap)
    }

    /// Try to borrow data immutably
    pub fn try_borrow_data(&self) -> ProgramResult<Ref<'_, &'a mut [u8]>> {
        self.data
            .try_borrow()
            .map_err(|_| ProgramError::BorrowsOverlap)
    }

    /// Try to borrow data mutably
    pub fn try_borrow_mut_data(&self) -> ProgramResult<RefMut<'_, &'a mut [u8]>> {
        if !self.is_writable {
            return Err(ProgramError::AccountNotWritable);
        }
        self.data
            .try_borrow_mut()
            .map_err(|_| ProgramError::BorrowsOverlap)
    }

    /// Get data length
    pub fn data_len(&self) -> usize {
        self.data.borrow().len()
    }

    /// Check if account is owned by given program
    pub fn is_owned_by(&self, program_id: &Pubkey) -> bool {
        self.owner == program_id
    }

    /// Safely transfer lamports from this account to another
    pub fn transfer_lamports(&self, to: &AccountInfo<'_>, amount: u64) -> ProgramResult<()> {
        let mut from_lamports = self.try_borrow_mut_lamports()?;
        let mut to_lamports = to.try_borrow_mut_lamports()?;

        if **from_lamports < amount {
            return Err(ProgramError::InsufficientFunds);
        }

        **from_lamports = from_lamports
            .checked_sub(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;
        **to_lamports = to_lamports
            .checked_add(amount)
            .ok_or(ProgramError::ArithmeticOverflow)?;

        Ok(())
    }
}

/// Account access tracker for CPI borrow checking
#[derive(Debug, Default)]
pub struct AccountAccessTracker {
    /// Accounts with active borrows (key -> borrow count)
    borrows: std::collections::HashMap<Pubkey, BorrowState>,
}

/// Borrow state for an account
#[derive(Debug, Clone, Copy, Default)]
pub struct BorrowState {
    /// Number of immutable borrows
    pub immutable: u32,
    /// Number of mutable borrows (should be 0 or 1)
    pub mutable: u32,
}

impl AccountAccessTracker {
    /// Create new tracker
    pub fn new() -> Self {
        Self::default()
    }

    /// Check if account can be borrowed immutably
    pub fn can_borrow_immutable(&self, key: &Pubkey) -> bool {
        match self.borrows.get(key) {
            Some(state) => state.mutable == 0,
            None => true,
        }
    }

    /// Check if account can be borrowed mutably
    pub fn can_borrow_mutable(&self, key: &Pubkey) -> bool {
        match self.borrows.get(key) {
            Some(state) => state.mutable == 0 && state.immutable == 0,
            None => true,
        }
    }

    /// Add immutable borrow
    pub fn add_immutable_borrow(&mut self, key: Pubkey) -> ProgramResult<()> {
        if !self.can_borrow_immutable(&key) {
            return Err(ProgramError::BorrowsOverlap);
        }
        let state = self.borrows.entry(key).or_default();
        state.immutable += 1;
        Ok(())
    }

    /// Add mutable borrow
    pub fn add_mutable_borrow(&mut self, key: Pubkey) -> ProgramResult<()> {
        if !self.can_borrow_mutable(&key) {
            return Err(ProgramError::BorrowsOverlap);
        }
        let state = self.borrows.entry(key).or_default();
        state.mutable += 1;
        Ok(())
    }

    /// Release immutable borrow
    pub fn release_immutable_borrow(&mut self, key: &Pubkey) {
        if let Some(state) = self.borrows.get_mut(key) {
            state.immutable = state.immutable.saturating_sub(1);
        }
    }

    /// Release mutable borrow
    pub fn release_mutable_borrow(&mut self, key: &Pubkey) {
        if let Some(state) = self.borrows.get_mut(key) {
            state.mutable = state.mutable.saturating_sub(1);
        }
    }

    /// Clear all borrows
    pub fn clear(&mut self) {
        self.borrows.clear();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pubkey_creation() {
        let bytes = [1u8; 32];
        let pk = Pubkey::new(bytes);
        assert_eq!(pk.as_bytes(), &bytes);
    }

    #[test]
    fn test_pubkey_hex_roundtrip() {
        let pk = Pubkey::new([0xab; 32]);
        let hex_str = pk.to_hex();
        let recovered = Pubkey::from_hex(&hex_str).unwrap();
        assert_eq!(pk, recovered);
    }

    #[test]
    fn test_pubkey_base58_roundtrip() {
        let pk = Pubkey::new([0x12; 32]);
        let b58_str = pk.to_base58();
        let recovered = Pubkey::from_base58(&b58_str).unwrap();
        assert_eq!(pk, recovered);
    }

    #[test]
    fn test_account_meta() {
        let pk = Pubkey::new([1u8; 32]);

        let signer_writable = AccountMeta::signer_writable(pk);
        assert!(signer_writable.is_signer);
        assert!(signer_writable.is_writable);

        let readonly = AccountMeta::readonly(pk);
        assert!(!readonly.is_signer);
        assert!(!readonly.is_writable);
    }

    #[test]
    fn test_account_access_tracker() {
        let mut tracker = AccountAccessTracker::new();
        let key = Pubkey::new([1u8; 32]);

        // Should allow immutable borrow
        assert!(tracker.can_borrow_immutable(&key));
        tracker.add_immutable_borrow(key).unwrap();

        // Should allow multiple immutable borrows
        assert!(tracker.can_borrow_immutable(&key));
        tracker.add_immutable_borrow(key).unwrap();

        // Should not allow mutable borrow while immutable borrows exist
        assert!(!tracker.can_borrow_mutable(&key));
        assert!(tracker.add_mutable_borrow(key).is_err());

        // Release immutable borrows
        tracker.release_immutable_borrow(&key);
        tracker.release_immutable_borrow(&key);

        // Now should allow mutable borrow
        assert!(tracker.can_borrow_mutable(&key));
        tracker.add_mutable_borrow(key).unwrap();

        // Should not allow any borrows while mutable borrow exists
        assert!(!tracker.can_borrow_immutable(&key));
        assert!(!tracker.can_borrow_mutable(&key));
    }
}
