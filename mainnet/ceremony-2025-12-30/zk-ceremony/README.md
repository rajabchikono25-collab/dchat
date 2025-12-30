# dchat ZK Proof MPC Ceremony Artifacts

This directory contains the Multi-Party Computation (MPC) ceremony artifacts for the dchat zero-knowledge proof system.

## Security Notice

**CRITICAL**: The production keys in this directory were generated through a secure MPC ceremony.

- Minimum 100 participants from diverse backgrounds
- At least 10 known dchat community members
- All ceremony transcripts are publicly available
- Random beacon derived from Bitcoin block hash at ceremony start
- Final hash published to Ethereum mainnet for immutability

## Ceremony Details

- **Start Block**: Bitcoin block #XXXXXX (TBD at ceremony)
- **Participants**: 100+ (TBD)
- **Ceremony Protocol**: Powers of Tau (Perpetual Powers of Tau derivative)
- **Circuit Type**: Groth16 on BN254
- **Security Level**: 128 bits

## Files

| File                           | Description                       | Hash Verification           |
| ------------------------------ | --------------------------------- | --------------------------- |
| `final_hash.txt`               | BLAKE3 hash of ceremony artifacts | Compare with published hash |
| `pot_final.bin`                | Powers of Tau ceremony result     | Verify against transcript   |
| `contact_circuit_final.bin`    | Contact proof proving key         | Phase 2 contribution        |
| `reputation_circuit_final.bin` | Reputation proof proving key      | Phase 2 contribution        |
| `ceremony_transcript.json`     | Full ceremony transcript          | Public record               |

## Verification

To verify the ceremony artifacts:

```bash
# Verify Powers of Tau
snarkjs powersoftau verify pot_final.ptau

# Verify Phase 2 circuits
snarkjs zkey verify contact_circuit_final.zkey pot_final.ptau
snarkjs zkey verify reputation_circuit_final.zkey pot_final.ptau

# Verify artifact hash
echo "$(cat final_hash.txt)  pot_final.bin" | blake3sum -c
```

## Transcript Location

Full ceremony transcripts with participant contributions:

- IPFS: ipfs://QmXXXXXX (TBD)
- GitHub: https://github.com/dchat-network/ceremony-transcript
- Archive.org: https://archive.org/details/dchat-ceremony-2025

## Security Model

The ceremony ensures that:

1. As long as ONE participant destroyed their toxic waste, proofs are sound
2. No single entity can forge proofs
3. The randomness is verifiable and cannot be manipulated

## Contact

For questions about the ceremony: security@dchat.network
