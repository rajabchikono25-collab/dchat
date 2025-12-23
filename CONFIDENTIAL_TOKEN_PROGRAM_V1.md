# Confidential Token Program v1 (Currency Chain)

## Why tokens (the “web3 surface area” problem)

A single native currency (DCHAT) is not enough to support a healthy on-chain application ecosystem.

Tokens are the core primitive that enables:

- **Mini-app economies**: in-app credits, points, memberships, subscriptions, tipping, paywalls.
- **Bots & automation**: pay-per-action, escrowed bounties, anti-spam deposits, rewards.
- **DeFi building blocks**: collateral assets, stable-value units, governance tokens, LP shares.
- **Issuer identity + trust**: a mint is a public on-chain object that wallets/indexers can identify, list, and verify.

Solana’s SPL tokens prove this: they create a permissionless surface where each app can define its own asset without requiring L1 changes.

## What’s different on dchat’s currency chain

dchat’s currency chain is designed around:

- **Solana-like accounts + explicit account metas** (parallel scheduling, no hot global accounts)
- **Deterministic execution** (copy-in/copy-out VM model)
- **ZK privacy scope**: **amounts/balances are hidden**, while **identities (accounts) remain public**

This means the token standard must preserve SPL-like composability while replacing “balance is a u64” with:

- a **commitment** to the balance
- an **encrypted balance** (or encrypted delta) for the owner’s wallet
- a **ZK proof** for any mutation that changes balances

This doc specifies a practical “Confidential SPL-like” token program that supports web3 use-cases without requiring full anonymous transfers.

## Goals / Non-goals

### Goals

- SPL-like semantics: **mint**, **ATA**, **transfer**, **burn**, **freeze/thaw**, **set authority**
- Privacy: hide **amounts and balances**; keep **owners and token accounts public**
- Parallel-friendly: each transfer touches only the involved token accounts (and mint readonly)
- Deterministic, metered verification (bounded sizes, pinned verifier)
- Composable: supports CPI-style calls and program-derived authorities

### Non-goals (v1)

- Full anonymous sender/receiver
- Fully private DeFi (private AMM pricing/volumes). This can be built later with dedicated circuits, but v1 focuses on confidential payments + app economies.

## Programs and IDs

### Programs

- **CONF_TOKEN**: Confidential Token Program (new standard program)
- **ATA**: Associated Token Account program (extended to support confidential token program)

### Program IDs

Program IDs are assigned by the chain (genesis / loader) and treated as protocol constants. This spec intentionally does not hardcode a base58 string in the document.

## State model (accounts)

All state is held in accounts. Programs are stateless aside from reading/writing passed accounts.

### Account: ConfidentialMint

Purpose: defines the mint configuration and authorities.

Fields (conceptual):

- `decimals: u8`
- `mint_authority: Option<Pubkey>`
- `freeze_authority: Option<Pubkey>`
- `supply_public: u64` (optional; may be public for audit/indexing)
- `mint_nonce: u64` (replay safety; binds proofs for mint)
- `commitment_params_hash: [u8; 32]` (domain separation / pinned parameters)

### Account: ConfidentialTokenAccount

Purpose: SPL-like token account, but balance is not plaintext.

Fields (conceptual):

- `mint: Pubkey`
- `owner: Pubkey`
- `state: Active | Frozen`
- `balance_commitment: [u8; 32]`
- `enc_balance_owner: bytes` (bounded; wallet decrypts)
- `account_nonce: u64` (increments on every mutation; proof must bind)

### PDA conventions (parallel-friendly)

- Associated token account derivation:
  - `ATA(owner, CONF_TOKEN_PROGRAM_ID, mint)`
- Avoid hot accounts:
  - no global vault updated per transfer
  - if fees must be collected, use sharded vaults: `FeeVault(epoch, shard)`

## Instruction set (SPL-like)

### InitializeMint

Creates/configures a mint.

Accounts:

- `mint` (writable)
- `payer` (signer)
- `system_program` (readonly)

Checks:

- account ownership, rent/space, authority validity

### InitializeAccount / CreateAssociatedTokenAccount (ATA program)

Creates a ConfidentialTokenAccount for (owner, mint).

Accounts:

- `payer` (signer, writable)
- `ata` (writable)
- `owner` (readonly)
- `mint` (readonly)
- `system_program` (readonly)

### TransferConfidential

Moves an **amount-hidden** value from sender token account to receiver token account.

Accounts (minimal parallel set):

- `sender_token` (writable)
- `receiver_token` (writable)
- `owner` (readonly signer) — proves the sender is authorized
- `mint` (readonly)

Inputs:

- `proof_bytes` (bounded)
- `public_inputs` (bounded)
- `enc_delta_sender?` (bounded; wallet UX)
- `enc_delta_receiver?` (bounded; wallet UX)

Invariants:

- no plaintext balances on-chain
- commitment transitions verified by ZK
- nonce increments are proof-bound

### MintToConfidential

Adds new confidential balance to a destination account.

Accounts:

- `mint` (writable)
- `dest_token` (writable)
- `mint_authority` (signer or PDA authority)

### BurnConfidential

Removes confidential balance from an owner’s account.

Accounts:

- `mint` (writable, optional if tracking public supply)
- `owner_token` (writable)
- `owner` (readonly signer)

### FreezeAccount / ThawAccount

Authority-based; does not require ZK.

Accounts:

- `token_account` (writable)
- `mint` (readonly)
- `freeze_authority` (readonly signer or PDA authority)

### SetAuthority

Changes mint or freeze authorities.

Accounts:

- `mint` (writable)
- `current_authority` (signer)

## ZK: privacy scope and proof binding

### What must be public

- token account pubkeys (sender/receiver accounts)
- owner pubkeys
- mint id
- account state (frozen/active)

### What must be hidden

- transfer amount
- per-account balances

### Required public inputs (v1 baseline)

Proofs must bind to:

- mint id
- sender token account pubkey
- receiver token account pubkey
- pre-commitments (sender/receiver)
- post-commitments (sender/receiver)
- sender/receiver account_nonce
- a transaction-binding value (e.g., recent block hash + intent hash)

This prevents:

- replay across blocks
- replay across accounts
- malleability where proof is reused for a different account pair

## Metering and determinism requirements

- hard caps: `max_proof_bytes`, `max_public_inputs_bytes`, `max_ciphertext_bytes`, `max_batch_size`
- verification is deterministic and pinned (same result on all nodes)
- strict account metas: readonly accounts cannot be mutated
- copy-in/copy-out execution model: only declared writable accounts may change

## How this enables web3 on dchat (concrete examples)

- **Mini-app token**: app issues a mint and uses `TransferConfidential` for in-app payments while keeping user balances private.
- **Token-gated channels**: channel access can require holding a given mint (ownership public, amount hidden).
- **Bots**: a bot can require a deposit in a mint before executing actions; deposits and payouts use confidential transfers.
- **Programmatic minting**: an issuer program can hold mint authority as a PDA and mint rewards on CPI.

## DeFi note (important reality check)

Confidential balances are compatible with many app patterns (payments, gating, rewards, escrow).

However, **price discovery mechanisms** (AMMs, orderbooks) often need amounts to compute prices. Fully private DeFi is possible but requires additional circuits and carefully designed public inputs (e.g., revealing prices while hiding user balances).

Recommendation:

- v1 ships confidential SPL-like tokens (enables the web3 ecosystem primitives)
- DeFi programs can start with:
  - public-amount tokens, or
  - hybrid disclosure (public amounts in pools, private in wallets)
  - then graduate to dedicated confidential swap/lending circuits

## References in this repo

- Solana-like account/passing + determinism notes: [crates/solana.md](crates/solana.md)
- Currency chain client surface (current): [crates/dchat-blockchain/src/currency_chain.rs](crates/dchat-blockchain/src/currency_chain.rs)
- Existing SPL token integration (bridge-side, not currency-chain native): [crates/dchat-blockchain/src/solana/spl_token.rs](crates/dchat-blockchain/src/solana/spl_token.rs)
