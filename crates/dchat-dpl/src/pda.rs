//! Program Derived Address (PDA) utilities

use crate::account::Pubkey;

/// Maximum number of seeds for PDA derivation
pub const MAX_SEEDS: usize = 16;

/// Maximum seed length
pub const MAX_SEED_LEN: usize = 32;

/// Derive a PDA from seeds and program ID
pub fn derive_pda(seeds: &[&[u8]], program_id: &Pubkey) -> Option<(Pubkey, u8)> {
    // Try bumps from 255 down to 0
    for bump in (0..=255u8).rev() {
        if let Some(pubkey) = try_derive_pda(seeds, bump, program_id) {
            return Some((pubkey, bump));
        }
    }
    None
}

/// Try to derive a PDA with a specific bump
pub fn try_derive_pda(seeds: &[&[u8]], bump: u8, program_id: &Pubkey) -> Option<Pubkey> {
    // Validate seeds
    if seeds.len() > MAX_SEEDS {
        return None;
    }

    for seed in seeds {
        if seed.len() > MAX_SEED_LEN {
            return None;
        }
    }

    // Construct the input for hashing
    let mut hasher = blake3::Hasher::new();

    for seed in seeds {
        hasher.update(seed);
    }
    hasher.update(&[bump]);
    hasher.update(program_id.as_ref());
    hasher.update(b"ProgramDerivedAddress");

    let hash = hasher.finalize();
    let bytes: [u8; 32] = *hash.as_bytes();

    // Check if the result is a valid PDA (not on the ed25519 curve)
    // In a real implementation, this would check curve membership
    // For now, we assume any hash result is valid
    if is_valid_pda(&bytes) {
        Some(Pubkey::new(bytes))
    } else {
        None
    }
}

/// Check if bytes represent a valid PDA (off-curve point)
fn is_valid_pda(_bytes: &[u8; 32]) -> bool {
    // In production, this checks if the point is NOT on the ed25519 curve
    // For simplicity, we return true (assume off-curve)
    true
}

/// Create a PDA with known bump (for efficiency when bump is already known)
pub fn create_pda_with_bump(seeds: &[&[u8]], bump: u8, program_id: &Pubkey) -> Option<Pubkey> {
    try_derive_pda(seeds, bump, program_id)
}
