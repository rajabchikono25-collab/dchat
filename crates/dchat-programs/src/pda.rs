//! Program Derived Addresses (PDAs)
//!
//! Deterministic address derivation for program-controlled accounts.

use serde::{Deserialize, Serialize};

use crate::account::Pubkey;
use crate::error::{ProgramError, ProgramResult};

/// Maximum number of seeds for PDA derivation
pub const MAX_SEEDS: usize = 16;

/// Maximum seed length
pub const MAX_SEED_LEN: usize = 32;

/// PDA derivation result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProgramDerivedAddress {
    /// The derived address
    pub address: Pubkey,
    /// The bump seed used
    pub bump: u8,
}

impl ProgramDerivedAddress {
    /// Create from address and bump
    pub fn new(address: Pubkey, bump: u8) -> Self {
        Self { address, bump }
    }
}

/// PDA derivation utilities
pub struct PdaDerivation;

impl PdaDerivation {
    /// Find a valid PDA and bump seed
    ///
    /// Searches for a bump seed that produces a valid off-curve point.
    pub fn find_program_address(
        seeds: &[&[u8]],
        program_id: &Pubkey,
    ) -> ProgramResult<ProgramDerivedAddress> {
        // Validate seeds
        Self::validate_seeds(seeds)?;

        // Try bump seeds from 255 down to 0
        for bump in (0..=255u8).rev() {
            match Self::create_program_address_internal(seeds, program_id, bump) {
                Ok(address) => {
                    return Ok(ProgramDerivedAddress { address, bump });
                }
                Err(ProgramError::InvalidSeeds) => {
                    // This bump produces an on-curve point, try next
                    continue;
                }
                Err(e) => return Err(e),
            }
        }

        Err(ProgramError::InvalidSeeds)
    }

    /// Create a PDA with a known bump seed
    pub fn create_program_address(seeds: &[&[u8]], program_id: &Pubkey) -> ProgramResult<Pubkey> {
        Self::validate_seeds(seeds)?;

        // Get the last seed as bump
        if seeds.is_empty() {
            return Err(ProgramError::InvalidSeeds);
        }

        let bump_seed = seeds.last().ok_or(ProgramError::InvalidSeeds)?;
        if bump_seed.len() != 1 {
            return Err(ProgramError::InvalidSeeds);
        }
        let bump = bump_seed[0];

        let actual_seeds = &seeds[..seeds.len() - 1];
        Self::create_program_address_internal(actual_seeds, program_id, bump)
    }

    /// Create PDA with explicit bump
    pub fn create_program_address_with_bump(
        seeds: &[&[u8]],
        program_id: &Pubkey,
        bump: u8,
    ) -> ProgramResult<Pubkey> {
        Self::validate_seeds(seeds)?;
        Self::create_program_address_internal(seeds, program_id, bump)
    }

    /// Internal PDA creation
    fn create_program_address_internal(
        seeds: &[&[u8]],
        program_id: &Pubkey,
        bump: u8,
    ) -> ProgramResult<Pubkey> {
        // Hash: seeds || program_id || bump || "ProgramDerivedAddress"
        let mut hasher = blake3::Hasher::new();

        for seed in seeds {
            hasher.update(seed);
        }

        hasher.update(program_id.as_bytes());
        hasher.update(&[bump]);
        hasher.update(b"ProgramDerivedAddress");

        let hash = hasher.finalize();
        let bytes: [u8; 32] = *hash.as_bytes();

        // Check that the derived address is off the Ed25519 curve
        // A valid PDA should NOT be a valid Ed25519 public key
        if Self::is_on_curve(&bytes) {
            return Err(ProgramError::InvalidSeeds);
        }

        Ok(Pubkey::new(bytes))
    }

    /// Check if point is on the Ed25519 curve
    ///
    /// For security, PDAs must be off-curve so they cannot be signed for directly.
    pub fn is_on_curve(bytes: &[u8; 32]) -> bool {
        // Use curve25519-dalek to check if this is a valid Edwards point
        // A valid PDA is one that is NOT on the curve
        use curve25519_dalek::edwards::CompressedEdwardsY;

        let compressed = CompressedEdwardsY::from_slice(bytes);
        match compressed {
            Ok(point) => point.decompress().is_some(),
            Err(_) => false,
        }
    }

    /// Validate seeds
    fn validate_seeds(seeds: &[&[u8]]) -> ProgramResult<()> {
        if seeds.len() > MAX_SEEDS {
            return Err(ProgramError::InvalidSeeds);
        }

        for seed in seeds {
            if seed.len() > MAX_SEED_LEN {
                return Err(ProgramError::InvalidSeeds);
            }
        }

        Ok(())
    }

    /// Derive associated token account address
    ///
    /// Standard derivation: [wallet, token_program, mint]
    pub fn derive_associated_token_address(
        wallet: &Pubkey,
        mint: &Pubkey,
        token_program: &Pubkey,
        ata_program: &Pubkey,
    ) -> ProgramResult<ProgramDerivedAddress> {
        Self::find_program_address(
            &[wallet.as_bytes(), token_program.as_bytes(), mint.as_bytes()],
            ata_program,
        )
    }

    /// Derive fee vault address with epoch+shard for sharding
    ///
    /// Avoids hot-account by spreading across epoch and shard
    pub fn derive_sharded_fee_vault(
        program_id: &Pubkey,
        epoch: u64,
        shard: u16,
    ) -> ProgramResult<ProgramDerivedAddress> {
        Self::find_program_address(
            &[b"fee_vault", &epoch.to_le_bytes(), &shard.to_le_bytes()],
            program_id,
        )
    }

    /// Derive counter address with epoch+shard for sharding
    pub fn derive_sharded_counter(
        program_id: &Pubkey,
        counter_name: &str,
        epoch: u64,
        shard: u16,
    ) -> ProgramResult<ProgramDerivedAddress> {
        Self::find_program_address(
            &[
                b"counter",
                counter_name.as_bytes(),
                &epoch.to_le_bytes(),
                &shard.to_le_bytes(),
            ],
            program_id,
        )
    }

    /// Derive user-specific PDA
    pub fn derive_user_account(
        program_id: &Pubkey,
        account_type: &str,
        user: &Pubkey,
    ) -> ProgramResult<ProgramDerivedAddress> {
        Self::find_program_address(&[account_type.as_bytes(), user.as_bytes()], program_id)
    }

    /// Derive program state PDA
    pub fn derive_program_state(program_id: &Pubkey) -> ProgramResult<ProgramDerivedAddress> {
        Self::find_program_address(&[b"state"], program_id)
    }

    /// Create canonical signing seeds for a PDA
    ///
    /// Returns seeds that can be used with invoke_signed
    pub fn create_signer_seeds<'a>(base_seeds: &'a [&'a [u8]], bump: &'a [u8; 1]) -> Vec<&'a [u8]> {
        let mut seeds: Vec<&[u8]> = base_seeds.to_vec();
        seeds.push(bump);
        seeds
    }
}

/// Builder for constructing PDA derivations
pub struct PdaBuilder<'a> {
    seeds: Vec<&'a [u8]>,
}

impl<'a> PdaBuilder<'a> {
    /// Create a new PDA builder
    pub fn new() -> Self {
        Self { seeds: Vec::new() }
    }

    /// Add a static string seed
    pub fn seed_str(mut self, s: &'a str) -> Self {
        self.seeds.push(s.as_bytes());
        self
    }

    /// Add a bytes seed
    pub fn seed_bytes(mut self, b: &'a [u8]) -> Self {
        self.seeds.push(b);
        self
    }

    /// Add a pubkey seed
    pub fn seed_pubkey(mut self, pk: &'a Pubkey) -> Self {
        self.seeds.push(pk.as_bytes());
        self
    }

    /// Derive the PDA
    pub fn derive(self, program_id: &Pubkey) -> ProgramResult<ProgramDerivedAddress> {
        PdaDerivation::find_program_address(&self.seeds, program_id)
    }
}

impl Default for PdaBuilder<'_> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pda_derivation() {
        let program_id = Pubkey::new([1u8; 32]);

        let pda = PdaDerivation::find_program_address(&[b"test", b"seed"], &program_id).unwrap();

        // PDA should be derivable
        assert!(!pda.address.is_zero());

        // Should be deterministic
        let pda2 = PdaDerivation::find_program_address(&[b"test", b"seed"], &program_id).unwrap();

        assert_eq!(pda.address, pda2.address);
        assert_eq!(pda.bump, pda2.bump);
    }

    #[test]
    fn test_pda_with_bump() {
        let program_id = Pubkey::new([1u8; 32]);

        let pda = PdaDerivation::find_program_address(&[b"test"], &program_id).unwrap();

        // Should be able to recreate with known bump
        let recreated =
            PdaDerivation::create_program_address_with_bump(&[b"test"], &program_id, pda.bump)
                .unwrap();

        assert_eq!(pda.address, recreated);
    }

    #[test]
    fn test_pda_different_seeds() {
        let program_id = Pubkey::new([1u8; 32]);

        let pda1 = PdaDerivation::find_program_address(&[b"seed1"], &program_id).unwrap();

        let pda2 = PdaDerivation::find_program_address(&[b"seed2"], &program_id).unwrap();

        assert_ne!(pda1.address, pda2.address);
    }

    #[test]
    fn test_sharded_fee_vault() {
        let program_id = Pubkey::new([5u8; 32]);

        // Different epochs and shards should produce different addresses
        let vault1 = PdaDerivation::derive_sharded_fee_vault(&program_id, 100, 0).unwrap();
        let vault2 = PdaDerivation::derive_sharded_fee_vault(&program_id, 100, 1).unwrap();
        let vault3 = PdaDerivation::derive_sharded_fee_vault(&program_id, 101, 0).unwrap();

        assert_ne!(vault1.address, vault2.address);
        assert_ne!(vault1.address, vault3.address);
        assert_ne!(vault2.address, vault3.address);
    }

    #[test]
    fn test_pda_builder() {
        let program_id = Pubkey::new([1u8; 32]);
        let user = Pubkey::new([2u8; 32]);

        let pda = PdaBuilder::new()
            .seed_str("account")
            .seed_pubkey(&user)
            .derive(&program_id)
            .unwrap();

        assert!(!pda.address.is_zero());
    }

    #[test]
    fn test_seed_validation() {
        let program_id = Pubkey::new([1u8; 32]);

        // Too many seeds
        let many_seeds: Vec<&[u8]> = (0..20).map(|_| &[1u8][..]).collect();
        let result = PdaDerivation::find_program_address(&many_seeds, &program_id);
        assert!(matches!(result, Err(ProgramError::InvalidSeeds)));

        // Seed too long
        let long_seed = [0u8; 64];
        let result = PdaDerivation::find_program_address(&[&long_seed[..]], &program_id);
        assert!(matches!(result, Err(ProgramError::InvalidSeeds)));
    }
}
