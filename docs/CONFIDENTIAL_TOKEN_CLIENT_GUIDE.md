# Confidential Token Client Guide

## Overview

This document describes how clients generate proofs for confidential token operations in the dchat token system. All proofs are generated client-side and verified on-chain by the runtime.

## Program ID

```rust
pub const CONF_TOKEN_PROGRAM_ID: Pubkey = [
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 7,
];
```

## Account Structures

### ConfidentialMint (150 bytes)

| Field                    | Size | Description                         |
| ------------------------ | ---- | ----------------------------------- |
| `is_initialized`         | 1    | Boolean flag                        |
| `decimals`               | 1    | Token decimals (0-9)                |
| `mint_authority`         | 32   | Ed25519 public key                  |
| `freeze_authority`       | 33   | Optional + 32 bytes                 |
| `supply_commitment`      | 32   | Pedersen commitment to total supply |
| `mint_nonce`             | 8    | Anti-replay counter                 |
| `commitment_params_hash` | 32   | BLAKE3 hash of commitment params    |
| `padding`                | 11   | Reserved for future use             |

### ConfidentialTokenAccount (250 bytes)

| Field                | Size | Description                             |
| -------------------- | ---- | --------------------------------------- |
| `is_initialized`     | 1    | Boolean flag                            |
| `mint`               | 32   | Associated mint pubkey                  |
| `owner`              | 32   | Account owner pubkey                    |
| `state`              | 1    | 0=Uninitialized, 1=Active, 2=Frozen     |
| `balance_commitment` | 32   | Pedersen commitment C = vG + rH         |
| `pending_balance_lo` | 32   | Pending balance (low 64 bits)           |
| `pending_balance_hi` | 32   | Pending balance (high 64 bits)          |
| `enc_balance`        | 64   | ChaCha20Poly1305 encrypted balance      |
| `account_nonce`      | 8    | Per-account nonce for replay protection |
| `padding`            | 16   | Reserved for future use                 |

## Deriving Confidential ATAs

```rust
use sha2::{Sha256, Digest};

pub fn derive_confidential_ata(
    owner: &[u8; 32],
    mint: &[u8; 32],
    program_id: &[u8; 32],
) -> ([u8; 32], u8) {
    let seeds: &[&[u8]] = &[
        b"conf_ata",
        owner,
        mint,
    ];

    // Try bump values 255 down to 0
    for bump in (0..=255u8).rev() {
        let mut hasher = Sha256::new();
        for seed in seeds {
            hasher.update(seed);
        }
        hasher.update(&[bump]);
        hasher.update(program_id);
        hasher.update(b"ProgramDerivedAddress");

        let hash = hasher.finalize();
        let mut address = [0u8; 32];
        address.copy_from_slice(&hash);

        // Verify address is off the Ed25519 curve (valid PDA)
        // Real implementation uses ed25519_dalek::CompressedEdwardsY::decompress()
        // to check if the point is NOT on the curve
        if !is_on_ed25519_curve(&address) {
            return (address, bump);
        }
    }
    panic!("No valid PDA found");
}

/// Check if a 32-byte address lies on the Ed25519 curve.
/// PDAs must NOT be on the curve (to prevent signing).
fn is_on_ed25519_curve(address: &[u8; 32]) -> bool {
    use ed25519_dalek::CompressedEdwardsY;
    let compressed = CompressedEdwardsY::from_slice(address).unwrap();
    compressed.decompress().is_some()
}
```

## Proof Generation

### 1. Creating Balance Commitments

Balance commitments use Pedersen commitments: `C = v*G + r*H`

```rust
use curve25519_dalek::{ristretto::RistrettoPoint, scalar::Scalar};
use rand::rngs::OsRng;

pub struct BalanceCommitment {
    pub commitment: RistrettoPoint,
    pub value: u64,
    pub blinding: Scalar,
}

impl BalanceCommitment {
    pub fn new(value: u64, g: &RistrettoPoint, h: &RistrettoPoint) -> Self {
        let blinding = Scalar::random(&mut OsRng);
        let commitment = g * Scalar::from(value) + h * blinding;

        Self {
            commitment,
            value,
            blinding,
        }
    }
}
```

### 2. Generating Range Proofs

Range proofs prove that a committed value lies in [0, 2^64) without revealing the value.

```rust
use bulletproofs::{BulletproofGens, PedersenGens, RangeProof};
use merlin::Transcript;

pub fn create_range_proof(
    value: u64,
    blinding: &Scalar,
) -> (RangeProof, RistrettoPoint) {
    let pc_gens = PedersenGens::default();
    let bp_gens = BulletproofGens::new(64, 1);

    let mut transcript = Transcript::new(b"dchat-range-proof");

    let (proof, commitment) = RangeProof::prove_single(
        &bp_gens,
        &pc_gens,
        &mut transcript,
        value,
        blinding,
        64, // 64-bit range
    ).expect("proof generation failed");

    (proof, commitment)
}

pub fn verify_range_proof(
    proof: &RangeProof,
    commitment: &RistrettoPoint,
) -> bool {
    let pc_gens = PedersenGens::default();
    let bp_gens = BulletproofGens::new(64, 1);

    let mut transcript = Transcript::new(b"dchat-range-proof");

    proof.verify_single(
        &bp_gens,
        &pc_gens,
        &mut transcript,
        &commitment.compress(),
        64,
    ).is_ok()
}
```

### 3. Building Transfer Proofs

Transfer proofs include:

- **Commitment Proof**: `C_new = C_old - amount` relationship
- **Range Proof**: sender has sufficient balance
- **Transaction Binding**: proof is bound to specific transaction

```rust
pub struct TransferProofData {
    pub sender_nonce: u64,
    pub recipient_nonce: u64,
    pub recent_blockhash: [u8; 32],
    pub tx_signature: [u8; 64],
    pub amount_commitment: [u8; 32],
    pub sender_new_balance_commitment: [u8; 32],
    pub recipient_new_balance_commitment: [u8; 32],
    pub equality_proof: Vec<u8>,
    pub range_proof: Vec<u8>,
}

impl TransferProofData {
    pub const MAX_SIZE: usize = 2048;

    pub fn create(
        amount: u64,
        sender_balance: u64,
        sender_blinding: &Scalar,
        sender_nonce: u64,
        recipient_nonce: u64,
        recent_blockhash: [u8; 32],
        tx_signature: [u8; 64],
    ) -> Result<Self, ProofError> {
        let pc_gens = PedersenGens::default();
        let bp_gens = BulletproofGens::new(64, 1);

        // 1. Commit to transfer amount
        let amount_blinding = Scalar::random(&mut OsRng);
        let amount_commitment = pc_gens.commit(Scalar::from(amount), amount_blinding);

        // 2. Compute new sender balance
        let new_sender_balance = sender_balance.checked_sub(amount)
            .ok_or(ProofError::InsufficientBalance)?;
        let new_sender_blinding = sender_blinding - amount_blinding;

        // 3. Generate range proof for new sender balance
        let mut transcript = Transcript::new(b"dchat-transfer-range");
        let (range_proof, _) = RangeProof::prove_single(
            &bp_gens,
            &pc_gens,
            &mut transcript,
            new_sender_balance,
            &new_sender_blinding,
            64,
        )?;

        // 4. Create equality proof (C_sender_new = C_sender_old - C_amount)
        let equality_proof = create_equality_proof(/* ... */);

        Ok(Self {
            sender_nonce,
            recipient_nonce,
            recent_blockhash,
            tx_signature,
            amount_commitment: amount_commitment.compress().to_bytes(),
            sender_new_balance_commitment: /* ... */,
            recipient_new_balance_commitment: /* ... */,
            equality_proof,
            range_proof: range_proof.to_bytes(),
        })
    }
}
```

## Instructions

### InitializeMint

```rust
let ix = ConfidentialTokenInstruction::InitializeMint {
    decimals: 9,
    mint_authority,
    freeze_authority: Some(freeze_auth),
    commitment_params_hash,
};
```

### InitializeAccount

```rust
let ix = ConfidentialTokenInstruction::InitializeAccount {
    owner,
};
```

### ConfidentialTransfer

```rust
let proof_data = TransferProofData::create(
    amount,
    sender_balance,
    &sender_blinding,
    sender_nonce,
    recipient_nonce,
    recent_blockhash,
    tx_signature,
)?;

let ix = ConfidentialTokenInstruction::ConfidentialTransfer {
    proof_data,
};
```

### ApplyPendingBalance

After receiving a transfer, recipients must apply the pending balance:

```rust
let ix = ConfidentialTokenInstruction::ApplyPendingBalance {
    expected_pending_balance_commitment,
    new_decryptable_balance,
};
```

## Encrypted Balance Payload

Balances are encrypted for owner visibility using ChaCha20Poly1305:

```rust
use chacha20poly1305::{ChaCha20Poly1305, Key, Nonce};
use chacha20poly1305::aead::Aead;

pub fn encrypt_balance(
    balance: u64,
    key: &[u8; 32],
    nonce: &[u8; 12],
) -> [u8; 64] {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));
    let plaintext = balance.to_le_bytes();

    let ciphertext = cipher.encrypt(
        Nonce::from_slice(nonce),
        plaintext.as_ref(),
    ).expect("encryption failed");

    let mut result = [0u8; 64];
    result[..ciphertext.len()].copy_from_slice(&ciphertext);
    result
}

pub fn decrypt_balance(
    ciphertext: &[u8; 64],
    key: &[u8; 32],
    nonce: &[u8; 12],
) -> Result<u64, DecryptError> {
    let cipher = ChaCha20Poly1305::new(Key::from_slice(key));

    let plaintext = cipher.decrypt(
        Nonce::from_slice(nonce),
        &ciphertext[..24], // 8 bytes plaintext + 16 bytes tag
    )?;

    let mut bytes = [0u8; 8];
    bytes.copy_from_slice(&plaintext);
    Ok(u64::from_le_bytes(bytes))
}

pub fn derive_decryption_key(
    owner_secret: &[u8; 32],
    mint: &[u8; 32],
) -> [u8; 32] {
    let mut hasher = blake3::Hasher::new();
    hasher.update(b"dchat-conf-token-key");
    hasher.update(owner_secret);
    hasher.update(mint);
    *hasher.finalize().as_bytes()
}
```

## Nonce Management

Each account maintains a nonce for replay protection:

1. **Mint Nonce**: Incremented on each mint operation
2. **Account Nonce**: Incremented on each transfer (sender and recipient)

The on-chain processor validates:

- `proof.sender_nonce == sender_account.account_nonce`
- `proof.recipient_nonce == recipient_account.account_nonce`

If validation fails, the transaction is rejected with `InvalidNonce` error.

## Transaction Binding

Proofs are bound to specific transactions via:

1. **recent_blockhash**: Must match the transaction's blockhash
2. **tx_signature**: Must match the transaction signature

This prevents proof replay across different transactions.

## Error Codes

| Code | Name                | Description                          |
| ---- | ------------------- | ------------------------------------ |
| 90   | InvalidAccountState | Account in wrong state for operation |
| 91   | InvalidProof        | ZK proof verification failed         |
| 92   | InvalidCommitment   | Commitment verification failed       |
| 95   | InvalidNonce        | Nonce mismatch (replay attempt)      |

## Security Considerations

1. **Keep blinding factors secret**: Store securely alongside balance values
2. **Never reuse nonces**: Always use fresh nonces for ChaCha20Poly1305
3. **Verify before signing**: Always verify proofs locally before submitting
4. **Backup encrypted balances**: Without the decryption key, balances are unrecoverable

## Example: Full Transfer Flow

```rust
// 1. Fetch current account states
let sender_account = fetch_account(&sender_ata)?;
let recipient_account = fetch_account(&recipient_ata)?;

// 2. Verify sender has sufficient balance (using local state)
let sender_balance = decrypt_balance(
    &sender_account.enc_balance,
    &derive_decryption_key(&sender_secret, &mint),
    &generate_nonce(&sender_ata, sender_account.account_nonce),
)?;
assert!(sender_balance >= amount);

// 3. Generate transfer proof
let proof_data = TransferProofData::create(
    amount,
    sender_balance,
    &sender_blinding,
    sender_account.account_nonce,
    recipient_account.account_nonce,
    recent_blockhash,
    tx_signature,
)?;

// 4. Build and sign transaction
let ix = ConfidentialTokenInstruction::ConfidentialTransfer { proof_data };
let tx = Transaction::new(&[sender_keypair], &[ix], recent_blockhash);

// 5. Submit transaction
submit_transaction(tx)?;

// 6. Update local state
let new_sender_balance = sender_balance - amount;
let new_enc_balance = encrypt_balance(
    new_sender_balance,
    &derive_decryption_key(&sender_secret, &mint),
    &generate_nonce(&sender_ata, sender_account.account_nonce + 1),
);
```

## Testing

Run the confidential token test suite:

```bash
cd crates/dchat-programs
cargo test confidential_token --features full
```

All 24 tests should pass, covering:

- Serialization/deserialization
- Initialization flows
- Authority management
- Freeze/thaw operations
- Event generation
- Encrypted balance operations
- Proof data validation
