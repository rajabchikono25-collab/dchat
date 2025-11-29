// dchat iOS Biometric Bridge
// Production implementation for biometric authentication
// Bridges Rust FFI to iOS LocalAuthentication framework

import Foundation
import LocalAuthentication

/// Biometric authentication result codes matching Rust FFI
@objc public enum BiometricResult: Int32 {
    case success = 0
    case userCancel = 1
    case userFallback = 2
    case systemCancel = 3
    case notAvailable = 4
    case notEnrolled = 5
    case lockout = 6
    case invalidContext = 7
    case appCancel = 8
    case timeout = 9
    case unknown = 10
}

/// Biometric type matching Rust BiometricType enum
@objc public enum BiometricType: Int32 {
    case none = 0
    case touchId = 1
    case faceId = 2
}

/// Thread-safe biometric authentication manager
@objc public final class DchatBiometricBridge: NSObject {
    
    /// Shared instance for FFI callbacks
    @objc public static let shared = DchatBiometricBridge()
    
    /// Active authentication contexts (thread-safe access)
    private var contexts: [String: LAContext] = [:]
    private let lock = NSLock()
    
    private override init() {
        super.init()
    }
    
    // MARK: - Public API
    
    /// Check if biometric authentication is available
    @objc public func isBiometricAvailable() -> Bool {
        let context = LAContext()
        var error: NSError?
        return context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error)
    }
    
    /// Get the type of biometric available on this device
    @objc public func getBiometricType() -> BiometricType {
        let context = LAContext()
        var error: NSError?
        
        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            return .none
        }
        
        switch context.biometryType {
        case .touchID:
            return .touchId
        case .faceID:
            return .faceId
        case .opticID:
            return .faceId // Treat Vision Pro optic ID as face-based
        case .none:
            return .none
        @unknown default:
            return .none
        }
    }
    
    /// Authenticate user with biometrics
    /// - Parameters:
    ///   - reason: Localized reason shown to user
    ///   - allowFallback: Whether to allow device passcode fallback
    ///   - completion: Callback with result code
    @objc public func authenticate(
        reason: String,
        allowFallback: Bool,
        completion: @escaping (BiometricResult) -> Void
    ) {
        let context = LAContext()
        
        // Configure fallback behavior
        if !allowFallback {
            context.localizedFallbackTitle = ""
        }
        
        // Set timeout for authentication
        context.touchIDAuthenticationAllowableReuseDuration = 0
        
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            completion(mapLAError(error))
            return
        }
        
        context.evaluatePolicy(
            .deviceOwnerAuthenticationWithBiometrics,
            localizedReason: reason
        ) { success, error in
            DispatchQueue.main.async {
                if success {
                    completion(.success)
                } else {
                    completion(self.mapLAError(error as NSError?))
                }
            }
        }
    }
    
    /// Authenticate and sign data with Secure Enclave key
    /// - Parameters:
    ///   - keyId: Identifier for the Secure Enclave key
    ///   - data: Data to sign
    ///   - reason: Localized reason shown to user
    ///   - completion: Callback with signature or nil on failure
    @objc public func authenticateAndSign(
        keyId: String,
        data: Data,
        reason: String,
        completion: @escaping (Data?, BiometricResult) -> Void
    ) {
        let context = LAContext()
        context.touchIDAuthenticationAllowableReuseDuration = 0
        
        var error: NSError?
        guard context.canEvaluatePolicy(.deviceOwnerAuthenticationWithBiometrics, error: &error) else {
            completion(nil, mapLAError(error))
            return
        }
        
        context.evaluatePolicy(
            .deviceOwnerAuthenticationWithBiometrics,
            localizedReason: reason
        ) { success, authError in
            DispatchQueue.main.async {
                if success {
                    // Perform signing with Secure Enclave
                    if let signature = DchatEnclaveBridge.shared.sign(keyId: keyId, data: data, context: context) {
                        completion(signature, .success)
                    } else {
                        completion(nil, .unknown)
                    }
                } else {
                    completion(nil, self.mapLAError(authError as NSError?))
                }
            }
        }
    }
    
    /// Create a new biometric-protected context
    /// - Returns: Context ID for subsequent operations
    @objc public func createContext() -> String {
        let contextId = UUID().uuidString
        let context = LAContext()
        
        lock.lock()
        contexts[contextId] = context
        lock.unlock()
        
        return contextId
    }
    
    /// Invalidate and remove a context
    @objc public func invalidateContext(_ contextId: String) {
        lock.lock()
        if let context = contexts.removeValue(forKey: contextId) {
            context.invalidate()
        }
        lock.unlock()
    }
    
    // MARK: - Private Helpers
    
    private func mapLAError(_ error: NSError?) -> BiometricResult {
        guard let error = error else {
            return .unknown
        }
        
        switch LAError.Code(rawValue: error.code) {
        case .userCancel:
            return .userCancel
        case .userFallback:
            return .userFallback
        case .systemCancel:
            return .systemCancel
        case .biometryNotAvailable:
            return .notAvailable
        case .biometryNotEnrolled:
            return .notEnrolled
        case .biometryLockout:
            return .lockout
        case .invalidContext:
            return .invalidContext
        case .appCancel:
            return .appCancel
        default:
            return .unknown
        }
    }
}

// MARK: - C FFI Exports

/// Check if biometric authentication is available
/// - Returns: true if available, false otherwise
@_cdecl("dchat_ios_biometric_is_available")
public func dchat_ios_biometric_is_available() -> Bool {
    return DchatBiometricBridge.shared.isBiometricAvailable()
}

/// Get the type of biometric available
/// - Returns: BiometricType raw value (0=none, 1=touchId, 2=faceId)
@_cdecl("dchat_ios_biometric_get_type")
public func dchat_ios_biometric_get_type() -> Int32 {
    return DchatBiometricBridge.shared.getBiometricType().rawValue
}

/// Authenticate with biometrics (blocking call via semaphore)
/// - Parameters:
///   - reason: C string with localized reason
///   - allow_fallback: Whether to allow passcode fallback
/// - Returns: BiometricResult raw value
@_cdecl("dchat_ios_biometric_authenticate")
public func dchat_ios_biometric_authenticate(
    reason: UnsafePointer<CChar>,
    allow_fallback: Bool
) -> Int32 {
    let reasonStr = String(cString: reason)
    let semaphore = DispatchSemaphore(value: 0)
    var result: BiometricResult = .unknown
    
    // Must dispatch to main thread for UI
    DispatchQueue.main.async {
        DchatBiometricBridge.shared.authenticate(
            reason: reasonStr,
            allowFallback: allow_fallback
        ) { authResult in
            result = authResult
            semaphore.signal()
        }
    }
    
    // Wait with timeout (60 seconds for user interaction)
    let waitResult = semaphore.wait(timeout: .now() + 60)
    if waitResult == .timedOut {
        return BiometricResult.timeout.rawValue
    }
    
    return result.rawValue
}

/// Authenticate and sign data
/// - Parameters:
///   - key_id: C string key identifier
///   - data: Pointer to data bytes
///   - data_len: Length of data
///   - reason: C string localized reason
///   - signature_out: Output buffer for signature
///   - signature_len: In/out length of signature buffer
/// - Returns: BiometricResult raw value
@_cdecl("dchat_ios_biometric_authenticate_and_sign")
public func dchat_ios_biometric_authenticate_and_sign(
    key_id: UnsafePointer<CChar>,
    data: UnsafePointer<UInt8>,
    data_len: UInt32,
    reason: UnsafePointer<CChar>,
    signature_out: UnsafeMutablePointer<UInt8>,
    signature_len: UnsafeMutablePointer<UInt32>
) -> Int32 {
    let keyIdStr = String(cString: key_id)
    let dataBytes = Data(bytes: data, count: Int(data_len))
    let reasonStr = String(cString: reason)
    
    let semaphore = DispatchSemaphore(value: 0)
    var result: BiometricResult = .unknown
    var signatureData: Data?
    
    DispatchQueue.main.async {
        DchatBiometricBridge.shared.authenticateAndSign(
            keyId: keyIdStr,
            data: dataBytes,
            reason: reasonStr
        ) { sig, authResult in
            signatureData = sig
            result = authResult
            semaphore.signal()
        }
    }
    
    let waitResult = semaphore.wait(timeout: .now() + 60)
    if waitResult == .timedOut {
        return BiometricResult.timeout.rawValue
    }
    
    // Copy signature to output buffer
    if let sig = signatureData {
        let copyLen = min(Int(signature_len.pointee), sig.count)
        sig.withUnsafeBytes { bytes in
            signature_out.update(from: bytes.bindMemory(to: UInt8.self).baseAddress!, count: copyLen)
        }
        signature_len.pointee = UInt32(sig.count)
    } else {
        signature_len.pointee = 0
    }
    
    return result.rawValue
}
