// dchat iOS Secure Enclave Bridge
// Production implementation for Secure Enclave key management
// Bridges Rust FFI to iOS Security framework

import Foundation
import Security
import LocalAuthentication
import CryptoKit

/// Enclave operation result codes matching Rust FFI
@objc public enum EnclaveResult: Int32 {
    case success = 0
    case keyNotFound = 1
    case keyAlreadyExists = 2
    case invalidKey = 3
    case signatureFailed = 4
    case encryptionFailed = 5
    case decryptionFailed = 6
    case notAvailable = 7
    case accessDenied = 8
    case invalidData = 9
    case unknown = 10
}

/// Thread-safe Secure Enclave key manager
@objc public final class DchatEnclaveBridge: NSObject {
    
    /// Shared instance for FFI callbacks
    @objc public static let shared = DchatEnclaveBridge()
    
    /// Key tag prefix for dchat keys
    private let keyTagPrefix = "com.dchat.enclave."
    
    /// Access control flags for biometric-protected keys
    private let accessFlags: SecAccessControlCreateFlags = [
        .privateKeyUsage,
        .biometryCurrentSet
    ]
    
    private override init() {
        super.init()
    }
    
    // MARK: - Public API
    
    /// Check if Secure Enclave is available on this device
    @objc public func isSecureEnclaveAvailable() -> Bool {
        // Check for Secure Enclave by attempting to create access control
        guard let _ = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            .privateKeyUsage,
            nil
        ) else {
            return false
        }
        
        // Verify the device has a Secure Enclave
        if #available(iOS 13.0, *) {
            return SecureEnclave.isAvailable
        } else {
            // Fallback check for older iOS versions
            var error: Unmanaged<CFError>?
            let access = SecAccessControlCreateWithFlags(
                kCFAllocatorDefault,
                kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
                [.privateKeyUsage],
                &error
            )
            return access != nil && error == nil
        }
    }
    
    /// Generate a new key pair in the Secure Enclave
    /// - Parameters:
    ///   - keyId: Unique identifier for the key
    ///   - requireBiometric: Whether key access requires biometric authentication
    /// - Returns: Public key data on success, nil on failure
    @objc public func generateKey(keyId: String, requireBiometric: Bool) -> Data? {
        let tag = keyTagPrefix + keyId
        
        // Check if key already exists
        if keyExists(keyId: keyId) {
            return nil
        }
        
        // Create access control
        var accessFlags: SecAccessControlCreateFlags = [.privateKeyUsage]
        if requireBiometric {
            accessFlags.insert(.biometryCurrentSet)
        }
        
        var error: Unmanaged<CFError>?
        guard let access = SecAccessControlCreateWithFlags(
            kCFAllocatorDefault,
            kSecAttrAccessibleWhenUnlockedThisDeviceOnly,
            accessFlags,
            &error
        ) else {
            return nil
        }
        
        // Key generation parameters
        let attributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeySizeInBits as String: 256,
            kSecAttrTokenID as String: kSecAttrTokenIDSecureEnclave,
            kSecPrivateKeyAttrs as String: [
                kSecAttrIsPermanent as String: true,
                kSecAttrApplicationTag as String: tag.data(using: .utf8)!,
                kSecAttrAccessControl as String: access
            ]
        ]
        
        // Generate key pair
        var privateKeyError: Unmanaged<CFError>?
        guard let privateKey = SecKeyCreateRandomKey(attributes as CFDictionary, &privateKeyError) else {
            return nil
        }
        
        // Extract public key
        guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
            return nil
        }
        
        // Export public key as raw bytes
        var exportError: Unmanaged<CFError>?
        guard let publicKeyData = SecKeyCopyExternalRepresentation(publicKey, &exportError) as Data? else {
            return nil
        }
        
        return publicKeyData
    }
    
    /// Sign data using a Secure Enclave key
    /// - Parameters:
    ///   - keyId: Key identifier
    ///   - data: Data to sign
    ///   - context: Optional LAContext for biometric authentication
    /// - Returns: Signature data on success, nil on failure
    @objc public func sign(keyId: String, data: Data, context: LAContext? = nil) -> Data? {
        guard let privateKey = getPrivateKey(keyId: keyId, context: context) else {
            return nil
        }
        
        // Create SHA256 digest
        let digest = SHA256.hash(data: data)
        let digestData = Data(digest)
        
        // Sign with ECDSA
        var signError: Unmanaged<CFError>?
        guard let signature = SecKeyCreateSignature(
            privateKey,
            .ecdsaSignatureMessageX962SHA256,
            digestData as CFData,
            &signError
        ) as Data? else {
            return nil
        }
        
        return signature
    }
    
    /// Get the public key for a key pair
    /// - Parameter keyId: Key identifier
    /// - Returns: Public key data on success, nil on failure
    @objc public func getPublicKey(keyId: String) -> Data? {
        guard let privateKey = getPrivateKey(keyId: keyId, context: nil) else {
            return nil
        }
        
        guard let publicKey = SecKeyCopyPublicKey(privateKey) else {
            return nil
        }
        
        var exportError: Unmanaged<CFError>?
        guard let publicKeyData = SecKeyCopyExternalRepresentation(publicKey, &exportError) as Data? else {
            return nil
        }
        
        return publicKeyData
    }
    
    /// Delete a key from the Secure Enclave
    /// - Parameter keyId: Key identifier
    /// - Returns: true if deleted, false otherwise
    @objc public func deleteKey(keyId: String) -> Bool {
        let tag = keyTagPrefix + keyId
        
        let query: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag.data(using: .utf8)!,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom
        ]
        
        let status = SecItemDelete(query as CFDictionary)
        return status == errSecSuccess
    }
    
    /// Check if a key exists
    /// - Parameter keyId: Key identifier
    /// - Returns: true if key exists
    @objc public func keyExists(keyId: String) -> Bool {
        return getPrivateKey(keyId: keyId, context: nil) != nil
    }
    
    // MARK: - ECDH Key Agreement
    
    /// Perform ECDH key agreement
    /// - Parameters:
    ///   - keyId: Local private key identifier
    ///   - peerPublicKeyData: Peer's public key (raw bytes)
    /// - Returns: Shared secret on success, nil on failure
    @objc public func performKeyAgreement(keyId: String, peerPublicKeyData: Data) -> Data? {
        guard let privateKey = getPrivateKey(keyId: keyId, context: nil) else {
            return nil
        }
        
        // Import peer public key
        let keyAttributes: [String: Any] = [
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecAttrKeyClass as String: kSecAttrKeyClassPublic,
            kSecAttrKeySizeInBits as String: 256
        ]
        
        var importError: Unmanaged<CFError>?
        guard let peerPublicKey = SecKeyCreateWithData(
            peerPublicKeyData as CFData,
            keyAttributes as CFDictionary,
            &importError
        ) else {
            return nil
        }
        
        // Perform ECDH
        var exchangeError: Unmanaged<CFError>?
        let params: [String: Any] = [:]
        guard let sharedSecret = SecKeyCopyKeyExchangeResult(
            privateKey,
            .ecdhKeyExchangeStandard,
            peerPublicKey,
            params as CFDictionary,
            &exchangeError
        ) as Data? else {
            return nil
        }
        
        return sharedSecret
    }
    
    // MARK: - Private Helpers
    
    private func getPrivateKey(keyId: String, context: LAContext?) -> SecKey? {
        let tag = keyTagPrefix + keyId
        
        var query: [String: Any] = [
            kSecClass as String: kSecClassKey,
            kSecAttrApplicationTag as String: tag.data(using: .utf8)!,
            kSecAttrKeyType as String: kSecAttrKeyTypeECSECPrimeRandom,
            kSecReturnRef as String: true
        ]
        
        // Add LAContext if provided (for biometric auth)
        if let ctx = context {
            query[kSecUseAuthenticationContext as String] = ctx
        }
        
        var item: CFTypeRef?
        let status = SecItemCopyMatching(query as CFDictionary, &item)
        
        guard status == errSecSuccess else {
            return nil
        }
        
        return (item as! SecKey)
    }
}

// MARK: - C FFI Exports

/// Check if Secure Enclave is available
/// - Returns: true if available
@_cdecl("dchat_ios_enclave_is_available")
public func dchat_ios_enclave_is_available() -> Bool {
    return DchatEnclaveBridge.shared.isSecureEnclaveAvailable()
}

/// Generate a new key pair in Secure Enclave
/// - Parameters:
///   - key_id: C string key identifier
///   - require_biometric: Whether key requires biometric auth
///   - public_key_out: Output buffer for public key
///   - public_key_len: In/out length of public key buffer
/// - Returns: EnclaveResult raw value
@_cdecl("dchat_ios_enclave_generate_key")
public func dchat_ios_enclave_generate_key(
    key_id: UnsafePointer<CChar>,
    require_biometric: Bool,
    public_key_out: UnsafeMutablePointer<UInt8>,
    public_key_len: UnsafeMutablePointer<UInt32>
) -> Int32 {
    let keyIdStr = String(cString: key_id)
    
    // Validate key_id (prevent path traversal)
    guard !keyIdStr.contains("/") && !keyIdStr.contains("\\") && !keyIdStr.contains("..") else {
        return EnclaveResult.invalidKey.rawValue
    }
    
    guard let publicKey = DchatEnclaveBridge.shared.generateKey(keyId: keyIdStr, requireBiometric: require_biometric) else {
        if DchatEnclaveBridge.shared.keyExists(keyId: keyIdStr) {
            return EnclaveResult.keyAlreadyExists.rawValue
        }
        return EnclaveResult.unknown.rawValue
    }
    
    let copyLen = min(Int(public_key_len.pointee), publicKey.count)
    publicKey.withUnsafeBytes { bytes in
        public_key_out.update(from: bytes.bindMemory(to: UInt8.self).baseAddress!, count: copyLen)
    }
    public_key_len.pointee = UInt32(publicKey.count)
    
    return EnclaveResult.success.rawValue
}

/// Sign data with a Secure Enclave key
/// - Parameters:
///   - key_id: C string key identifier
///   - data: Pointer to data bytes
///   - data_len: Length of data
///   - signature_out: Output buffer for signature
///   - signature_len: In/out length of signature buffer
/// - Returns: EnclaveResult raw value
@_cdecl("dchat_ios_enclave_sign")
public func dchat_ios_enclave_sign(
    key_id: UnsafePointer<CChar>,
    data: UnsafePointer<UInt8>,
    data_len: UInt32,
    signature_out: UnsafeMutablePointer<UInt8>,
    signature_len: UnsafeMutablePointer<UInt32>
) -> Int32 {
    let keyIdStr = String(cString: key_id)
    let dataBytes = Data(bytes: data, count: Int(data_len))
    
    guard let signature = DchatEnclaveBridge.shared.sign(keyId: keyIdStr, data: dataBytes) else {
        if !DchatEnclaveBridge.shared.keyExists(keyId: keyIdStr) {
            return EnclaveResult.keyNotFound.rawValue
        }
        return EnclaveResult.signatureFailed.rawValue
    }
    
    let copyLen = min(Int(signature_len.pointee), signature.count)
    signature.withUnsafeBytes { bytes in
        signature_out.update(from: bytes.bindMemory(to: UInt8.self).baseAddress!, count: copyLen)
    }
    signature_len.pointee = UInt32(signature.count)
    
    return EnclaveResult.success.rawValue
}

/// Get public key for a key pair
/// - Parameters:
///   - key_id: C string key identifier
///   - public_key_out: Output buffer for public key
///   - public_key_len: In/out length of public key buffer
/// - Returns: EnclaveResult raw value
@_cdecl("dchat_ios_enclave_get_public_key")
public func dchat_ios_enclave_get_public_key(
    key_id: UnsafePointer<CChar>,
    public_key_out: UnsafeMutablePointer<UInt8>,
    public_key_len: UnsafeMutablePointer<UInt32>
) -> Int32 {
    let keyIdStr = String(cString: key_id)
    
    guard let publicKey = DchatEnclaveBridge.shared.getPublicKey(keyId: keyIdStr) else {
        return EnclaveResult.keyNotFound.rawValue
    }
    
    let copyLen = min(Int(public_key_len.pointee), publicKey.count)
    publicKey.withUnsafeBytes { bytes in
        public_key_out.update(from: bytes.bindMemory(to: UInt8.self).baseAddress!, count: copyLen)
    }
    public_key_len.pointee = UInt32(publicKey.count)
    
    return EnclaveResult.success.rawValue
}

/// Delete a key from Secure Enclave
/// - Parameter key_id: C string key identifier
/// - Returns: EnclaveResult raw value
@_cdecl("dchat_ios_enclave_delete_key")
public func dchat_ios_enclave_delete_key(key_id: UnsafePointer<CChar>) -> Int32 {
    let keyIdStr = String(cString: key_id)
    
    if DchatEnclaveBridge.shared.deleteKey(keyId: keyIdStr) {
        return EnclaveResult.success.rawValue
    }
    return EnclaveResult.keyNotFound.rawValue
}

/// Perform ECDH key agreement
/// - Parameters:
///   - key_id: C string local private key identifier
///   - peer_public_key: Pointer to peer public key bytes
///   - peer_public_key_len: Length of peer public key
///   - shared_secret_out: Output buffer for shared secret
///   - shared_secret_len: In/out length of shared secret buffer
/// - Returns: EnclaveResult raw value
@_cdecl("dchat_ios_enclave_key_agreement")
public func dchat_ios_enclave_key_agreement(
    key_id: UnsafePointer<CChar>,
    peer_public_key: UnsafePointer<UInt8>,
    peer_public_key_len: UInt32,
    shared_secret_out: UnsafeMutablePointer<UInt8>,
    shared_secret_len: UnsafeMutablePointer<UInt32>
) -> Int32 {
    let keyIdStr = String(cString: key_id)
    let peerKeyData = Data(bytes: peer_public_key, count: Int(peer_public_key_len))
    
    guard let sharedSecret = DchatEnclaveBridge.shared.performKeyAgreement(keyId: keyIdStr, peerPublicKeyData: peerKeyData) else {
        if !DchatEnclaveBridge.shared.keyExists(keyId: keyIdStr) {
            return EnclaveResult.keyNotFound.rawValue
        }
        return EnclaveResult.invalidKey.rawValue
    }
    
    let copyLen = min(Int(shared_secret_len.pointee), sharedSecret.count)
    sharedSecret.withUnsafeBytes { bytes in
        shared_secret_out.update(from: bytes.bindMemory(to: UInt8.self).baseAddress!, count: copyLen)
    }
    shared_secret_len.pointee = UInt32(sharedSecret.count)
    
    return EnclaveResult.success.rawValue
}

/// Check if a key exists
/// - Parameter key_id: C string key identifier
/// - Returns: true if key exists
@_cdecl("dchat_ios_enclave_key_exists")
public func dchat_ios_enclave_key_exists(key_id: UnsafePointer<CChar>) -> Bool {
    let keyIdStr = String(cString: key_id)
    return DchatEnclaveBridge.shared.keyExists(keyId: keyIdStr)
}
