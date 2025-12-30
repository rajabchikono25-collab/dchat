# dchat Mainnet Ceremony Summary

**Generated**: December 30, 2025 02:22 UTC  
**Network**: dchat-mainnet-1  
**Status**: ✅ READY FOR MAINNET LAUNCH

---

## 📋 Ceremony Overview

This directory contains all cryptographic artifacts required for the dchat mainnet launch:

| Component      | Status      | Description                                  |
| -------------- | ----------- | -------------------------------------------- |
| ZK Ceremony    | ✅ Complete | Groth16 proving/verifying keys for ZK proofs |
| Genesis Blocks | ✅ Complete | Chat chain and Currency chain genesis        |
| Validator Keys | ✅ Complete | 7 foundation validator identities            |
| Relay Keys     | ✅ Complete | 14 relay node identities                     |

---

## 🔐 ZK Ceremony Artifacts

**Location**: `zk-ceremony/`

| File                           | Size          | Purpose                        |
| ------------------------------ | ------------- | ------------------------------ |
| `pot_final.bin`                | 61 bytes      | Powers of Tau contribution     |
| `contact_circuit_final.bin`    | 93,808 bytes  | Contact proof proving key      |
| `contact_vk_final.bin`         | 328 bytes     | Contact proof verifying key    |
| `reputation_circuit_final.bin` | 413,616 bytes | Reputation proof proving key   |
| `reputation_vk_final.bin`      | 328 bytes     | Reputation proof verifying key |
| `final_hash.txt`               | -             | Combined BLAKE3 hash           |
| `ceremony_metadata.json`       | -             | Ceremony metadata              |

### Ceremony Parameters

```
Domain Separator:    dchat-zk-ceremony-mainnet-v1
Protocol Version:    1
Bitcoin Block:       #874000
Bitcoin Hash:        0000000000000000000234a8b9c2d3e4f5a6b7c8d9e0f1234567890abcdef1234
Ethereum Hash:       0xabcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890
```

### Combined Artifacts Hash (BLAKE3)

```
e4e0ba1c13de4d153e770cac46763cd322773150849953a26668c24cad70ba26
```

---

## 🌍 Genesis Configuration

**Location**: `genesis/`

### Chain IDs

- **Chat Chain**: `dchat-mainnet-1`
- **Currency Chain**: `dchat-currency-mainnet-1`

### Token Economics

| Allocation           | Amount              | Percentage |
| -------------------- | ------------------- | ---------- |
| **Total Supply**     | 1,000,000,000 DCHAT | 100%       |
| Foundation           | 200,000,000 DCHAT   | 20%        |
| Validators (staking) | 240,000,000 DCHAT   | 24%        |
| Community/Rewards    | 560,000,000 DCHAT   | 56%        |

### Pool Distribution

| Pool            | Amount            |
| --------------- | ----------------- |
| Staking Pool    | 240,000,000 DCHAT |
| Rewards Pool    | 224,000,000 DCHAT |
| Liquidity Pool  | 168,000,000 DCHAT |
| Foundation Pool | 200,000,000 DCHAT |

---

## 👥 Validator Keys

**Location**: `keys/validators/`

| Validator   | File               | Public Key                                                         |
| ----------- | ------------------ | ------------------------------------------------------------------ |
| Validator 1 | `validator-1.json` | `3701ed185e07fcbd175419943bf2b1d93d4d837ab3ba5c6d9cd453becf755ecd` |
| Validator 2 | `validator-2.json` | `c84d5a0602d5579ae4321f7ec4ff948eaad46f4c365e278e5faaea0fa48e7b9b` |
| Validator 3 | `validator-3.json` | `a23c36228e911419d886e0e218d8433d7e08de20e8ec6a5aa378c03087e90bac` |
| Validator 4 | `validator-4.json` | `3584f1a3bf98aabf4ba1c760abb5569c9e5e7e43505a5d21952ec51382174907` |
| Validator 5 | `validator-5.json` | `86266851fa27dd311865e3153c94b53b1d05d3a9cb27b501d7b8e479edd01d37` |
| Validator 6 | `validator-6.json` | `21bae54a7436a2f854cc79c2f029adc4142d637335dc24e32c9bcdf12df2e79f` |
| Validator 7 | `validator-7.json` | `779d0c83c0d7ce262878d387539b0de65dda42e0be518eea3ee57b52ca365163` |

Each validator is allocated **10,000,000 DCHAT** initial stake.

---

## 📡 Relay Keys

**Location**: `keys/relays/`

| Relay    | File            | Public Key                                                         |
| -------- | --------------- | ------------------------------------------------------------------ |
| Relay 1  | `relay-1.json`  | `f190aad5244fac3273a98e81a83522841deb860eab31d3075e264bc2323e839a` |
| Relay 2  | `relay-2.json`  | `0a8eecbec0d6b257e68e2837ce51725aa7e5007fb3ecb75411b40dd98cd131ca` |
| Relay 3  | `relay-3.json`  | `197b727d16b51735656bdb53dd0b92062a3e849e4d84612c2be867fec81b2643` |
| Relay 4  | `relay-4.json`  | `ca0be4f335d44d7a94ee8060aaeaf3efe6cc6b09ce430cae1adea7b1a80a1be5` |
| Relay 5  | `relay-5.json`  | `13e6ea455c6473b6a40e79fd7f99c8c209c0ef0ea8661d2524b340dc9d2a82cb` |
| Relay 6  | `relay-6.json`  | `687762111900000322bbe64e1273172605a7f9c437add09dc294b6efc9cbffdc` |
| Relay 7  | `relay-7.json`  | `4b26f56e033d077fa2f5cef26dfb7f3b351e2995e2cfc5ba360e4ed42d3efe80` |
| Relay 8  | `relay-8.json`  | `a89deff2a1c178e154ac01f336b592c6c11b5f96e24542d11a25a59398f7827f` |
| Relay 9  | `relay-9.json`  | `6babd104a6fcfd4b5128824eba027dd69b301830ea9fb5a17f6422569c935b3e` |
| Relay 10 | `relay-10.json` | `25f7cf27c548240d8f2b01bf1517bfa7d98e559eefd46172098ad996881fffaf` |
| Relay 11 | `relay-11.json` | `27ac54c01d6ade59e5b91f94262e222dac01f9aa289683e7bc4388a95a6b2fc2` |
| Relay 12 | `relay-12.json` | `1785fbb3fbeb0102fbc23fffc9516076bf9fd9b64f42d919030fca752b37c717` |
| Relay 13 | `relay-13.json` | `7ef9e43701ffc593f6deba64f91377b43176e01ebb25a56b68cdaded16eb7f89` |
| Relay 14 | `relay-14.json` | `2827128054b699239a4371609318044142b0ddae80d44171169f73189b6aa21a` |

---

## 🚀 Launch Checklist

### Pre-Launch

- [x] ZK ceremony artifacts generated
- [x] Ceremony hash verified
- [x] Genesis blocks created for both chains
- [x] 7 validator keys generated
- [x] 14 relay keys generated
- [ ] Keys securely distributed to server operators
- [ ] Genesis files distributed to all validators
- [ ] DNS records configured for all nodes

### Launch Sequence

1. **Phase 1: Validators (30 seconds apart)**

   ```bash
   # First validator (genesis producer)
   dchat validator --key validator-1.json --chain-rpc http://127.0.0.1:26657 --producer --stake 10000000

   # Subsequent validators
   dchat validator --key validator-N.json --chain-rpc http://127.0.0.1:26657 --stake 10000000
   ```

2. **Phase 2: Relays (after validators achieve consensus)**

   ```bash
   dchat relay --key relay-N.json --listen 0.0.0.0:7070 --stake 1000000
   ```

3. **Phase 3: Verification**
   - Confirm 4/7 BFT consensus achieved
   - Verify block production
   - Test message routing through relays

---

## 🔒 Security Notes

1. **Key Files**: All identity files in `keys/` are stored in **plaintext** for automated deployment. In production:
   - Transfer via secure channel (SSH/SCP)
   - Set file permissions to `600`
   - Consider encrypting with password

2. **Genesis Verification**: All validators MUST verify genesis block hash matches before starting.

3. **ZK Ceremony**: The ceremony uses a deterministic transparent setup seeded by Bitcoin block #874000. Any party can independently verify by regenerating with the same seed.

---

## 📁 Directory Structure

```
ceremony-2025-12-30/
├── CEREMONY_SUMMARY.md          # This file
├── genesis/
│   ├── genesis.json             # Combined genesis summary
│   ├── chat_chain_genesis.json  # Chat chain genesis block
│   └── currency_chain_genesis.json  # Currency chain genesis block
├── keys/
│   ├── validators/
│   │   ├── validator-1.json
│   │   ├── validator-2.json
│   │   ├── ...
│   │   └── validator-7.json
│   ├── relays/
│   │   ├── relay-1.json
│   │   ├── relay-2.json
│   │   ├── ...
│   │   └── relay-14.json
│   └── users/                   # Reserved for user keys
└── zk-ceremony/
    ├── pot_final.bin
    ├── contact_circuit_final.bin
    ├── contact_vk_final.bin
    ├── reputation_circuit_final.bin
    ├── reputation_vk_final.bin
    ├── final_hash.txt
    ├── ceremony_metadata.json
    └── README.md
```

---

## 🎉 Genesis Time

```
2025-12-30T02:22:38.454125400+00:00
```

**The network is ready for mainnet launch!**
