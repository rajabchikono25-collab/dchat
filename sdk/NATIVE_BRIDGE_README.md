# dchat Native Bridge SDK

Production-ready native implementations for iOS and Android platform security features.

## Overview

The dchat-identity crate uses platform-specific secure hardware features:
- **iOS**: Secure Enclave, LocalAuthentication (Face ID/Touch ID)
- **Android**: StrongBox/TEE, BiometricPrompt

These native bridges provide the FFI implementations that the Rust code calls into.

## iOS Bridge (Swift)

### Location
```
sdk/ios/DchatBridge/
├── Package.swift
└── Sources/
    ├── BiometricBridge.swift    # LocalAuthentication FFI
    ├── EnclaveBridge.swift      # Secure Enclave FFI
    └── DchatBridge.h            # C header for Rust FFI
```

### Integration

1. **Add as dependency** in your Xcode project:
   ```swift
   // Package.swift
   dependencies: [
       .package(path: "../sdk/ios/DchatBridge")
   ]
   ```

2. **Link the static library** in your app target

3. **Add required capabilities**:
   - Keychain Sharing (for Secure Enclave keys)
   - App Attest (for device attestation)

### FFI Functions

| Function | Description |
|----------|-------------|
| `dchat_ios_biometric_is_available()` | Check if biometrics available |
| `dchat_ios_biometric_authenticate()` | Authenticate with Face ID/Touch ID |
| `dchat_ios_enclave_generate_key()` | Generate key in Secure Enclave |
| `dchat_ios_enclave_sign()` | Sign data with Secure Enclave key |
| `dchat_ios_enclave_key_agreement()` | ECDH key agreement |

## Android Bridge (Kotlin)

### Location
```
sdk/android/dchat-bridge/
├── build.gradle.kts
├── src/main/
│   ├── AndroidManifest.xml
│   ├── kotlin/com/dchat/bridge/
│   │   ├── BiometricBridge.kt
│   │   ├── EnclaveBridge.kt
│   │   └── DchatBridgeApplication.kt
│   └── cpp/
│       ├── CMakeLists.txt
│       └── dchat_jni.cpp
```

### Integration

1. **Add as module** in your Android project:
   ```kotlin
   // settings.gradle.kts
   include(":dchat-bridge")
   project(":dchat-bridge").projectDir = file("../sdk/android/dchat-bridge")
   ```

2. **Add dependency**:
   ```kotlin
   implementation(project(":dchat-bridge"))
   ```

3. **Initialize in Application**:
   ```kotlin
   class MyApp : DchatBridgeApplication() {
       // or manually:
       override fun onCreate() {
           super.onCreate()
           DchatBridgeInit.initialize(this)
       }
   }
   ```

4. **Add permissions** (already in manifest):
   ```xml
   <uses-permission android:name="android.permission.USE_BIOMETRIC" />
   ```

### JNI Functions

| Function | Description |
|----------|-------------|
| `dchat_android_biometric_is_available()` | Check biometric availability |
| `dchat_android_biometric_authenticate()` | BiometricPrompt authentication |
| `dchat_android_strongbox_available()` | Check StrongBox availability |
| `dchat_android_keystore_generate()` | Generate key in Keystore |
| `dchat_android_keystore_sign()` | Sign with Keystore key |

## Security Features

### iOS Secure Enclave
- Hardware-isolated key storage (A7+ chips)
- Keys never leave the enclave
- Biometric-protected operations
- ECDH key agreement in hardware

### Android StrongBox/TEE
- Hardware-backed key storage
- StrongBox for certified devices (Titan M, etc.)
- TEE fallback for all modern devices
- BiometricPrompt for Class 3 (Strong) biometrics

## Building

### iOS
```bash
cd sdk/ios/DchatBridge
swift build -c release
```

### Android
```bash
cd sdk/android
./gradlew :dchat-bridge:assembleRelease
```

## Testing

### iOS Simulator Limitations
- Secure Enclave not available in simulator
- Use `#if targetEnvironment(simulator)` for test mocks

### Android Emulator Limitations
- StrongBox not available in emulator
- TEE-backed keystore works in emulator
- Biometrics can be simulated via `adb emu finger touch`

## Security Notes

1. **Key ID Validation**: All key IDs are validated to prevent path traversal attacks
2. **Biometric Lockout**: After 5 failed attempts, biometric is locked for 30 seconds
3. **No Key Export**: Private keys never leave secure hardware
4. **Attestation**: Both platforms support hardware attestation for proof-of-device

## Related Files

- `crates/dchat-identity/src/biometric.rs` - Rust biometric abstraction
- `crates/dchat-identity/src/enclave.rs` - Rust enclave abstraction
- `crates/dchat-identity/src/attestation.rs` - Device attestation verification
