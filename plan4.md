# dchat Production Implementation Plan v4.0

> **Comprehensive plan to replace all placeholder code, simulations, mocks, and incomplete implementations with production-ready code for mainnet launch.**

**Generated**: December 11, 2025  
**Status**: Production Readiness Plan  
**Total Items**: ~146 items across 15 crates (83 "In production" comments + 63 additional issues)

---

## Table of Contents

1. [Executive Summary](#1-executive-summary)
2. [Priority Classification](#2-priority-classification)
3. [Critical Path Items (P0)](#3-critical-path-items-p0)
4. [High Priority Items (P1)](#4-high-priority-items-p1)
5. [Medium Priority Items (P2)](#5-medium-priority-items-p2)
6. [Lower Priority Items (P3)](#6-lower-priority-items-p3)
7. [Implementation Schedule](#7-implementation-schedule)
8. [Testing Requirements](#8-testing-requirements)
9. [Security Audit Checklist](#9-security-audit-checklist)
10. [Additional Code Quality Issues](#10-additional-code-quality-issues)
11. [Production Build Verification Checklist](#11-production-build-verification-checklist)
12. [Simulation/Mock Removal Summary](#12-simulationmock-removal-summary)

---

## 1. Executive Summary

### Current State

The codebase contains **83 instances** of "// In production..." comments indicating placeholder, simulated, or incomplete implementations. These MUST be replaced with production-ready code before mainnet launch.

### By Crate Distribution

| Crate             | Count | Criticality  |
| ----------------- | ----- | ------------ |
| dchat-bots        | 15    | Medium       |
| dchat-blockchain  | 12    | **Critical** |
| dchat-identity    | 10    | **Critical** |
| dchat-chain       | 8     | High         |
| dchat-messaging   | 6     | **Critical** |
| dchat-bridge      | 5     | High         |
| dchat-storage     | 4     | Medium       |
| dchat-network     | 4     | **Critical** |
| dchat-deployment  | 3     | Medium       |
| dchat-privacy     | 2     | **Critical** |
| dchat-sdk-rust    | 2     | Medium       |
| dchat-marketplace | 1     | Low          |
| dchat-validator   | 1     | High         |
| dchat-crypto      | 1     | **Critical** |
| dchat-vr          | 1     | Low          |

---

## 2. Priority Classification

### P0 - Critical (Blocks Mainnet)

Must be implemented before any mainnet launch. Security-critical or core functionality.

### P1 - High (Required for Launch)

Required for production but can use temporary workarounds initially.

### P2 - Medium (Post-Launch Acceptable)

Can be implemented after initial mainnet launch with limited functionality.

### P3 - Low (Future Enhancement)

Nice-to-have features, can be deferred to future releases.

---

## 3. Critical Path Items (P0)

### 3.1 Payment Channel On-Chain Integration

**Location**: `crates/dchat-messaging/src/message_service.rs:531, 646`

**Current State**:

```rust
// In production, this would interact with on-chain payment channel contract
// In production, this would submit the final state to the on-chain contract
```

**Required Implementation**:

```rust
// message_service.rs - Payment Channel Opening
impl MessageCreditsChannel {
    pub async fn open(
        sender: UserId,
        relay: UserId,
        credit_amount: u64,
        currency_chain: &CurrencyChainClient,
    ) -> Result<Self, CreditsChannelError> {
        // 1. Build payment channel contract transaction
        let channel_params = PaymentChannelParams {
            sender: sender.to_address(),
            relay: relay.to_address(),
            amount: credit_amount,
            timeout_blocks: 1000, // ~3 hours
        };

        // 2. Submit to currency chain
        let tx = currency_chain.build_open_channel_tx(&channel_params)?;
        let tx_hash = currency_chain.submit_transaction(tx).await?;

        // 3. Wait for confirmation (at least 2 blocks)
        currency_chain.wait_for_confirmations(&tx_hash, 2).await?;

        // 4. Get channel ID from on-chain event
        let channel_id = currency_chain.get_channel_id_from_tx(&tx_hash).await?;

        Ok(Self {
            channel_id,
            sender_id: sender,
            relay_id: relay,
            // ... rest of initialization from on-chain state
        })
    }

    pub async fn close(&self, currency_chain: &CurrencyChainClient) -> Result<String> {
        // Submit final state with both signatures
        let close_tx = currency_chain.build_close_channel_tx(
            &self.channel_id,
            self.sender_balance,
            self.relay_balance,
            &self.get_final_signatures()?,
        )?;

        let tx_hash = currency_chain.submit_transaction(close_tx).await?;
        currency_chain.wait_for_confirmations(&tx_hash, 6).await?;

        Ok(tx_hash.to_string())
    }
}
```

**Dependencies**:

- Smart contract deployment on currency chain
- RPC client integration
- Multi-signature support

**Estimated Effort**: 3-5 days

---

### 3.2 Staking Backend Chain Integration

**Location**: `crates/dchat-blockchain/src/staking_backend.rs:83, 104, 129, 170, 192`

**Current State**:

```rust
// In production, this would poll the chain for confirmation.
// Poll for confirmations (simplified - in production would actually wait)
// This is a simplified implementation - in production would handle cooldown periods
// In production, this would be more sophisticated with different slash severities
// In production, update the staked balance on-chain
```

**Required Implementation**:

```rust
// staking_backend.rs - Full Chain Integration
impl StakingBackend {
    /// Wait for stake confirmation with exponential backoff
    async fn wait_for_confirmation(
        &self,
        tx_id: &str,
        min_confirmations: u32,
    ) -> Result<bool> {
        let mut attempts = 0;
        let max_attempts = 30; // ~5 minutes with backoff

        while attempts < max_attempts {
            let tx = self.currency_chain.get_transaction_status(tx_id).await?;

            match tx.status {
                TxStatus::Confirmed { confirmations } if confirmations >= min_confirmations => {
                    return Ok(true);
                }
                TxStatus::Failed { reason } => {
                    return Err(Error::transaction(format!("Stake tx failed: {}", reason)));
                }
                TxStatus::Pending | TxStatus::Confirmed { .. } => {
                    // Exponential backoff: 1s, 2s, 4s, 8s, max 30s
                    let delay = std::cmp::min(1 << attempts, 30);
                    tokio::time::sleep(Duration::from_secs(delay)).await;
                    attempts += 1;
                }
            }
        }

        Err(Error::timeout("Confirmation timeout"))
    }

    /// Unstake with proper cooldown period
    async fn unstake(&self, operator: &UserId, stake_tx_id: &str) -> Result<String> {
        // 1. Initiate unstake (starts cooldown)
        let unstake_tx = self.currency_chain.build_unstake_tx(operator, stake_tx_id)?;
        let tx_hash = self.currency_chain.submit_transaction(unstake_tx).await?;

        // 2. Register cooldown tracker
        let cooldown_end = Utc::now() + chrono::Duration::days(UNSTAKE_COOLDOWN_DAYS);
        self.cooldown_tracker.register(operator.clone(), tx_hash.clone(), cooldown_end).await;

        Ok(tx_hash)
    }

    /// Execute pending unstakes after cooldown
    pub async fn process_cooldown_completions(&self) -> Result<Vec<String>> {
        let ready = self.cooldown_tracker.get_ready_for_completion().await;
        let mut completed = Vec::new();

        for pending in ready {
            let complete_tx = self.currency_chain.build_complete_unstake_tx(&pending.tx_hash)?;
            match self.currency_chain.submit_transaction(complete_tx).await {
                Ok(hash) => completed.push(hash),
                Err(e) => tracing::error!("Failed to complete unstake {}: {}", pending.tx_hash, e),
            }
        }

        Ok(completed)
    }

    /// Slash with severity levels
    async fn slash(
        &self,
        operator: &UserId,
        amount: u64,
        reason: &str,
        severity: SlashSeverity,
    ) -> Result<String> {
        let slash_params = SlashParams {
            operator: operator.clone(),
            amount,
            reason: reason.to_string(),
            severity,
            evidence_hash: self.compute_evidence_hash(reason),
        };

        // Submit to governance for major slashes
        if severity == SlashSeverity::Major {
            return self.submit_slash_proposal(slash_params).await;
        }

        // Direct slash for minor infractions
        let slash_tx = self.currency_chain.build_slash_tx(&slash_params)?;
        let tx_hash = self.currency_chain.submit_transaction(slash_tx).await?;

        // Emit slashing event for transparency
        self.emit_slash_event(&slash_params, &tx_hash).await;

        Ok(tx_hash)
    }
}

#[derive(Clone, Copy, PartialEq)]
pub enum SlashSeverity {
    Minor,  // 1% - Downtime
    Medium, // 5% - Repeated violations
    Major,  // 10%+ - Malicious behavior (requires governance)
}

const UNSTAKE_COOLDOWN_DAYS: i64 = 14;
```

**Estimated Effort**: 5-7 days

---

### 3.3 ZK Proof MPC Trusted Setup

**Location**: `crates/dchat-privacy/src/zk_proofs.rs:88, 334`

**Current State**:

```rust
// Round constants (simplified - in production use generated constants from script)
// SECURITY NOTE: In production deployments, these keys MUST be generated
// through a Multi-Party Computation (MPC) ceremony
```

**Required Implementation**:

```rust
// zk_proofs.rs - MPC Ceremony Integration
impl Groth16Keys {
    /// Load production keys from MPC ceremony artifacts
    ///
    /// Keys are generated via Powers of Tau ceremony with 100+ participants
    /// Ceremony transcript available at: https://dchat.network/ceremony
    pub fn load_production_keys() -> Result<Self> {
        // 1. Load ceremony artifacts
        let ceremony_hash = include_str!("../ceremony/final_hash.txt");
        let powers_of_tau = include_bytes!("../ceremony/pot_final.bin");
        let phase2_contact = include_bytes!("../ceremony/contact_circuit_final.bin");
        let phase2_reputation = include_bytes!("../ceremony/reputation_circuit_final.bin");

        // 2. Verify ceremony integrity
        let computed_hash = blake3::hash(powers_of_tau);
        if computed_hash.to_hex().as_str() != ceremony_hash.trim() {
            return Err(Error::crypto("Ceremony artifact hash mismatch - DO NOT USE"));
        }

        // 3. Deserialize proving keys
        let contact_pk = ProvingKey::deserialize_compressed(&phase2_contact[..])?;
        let reputation_pk = ProvingKey::deserialize_compressed(&phase2_reputation[..])?;

        // 4. Extract and prepare verifying keys
        let contact_vk = contact_pk.vk.clone();
        let reputation_vk = reputation_pk.vk.clone();

        Ok(Self {
            poseidon_config: get_production_poseidon_config(),
            contact_pk,
            contact_vk: contact_vk.clone(),
            contact_pvk: prepare_verifying_key(&contact_vk),
            reputation_pk,
            reputation_vk: reputation_vk.clone(),
            reputation_pvk: prepare_verifying_key(&reputation_vk),
        })
    }

    /// Verify a proof was created with production keys
    pub fn verify_with_production_keys(proof: &Proof<Bn254>, public_inputs: &[Bn254Fr]) -> Result<bool> {
        let keys = Self::load_production_keys()?;
        let pvk = prepare_verifying_key(&keys.contact_vk);

        Groth16::<Bn254>::verify_with_processed_vk(&pvk, public_inputs, proof)
            .map_err(|e| Error::crypto(format!("Proof verification failed: {:?}", e)))
    }
}

/// Poseidon constants generated from deterministic seed
fn get_production_poseidon_config() -> PoseidonConfig<Bn254Fr> {
    // Constants generated using: https://github.com/arnaucube/poseidon-rs
    // Seed: "dchat-poseidon-production-v1"
    // Full round: 8, Partial rounds: 57, Width: 3
    let full_rounds = 8;
    let partial_rounds = 57;
    let alpha = 5;

    // Load pre-computed round constants
    let round_constants = include!("../constants/poseidon_rc.rs");
    let mds_matrix = include!("../constants/poseidon_mds.rs");

    PoseidonConfig::new(full_rounds, partial_rounds, alpha, mds_matrix, round_constants)
}
```

**MPC Ceremony Requirements**:

1. Minimum 100 participants from diverse backgrounds
2. At least 10 known dchat community members
3. Ceremony coordinator publishes all transcripts
4. Random beacon from Bitcoin block hash at ceremony start
5. Final hash published to Ethereum mainnet for immutability

**Estimated Effort**: 2-3 weeks (ceremony coordination)

---

### 3.4 MPC Signer Production Architecture

**Location**: `crates/dchat-identity/src/mpc.rs:638, 649, 682, 800, 888`

**Current State**:

```rust
// SECURITY WARNING: This coordinator stores ALL private key shares, which is
// only appropriate for testing and development. In production:
// - Each signer must only hold their own share
// SECURITY: In production, each signer has ONLY their own share
// In production, each signer would only know their own share
// MpcCoordinator is not available in production builds
```

**Required Implementation**:

The current `MpcCoordinator` is correctly gated with `#[cfg(debug_assertions)]`. For production, use the `FrostCoordinator` in `mpc_frost.rs`:

```rust
// mpc_production.rs - Production MPC Interface

/// Production MPC signer that only holds its own share
pub struct ProductionMpcSigner {
    /// This signer's ID
    signer_id: SignerId,
    /// ONLY this signer's private key share (encrypted at rest)
    encrypted_share: EncryptedShare,
    /// Public information about all signers (no private data)
    signer_registry: SignerRegistry,
    /// Communication channel to other signers
    signing_network: Box<dyn SigningNetwork>,
}

impl ProductionMpcSigner {
    /// Create from encrypted share stored on this device
    pub async fn from_encrypted_storage(
        signer_id: SignerId,
        storage: &SecureStorage,
        password: &[u8],
    ) -> Result<Self> {
        let encrypted_share = storage.load_encrypted_share(&signer_id).await?;

        // Decrypt share into memory (will be zeroized on drop)
        let _decrypted = encrypted_share.decrypt_in_memory(password)?;

        // Load public signer registry from DHT or local cache
        let signer_registry = SignerRegistry::load_or_fetch().await?;

        Ok(Self {
            signer_id,
            encrypted_share,
            signer_registry,
            signing_network: Box::new(P2PSigningNetwork::new().await?),
        })
    }

    /// Participate in distributed signing ceremony
    pub async fn participate_in_signing(
        &self,
        message: &[u8],
        session_id: Uuid,
        password: &[u8],
    ) -> Result<SigningContribution> {
        // 1. Decrypt our share
        let share = self.encrypted_share.decrypt(password)?;

        // 2. Generate our commitment
        let (commitment, nonce) = self.generate_commitment(&share)?;

        // 3. Broadcast commitment to other signers
        self.signing_network.broadcast_commitment(session_id, commitment).await?;

        // 4. Wait for threshold commitments
        let commitments = self.signing_network
            .await_commitments(session_id, self.signer_registry.threshold)
            .await?;

        // 5. Generate our signature share
        let sig_share = self.generate_signature_share(&share, &nonce, message, &commitments)?;

        // 6. Broadcast signature share
        self.signing_network.broadcast_signature_share(session_id, sig_share.clone()).await?;

        // 7. Zeroize sensitive data
        share.zeroize();
        nonce.zeroize();

        Ok(sig_share)
    }
}

/// Secure share storage with encryption at rest
pub struct EncryptedShare {
    /// Encrypted share data (AES-256-GCM)
    ciphertext: Vec<u8>,
    /// Salt for key derivation
    salt: [u8; 32],
    /// Nonce for AES-GCM
    nonce: [u8; 12],
}

impl EncryptedShare {
    pub fn decrypt(&self, password: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
        use argon2::Argon2;
        use aes_gcm::{Aes256Gcm, KeyInit, aead::Aead};

        // Derive key from password
        let mut key = [0u8; 32];
        Argon2::default()
            .hash_password_into(password, &self.salt, &mut key)
            .map_err(|_| Error::crypto("Key derivation failed"))?;

        // Decrypt share
        let cipher = Aes256Gcm::new_from_slice(&key)?;
        let plaintext = cipher.decrypt(&self.nonce.into(), self.ciphertext.as_ref())?;

        // Zeroize key immediately
        key.zeroize();

        Ok(Zeroizing::new(plaintext))
    }
}
```

**Security Requirements**:

- Shares MUST be encrypted at rest with user password
- Shares MUST be zeroized immediately after use
- Network communication MUST use authenticated encryption
- Signing sessions MUST have timeouts to prevent DoS

**Estimated Effort**: 5-7 days

---

### 3.5 HTTPS Enforcement for RPC

**Location**: `crates/dchat-messaging/src/staking_verifier.rs:167, 172, 204`

**Current State**:

```rust
/// In production (non-debug) builds, the RPC URL MUST use HTTPS.
/// Returns an error in production if the URL does not use HTTPS
// In production builds, enforce HTTPS (except for localhost)
```

**Required Implementation**:

This is already partially implemented. Verify and strengthen:

```rust
// staking_verifier.rs - HTTPS Enforcement
impl StakingVerifier {
    /// Create a new staking verifier with HTTPS enforcement
    ///
    /// # Security
    /// - Production builds REQUIRE HTTPS for all non-localhost URLs
    /// - Certificate validation is always enabled
    /// - TLS 1.3 is preferred
    pub fn new(config: StakingVerifierConfig) -> Result<Self> {
        // Validate RPC URL
        let url = Url::parse(&config.rpc_url)
            .map_err(|e| Error::config(format!("Invalid RPC URL: {}", e)))?;

        // HTTPS enforcement (production builds only)
        #[cfg(not(debug_assertions))]
        {
            let is_localhost = url.host_str()
                .map(|h| h == "localhost" || h == "127.0.0.1" || h == "::1")
                .unwrap_or(false);

            if !is_localhost && url.scheme() != "https" {
                return Err(Error::security(format!(
                    "HTTPS required for RPC URL in production: {}",
                    config.rpc_url
                )));
            }
        }

        // Build HTTP client with TLS configuration
        let client = reqwest::Client::builder()
            .min_tls_version(reqwest::tls::Version::TLS_1_2)
            .danger_accept_invalid_certs(false) // Never disable cert validation
            .timeout(Duration::from_secs(30))
            .build()?;

        Ok(Self {
            rpc_url: config.rpc_url,
            client,
            cache: RwLock::new(HashMap::new()),
        })
    }
}
```

**Estimated Effort**: 1 day (verification + tests)

---

### 3.6 Noise Protocol Key Management

**Location**: `crates/dchat-network/src/gossip/protocol.rs:100, 167`

**Current State**:

```rust
/// This MUST be loaded from a persistent keystore in production
/// The signing_key MUST come from a persistent keystore in production.
```

**Required Implementation**:

```rust
// protocol.rs - Production Key Management
impl GossipProtocol {
    /// Create protocol with persistent identity from keystore
    pub fn new_with_keystore(config: GossipConfig, keystore: &Keystore) -> Result<Self> {
        // Load or generate persistent signing key
        let signing_key = keystore.load_or_create_signing_key("gossip_identity")?;

        // Derive peer ID from signing key
        let verifying_key = signing_key.verifying_key();
        let peer_id = PeerId::from_public_key(&verifying_key);

        tracing::info!("Gossip protocol initialized with peer ID: {}", peer_id);

        Ok(Self {
            config,
            signing_key,
            peer_id,
            // ... rest of initialization
        })
    }
}

/// Production keystore with encrypted storage
pub struct Keystore {
    storage_path: PathBuf,
    master_key: Zeroizing<[u8; 32]>,
}

impl Keystore {
    pub fn open(path: impl AsRef<Path>, password: &[u8]) -> Result<Self> {
        let storage_path = path.as_ref().to_path_buf();

        // Derive master key from password
        let salt = Self::load_or_create_salt(&storage_path)?;
        let master_key = Self::derive_master_key(password, &salt)?;

        Ok(Self {
            storage_path,
            master_key: Zeroizing::new(master_key),
        })
    }

    pub fn load_or_create_signing_key(&self, key_id: &str) -> Result<SigningKey> {
        let key_path = self.storage_path.join(format!("{}.key.enc", key_id));

        if key_path.exists() {
            // Load and decrypt existing key
            let encrypted = std::fs::read(&key_path)?;
            let decrypted = self.decrypt(&encrypted)?;
            let key_bytes: [u8; 32] = decrypted.as_slice().try_into()?;
            Ok(SigningKey::from_bytes(&key_bytes))
        } else {
            // Generate new key and persist
            let signing_key = SigningKey::generate(&mut OsRng);
            let encrypted = self.encrypt(signing_key.as_bytes())?;
            std::fs::write(&key_path, &encrypted)?;
            Ok(signing_key)
        }
    }
}
```

**Estimated Effort**: 3-4 days

---

## 4. High Priority Items (P1)

### 4.1 IPFS Production Integration

**Location**: `crates/dchat-storage/src/ipfs.rs:172, 184, 237, 385`

**Current State**:

```rust
// In production: Use IPFS HTTP API
// Simulate IPFS upload (in production, make actual HTTP request)
// In production: Use IPFS gateway
// In production: Create directory object with all file CIDs
```

**Required Implementation**:

```rust
// ipfs.rs - Full IPFS API Integration
impl IpfsClient {
    /// Upload file to IPFS via HTTP API
    pub async fn upload(&self, filename: String, data: Vec<u8>) -> Result<IpfsFile> {
        let url = format!("{}/api/v0/add?pin={}",
            self.config.api_url,
            self.config.enable_pinning
        );

        let form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(data.clone())
                .file_name(filename.clone())
                .mime_str(&Self::detect_mime_type(&data, &filename))?,
        );

        let response = self.http_client
            .post(&url)
            .multipart(form)
            .timeout(Duration::from_secs(60))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS upload failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::network(format!("IPFS API error: {}", error_text)));
        }

        #[derive(Deserialize)]
        struct AddResponse {
            #[serde(rename = "Hash")]
            hash: String,
            #[serde(rename = "Size")]
            size: String,
        }

        let add_result: AddResponse = response.json().await?;

        Ok(IpfsFile {
            cid: add_result.hash,
            name: filename,
            size: add_result.size.parse().unwrap_or(data.len() as u64),
            mime_type: Self::detect_mime_type(&data, &filename),
            pinned: self.config.enable_pinning,
            uploaded_at: Utc::now(),
        })
    }

    /// Download from IPFS with fallback gateways
    pub async fn download(&self, cid: &str) -> Result<Vec<u8>> {
        // Try primary gateway first
        match self.download_from_gateway(&self.config.gateway_url, cid).await {
            Ok(data) => return Ok(data),
            Err(e) => tracing::warn!("Primary gateway failed: {}", e),
        }

        // Try fallback gateways
        for gateway in &self.config.fallback_gateways {
            match self.download_from_gateway(gateway, cid).await {
                Ok(data) => return Ok(data),
                Err(e) => tracing::warn!("Fallback gateway {} failed: {}", gateway, e),
            }
        }

        Err(Error::network(format!("All IPFS gateways failed for CID: {}", cid)))
    }

    /// Create IPFS directory from multiple files
    pub async fn create_directory(&self, files: Vec<(String, Vec<u8>)>) -> Result<String> {
        // Upload all files first
        let mut file_entries = Vec::new();
        for (name, data) in files {
            let file = self.upload(name.clone(), data).await?;
            file_entries.push((name, file.cid));
        }

        // Create directory object using IPFS MFS or dag API
        let url = format!("{}/api/v0/object/new?arg=unixfs-dir", self.config.api_url);
        let response = self.http_client.post(&url).send().await?;
        let dir: serde_json::Value = response.json().await?;
        let dir_hash = dir["Hash"].as_str().unwrap_or_default();

        // Patch in each file
        let mut current_hash = dir_hash.to_string();
        for (name, cid) in file_entries {
            let patch_url = format!(
                "{}/api/v0/object/patch/add-link?arg={}&arg={}&arg={}",
                self.config.api_url, current_hash, name, cid
            );
            let response = self.http_client.post(&patch_url).send().await?;
            let patched: serde_json::Value = response.json().await?;
            current_hash = patched["Hash"].as_str().unwrap_or_default().to_string();
        }

        Ok(current_hash)
    }
}
```

**Estimated Effort**: 3-4 days

---

### 4.2 Solana Bridge Production Integration

**Location**: `crates/dchat-bridge/src/solana_bridge.rs:356, 495, 521, 538, 591`

**Current State**:

```rust
// In production, this would:
// 1. Build the mint instruction using BridgeProgram
// 2. Sign with bridge authority multi-sig
// 3. Submit to Solana
// For now, we'll simulate the signature
```

**Required Implementation**:

```rust
// solana_bridge.rs - Full Solana Integration
impl SolanaBridgeClient {
    /// Execute mint on Solana after dchat lock is confirmed
    pub async fn execute_mint(&self, transfer_id: Uuid) -> Result<String> {
        let transfer = self.get_transfer(transfer_id).await
            .ok_or_else(|| Error::not_found("Transfer not found"))?;

        // 1. Build mint instruction
        let mint_ix = self.build_mint_instruction(&transfer)?;

        // 2. Get multi-sig signatures from bridge authorities
        let signatures = self.collect_authority_signatures(&mint_ix, transfer_id).await?;

        if signatures.len() < self.config.multisig_threshold as usize {
            return Err(Error::validation(format!(
                "Insufficient signatures: {} of {} required",
                signatures.len(),
                self.config.multisig_threshold
            )));
        }

        // 3. Build transaction with all signatures
        let tx = Transaction::new_with_payer(&[mint_ix], Some(&self.fee_payer));
        let signed_tx = self.apply_multisig_signatures(tx, signatures)?;

        // 4. Submit to Solana
        let signature = self.rpc_client
            .send_and_confirm_transaction(&signed_tx)
            .await
            .map_err(|e| Error::network(format!("Solana tx failed: {}", e)))?;

        // 5. Update transfer status
        self.update_transfer_signature(transfer_id, signature.to_string()).await?;

        Ok(signature.to_string())
    }

    fn build_mint_instruction(&self, transfer: &SolanaBridgeTransfer) -> Result<Instruction> {
        let recipient = Pubkey::from_str(&transfer.solana_recipient)
            .map_err(|e| Error::validation(format!("Invalid Solana address: {}", e)))?;

        let mint = Pubkey::from_str(&self.config.wdchat_mint)?;
        let bridge_authority = Pubkey::from_str(&self.config.bridge_authority)?;

        // Get or create ATA for recipient
        let ata = get_associated_token_address(&recipient, &mint);

        Ok(Instruction::new_with_borsh(
            self.config.bridge_program_id.parse()?,
            &BridgeInstruction::Mint {
                amount: transfer.amount,
                dchat_tx_hash: transfer.dchat_lock_tx.clone().unwrap_or_default(),
            },
            vec![
                AccountMeta::new(bridge_authority, true),  // Signer
                AccountMeta::new(mint, false),
                AccountMeta::new(ata, false),
                AccountMeta::new_readonly(spl_token::id(), false),
            ],
        ))
    }

    /// Generate SPV proof for dchat transaction
    pub async fn generate_lock_proof(&self, dchat_tx_hash: &str) -> Result<LockProof> {
        // 1. Get transaction and block
        let tx = self.dchat_client.get_transaction(dchat_tx_hash).await?;
        let block = self.dchat_client.get_block(tx.block_height).await?;

        // 2. Build Merkle proof
        let merkle_proof = block.build_merkle_proof(dchat_tx_hash)?;

        // 3. Get block header chain for verification
        let header_chain = self.dchat_client
            .get_block_headers(tx.block_height, tx.block_height + 6)
            .await?;

        Ok(LockProof {
            tx_hash: dchat_tx_hash.to_string(),
            block_height: tx.block_height,
            merkle_proof,
            header_chain,
            confirmations: tx.confirmations,
        })
    }
}
```

**Estimated Effort**: 5-7 days

---

### 4.3 Device Attestation API Integration

**Location**: `crates/dchat-identity/src/attestation.rs:346, 648`

**Current State**:

```rust
// In production, call Google Play Integrity API:
// POST https://playintegrity.googleapis.com/v1/{packageName}:decodeIntegrityToken
signature: Signature::new(vec![0u8; 64]), // Would be actual signature in production
```

**Required Implementation**:

```rust
// attestation.rs - Google Play Integrity Production Integration
impl DeviceAttestor {
    /// Verify Play Integrity token via Google API
    async fn verify_play_integrity_production(&self, token: &str, nonce: &[u8]) -> Result<AttestationResult> {
        let url = format!(
            "https://playintegrity.googleapis.com/v1/{}:decodeIntegrityToken",
            self.config.package_name
        );

        #[derive(Serialize)]
        struct DecodeRequest {
            integrity_token: String,
        }

        let response = self.http_client
            .post(&url)
            .bearer_auth(&self.config.google_api_key)
            .json(&DecodeRequest { integrity_token: token.to_string() })
            .send()
            .await
            .map_err(|e| Error::network(format!("Play Integrity API failed: {}", e)))?;

        if !response.status().is_success() {
            let error_text = response.text().await.unwrap_or_default();
            return Err(Error::attestation(format!("Play Integrity API error: {}", error_text)));
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct IntegrityVerdict {
            request_details: RequestDetails,
            app_integrity: AppIntegrity,
            device_integrity: DeviceIntegrity,
            account_details: AccountDetails,
        }

        #[derive(Deserialize)]
        #[serde(rename_all = "camelCase")]
        struct DeviceIntegrity {
            device_recognition_verdict: Vec<String>,
        }

        let verdict: IntegrityVerdict = response.json().await?;

        // Verify nonce matches
        let expected_nonce = base64::engine::general_purpose::STANDARD.encode(nonce);
        if verdict.request_details.nonce != expected_nonce {
            return Err(Error::attestation("Nonce mismatch in integrity token"));
        }

        // Check device verdicts
        let verdicts = &verdict.device_integrity.device_recognition_verdict;
        let meets_device = verdicts.contains(&"MEETS_DEVICE_INTEGRITY".to_string());
        let meets_strong = verdicts.contains(&"MEETS_STRONG_INTEGRITY".to_string());

        Ok(AttestationResult {
            verified: meets_device,
            platform: AttestationPlatform::AndroidPlayIntegrity,
            device_integrity: crate::attestation::DeviceIntegrity {
                has_secure_hardware: meets_strong,
                is_genuine: meets_device,
                is_official_app: verdicts.contains(&"MEETS_BASIC_INTEGRITY".to_string()),
                integrity_passed: meets_device,
            },
            reason: if meets_device { None } else { Some("Device integrity check failed".to_string()) },
            metadata: HashMap::from([
                ("verdicts".to_string(), verdicts.join(",")),
            ]),
        })
    }
}
```

**Estimated Effort**: 2-3 days

---

### 4.4 Chain Bootstrap Production Mode

**Location**: `crates/dchat-chain/src/chain/bootstrap.rs:197, 276`

**Current State**:

```rust
// In production, this would start the chat chain consensus
// In production, this would initialize the bridge with both chain states
```

**Required Implementation**:

```rust
// bootstrap.rs - Production Chain Bootstrap
impl ChainBootstrap {
    /// Bootstrap chat chain in production mode
    pub async fn bootstrap_production(&self) -> Result<ChatChainHandle> {
        // 1. Load genesis configuration
        let genesis_config = GenesisConfig::load_production()?;

        // 2. Initialize storage backend
        let storage = StorageBackend::open_production(&self.config.data_dir)?;

        // 3. Initialize consensus engine
        let consensus = ConsensusEngine::new(
            self.config.validator_key.clone(),
            genesis_config.validator_set.clone(),
            storage.clone(),
        )?;

        // 4. Start P2P networking
        let network = ChainNetwork::new(
            self.config.listen_addr.clone(),
            genesis_config.bootstrap_peers.clone(),
        ).await?;

        // 5. Sync to head if not at genesis
        if storage.get_head_height()? < network.get_network_height().await? {
            self.sync_to_head(&storage, &network).await?;
        }

        // 6. Start consensus participation
        consensus.start_participation().await?;

        // 7. Initialize bridge coordinator
        let bridge = BridgeCoordinator::new(
            storage.clone(),
            self.currency_chain_client.clone(),
        )?;
        bridge.start_sync().await?;

        Ok(ChatChainHandle {
            consensus,
            network,
            storage,
            bridge,
        })
    }

    /// Initialize cross-chain bridge with atomic state commitment
    async fn initialize_bridge(
        &self,
        chat_chain: &ChatChainHandle,
        currency_chain: &CurrencyChainClient,
    ) -> Result<BridgeState> {
        // 1. Get state commitments from both chains
        let chat_commitment = chat_chain.storage.get_state_commitment()?;
        let currency_commitment = currency_chain.get_state_commitment().await?;

        // 2. Create bridge initialization transaction on both chains
        let bridge_init = BridgeInitialization {
            chat_chain_root: chat_commitment.root,
            currency_chain_root: currency_commitment.root,
            chat_chain_height: chat_commitment.height,
            currency_chain_height: currency_commitment.height,
            timestamp: Utc::now(),
        };

        // 3. Submit to both chains atomically (2PC)
        let chat_tx = chat_chain.submit_bridge_init(&bridge_init).await?;

        match currency_chain.submit_bridge_init(&bridge_init).await {
            Ok(currency_tx) => {
                // Commit on chat chain
                chat_chain.commit_bridge_init(&chat_tx).await?;

                Ok(BridgeState {
                    initialized_at: Utc::now(),
                    chat_init_tx: chat_tx,
                    currency_init_tx: currency_tx,
                })
            }
            Err(e) => {
                // Rollback chat chain
                chat_chain.rollback_bridge_init(&chat_tx).await?;
                Err(e)
            }
        }
    }
}
```

**Estimated Effort**: 5-7 days

---

## 5. Medium Priority Items (P2)

### 5.1 Bot Messaging Relay Network Integration

**Location**: `crates/dchat-bots/src/messaging_integration.rs` (15 instances)

**Current State**:

```rust
// In production, derive from Ed25519 using BLAKE3 KDF
// In production: Send msg1 to peer via relay network
// In production, look up peer's public key via DHT
// In production: Query blockchain for message ownership
```

**Required Implementation**:

```rust
// messaging_integration.rs - Production Relay Integration
impl BotMessagingClient {
    /// Initialize encryption session with real network transport
    pub async fn init_encryption_session_production(
        &self,
        peer_id: UserId,
        relay_network: &RelayNetwork,
        dht: &DhtClient,
    ) -> Result<()> {
        // 1. Look up peer's public key from DHT
        let peer_info = dht.lookup_user(&peer_id).await
            .ok_or_else(|| Error::not_found(format!("Peer {} not found in DHT", peer_id)))?;

        // 2. Build Noise handshake
        let builder = snow::Builder::new("Noise_XX_25519_ChaChaPoly_BLAKE2s".parse().unwrap());
        let mut handshake = builder
            .local_private_key(&self.noise_keypair.private)
            .remote_public_key(&peer_info.noise_public_key)
            .build_initiator()?;

        // 3. Send ephemeral key via relay
        let mut msg1 = vec![0u8; 48];
        let len1 = handshake.write_message(&[], &mut msg1)?;
        msg1.truncate(len1);

        let response = relay_network.send_handshake(&peer_id, &msg1).await?;

        // 4. Process response
        let mut payload = vec![0u8; 128];
        handshake.read_message(&response, &mut payload)?;

        // 5. Send final handshake message
        let mut msg3 = vec![0u8; 64];
        let len3 = handshake.write_message(&[], &mut msg3)?;
        msg3.truncate(len3);

        relay_network.send_handshake(&peer_id, &msg3).await?;

        // 6. Store transport session
        let transport = handshake.into_transport_mode()?;
        self.store_session(peer_id, transport).await;

        Ok(())
    }

    /// Query blockchain for message ownership verification
    pub async fn verify_message_ownership(
        &self,
        message_id: &str,
        claimed_owner: &UserId,
        chat_chain: &ChatChainClient,
    ) -> Result<bool> {
        // Query on-chain message registry
        let message_record = chat_chain.get_message_record(message_id).await?;

        match message_record {
            Some(record) => Ok(record.sender == *claimed_owner),
            None => Ok(false), // Message not registered on-chain
        }
    }
}
```

**Estimated Effort**: 4-5 days

---

### 5.2 Watchtower Blockchain Polling

**Location**: `crates/dchat-blockchain/src/watchtower.rs:668`

**Current State**:

```rust
// In production: poll blockchain for unilateral close events
```

**Required Implementation**:

```rust
// watchtower.rs - Production Blockchain Monitoring
impl Watchtower {
    /// Start production blockchain monitoring
    pub async fn start_monitoring(&mut self) -> Result<()> {
        let poll_interval = Duration::from_secs(self.config.poll_interval_secs);
        let mut last_block = self.get_last_processed_block().await?;

        loop {
            tokio::select! {
                _ = tokio::time::sleep(poll_interval) => {
                    match self.poll_new_blocks(last_block).await {
                        Ok(new_last) => {
                            last_block = new_last;
                        }
                        Err(e) => {
                            tracing::error!("Watchtower poll error: {}", e);
                            // Continue polling on transient errors
                        }
                    }
                }
                _ = self.shutdown.recv() => {
                    tracing::info!("Watchtower shutting down");
                    break;
                }
            }
        }

        Ok(())
    }

    async fn poll_new_blocks(&self, from_block: u64) -> Result<u64> {
        let current_block = self.blockchain_client.get_latest_block().await?;

        for block_num in from_block..=current_block {
            let block = self.blockchain_client.get_block(block_num).await?;

            for tx in &block.transactions {
                self.check_for_disputes(tx).await?;
            }
        }

        Ok(current_block)
    }

    async fn check_for_disputes(&self, tx: &Transaction) -> Result<()> {
        // Check for unilateral channel closes
        if let Some(close_event) = tx.extract_channel_close() {
            let channel_id = &close_event.channel_id;

            // Check if we have a more recent state
            if let Some(our_state) = self.state_store.get_latest_state(channel_id).await? {
                if our_state.nonce > close_event.nonce {
                    // Dispute! Submit our state
                    tracing::warn!(
                        "Disputing channel {} close: their nonce {}, our nonce {}",
                        channel_id, close_event.nonce, our_state.nonce
                    );

                    self.submit_dispute(channel_id, &our_state).await?;
                }
            }
        }

        Ok(())
    }
}
```

**Estimated Effort**: 3-4 days

---

### 5.3 Chain Synchronizer BLS Aggregation

**Location**: `crates/dchat-blockchain/src/chain_synchronizer.rs:831`

**Current State**:

```rust
// In production, this would use actual BLS aggregation via blst or similar
```

**Required Implementation**:

```rust
// chain_synchronizer.rs - BLS Signature Aggregation
use blst::min_pk::{AggregateSignature, PublicKey, Signature};

impl ChainSynchronizer {
    /// Aggregate validator signatures using BLS
    pub fn aggregate_signatures(
        &self,
        signatures: &[(ValidatorId, Vec<u8>)],
        message: &[u8],
    ) -> Result<AggregatedSignature> {
        if signatures.len() < self.config.min_validators {
            return Err(Error::validation(format!(
                "Insufficient signatures: {} of {} required",
                signatures.len(),
                self.config.min_validators
            )));
        }

        // Parse signatures
        let parsed_sigs: Vec<Signature> = signatures
            .iter()
            .map(|(_, sig_bytes)| {
                Signature::from_bytes(sig_bytes)
                    .map_err(|e| Error::crypto(format!("Invalid BLS signature: {:?}", e)))
            })
            .collect::<Result<Vec<_>>>()?;

        // Aggregate
        let mut agg_sig = AggregateSignature::from_signature(&parsed_sigs[0]);
        for sig in parsed_sigs.iter().skip(1) {
            agg_sig.add_signature(sig, true)
                .map_err(|e| Error::crypto(format!("BLS aggregation failed: {:?}", e)))?;
        }

        // Get public keys for verification
        let public_keys: Vec<PublicKey> = signatures
            .iter()
            .map(|(validator_id, _)| {
                self.validator_registry.get_bls_pubkey(validator_id)
                    .ok_or_else(|| Error::not_found(format!("Validator {} not found", validator_id)))
            })
            .collect::<Result<Vec<_>>>()?;

        // Verify aggregate signature
        let pk_refs: Vec<&PublicKey> = public_keys.iter().collect();
        let result = agg_sig.to_signature().aggregate_verify(
            true,
            &[message],
            &[],  // No DST
            &pk_refs,
            true,
        );

        if result != blst::BLST_ERROR::BLST_SUCCESS {
            return Err(Error::crypto("Aggregate signature verification failed"));
        }

        Ok(AggregatedSignature {
            signature: agg_sig.to_signature().to_bytes().to_vec(),
            signers: signatures.iter().map(|(v, _)| v.clone()).collect(),
            message_hash: blake3::hash(message).as_bytes().to_vec(),
        })
    }
}
```

**Estimated Effort**: 2-3 days

---

## 6. Lower Priority Items (P3)

### 6.1 VR Haptics Full Waveform

**Location**: `crates/dchat-vr/src/haptics.rs:63`

Can be deferred to post-launch VR feature release.

**Estimated Effort**: 1-2 days

---

### 6.2 Bot Inline Image Search

**Location**: `crates/dchat-bots/src/inline.rs:209`

Can use placeholder responses initially.

**Estimated Effort**: 1 day

---

### 6.3 Marketplace Transaction Hash

**Location**: `crates/dchat-marketplace/src/creator_economy.rs:216`

Non-critical for initial launch if marketplace is not enabled.

**Estimated Effort**: 1 day

---

## 7. Implementation Schedule

### Week 1-2: Security Critical (P0)

| Item                        | Days | Owner           |
| --------------------------- | ---- | --------------- |
| Payment Channel Integration | 5    | Blockchain Team |
| HTTPS Enforcement           | 1    | Security Team   |
| Noise Key Management        | 4    | Crypto Team     |
| MPC Production Architecture | 5    | Crypto Team     |

### Week 3-4: Chain Integration (P0)

| Item                             | Days | Owner           |
| -------------------------------- | ---- | --------------- |
| Staking Backend Full Integration | 7    | Blockchain Team |
| ZK Proof MPC Ceremony Prep       | 5    | Crypto Team     |
| Chain Bootstrap Production       | 5    | Core Team       |

### Week 5-6: External Integrations (P1)

| Item                      | Days | Owner         |
| ------------------------- | ---- | ------------- |
| IPFS Production API       | 4    | Storage Team  |
| Solana Bridge Integration | 7    | Bridge Team   |
| Device Attestation APIs   | 3    | Identity Team |

### Week 7-8: Relay & Monitoring (P2)

| Item                  | Days | Owner           |
| --------------------- | ---- | --------------- |
| Bot Relay Integration | 5    | Messaging Team  |
| Watchtower Production | 4    | Blockchain Team |
| BLS Aggregation       | 3    | Crypto Team     |

### Week 9+: Polish & Testing

| Item                 | Days | Owner         |
| -------------------- | ---- | ------------- |
| Integration Testing  | 5    | QA Team       |
| Load Testing         | 3    | DevOps        |
| Security Audit Fixes | 5    | Security Team |

---

## 8. Testing Requirements

### Unit Tests Required

- [ ] Payment channel state transitions
- [ ] Staking cooldown period enforcement
- [ ] ZK proof generation and verification with production keys
- [ ] MPC signing with isolated shares
- [ ] HTTPS URL validation
- [ ] BLS signature aggregation
- [ ] IPFS upload/download roundtrip

### Integration Tests Required

- [ ] End-to-end payment channel lifecycle
- [ ] Cross-chain bridge transfer completion
- [ ] Multi-validator consensus with BLS
- [ ] Bot message delivery through relays
- [ ] Device attestation flow (Android/iOS)

### Chaos Testing Required

- [ ] Network partition during channel close
- [ ] Validator failure during signing ceremony
- [ ] IPFS gateway unavailability
- [ ] Bridge transaction timeout recovery

---

## 9. Security Audit Checklist

### Pre-Audit Requirements

- [ ] Remove ALL debug_assertions-only test code paths
- [ ] Verify no private keys in logs
- [ ] Confirm HTTPS enforcement is active
- [ ] Validate MPC ceremony artifacts
- [ ] Review all `unsafe` blocks

### Audit Focus Areas

1. **Payment Channels**: State channel security, dispute resolution
2. **MPC Signing**: Share isolation, communication security
3. **Bridge**: Atomic execution, replay protection
4. **ZK Proofs**: Trusted setup verification, proof soundness
5. **Key Management**: Storage encryption, key derivation

### Post-Audit

- [ ] Address all Critical/High findings
- [ ] Document accepted risks for Medium findings
- [ ] Re-test all fixed vulnerabilities
- [ ] Update threat model documentation

---

## Summary

**Total Effort Estimate**: 12-16 weeks with a team of 4-6 developers

**Critical Dependencies**:

1. MPC ceremony must complete before ZK proofs go live
2. Smart contracts must be deployed before payment channels
3. Solana bridge program must be audited separately

**Risk Mitigation**:

- Start MPC ceremony coordination immediately (longest lead time)
- Deploy to testnet 4 weeks before mainnet target
- Run bug bounty program during testnet phase

---

## 10. Additional Code Quality Issues

Beyond the "// In production..." comments, the following issues were identified that need attention before mainnet.

### 10.1 Mock/Simulation Code in Production Paths

| Location                                           | Issue                                            | Action Required                                                          |
| -------------------------------------------------- | ------------------------------------------------ | ------------------------------------------------------------------------ |
| `dchat-messaging/src/staking_verifier.rs:72-151`   | `MockStakingVerifier` with hardcoded stakes      | Replace with real chain queries; currently gated by `test-mocks` feature |
| `dchat-messaging/src/delivery.rs:436-437`          | `BlockchainClient::new_mock()` in delivery tests | Ensure not reachable in production builds                                |
| `dchat-blockchain/src/client.rs:219`               | Mock RPC client for simulated responses          | Add compile-time guards                                                  |
| `dchat-network/tests/onion_routing_tests.rs:18-23` | `MockRelayKeystore`                              | Test-only, verify not linked in release                                  |
| `dchat-bridge/src/multisig.rs:536-547`             | `create_dummy_signature()` helper                | Add `#[cfg(test)]` guard                                                 |
| `dchat-bridge/src/solana_bridge.rs:362`            | Simulated mint signature                         | Implement real Solana transaction                                        |
| `dchat-storage/src/ipfs.rs:184`                    | Simulated IPFS upload                            | Implement actual HTTP API call                                           |
| `dchat-bots/src/messaging_integration.rs:210`      | Placeholder session creation                     | Implement real DHT/relay lookup                                          |

**Build Verification Required**:

```bash
# Ensure test-mocks feature cannot be enabled in release
cargo build --release --features test-mocks  # MUST FAIL
```

### 10.2 TODO/FIXME Comments Requiring Resolution

| Location                                          | TODO                                                                 | Priority                |
| ------------------------------------------------- | -------------------------------------------------------------------- | ----------------------- |
| `dchat-bots/src/bot_api.rs:246`                   | `// TODO: Parse chat_id to UserId for DMs`                           | P1 - Core functionality |
| `dchat-data/src/dedup.rs:92`                      | `// TODO: Implement proper delta computation (e.g., xdelta, bsdiff)` | P2 - Optimization       |
| `dchat-blockchain/src/block_hierarchy.rs:863-866` | `// TODO: BlockchainState type not defined - test disabled`          | P1 - Test coverage      |

### 10.3 Unused Imports with Allow Attributes

| Location                                       | Issue                      | Action                            |
| ---------------------------------------------- | -------------------------- | --------------------------------- |
| `dchat-blockchain/src/vote_persistence.rs:439` | `#[allow(unused_imports)]` | Review and remove if truly unused |

### 10.4 Hardcoded Values Requiring Configuration

| Location                                            | Hardcoded Value                | Required Change                       |
| --------------------------------------------------- | ------------------------------ | ------------------------------------- |
| `dchat-messaging/src/staking_verifier.rs:79`        | Hardcoded stakes for testing   | Load from chain in production         |
| `dchat-validator/src/validator/thresholds.rs:4`     | Hardcoded validator thresholds | Runtime-computed from validator set   |
| `dchat-deployment/src/backup_system.rs:388`         | AWS credentials pattern        | Use IAM roles in production           |
| `dchat-deployment/src/health_monitor.rs:526`        | Placeholder Slack webhook URLs | Load from environment                 |
| `dchat-deployment/src/bin/deploy-monitoring.rs:882` | Admin password handling        | Ensure loaded from secure environment |

### 10.5 Placeholder/Stub Implementations

| Location                                                        | Description                           | Production Requirement           |
| --------------------------------------------------------------- | ------------------------------------- | -------------------------------- |
| `dchat-storage/README.md:43`                                    | `tier_management.rs` marked as "mock" | Implement real tier logic        |
| `dchat-storage/README.md:51`                                    | `economics.rs` marked as "mock"       | Implement real storage economics |
| `dchat-storage/src/distributed/resilient_object_storage.rs:251` | Returns placeholder metadata          | Return real object metadata      |
| `dchat-network/src/discovery/dht_legacy.rs:331`                 | Placeholder connection address        | Implement proper peer resolution |
| `dchat-bots/src/messaging_integration.rs:374`                   | Placeholder key used                  | Derive from real identity        |

### 10.6 "For Now" Temporary Implementations

These comments indicate temporary solutions that need production implementations:

| Location                                          | Current State                               | Production State                    |
| ------------------------------------------------- | ------------------------------------------- | ----------------------------------- |
| `dchat-storage/src/ipfs.rs:444`                   | BLAKE3 hash as CID                          | Use proper IPFS multihash           |
| `dchat-sdk-rust/src/network.rs:388`               | Kademlia PutRecord for messaging            | Dedicated request-response protocol |
| `dchat-sdk-rust/src/relay.rs:553,602`             | Logging attestation, local proof validation | On-chain attestation verification   |
| `dchat-network/src/relay_network.rs:655`          | Round-robin relay selection                 | Geographic/latency-based selection  |
| `dchat-network/src/onion_routing.rs:1228`         | Store with None for circuits                | Proper circuit extension            |
| `dchat-messaging/src/message_service.rs:647`      | Direct balance transfer                     | On-chain channel settlement         |
| `dchat-identity/src/mpc.rs:763`                   | Simulated share receiving                   | Real network share collection       |
| `dchat-identity/src/sync.rs:204`                  | Simple merge strategies                     | Conflict resolution protocol        |
| `dchat-identity/src/attestation.rs:355,436`       | Basic token validation                      | Full API verification               |
| `dchat-blockchain/src/chain_synchronizer.rs:832`  | BLAKE3 for BLS aggregation                  | Real BLS via blst library           |
| `dchat-blockchain/src/currency_chain.rs:922`      | Delayed staked balance reduction            | Immediate on-chain update           |
| `dchat-blockchain/src/faucet.rs:445`              | New wallet with updated balance             | Proper transaction-based update     |
| `dchat-blockchain/src/staking_backend.rs:193,250` | Slash transaction record only               | Full on-chain slashing              |

### 10.7 Debug-Only Code That Must Not Reach Production

| Location                                      | Code                                          | Verification                          |
| --------------------------------------------- | --------------------------------------------- | ------------------------------------- |
| `dchat-messaging/src/staking_verifier.rs:221` | `#[cfg(debug_assertions)]` for HTTP allowance | Verify HTTPS enforced in release      |
| `dchat-identity/src/mpc.rs:650`               | `#[cfg(debug_assertions)]` for MpcCoordinator | Verify unavailable in release         |
| `dchat-identity/src/identity.rs:533-552`      | PoW disabled constructor                      | Ensure not callable in production     |
| `dchat-messaging/build.rs:19`                 | `test-mocks` feature guard                    | Build must fail if enabled in release |

### 10.8 Expect/Unwrap Calls Requiring Error Handling

Critical locations with `.expect()` or `.unwrap()` that could panic in production:

| Location                                             | Call                                            | Risk Level                      |
| ---------------------------------------------------- | ----------------------------------------------- | ------------------------------- |
| `dchat-crypto/src/keys.rs:86`                        | `try_generate().expect("CSPRNG failure")`       | Low - system entropy failure    |
| `dchat-bots/src/messaging_integration.rs:101`        | `.expect("Failed to generate Noise keypair")`   | Medium - should propagate error |
| `dchat-governance/src/abuse_reporting.rs:26`         | `.expect("Failed to setup ZK keys")`            | High - should fail gracefully   |
| `dchat-blockchain/src/solana/transaction.rs:251,256` | `.expect("Account should be in accounts")`      | High - needs proper error       |
| `dchat-observability/src/handshake_metrics.rs:17`    | `.expect("Failed to create handshake metrics")` | Medium - startup failure        |
| `dchat-bridge/src/lib.rs:236`                        | `.expect("Failed to create BridgeManager")`     | High - needs graceful fallback  |

### 10.9 Feature Flags Requiring Production Review

| Feature                     | Location                                             | Production Setting  |
| --------------------------- | ---------------------------------------------------- | ------------------- |
| `test-mocks`                | `dchat-messaging/Cargo.toml:9`                       | MUST be disabled    |
| `CaptchaProvider::Disabled` | `dchat-identity/src/captcha.rs:55`                   | MUST NOT be default |
| `offline_mode`              | `dchat-storage/src/economics/production_bonds.rs:54` | MUST be disabled    |

---

## 11. Production Build Verification Checklist

### Pre-Release Build Commands

```bash
# 1. Verify test-mocks cannot be enabled in release
cargo build --release --features test-mocks 2>&1 | grep -q "SECURITY ERROR"

# 2. Check for debug_assertions code
cargo build --release 2>&1 | grep -v "debug_assertions"

# 3. Verify no mock implementations linked
nm target/release/dchat | grep -i mock  # Should be empty

# 4. Check for placeholder strings in binary
strings target/release/dchat | grep -i "placeholder\|xxx\|todo" | wc -l  # Should be 0

# 5. Verify HTTPS enforcement
RUST_LOG=debug cargo run --release -- --rpc-url http://example.com 2>&1 | grep -q "HTTPS required"
```

### Cargo Deny Configuration

Ensure `deny.toml` blocks:

- Unmaintained dependencies
- Known vulnerabilities
- Prohibited licenses

```bash
cargo deny check
```

---

## 12. Simulation/Mock Removal Summary

### Total Items by Category

| Category                    | Count | Criticality |
| --------------------------- | ----- | ----------- |
| "In production..." comments | 83    | High        |
| Mock/simulation code        | 12    | High        |
| TODO/FIXME comments         | 5     | Medium      |
| Hardcoded values            | 6     | Medium      |
| Placeholder implementations | 8     | High        |
| "For now" implementations   | 18    | High        |
| Debug-only code             | 4     | Critical    |
| Risky expect/unwrap         | 7     | Medium      |
| Feature flags to review     | 3     | Critical    |

**Grand Total**: ~146 items requiring attention

### Effort Estimate Update

Adding the new findings to the original estimate:

| Category      | Original Estimate | Additional Items               | New Estimate |
| ------------- | ----------------- | ------------------------------ | ------------ |
| P0 - Critical | 6-8 weeks         | +2 weeks (debug guards, mocks) | 8-10 weeks   |
| P1 - High     | 4-5 weeks         | +1 week (hardcoded values)     | 5-6 weeks    |
| P2 - Medium   | 2-3 weeks         | +1 week (optimizations)        | 3-4 weeks    |
| P3 - Low      | 1-2 weeks         | No change                      | 1-2 weeks    |

**Updated Total**: 17-22 weeks with 4-6 developers

---

_End of Plan v4.0_
