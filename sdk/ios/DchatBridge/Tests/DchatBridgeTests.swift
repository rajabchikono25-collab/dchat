// iOS Native Bridge Tests
// Tests for DchatBridge functionality

import XCTest
@testable import DchatBridge

final class DchatBridgeTests: XCTestCase {
    
    // MARK: - Biometric Tests
    
    func testBiometricAvailability() {
        // In simulator, biometrics are not available
        #if targetEnvironment(simulator)
        // Simulator has no biometric hardware
        // Just verify the function doesn't crash
        let _ = DchatBiometricBridge.shared.isBiometricAvailable()
        #else
        let available = DchatBiometricBridge.shared.isBiometricAvailable()
        // On device, this depends on hardware and enrollment
        XCTAssertNotNil(available)
        #endif
    }
    
    func testBiometricType() {
        let type = DchatBiometricBridge.shared.getBiometricType()
        // Should return a valid type
        XCTAssertTrue([.none, .touchId, .faceId].contains(type))
    }
    
    func testContextCreation() {
        let contextId = DchatBiometricBridge.shared.createContext()
        XCTAssertFalse(contextId.isEmpty)
        
        // Invalidate should not crash
        DchatBiometricBridge.shared.invalidateContext(contextId)
    }
    
    // MARK: - Enclave Tests
    
    func testEnclaveAvailability() {
        #if targetEnvironment(simulator)
        // Secure Enclave not available in simulator
        XCTAssertFalse(DchatEnclaveBridge.shared.isSecureEnclaveAvailable())
        #else
        // On real device, should be available
        XCTAssertTrue(DchatEnclaveBridge.shared.isSecureEnclaveAvailable())
        #endif
    }
    
    func testKeyGeneration() {
        #if !targetEnvironment(simulator)
        let keyId = "test_key_\(UUID().uuidString)"
        
        // Generate key
        let publicKey = DchatEnclaveBridge.shared.generateKey(
            keyId: keyId,
            requireBiometric: false
        )
        
        XCTAssertNotNil(publicKey)
        XCTAssertFalse(publicKey?.isEmpty ?? true)
        
        // Verify key exists
        XCTAssertTrue(DchatEnclaveBridge.shared.keyExists(keyId: keyId))
        
        // Clean up
        XCTAssertTrue(DchatEnclaveBridge.shared.deleteKey(keyId: keyId))
        XCTAssertFalse(DchatEnclaveBridge.shared.keyExists(keyId: keyId))
        #endif
    }
    
    func testKeyDoesNotExist() {
        let keyId = "nonexistent_key_\(UUID().uuidString)"
        XCTAssertFalse(DchatEnclaveBridge.shared.keyExists(keyId: keyId))
    }
    
    func testDuplicateKeyGeneration() {
        #if !targetEnvironment(simulator)
        let keyId = "duplicate_test_\(UUID().uuidString)"
        
        // Generate first key
        let firstKey = DchatEnclaveBridge.shared.generateKey(
            keyId: keyId,
            requireBiometric: false
        )
        XCTAssertNotNil(firstKey)
        
        // Attempt duplicate generation should fail
        let secondKey = DchatEnclaveBridge.shared.generateKey(
            keyId: keyId,
            requireBiometric: false
        )
        XCTAssertNil(secondKey)
        
        // Clean up
        DchatEnclaveBridge.shared.deleteKey(keyId: keyId)
        #endif
    }
    
    func testSigning() {
        #if !targetEnvironment(simulator)
        let keyId = "signing_test_\(UUID().uuidString)"
        
        // Generate key
        guard let _ = DchatEnclaveBridge.shared.generateKey(
            keyId: keyId,
            requireBiometric: false
        ) else {
            XCTFail("Key generation failed")
            return
        }
        
        // Sign data
        let testData = "Hello, dchat!".data(using: .utf8)!
        let signature = DchatEnclaveBridge.shared.sign(keyId: keyId, data: testData)
        
        XCTAssertNotNil(signature)
        XCTAssertFalse(signature?.isEmpty ?? true)
        
        // Signature should be ECDSA (typically 70-72 bytes for P-256)
        XCTAssertTrue((signature?.count ?? 0) >= 64)
        
        // Clean up
        DchatEnclaveBridge.shared.deleteKey(keyId: keyId)
        #endif
    }
    
    func testKeyAgreement() {
        #if !targetEnvironment(simulator)
        let keyIdA = "ecdh_test_a_\(UUID().uuidString)"
        let keyIdB = "ecdh_test_b_\(UUID().uuidString)"
        
        // Generate two key pairs
        guard let publicKeyA = DchatEnclaveBridge.shared.generateKey(
            keyId: keyIdA,
            requireBiometric: false
        ) else {
            XCTFail("Key A generation failed")
            return
        }
        
        guard let publicKeyB = DchatEnclaveBridge.shared.generateKey(
            keyId: keyIdB,
            requireBiometric: false
        ) else {
            DchatEnclaveBridge.shared.deleteKey(keyId: keyIdA)
            XCTFail("Key B generation failed")
            return
        }
        
        // Perform key agreement A -> B
        let secretAB = DchatEnclaveBridge.shared.performKeyAgreement(
            keyId: keyIdA,
            peerPublicKeyData: publicKeyB
        )
        
        // Perform key agreement B -> A
        let secretBA = DchatEnclaveBridge.shared.performKeyAgreement(
            keyId: keyIdB,
            peerPublicKeyData: publicKeyA
        )
        
        XCTAssertNotNil(secretAB)
        XCTAssertNotNil(secretBA)
        
        // Shared secrets should match
        XCTAssertEqual(secretAB, secretBA)
        
        // Clean up
        DchatEnclaveBridge.shared.deleteKey(keyId: keyIdA)
        DchatEnclaveBridge.shared.deleteKey(keyId: keyIdB)
        #endif
    }
    
    // MARK: - FFI Tests
    
    func testFFIBiometricAvailable() {
        let result = dchat_ios_biometric_is_available()
        // Just verify it returns without crashing
        XCTAssertNotNil(result)
    }
    
    func testFFIBiometricType() {
        let result = dchat_ios_biometric_get_type()
        // Should be 0, 1, or 2
        XCTAssertTrue(result >= 0 && result <= 2)
    }
    
    func testFFIEnclaveAvailable() {
        let result = dchat_ios_enclave_is_available()
        XCTAssertNotNil(result)
    }
}
