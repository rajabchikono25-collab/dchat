/**
 * dchat Android Biometric Bridge
 * Production implementation for biometric authentication
 * Bridges Rust FFI to Android BiometricPrompt API
 */
package com.dchat.bridge

import android.content.Context
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.security.keystore.KeyGenParameterSpec
import android.security.keystore.KeyProperties
import androidx.biometric.BiometricManager
import androidx.biometric.BiometricPrompt
import androidx.core.content.ContextCompat
import androidx.fragment.app.FragmentActivity
import java.security.KeyStore
import java.security.Signature
import java.util.concurrent.CountDownLatch
import java.util.concurrent.Executor
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicInteger
import java.util.concurrent.atomic.AtomicReference

/**
 * Biometric authentication result codes matching Rust FFI
 */
object BiometricResult {
    const val SUCCESS = 0
    const val USER_CANCEL = 1
    const val USER_FALLBACK = 2
    const val SYSTEM_CANCEL = 3
    const val NOT_AVAILABLE = 4
    const val NOT_ENROLLED = 5
    const val LOCKOUT = 6
    const val INVALID_CONTEXT = 7
    const val APP_CANCEL = 8
    const val TIMEOUT = 9
    const val UNKNOWN = 10
}

/**
 * Biometric type matching Rust BiometricType enum
 */
object BiometricType {
    const val NONE = 0
    const val FINGERPRINT = 1
    const val FACE = 2
    const val IRIS = 3
}

/**
 * Thread-safe biometric authentication manager
 */
class BiometricBridge private constructor() {
    
    companion object {
        @Volatile
        private var instance: BiometricBridge? = null
        
        private var applicationContext: Context? = null
        private var currentActivity: FragmentActivity? = null
        
        @JvmStatic
        fun getInstance(): BiometricBridge {
            return instance ?: synchronized(this) {
                instance ?: BiometricBridge().also { instance = it }
            }
        }
        
        /**
         * Initialize with application context (call from Application.onCreate())
         */
        @JvmStatic
        fun initialize(context: Context) {
            applicationContext = context.applicationContext
        }
        
        /**
         * Set the current activity for biometric prompts
         */
        @JvmStatic
        fun setCurrentActivity(activity: FragmentActivity?) {
            currentActivity = activity
        }
    }
    
    private val mainHandler = Handler(Looper.getMainLooper())
    
    /**
     * Check if biometric authentication is available
     */
    fun isBiometricAvailable(): Boolean {
        val context = applicationContext ?: return false
        val biometricManager = BiometricManager.from(context)
        
        return when (biometricManager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG)) {
            BiometricManager.BIOMETRIC_SUCCESS -> true
            else -> false
        }
    }
    
    /**
     * Get the type of biometric available
     */
    fun getBiometricType(): Int {
        val context = applicationContext ?: return BiometricType.NONE
        val biometricManager = BiometricManager.from(context)
        
        // Check if any biometric is available
        if (biometricManager.canAuthenticate(BiometricManager.Authenticators.BIOMETRIC_STRONG) 
            != BiometricManager.BIOMETRIC_SUCCESS) {
            return BiometricType.NONE
        }
        
        // Android doesn't directly expose biometric type, but we can infer
        // from device capabilities
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            // Modern devices with face unlock
            if (context.packageManager.hasSystemFeature("android.hardware.biometrics.face")) {
                BiometricType.FACE
            } else if (context.packageManager.hasSystemFeature("android.hardware.biometrics.iris")) {
                BiometricType.IRIS
            } else {
                BiometricType.FINGERPRINT
            }
        } else {
            BiometricType.FINGERPRINT
        }
    }
    
    /**
     * Authenticate with biometrics (blocking call)
     * @param title Dialog title
     * @param subtitle Dialog subtitle
     * @param negativeButtonText Negative button text
     * @param allowDeviceCredential Whether to allow device PIN/pattern fallback
     * @return BiometricResult code
     */
    fun authenticate(
        title: String,
        subtitle: String,
        negativeButtonText: String,
        allowDeviceCredential: Boolean
    ): Int {
        val activity = currentActivity ?: return BiometricResult.INVALID_CONTEXT
        
        val latch = CountDownLatch(1)
        val result = AtomicInteger(BiometricResult.UNKNOWN)
        
        mainHandler.post {
            val executor: Executor = ContextCompat.getMainExecutor(activity)
            
            val callback = object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(authResult: BiometricPrompt.AuthenticationResult) {
                    result.set(BiometricResult.SUCCESS)
                    latch.countDown()
                }
                
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    result.set(mapErrorCode(errorCode))
                    latch.countDown()
                }
                
                override fun onAuthenticationFailed() {
                    // Don't count down - this is a single failed attempt, not final
                }
            }
            
            val biometricPrompt = BiometricPrompt(activity, executor, callback)
            
            val promptInfoBuilder = BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
            
            if (allowDeviceCredential) {
                promptInfoBuilder.setAllowedAuthenticators(
                    BiometricManager.Authenticators.BIOMETRIC_STRONG or
                    BiometricManager.Authenticators.DEVICE_CREDENTIAL
                )
            } else {
                promptInfoBuilder.setNegativeButtonText(negativeButtonText)
                promptInfoBuilder.setAllowedAuthenticators(
                    BiometricManager.Authenticators.BIOMETRIC_STRONG
                )
            }
            
            try {
                biometricPrompt.authenticate(promptInfoBuilder.build())
            } catch (e: Exception) {
                result.set(BiometricResult.UNKNOWN)
                latch.countDown()
            }
        }
        
        // Wait with timeout (60 seconds for user interaction)
        return if (latch.await(60, TimeUnit.SECONDS)) {
            result.get()
        } else {
            BiometricResult.TIMEOUT
        }
    }
    
    /**
     * Authenticate and sign data with biometric-protected key
     * @param keyAlias Android KeyStore alias
     * @param data Data to sign
     * @param title Dialog title
     * @param subtitle Dialog subtitle
     * @return Pair of (signature bytes, result code)
     */
    fun authenticateAndSign(
        keyAlias: String,
        data: ByteArray,
        title: String,
        subtitle: String
    ): Pair<ByteArray?, Int> {
        val activity = currentActivity ?: return Pair(null, BiometricResult.INVALID_CONTEXT)
        
        // Load the key and prepare signature
        val keyStore = KeyStore.getInstance("AndroidKeyStore")
        keyStore.load(null)
        
        val privateKey = try {
            keyStore.getKey(keyAlias, null) as? java.security.PrivateKey
        } catch (e: Exception) {
            return Pair(null, BiometricResult.NOT_AVAILABLE)
        } ?: return Pair(null, BiometricResult.NOT_AVAILABLE)
        
        val signature = try {
            Signature.getInstance("SHA256withECDSA").apply {
                initSign(privateKey)
            }
        } catch (e: Exception) {
            return Pair(null, BiometricResult.NOT_AVAILABLE)
        }
        
        val latch = CountDownLatch(1)
        val result = AtomicInteger(BiometricResult.UNKNOWN)
        val signatureResult = AtomicReference<ByteArray?>(null)
        
        mainHandler.post {
            val executor: Executor = ContextCompat.getMainExecutor(activity)
            
            val callback = object : BiometricPrompt.AuthenticationCallback() {
                override fun onAuthenticationSucceeded(authResult: BiometricPrompt.AuthenticationResult) {
                    try {
                        // Use the crypto object from the result
                        val cryptoSignature = authResult.cryptoObject?.signature
                        if (cryptoSignature != null) {
                            cryptoSignature.update(data)
                            signatureResult.set(cryptoSignature.sign())
                        }
                        result.set(BiometricResult.SUCCESS)
                    } catch (e: Exception) {
                        result.set(BiometricResult.UNKNOWN)
                    }
                    latch.countDown()
                }
                
                override fun onAuthenticationError(errorCode: Int, errString: CharSequence) {
                    result.set(mapErrorCode(errorCode))
                    latch.countDown()
                }
                
                override fun onAuthenticationFailed() {
                    // Single failed attempt, not final
                }
            }
            
            val biometricPrompt = BiometricPrompt(activity, executor, callback)
            
            val promptInfo = BiometricPrompt.PromptInfo.Builder()
                .setTitle(title)
                .setSubtitle(subtitle)
                .setNegativeButtonText("Cancel")
                .setAllowedAuthenticators(BiometricManager.Authenticators.BIOMETRIC_STRONG)
                .build()
            
            try {
                biometricPrompt.authenticate(
                    promptInfo,
                    BiometricPrompt.CryptoObject(signature)
                )
            } catch (e: Exception) {
                result.set(BiometricResult.UNKNOWN)
                latch.countDown()
            }
        }
        
        return if (latch.await(60, TimeUnit.SECONDS)) {
            Pair(signatureResult.get(), result.get())
        } else {
            Pair(null, BiometricResult.TIMEOUT)
        }
    }
    
    /**
     * Enroll user for biometric authentication (redirects to settings)
     */
    fun enrollBiometric(): Int {
        val activity = currentActivity ?: return BiometricResult.INVALID_CONTEXT
        
        return try {
            val intent = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
                android.content.Intent(android.provider.Settings.ACTION_BIOMETRIC_ENROLL).apply {
                    putExtra(
                        android.provider.Settings.EXTRA_BIOMETRIC_AUTHENTICATORS_ALLOWED,
                        BiometricManager.Authenticators.BIOMETRIC_STRONG
                    )
                }
            } else {
                android.content.Intent(android.provider.Settings.ACTION_SECURITY_SETTINGS)
            }
            activity.startActivity(intent)
            BiometricResult.SUCCESS
        } catch (e: Exception) {
            BiometricResult.UNKNOWN
        }
    }
    
    private fun mapErrorCode(errorCode: Int): Int {
        return when (errorCode) {
            BiometricPrompt.ERROR_USER_CANCELED -> BiometricResult.USER_CANCEL
            BiometricPrompt.ERROR_NEGATIVE_BUTTON -> BiometricResult.USER_FALLBACK
            BiometricPrompt.ERROR_CANCELED -> BiometricResult.SYSTEM_CANCEL
            BiometricPrompt.ERROR_HW_NOT_PRESENT,
            BiometricPrompt.ERROR_HW_UNAVAILABLE -> BiometricResult.NOT_AVAILABLE
            BiometricPrompt.ERROR_NO_BIOMETRICS -> BiometricResult.NOT_ENROLLED
            BiometricPrompt.ERROR_LOCKOUT,
            BiometricPrompt.ERROR_LOCKOUT_PERMANENT -> BiometricResult.LOCKOUT
            BiometricPrompt.ERROR_TIMEOUT -> BiometricResult.TIMEOUT
            else -> BiometricResult.UNKNOWN
        }
    }
}

// =============================================================================
// JNI FFI Exports
// =============================================================================

/**
 * JNI bridge functions for Rust FFI
 * These are called from native code via JNI
 */
@Suppress("unused")
object BiometricBridgeJNI {
    
    @JvmStatic
    external fun nativeInit()
    
    init {
        try {
            System.loadLibrary("dchat_jni")
        } catch (e: UnsatisfiedLinkError) {
            // Library may not be available during testing
        }
    }
    
    /**
     * JNI: Check if biometric authentication is available
     */
    @JvmStatic
    fun dchat_android_biometric_is_available(): Boolean {
        return BiometricBridge.getInstance().isBiometricAvailable()
    }
    
    /**
     * JNI: Get biometric type
     */
    @JvmStatic
    fun dchat_android_biometric_get_type(): Int {
        return BiometricBridge.getInstance().getBiometricType()
    }
    
    /**
     * JNI: Authenticate with biometrics
     */
    @JvmStatic
    fun dchat_android_biometric_authenticate(
        title: String,
        subtitle: String,
        negativeButtonText: String,
        allowDeviceCredential: Boolean
    ): Int {
        return BiometricBridge.getInstance().authenticate(
            title, subtitle, negativeButtonText, allowDeviceCredential
        )
    }
    
    /**
     * JNI: Enroll biometric
     */
    @JvmStatic
    fun dchat_android_biometric_enroll(): Int {
        return BiometricBridge.getInstance().enrollBiometric()
    }
}
