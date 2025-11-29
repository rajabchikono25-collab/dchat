/**
 * dchat Android Bridge Instrumented Tests
 * Tests that run on Android device/emulator
 */
package com.dchat.bridge

import androidx.test.ext.junit.runners.AndroidJUnit4
import androidx.test.platform.app.InstrumentationRegistry
import org.junit.Assert.*
import org.junit.Before
import org.junit.Test
import org.junit.runner.RunWith
import java.util.UUID

@RunWith(AndroidJUnit4::class)
class EnclaveBridgeTest {
    
    private lateinit var enclaveBridge: EnclaveBridge
    
    @Before
    fun setup() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        EnclaveBridge.initialize(context)
        enclaveBridge = EnclaveBridge.getInstance()
    }
    
    @Test
    fun testHardwareBackedAvailable() {
        // TEE-backed keystore should be available on all modern devices
        assertTrue(enclaveBridge.isHardwareBackedAvailable())
    }
    
    @Test
    fun testStrongBoxAvailability() {
        // StrongBox may or may not be available - just verify no crash
        val available = enclaveBridge.isStrongBoxAvailable()
        assertNotNull(available)
    }
    
    @Test
    fun testKeyGeneration() {
        val keyId = "test_key_${UUID.randomUUID()}"
        
        try {
            // Generate key without biometric requirement (for testing)
            val publicKey = enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            
            assertNotNull(publicKey)
            assertTrue(publicKey!!.isNotEmpty())
            
            // Verify key exists
            assertTrue(enclaveBridge.keyExists(keyId))
            
            // Get public key should return same key
            val retrievedKey = enclaveBridge.getPublicKey(keyId)
            assertArrayEquals(publicKey, retrievedKey)
            
        } finally {
            // Clean up
            enclaveBridge.deleteKey(keyId)
        }
    }
    
    @Test
    fun testKeyDoesNotExist() {
        val keyId = "nonexistent_key_${UUID.randomUUID()}"
        assertFalse(enclaveBridge.keyExists(keyId))
    }
    
    @Test
    fun testDuplicateKeyGeneration() {
        val keyId = "duplicate_test_${UUID.randomUUID()}"
        
        try {
            // Generate first key
            val firstKey = enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNotNull(firstKey)
            
            // Attempt duplicate generation should return null
            val secondKey = enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNull(secondKey)
            
        } finally {
            enclaveBridge.deleteKey(keyId)
        }
    }
    
    @Test
    fun testSigning() {
        val keyId = "signing_test_${UUID.randomUUID()}"
        
        try {
            // Generate key
            val publicKey = enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNotNull(publicKey)
            
            // Sign data
            val testData = "Hello, dchat!".toByteArray(Charsets.UTF_8)
            val signature = enclaveBridge.sign(keyId, testData)
            
            assertNotNull(signature)
            assertTrue(signature!!.isNotEmpty())
            
            // ECDSA signature should be at least 64 bytes
            assertTrue(signature.size >= 64)
            
        } finally {
            enclaveBridge.deleteKey(keyId)
        }
    }
    
    @Test
    fun testKeyAgreement() {
        val keyIdA = "ecdh_test_a_${UUID.randomUUID()}"
        val keyIdB = "ecdh_test_b_${UUID.randomUUID()}"
        
        try {
            // Generate two key pairs
            val publicKeyA = enclaveBridge.generateKey(
                keyId = keyIdA,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNotNull(publicKeyA)
            
            val publicKeyB = enclaveBridge.generateKey(
                keyId = keyIdB,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNotNull(publicKeyB)
            
            // Perform key agreement A -> B
            val secretAB = enclaveBridge.performKeyAgreement(keyIdA, publicKeyB!!)
            assertNotNull(secretAB)
            
            // Perform key agreement B -> A
            val secretBA = enclaveBridge.performKeyAgreement(keyIdB, publicKeyA!!)
            assertNotNull(secretBA)
            
            // Shared secrets should match
            assertArrayEquals(secretAB, secretBA)
            
        } finally {
            enclaveBridge.deleteKey(keyIdA)
            enclaveBridge.deleteKey(keyIdB)
        }
    }
    
    @Test
    fun testKeyDeletion() {
        val keyId = "delete_test_${UUID.randomUUID()}"
        
        // Generate key
        val publicKey = enclaveBridge.generateKey(
            keyId = keyId,
            requireBiometric = false,
            useStrongBox = false
        )
        assertNotNull(publicKey)
        assertTrue(enclaveBridge.keyExists(keyId))
        
        // Delete key
        assertTrue(enclaveBridge.deleteKey(keyId))
        assertFalse(enclaveBridge.keyExists(keyId))
        
        // Delete non-existent key should return false
        assertFalse(enclaveBridge.deleteKey(keyId))
    }
    
    @Test
    fun testKeyInfo() {
        val keyId = "info_test_${UUID.randomUUID()}"
        
        try {
            // Generate key
            enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            
            // Get key info
            val info = enclaveBridge.getKeyInfo(keyId)
            assertNotNull(info)
            
            // Should have isInsideSecureHardware property
            assertTrue(info!!.containsKey("isInsideSecureHardware"))
            
            // On real device with TEE, this should be true
            // In emulator, it might be false
            
        } finally {
            enclaveBridge.deleteKey(keyId)
        }
    }
    
    @Test
    fun testInvalidKeyId() {
        // Key IDs with path traversal should be rejected
        val invalidIds = listOf(
            "../test",
            "test/key",
            "test\\key",
            "..\\test"
        )
        
        for (keyId in invalidIds) {
            val result = enclaveBridge.generateKey(
                keyId = keyId,
                requireBiometric = false,
                useStrongBox = false
            )
            assertNull("Key ID '$keyId' should be rejected", result)
        }
    }
}

@RunWith(AndroidJUnit4::class)
class BiometricBridgeTest {
    
    @Before
    fun setup() {
        val context = InstrumentationRegistry.getInstrumentation().targetContext
        BiometricBridge.initialize(context)
    }
    
    @Test
    fun testBiometricAvailability() {
        val bridge = BiometricBridge.getInstance()
        // Just verify it doesn't crash - availability depends on device
        val available = bridge.isBiometricAvailable()
        assertNotNull(available)
    }
    
    @Test
    fun testBiometricType() {
        val bridge = BiometricBridge.getInstance()
        val type = bridge.getBiometricType()
        
        // Should return a valid type constant
        assertTrue(type in listOf(
            BiometricType.NONE,
            BiometricType.FINGERPRINT,
            BiometricType.FACE,
            BiometricType.IRIS
        ))
    }
}
