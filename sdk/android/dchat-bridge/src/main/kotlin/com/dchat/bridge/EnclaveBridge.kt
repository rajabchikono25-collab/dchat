/**
 * dchat Android Keystore/StrongBox Bridge
 * Production implementation for hardware-backed key management
 * Bridges Rust FFI to Android Keystore and StrongBox APIs
 */
package com.dchat.bridge

import android.content.Context
import android.os.Build
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyInfo
import android.security.keystore.KeyProperties
import java.security.KeyFactory
import java.security.KeyPairGenerator
import java.security.KeyStore
import java.security.PrivateKey
import java.security.Signature
import javax.crypto.KeyAgreement

/**
 * Enclave operation result codes matching Rust FFI
 */
object EnclaveResult {
    const val SUCCESS = 0
    const val KEY_NOT_FOUND = 1
    const val KEY_ALREADY_EXISTS = 2
    const val INVALID_KEY = 3
    const val SIGNATURE_FAILED = 4
    const val ENCRYPTION_FAILED = 5
    const val DECRYPTION_FAILED = 6
    const val NOT_AVAILABLE = 7
    const val ACCESS_DENIED = 8
    const val INVALID_DATA = 9
    const val UNKNOWN = 10
}

/**
 * Thread-safe Android Keystore/StrongBox manager
 */
class EnclaveBridge private constructor() {
    
    companion object {
        @Volatile
        private var instance: EnclaveBridge? = null
        
        private var applicationContext: Context? = null
        
        private const val KEY_ALIAS_PREFIX = "dchat_enclave_"
        private const val ANDROID_KEYSTORE = "AndroidKeyStore"
        
        @JvmStatic
        fun getInstance(): EnclaveBridge {
            return instance ?: synchronized(this) {
                instance ?: EnclaveBridge().also { instance = it }
            }
        }
        
        /**
         * Initialize with application context
         */
        @JvmStatic
        fun initialize(context: Context) {
            applicationContext = context.applicationContext
        }
    }
    
    private val keyStore: KeyStore by lazy {
        KeyStore.getInstance(ANDROID_KEYSTORE).apply { load(null) }
    }
    
    /**
     * Check if StrongBox (hardware-backed secure element) is available
     */
    fun isStrongBoxAvailable(): Boolean {
        if (Build.VERSION.SDK_INT < Build.VERSION_CODES.P) {
            return false
        }
        
        val context = applicationContext ?: return false
        return context.packageManager.hasSystemFeature("android.hardware.strongbox_keystore")
    }
    
    /**
     * Check if hardware-backed Keystore is available
     */
    fun isHardwareBackedAvailable(): Boolean {
        // All modern Android devices (6.0+) have TEE-backed keystore
        return Build.VERSION.SDK_INT >= Build.VERSION_CODES.M
    }
    
    /**
     * Generate a new key pair in hardware-backed storage
     * @param keyId Unique key identifier
     * @param requireBiometric Whether key requires biometric authentication
     * @param useStrongBox Whether to use StrongBox (if available)
     * @return Public key bytes on success, null on failure
     */
    fun generateKey(
        keyId: String,
        requireBiometric: Boolean,
        useStrongBox: Boolean
    ): ByteArray? {
        // Validate key ID (prevent path traversal)
        if (keyId.contains("/") || keyId.contains("\\") || keyId.contains("..")) {
            return null
        }
        
        val alias = KEY_ALIAS_PREFIX + keyId
        
        // Check if key already exists
        if (keyExists(keyId)) {
            return null
        }
        
        return try {
            val keyPairGenerator = KeyPairGenerator.getInstance(
                KeyProperties.KEY_ALGORITHM_EC,
                ANDROID_KEYSTORE
            )
            
            val builder = KeyGenParameterSpec.Builder(
                alias,
                KeyProperties.PURPOSE_SIGN or KeyProperties.PURPOSE_AGREE_KEY
            )
                .setAlgorithmParameterSpec(java.security.spec.ECGenParameterSpec("secp256r1"))
                .setDigests(KeyProperties.DIGEST_SHA256, KeyProperties.DIGEST_SHA384, KeyProperties.DIGEST_SHA512)
                .setUserAuthenticationRequired(requireBiometric)
            
            // StrongBox requires Android P+
            if (useStrongBox && Build.VERSION.SDK_INT >= Build.VERSION_CODES.P) {
                builder.setIsStrongBoxBacked(true)
            }
            
            // Configure biometric requirements
            if (requireBiometric) {
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                    builder.setUserAuthenticationParameters(
                        0, // timeout = 0 means require auth for every use
                        KeyProperties.AUTH_BIOMETRIC_STRONG
                    )
                } else if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.N) {
                    @Suppress("DEPRECATION")
                    builder.setUserAuthenticationValidityDurationSeconds(-1)
                }
            }
            
            keyPairGenerator.initialize(builder.build())
            val keyPair = keyPairGenerator.generateKeyPair()
            
            // Export public key as raw bytes (X.509 SubjectPublicKeyInfo format)
            keyPair.public.encoded
        } catch (e: Exception) {
            null
        }
    }
    
    /**
     * Sign data with a hardware-backed key
     * @param keyId Key identifier
     * @param data Data to sign
     * @return Signature bytes on success, null on failure
     */
    fun sign(keyId: String, data: ByteArray): ByteArray? {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            val privateKey = keyStore.getKey(alias, null) as? PrivateKey
                ?: return null
            
            val signature = Signature.getInstance("SHA256withECDSA")
            signature.initSign(privateKey)
            signature.update(data)
            signature.sign()
        } catch (e: Exception) {
            null
        }
    }
    
    /**
     * Get public key for a key pair
     * @param keyId Key identifier
     * @return Public key bytes on success, null on failure
     */
    fun getPublicKey(keyId: String): ByteArray? {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            val certificate = keyStore.getCertificate(alias)
            certificate?.publicKey?.encoded
        } catch (e: Exception) {
            null
        }
    }
    
    /**
     * Delete a key from the Keystore
     * @param keyId Key identifier
     * @return true if deleted, false otherwise
     */
    fun deleteKey(keyId: String): Boolean {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            if (keyStore.containsAlias(alias)) {
                keyStore.deleteEntry(alias)
                true
            } else {
                false
            }
        } catch (e: Exception) {
            false
        }
    }
    
    /**
     * Check if a key exists
     * @param keyId Key identifier
     * @return true if key exists
     */
    fun keyExists(keyId: String): Boolean {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            keyStore.containsAlias(alias)
        } catch (e: Exception) {
            false
        }
    }
    
    /**
     * Perform ECDH key agreement
     * @param keyId Local private key identifier
     * @param peerPublicKey Peer's public key (X.509 encoded)
     * @return Shared secret on success, null on failure
     */
    fun performKeyAgreement(keyId: String, peerPublicKey: ByteArray): ByteArray? {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            val privateKey = keyStore.getKey(alias, null) as? PrivateKey
                ?: return null
            
            // Decode peer public key
            val keyFactory = KeyFactory.getInstance("EC")
            val peerKey = keyFactory.generatePublic(
                java.security.spec.X509EncodedKeySpec(peerPublicKey)
            )
            
            // Perform ECDH
            val keyAgreement = KeyAgreement.getInstance("ECDH")
            keyAgreement.init(privateKey)
            keyAgreement.doPhase(peerKey, true)
            keyAgreement.generateSecret()
        } catch (e: Exception) {
            null
        }
    }
    
    /**
     * Get key info (for debugging/verification)
     * @param keyId Key identifier
     * @return Map of key properties, or null if key not found
     */
    fun getKeyInfo(keyId: String): Map<String, Any>? {
        val alias = KEY_ALIAS_PREFIX + keyId
        
        return try {
            val privateKey = keyStore.getKey(alias, null) as? PrivateKey
                ?: return null
            
            val keyFactory = KeyFactory.getInstance(privateKey.algorithm, ANDROID_KEYSTORE)
            val keyInfo = keyFactory.getKeySpec(privateKey, KeyInfo::class.java)
            
            buildMap {
                put("isInsideSecureHardware", keyInfo.isInsideSecureHardware)
                put("isUserAuthenticationRequired", keyInfo.isUserAuthenticationRequired)
                if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
                    put("securityLevel", keyInfo.securityLevel)
                }
            }
        } catch (e: Exception) {
            null
        }
    }
}

// =============================================================================
// JNI FFI Exports
// =============================================================================

/**
 * JNI bridge functions for Rust FFI
 */
@Suppress("unused")
object EnclaveBridgeJNI {
    
    /**
     * JNI: Check if StrongBox is available
     */
    @JvmStatic
    fun dchat_android_strongbox_available(): Boolean {
        return EnclaveBridge.getInstance().isStrongBoxAvailable()
    }
    
    /**
     * JNI: Check if hardware-backed keystore is available
     */
    @JvmStatic
    fun dchat_android_keystore_available(): Boolean {
        return EnclaveBridge.getInstance().isHardwareBackedAvailable()
    }
    
    /**
     * JNI: Generate a new key pair
     * @return Public key bytes, or empty array on failure
     */
    @JvmStatic
    fun dchat_android_keystore_generate(
        keyId: String,
        requireBiometric: Boolean,
        useStrongBox: Boolean
    ): ByteArray {
        return EnclaveBridge.getInstance().generateKey(keyId, requireBiometric, useStrongBox)
            ?: ByteArray(0)
    }
    
    /**
     * JNI: Sign data with a key
     * @return Signature bytes, or empty array on failure
     */
    @JvmStatic
    fun dchat_android_keystore_sign(keyId: String, data: ByteArray): ByteArray {
        return EnclaveBridge.getInstance().sign(keyId, data)
            ?: ByteArray(0)
    }
    
    /**
     * JNI: Get public key
     * @return Public key bytes, or empty array on failure
     */
    @JvmStatic
    fun dchat_android_keystore_get_public_key(keyId: String): ByteArray {
        return EnclaveBridge.getInstance().getPublicKey(keyId)
            ?: ByteArray(0)
    }
    
    /**
     * JNI: Delete a key
     * @return Result code
     */
    @JvmStatic
    fun dchat_android_keystore_delete(keyId: String): Int {
        return if (EnclaveBridge.getInstance().deleteKey(keyId)) {
            EnclaveResult.SUCCESS
        } else {
            EnclaveResult.KEY_NOT_FOUND
        }
    }
    
    /**
     * JNI: Check if key exists
     */
    @JvmStatic
    fun dchat_android_keystore_key_exists(keyId: String): Boolean {
        return EnclaveBridge.getInstance().keyExists(keyId)
    }
    
    /**
     * JNI: Perform ECDH key agreement
     * @return Shared secret bytes, or empty array on failure
     */
    @JvmStatic
    fun dchat_android_keystore_key_agreement(keyId: String, peerPublicKey: ByteArray): ByteArray {
        return EnclaveBridge.getInstance().performKeyAgreement(keyId, peerPublicKey)
            ?: ByteArray(0)
    }
}
