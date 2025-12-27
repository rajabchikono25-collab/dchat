# DPL Token Minter Contract

A DPL example contract demonstrating how to mint both **regular (SPL-like) tokens** and **confidential tokens** via Cross-Program Invocation (CPI) to the native token programs.

## Overview

This contract serves as a "minter factory" that:

1. **Holds mint authority** for both regular and confidential token mints
2. **Demonstrates CPI** to native token programs (`TOKEN_PROGRAM_ID` and `CONF_TOKEN_PROGRAM_ID`)
3. **Uses PDA signing** to authorize minting operations
4. **Shows account validation** patterns for token accounts

## Architecture

```
┌─────────────────────────────────────────────────────────────────┐
│                     DPL Token Minter                            │
│  ┌──────────────────────────────────────────────────────────┐   │
│  │ MinterState (PDA)                                        │   │
│  │  - admin: Pubkey                                         │   │
│  │  - bump: u8                                              │   │
│  │  - active: bool                                          │   │
│  │  - total_regular_minted: u64                             │   │
│  │  - total_confidential_minted: u64                        │   │
│  └──────────────────────────────────────────────────────────┘   │
│                              │                                  │
│                    ┌─────────┴─────────┐                        │
│                    │                   │                        │
│                    ▼                   ▼                        │
│  ┌─────────────────────┐  ┌─────────────────────────────────┐   │
│  │  CPI: MintTo        │  │  CPI: MintToConfidential        │   │
│  │  (TOKEN_PROGRAM_ID) │  │  (CONF_TOKEN_PROGRAM_ID)        │   │
│  │                     │  │                                 │   │
│  │  - mint             │  │  - mint                         │   │
│  │  - destination      │  │  - destination                  │   │
│  │  - authority (PDA)  │  │  - authority (PDA)              │   │
│  │  - amount           │  │  - amount                       │   │
│  └─────────────────────┘  │  - new_commitment               │   │
│                           │  - encrypted_balance            │   │
│                           │  - range_proof                  │   │
│                           └─────────────────────────────────┘   │
└─────────────────────────────────────────────────────────────────┘
```

## Instructions

### 1. Initialize

Create a new minter authority PDA controlled by an admin.

```rust
pub fn initialize(ctx: Context<Initialize>) -> Result<()>
```

**Accounts:**

- `minter`: The minter PDA (init, writable)
- `admin`: The admin signer
- `system_program`: System program

### 2. Mint Regular Tokens

Mint regular (public) tokens via CPI to the native token program.

```rust
pub fn mint_regular(ctx: Context<MintRegular>, amount: u64) -> Result<()>
```

**Accounts:**

- `minter`: The minter PDA (writable)
- `mint`: The token mint (writable)
- `destination`: Destination token account (writable)
- `admin`: Admin signer
- `token_program`: Native TOKEN_PROGRAM_ID

### 3. Mint Confidential Tokens

Mint confidential tokens via CPI to the native confidential token program.

```rust
pub fn mint_confidential(
    ctx: Context<MintConfidential>,
    amount: u64,
    new_dest_commitment: [u8; 32],
    new_dest_encrypted: Vec<u8>,
    proof_data: MintProofData,
) -> Result<()>
```

**Accounts:**

- `minter`: The minter PDA (writable)
- `mint`: The confidential mint (writable)
- `destination`: Destination confidential token account (writable)
- `admin`: Admin signer
- `conf_token_program`: Native CONF_TOKEN_PROGRAM_ID

**Proof Data:**

```rust
pub struct MintProofData {
    pub range_proof: Vec<u8>,         // Range proof for minted amount
    pub expected_mint_nonce: u64,     // For replay protection
    pub expected_dest_nonce: u64,     // For transaction binding
    pub recent_blockhash: [u8; 32],   // For freshness
}
```

### 4. Deactivate

Deactivate the minter (admin only). No more tokens can be minted.

```rust
pub fn deactivate(ctx: Context<AdminOnly>) -> Result<()>
```

### 5. Transfer Admin

Transfer admin authority to a new admin.

```rust
pub fn transfer_admin(ctx: Context<AdminOnly>, new_admin: Pubkey) -> Result<()>
```

## Events

| Event                      | Description                                 |
| -------------------------- | ------------------------------------------- |
| `MinterInitialized`        | Emitted when a minter is created            |
| `RegularMintExecuted`      | Emitted when regular tokens are minted      |
| `ConfidentialMintExecuted` | Emitted when confidential tokens are minted |
| `MinterDeactivated`        | Emitted when a minter is deactivated        |
| `AdminTransferred`         | Emitted when admin authority is transferred |

## Error Codes

| Code | Name                      | Description                 |
| ---- | ------------------------- | --------------------------- |
| 6000 | `Unauthorized`            | Caller is not the admin     |
| 6001 | `MinterInactive`          | Minter has been deactivated |
| 6002 | `ZeroAmount`              | Mint amount must be > 0     |
| 6003 | `AmountTooLarge`          | Mint amount exceeds maximum |
| 6004 | `InvalidEncryptedBalance` | Encrypted balance too large |
| 6005 | `InvalidProofData`        | Proof data is invalid       |
| 6006 | `Overflow`                | Arithmetic overflow         |
| 6007 | `CpiFailure`              | CPI to token program failed |

## Example Usage

### TypeScript Client

```typescript
import { TokenMinter } from "./idl/token_minter";

// Initialize a minter
const [minterPDA, bump] = PublicKey.findProgramAddressSync(
  [Buffer.from("minter_authority"), wallet.publicKey.toBuffer()],
  programId,
);

await program.methods
  .initialize()
  .accounts({
    minter: minterPDA,
    admin: wallet.publicKey,
    systemProgram: SystemProgram.programId,
  })
  .rpc();

// Mint regular tokens
await program.methods
  .mintRegular(new BN(1_000_000))
  .accounts({
    minter: minterPDA,
    mint: tokenMint,
    destination: tokenAccount,
    admin: wallet.publicKey,
    tokenProgram: TOKEN_PROGRAM_ID,
  })
  .rpc();

// Mint confidential tokens (with proof generation)
const commitment = computeNewCommitment(currentBalance, mintAmount);
const encrypted = encryptBalance(newBalance, ownerElGamalKey);
const proofData = generateMintProof(mintAmount, commitment, nonces, blockhash);

await program.methods
  .mintConfidential(new BN(1_000_000), commitment, encrypted, proofData)
  .accounts({
    minter: minterPDA,
    mint: confidentialMint,
    destination: confidentialTokenAccount,
    admin: wallet.publicKey,
    confTokenProgram: CONF_TOKEN_PROGRAM_ID,
  })
  .rpc();
```

## Building

```bash
# Build for wasm32-wasip1
cargo build --target wasm32-wasip1 --release

# Run tests
cargo test
```

## Key Concepts Demonstrated

### 1. Cross-Program Invocation (CPI)

This contract shows how to invoke native token programs:

```rust
// Build the instruction
let mint_to_ix = build_mint_to_instruction(mint, destination, authority, amount);

// Invoke with PDA signer
CrossProgramInvocation::invoke_signed(&ctx, &mint_to_ix, &[signer_seeds])?;
```

### 2. PDA Signing

The minter PDA acts as the mint authority and can sign transactions:

```rust
let signer_seeds: &[&[u8]] = &[
    MINTER_AUTHORITY_SEED,
    &admin_bytes,
    &[bump],
];
```

### 3. Confidential Token Proofs

Confidential minting requires cryptographic proofs:

- **Range Proof**: Proves the minted amount is in valid range [0, 2^64)
- **Transaction Binding**: Nonces and blockhash prevent replay attacks
- **Commitment Update**: New Pedersen commitment for recipient's balance

### 4. Account Validation

DPL macros provide declarative account validation:

```rust
#[derive(Accounts)]
pub struct MintRegular<'info> {
    #[account(mut)]
    pub minter: Account<'info, MinterState>,

    #[account(signer)]
    pub admin: Signer<'info>,

    // ... other accounts
}
```

## Security Considerations

1. **Authority Verification**: Always verify the minter's admin matches the signer
2. **Active Check**: Ensure the minter is active before minting
3. **Amount Limits**: Enforce maximum mint amounts to prevent accidents
4. **Proof Validation**: For confidential mints, validate all proof data sizes
5. **Nonce Tracking**: Use nonces to prevent replay attacks

## License

MIT
