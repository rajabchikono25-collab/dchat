# Keyless Onboarding (dev)

This folder contains a simulated Keyless UX used for local development:

- `biometric.rs` — env-driven biometric simulation (`DCHAT_BIOMETRIC_OK=1` to succeed).
- `enclave.rs` — file-backed simulated enclave storing a 32-byte device key and returning a placeholder attestation string.
- `mpc_fallback.rs` — deterministic HMAC-based fallback (mock of MPC) for development.

Developer notes — running tests locally

1. Recommended: install native build tools required by workspace crates:

PowerShell (Chocolatey):
```powershell
choco install -y cmake nasm
```

PowerShell (winget):
```powershell
winget install --id Kitware.CMake -e
winget install --id NASM.NASM -e
```

2. Quick workaround (may still require `cmake`):
```powershell
# $env:AWS_LC_SYS_NO_ASM = "1"
cargo test -p dchat-identity --lib
```

3. To run only the keyless unit tests (no workspace rebuild):
```powershell
# Run the identity crate tests (attestation & badge helper)
cargo test -p dchat-identity --lib

# Run the main crate library tests (includes onboarding integration)
cargo test -p dchat --lib
```

4. Biometric simulation
```powershell
# Success
$env:DCHAT_BIOMETRIC_OK = "1"
# Failure
$env:DCHAT_BIOMETRIC_OK = "0"
```

Security note
---
All code in this folder is strictly for developer/testing purposes and uses simulated attestation and deterministic fallbacks. Replace with platform SDKs and real MPC for production.
