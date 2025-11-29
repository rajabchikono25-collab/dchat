// dchat iOS Native Bridge Header
// C FFI declarations for Rust integration

#ifndef DCHAT_BRIDGE_H
#define DCHAT_BRIDGE_H

#include <stdint.h>
#include <stdbool.h>

#ifdef __cplusplus
extern "C" {
#endif

// =============================================================================
// Biometric Authentication FFI
// =============================================================================

/// Check if biometric authentication is available
/// @return true if available, false otherwise
bool dchat_ios_biometric_is_available(void);

/// Get the type of biometric available
/// @return 0=none, 1=touchId, 2=faceId
int32_t dchat_ios_biometric_get_type(void);

/// Authenticate with biometrics (blocking)
/// @param reason Localized reason shown to user
/// @param allow_fallback Whether to allow passcode fallback
/// @return BiometricResult code (0=success)
int32_t dchat_ios_biometric_authenticate(
    const char* reason,
    bool allow_fallback
);

/// Authenticate and sign data with biometrics
/// @param key_id Key identifier for signing
/// @param data Data to sign
/// @param data_len Length of data
/// @param reason Localized reason shown to user
/// @param signature_out Output buffer for signature
/// @param signature_len In/out length of signature buffer
/// @return BiometricResult code (0=success)
int32_t dchat_ios_biometric_authenticate_and_sign(
    const char* key_id,
    const uint8_t* data,
    uint32_t data_len,
    const char* reason,
    uint8_t* signature_out,
    uint32_t* signature_len
);

// =============================================================================
// Secure Enclave FFI
// =============================================================================

/// Check if Secure Enclave is available
/// @return true if available
bool dchat_ios_enclave_is_available(void);

/// Generate a new key pair in Secure Enclave
/// @param key_id Unique key identifier
/// @param require_biometric Whether key requires biometric auth
/// @param public_key_out Output buffer for public key
/// @param public_key_len In/out length of public key buffer
/// @return EnclaveResult code (0=success)
int32_t dchat_ios_enclave_generate_key(
    const char* key_id,
    bool require_biometric,
    uint8_t* public_key_out,
    uint32_t* public_key_len
);

/// Sign data with a Secure Enclave key
/// @param key_id Key identifier
/// @param data Data to sign
/// @param data_len Length of data
/// @param signature_out Output buffer for signature
/// @param signature_len In/out length of signature buffer
/// @return EnclaveResult code (0=success)
int32_t dchat_ios_enclave_sign(
    const char* key_id,
    const uint8_t* data,
    uint32_t data_len,
    uint8_t* signature_out,
    uint32_t* signature_len
);

/// Get public key for a key pair
/// @param key_id Key identifier
/// @param public_key_out Output buffer for public key
/// @param public_key_len In/out length of public key buffer
/// @return EnclaveResult code (0=success)
int32_t dchat_ios_enclave_get_public_key(
    const char* key_id,
    uint8_t* public_key_out,
    uint32_t* public_key_len
);

/// Delete a key from Secure Enclave
/// @param key_id Key identifier
/// @return EnclaveResult code (0=success)
int32_t dchat_ios_enclave_delete_key(const char* key_id);

/// Perform ECDH key agreement
/// @param key_id Local private key identifier
/// @param peer_public_key Peer's public key bytes
/// @param peer_public_key_len Length of peer public key
/// @param shared_secret_out Output buffer for shared secret
/// @param shared_secret_len In/out length of shared secret buffer
/// @return EnclaveResult code (0=success)
int32_t dchat_ios_enclave_key_agreement(
    const char* key_id,
    const uint8_t* peer_public_key,
    uint32_t peer_public_key_len,
    uint8_t* shared_secret_out,
    uint32_t* shared_secret_len
);

/// Check if a key exists
/// @param key_id Key identifier
/// @return true if key exists
bool dchat_ios_enclave_key_exists(const char* key_id);

#ifdef __cplusplus
}
#endif

#endif // DCHAT_BRIDGE_H
