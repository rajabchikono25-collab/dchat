/**
 * dchat JNI Bridge for Rust FFI
 * Provides native method declarations for Rust integration
 */

#include <jni.h>
#include <string>

// Cache for JNI references
static JavaVM* g_jvm = nullptr;
static jclass g_biometricBridgeClass = nullptr;
static jclass g_enclaveBridgeClass = nullptr;

extern "C" {

JNIEXPORT jint JNI_OnLoad(JavaVM* vm, void* reserved) {
    g_jvm = vm;
    
    JNIEnv* env;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) != JNI_OK) {
        return JNI_ERR;
    }
    
    // Cache class references
    jclass biometricClass = env->FindClass("com/dchat/bridge/BiometricBridgeJNI");
    if (biometricClass != nullptr) {
        g_biometricBridgeClass = reinterpret_cast<jclass>(env->NewGlobalRef(biometricClass));
    }
    
    jclass enclaveClass = env->FindClass("com/dchat/bridge/EnclaveBridgeJNI");
    if (enclaveClass != nullptr) {
        g_enclaveBridgeClass = reinterpret_cast<jclass>(env->NewGlobalRef(enclaveClass));
    }
    
    return JNI_VERSION_1_6;
}

JNIEXPORT void JNI_OnUnload(JavaVM* vm, void* reserved) {
    JNIEnv* env;
    if (vm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) == JNI_OK) {
        if (g_biometricBridgeClass != nullptr) {
            env->DeleteGlobalRef(g_biometricBridgeClass);
        }
        if (g_enclaveBridgeClass != nullptr) {
            env->DeleteGlobalRef(g_enclaveBridgeClass);
        }
    }
    g_jvm = nullptr;
}

// Helper to get JNIEnv for current thread
static JNIEnv* getEnv() {
    JNIEnv* env;
    if (g_jvm->GetEnv(reinterpret_cast<void**>(&env), JNI_VERSION_1_6) == JNI_OK) {
        return env;
    }
    
    // Attach current thread if needed
    if (g_jvm->AttachCurrentThread(&env, nullptr) == JNI_OK) {
        return env;
    }
    
    return nullptr;
}

// =============================================================================
// Biometric FFI Functions (called from Rust)
// =============================================================================

JNIEXPORT jboolean dchat_android_biometric_is_available() {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_biometricBridgeClass == nullptr) {
        return JNI_FALSE;
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_biometricBridgeClass,
        "dchat_android_biometric_is_available",
        "()Z"
    );
    
    if (method == nullptr) {
        return JNI_FALSE;
    }
    
    return env->CallStaticBooleanMethod(g_biometricBridgeClass, method);
}

JNIEXPORT jint dchat_android_biometric_authenticate(
    const char* title,
    const char* subtitle,
    const char* negative_button,
    jboolean allow_device_credential
) {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_biometricBridgeClass == nullptr) {
        return 7; // INVALID_CONTEXT
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_biometricBridgeClass,
        "dchat_android_biometric_authenticate",
        "(Ljava/lang/String;Ljava/lang/String;Ljava/lang/String;Z)I"
    );
    
    if (method == nullptr) {
        return 10; // UNKNOWN
    }
    
    jstring jTitle = env->NewStringUTF(title);
    jstring jSubtitle = env->NewStringUTF(subtitle);
    jstring jNegativeButton = env->NewStringUTF(negative_button);
    
    jint result = env->CallStaticIntMethod(
        g_biometricBridgeClass,
        method,
        jTitle,
        jSubtitle,
        jNegativeButton,
        allow_device_credential
    );
    
    env->DeleteLocalRef(jTitle);
    env->DeleteLocalRef(jSubtitle);
    env->DeleteLocalRef(jNegativeButton);
    
    return result;
}

JNIEXPORT jint dchat_android_biometric_enroll() {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_biometricBridgeClass == nullptr) {
        return 7; // INVALID_CONTEXT
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_biometricBridgeClass,
        "dchat_android_biometric_enroll",
        "()I"
    );
    
    if (method == nullptr) {
        return 10; // UNKNOWN
    }
    
    return env->CallStaticIntMethod(g_biometricBridgeClass, method);
}

// =============================================================================
// Keystore/Enclave FFI Functions (called from Rust)
// =============================================================================

JNIEXPORT jboolean dchat_android_strongbox_available() {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_enclaveBridgeClass == nullptr) {
        return JNI_FALSE;
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_enclaveBridgeClass,
        "dchat_android_strongbox_available",
        "()Z"
    );
    
    if (method == nullptr) {
        return JNI_FALSE;
    }
    
    return env->CallStaticBooleanMethod(g_enclaveBridgeClass, method);
}

JNIEXPORT jint dchat_android_keystore_generate(
    const char* key_id,
    jboolean require_biometric,
    jboolean use_strongbox,
    unsigned char* public_key_out,
    unsigned int* public_key_len
) {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_enclaveBridgeClass == nullptr) {
        return 7; // NOT_AVAILABLE
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_enclaveBridgeClass,
        "dchat_android_keystore_generate",
        "(Ljava/lang/String;ZZ)[B"
    );
    
    if (method == nullptr) {
        return 10; // UNKNOWN
    }
    
    jstring jKeyId = env->NewStringUTF(key_id);
    
    jbyteArray result = static_cast<jbyteArray>(env->CallStaticObjectMethod(
        g_enclaveBridgeClass,
        method,
        jKeyId,
        require_biometric,
        use_strongbox
    ));
    
    env->DeleteLocalRef(jKeyId);
    
    if (result == nullptr || env->GetArrayLength(result) == 0) {
        return 2; // KEY_ALREADY_EXISTS or generation failed
    }
    
    jsize len = env->GetArrayLength(result);
    jbyte* bytes = env->GetByteArrayElements(result, nullptr);
    
    unsigned int copyLen = (len < *public_key_len) ? len : *public_key_len;
    memcpy(public_key_out, bytes, copyLen);
    *public_key_len = len;
    
    env->ReleaseByteArrayElements(result, bytes, 0);
    env->DeleteLocalRef(result);
    
    return 0; // SUCCESS
}

JNIEXPORT jint dchat_android_keystore_sign(
    const char* key_id,
    const unsigned char* data,
    unsigned int data_len,
    unsigned char* signature_out,
    unsigned int* signature_len
) {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_enclaveBridgeClass == nullptr) {
        return 7; // NOT_AVAILABLE
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_enclaveBridgeClass,
        "dchat_android_keystore_sign",
        "(Ljava/lang/String;[B)[B"
    );
    
    if (method == nullptr) {
        return 10; // UNKNOWN
    }
    
    jstring jKeyId = env->NewStringUTF(key_id);
    jbyteArray jData = env->NewByteArray(data_len);
    env->SetByteArrayRegion(jData, 0, data_len, reinterpret_cast<const jbyte*>(data));
    
    jbyteArray result = static_cast<jbyteArray>(env->CallStaticObjectMethod(
        g_enclaveBridgeClass,
        method,
        jKeyId,
        jData
    ));
    
    env->DeleteLocalRef(jKeyId);
    env->DeleteLocalRef(jData);
    
    if (result == nullptr || env->GetArrayLength(result) == 0) {
        return 4; // SIGNATURE_FAILED
    }
    
    jsize len = env->GetArrayLength(result);
    jbyte* bytes = env->GetByteArrayElements(result, nullptr);
    
    unsigned int copyLen = (len < *signature_len) ? len : *signature_len;
    memcpy(signature_out, bytes, copyLen);
    *signature_len = len;
    
    env->ReleaseByteArrayElements(result, bytes, 0);
    env->DeleteLocalRef(result);
    
    return 0; // SUCCESS
}

JNIEXPORT jint dchat_android_keystore_delete(const char* key_id) {
    JNIEnv* env = getEnv();
    if (env == nullptr || g_enclaveBridgeClass == nullptr) {
        return 7; // NOT_AVAILABLE
    }
    
    jmethodID method = env->GetStaticMethodID(
        g_enclaveBridgeClass,
        "dchat_android_keystore_delete",
        "(Ljava/lang/String;)I"
    );
    
    if (method == nullptr) {
        return 10; // UNKNOWN
    }
    
    jstring jKeyId = env->NewStringUTF(key_id);
    jint result = env->CallStaticIntMethod(g_enclaveBridgeClass, method, jKeyId);
    env->DeleteLocalRef(jKeyId);
    
    return result;
}

} // extern "C"
