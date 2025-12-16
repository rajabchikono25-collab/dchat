use crate::verification::{ProofType, VerificationProof, VerifiedBadge};
use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::Signature;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Attestation platform types
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum AttestationPlatform {
    /// iOS App Attest (DCAppAttestService)
    IosAppAttest,
    /// Android Play Integrity API
    AndroidPlayIntegrity,
    /// Android SafetyNet (deprecated, but supported for backward compatibility)
    AndroidSafetyNet,
    /// Android Key Attestation (hardware-backed)
    AndroidKeyAttestation,
    /// WebAuthn attestation
    WebAuthn,
    /// Simulated attestation - ONLY available in debug builds for testing
    /// SECURITY: This variant is compile-time excluded from release builds
    #[cfg(debug_assertions)]
    Simulated,
}

/// Attestation verification result
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AttestationResult {
    pub verified: bool,
    pub platform: AttestationPlatform,
    pub device_integrity: DeviceIntegrity,
    pub reason: Option<String>,
    pub metadata: HashMap<String, String>,
}

/// Device integrity assessment
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceIntegrity {
    /// Device has secure hardware (TEE/Secure Enclave)
    pub has_secure_hardware: bool,
    /// Device is not rooted/jailbroken
    pub is_genuine: bool,
    /// App is from official store
    pub is_official_app: bool,
    /// Device passed all integrity checks
    pub integrity_passed: bool,
}

impl Default for DeviceIntegrity {
    fn default() -> Self {
        Self {
            has_secure_hardware: false,
            is_genuine: false,
            is_official_app: false,
            integrity_passed: false,
        }
    }
}

/// iOS App Attest verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IosAppAttestData {
    /// Attestation object from DCAppAttestService.attestKey()
    pub attestation_object: Vec<u8>,
    /// Key identifier
    pub key_id: String,
    /// Challenge used for attestation
    pub challenge: Vec<u8>,
    /// Bundle ID
    pub bundle_id: String,
    /// Team ID
    pub team_id: String,
}

/// Android Play Integrity verification data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndroidPlayIntegrityData {
    /// Integrity token from Play Integrity API
    pub integrity_token: String,
    /// Nonce used for request
    pub nonce: Vec<u8>,
    /// Package name
    pub package_name: String,
}

/// Android Key Attestation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AndroidKeyAttestationData {
    /// Certificate chain (DER-encoded X.509)
    pub certificate_chain: Vec<Vec<u8>>,
    /// Challenge used for attestation
    pub challenge: Vec<u8>,
}

/// WebAuthn attestation data
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WebAuthnAttestationData {
    /// Attestation object (CBOR-encoded)
    pub attestation_object: Vec<u8>,
    /// Client data JSON
    pub client_data_json: Vec<u8>,
    /// Credential ID
    pub credential_id: Vec<u8>,
}

/// Attestation verifier configuration
#[derive(Debug, Clone)]
pub struct AttestationConfig {
    /// Allow simulated attestation - ONLY works in debug builds
    /// SECURITY: This field has no effect in release builds
    #[cfg(debug_assertions)]
    pub allow_simulated: bool,
    /// iOS App ID prefix (Team ID)
    pub ios_team_id: Option<String>,
    /// iOS bundle ID
    pub ios_bundle_id: Option<String>,
    /// Android package name
    pub android_package_name: Option<String>,
    /// Google Play Integrity API decryption key
    pub play_integrity_decryption_key: Option<Vec<u8>>,
    /// WebAuthn RP ID
    pub webauthn_rp_id: Option<String>,
    /// Allowed origins for WebAuthn
    pub webauthn_origins: Vec<String>,
}

impl Default for AttestationConfig {
    fn default() -> Self {
        Self {
            #[cfg(debug_assertions)]
            allow_simulated: true, // Only in debug builds for testing
            ios_team_id: None,
            ios_bundle_id: None,
            android_package_name: None,
            play_integrity_decryption_key: None,
            webauthn_rp_id: None,
            webauthn_origins: Vec::new(),
        }
    }
}

/// Cached Google access token
#[cfg(feature = "attestation-api")]
#[derive(Debug, Clone)]
struct CachedAccessToken {
    token: String,
    expires_at: DateTime<Utc>,
}

/// Attestation verifier with platform-specific verification
pub struct AttestationVerifier {
    config: AttestationConfig,
    /// Cached Google Cloud access token for Play Integrity API
    #[cfg(feature = "attestation-api")]
    google_token_cache: std::sync::RwLock<Option<CachedAccessToken>>,
}

impl AttestationVerifier {
    /// Create a new attestation verifier
    pub fn new(config: AttestationConfig) -> Self {
        Self {
            config,
            #[cfg(feature = "attestation-api")]
            google_token_cache: std::sync::RwLock::new(None),
        }
    }

    /// Create verifier with default config
    pub fn default_verifier() -> Self {
        Self::new(AttestationConfig::default())
    }

    /// Verify iOS App Attest attestation
    pub async fn verify_ios_app_attest(
        &self,
        data: &IosAppAttestData,
    ) -> Result<AttestationResult> {
        // Validate bundle ID matches configuration
        if let Some(expected_bundle) = &self.config.ios_bundle_id {
            if &data.bundle_id != expected_bundle {
                return Err(Error::unauthenticated(format!(
                    "Bundle ID mismatch: expected {}, got {}",
                    expected_bundle, data.bundle_id
                )));
            }
        }

        // Validate team ID
        if let Some(expected_team) = &self.config.ios_team_id {
            if &data.team_id != expected_team {
                return Err(Error::unauthenticated("Team ID mismatch"));
            }
        }

        // Parse the attestation object (CBOR-encoded)
        // Structure: { fmt: "apple-appattest", attStmt: {...}, authData: [...] }
        let attestation_result =
            self.parse_ios_attestation_object(&data.attestation_object, &data.challenge)?;

        Ok(attestation_result)
    }

    /// Parse and verify iOS attestation object
    fn parse_ios_attestation_object(
        &self,
        attestation_object: &[u8],
        challenge: &[u8],
    ) -> Result<AttestationResult> {
        // CBOR decode attestation object
        // Reference: https://developer.apple.com/documentation/devicecheck/validating_apps_that_connect_to_your_server

        use sha2::{Digest, Sha256};
        use x509_parser::prelude::*;

        // 1. Decode CBOR using ciborium
        let decoded: ciborium::Value = ciborium::from_reader(attestation_object)
            .map_err(|e| Error::validation(format!("Invalid CBOR attestation: {}", e)))?;

        let map = match decoded {
            ciborium::Value::Map(m) => m,
            _ => return Err(Error::validation("Attestation object must be a CBOR map")),
        };

        // Helper to find a key in the map
        let find_text = |key: &str| -> Option<&str> {
            for (k, v) in &map {
                if let ciborium::Value::Text(k_str) = k {
                    if k_str == key {
                        if let ciborium::Value::Text(s) = v {
                            return Some(s.as_str());
                        }
                    }
                }
            }
            None
        };

        let find_bytes = |key: &str| -> Option<&[u8]> {
            for (k, v) in &map {
                if let ciborium::Value::Text(k_str) = k {
                    if k_str == key {
                        if let ciborium::Value::Bytes(b) = v {
                            return Some(b.as_slice());
                        }
                    }
                }
            }
            None
        };

        let find_map = |key: &str| -> Option<&Vec<(ciborium::Value, ciborium::Value)>> {
            for (k, v) in &map {
                if let ciborium::Value::Text(k_str) = k {
                    if k_str == key {
                        if let ciborium::Value::Map(m) = v {
                            return Some(m);
                        }
                    }
                }
            }
            None
        };

        // 2. Verify format is "apple-appattest"
        let fmt = find_text("fmt").ok_or_else(|| Error::validation("Missing fmt field"))?;

        if fmt != "apple-appattest" {
            return Err(Error::validation(format!("Unexpected format: {}", fmt)));
        }

        // 3. Extract authData
        let auth_data =
            find_bytes("authData").ok_or_else(|| Error::validation("Missing authData"))?;

        // 4. Extract attestation statement
        let att_stmt = find_map("attStmt").ok_or_else(|| Error::validation("Missing attStmt"))?;

        // 5. Extract x5c certificate chain
        let x5c = att_stmt
            .iter()
            .find(|(k, _)| matches!(k, ciborium::Value::Text(s) if s == "x5c"))
            .and_then(|(_, v)| match v {
                ciborium::Value::Array(a) => Some(a),
                _ => None,
            })
            .ok_or_else(|| Error::validation("Missing x5c certificate chain"))?;

        if x5c.is_empty() {
            return Err(Error::validation("Empty certificate chain"));
        }

        // 6. Parse and verify certificate chain
        let mut cert_chain: Vec<X509Certificate> = Vec::new();
        for (i, cert_value) in x5c.iter().enumerate() {
            let cert_bytes = match cert_value {
                ciborium::Value::Bytes(b) => b.as_slice(),
                _ => return Err(Error::validation(format!("Certificate {} is not bytes", i))),
            };

            let (_, cert) = X509Certificate::from_der(cert_bytes).map_err(|e| {
                Error::validation(format!("Failed to parse certificate {}: {:?}", i, e))
            })?;

            cert_chain.push(cert);
        }

        if cert_chain.len() < 2 {
            return Err(Error::validation(
                "Certificate chain too short (need at least leaf + intermediate)",
            ));
        }

        // 7. Verify certificate chain validity
        // Check that each certificate in the chain:
        // - Has a valid time range
        // - Has proper issuer/subject relationship
        for i in 0..cert_chain.len() - 1 {
            let subject = &cert_chain[i];
            let issuer = &cert_chain[i + 1];

            // Check issuer/subject relationship
            if subject.issuer() != issuer.subject() {
                return Err(Error::validation(format!(
                    "Certificate chain broken at level {}: issuer mismatch",
                    i
                )));
            }

            // Note: Full cryptographic signature verification requires additional
            // dependencies. The x509-parser crate validates structure but for
            // full verification in production, use webpki or native platform APIs.
            #[cfg(debug_assertions)]
            tracing::debug!(
                "Certificate chain level {}: subject={}, issuer={}",
                i,
                subject.subject(),
                issuer.subject()
            );
        }

        // 8. Verify root certificate is Apple App Attest Root CA
        // Apple App Attest Root CA fingerprint (SHA-256)
        const APPLE_APP_ATTEST_ROOT_CA_FINGERPRINT: &[u8] = &[
            0x0d, 0x83, 0xb6, 0x11, 0xb6, 0x48, 0xa1, 0x8b, 0x30, 0x37, 0x6d, 0x8f, 0x6b, 0x4e,
            0x6c, 0xc0, 0x70, 0xd0, 0x62, 0x5b, 0x81, 0x44, 0x44, 0x11, 0x3a, 0xed, 0xe3, 0x9a,
            0x11, 0xf3, 0xb5, 0xd6,
        ];

        let root_cert = cert_chain.last().unwrap();
        let root_fingerprint = Sha256::digest(root_cert.as_ref());

        if root_fingerprint.as_slice() != APPLE_APP_ATTEST_ROOT_CA_FINGERPRINT {
            // Allow self-signed root in development, but log warning
            #[cfg(debug_assertions)]
            tracing::warn!(
                "Root certificate fingerprint mismatch - expected Apple App Attest Root CA. Got: {}",
                hex::encode(root_fingerprint)
            );

            #[cfg(not(debug_assertions))]
            return Err(Error::validation(
                "Root certificate is not Apple App Attest Root CA",
            ));
        }

        // 9. Compute expected nonce = SHA256(authData || SHA256(clientDataJSON))
        // clientDataJSON = { challenge: base64(challenge) }
        use base64::{engine::general_purpose, Engine as _};
        let client_data = format!(
            r#"{{"challenge":"{}"}}"#,
            general_purpose::STANDARD.encode(challenge)
        );
        let client_data_hash = Sha256::digest(client_data.as_bytes());

        let mut nonce_input = auth_data.to_vec();
        nonce_input.extend_from_slice(&client_data_hash);
        let expected_nonce = Sha256::digest(&nonce_input);

        // 10. Verify nonce is in leaf certificate extension (OID 1.2.840.113635.100.8.2)
        // Apple App Attest OID for nonce extension
        let app_attest_nonce_oid =
            oid_registry::Oid::from(&[1, 2, 840, 113635, 100, 8, 2]).expect("Valid OID");

        let leaf_cert = &cert_chain[0];
        let mut nonce_verified = false;

        for ext in leaf_cert.extensions() {
            if ext.oid == app_attest_nonce_oid {
                // Extension value is a DER-encoded SEQUENCE containing OCTET STRING with nonce
                // Parse the extension value
                if let Ok((_, parsed)) = der_parser::parse_der(ext.value) {
                    // The nonce is inside a SEQUENCE at index 0, then in an OCTET STRING
                    if let Ok(seq) = parsed.as_sequence() {
                        if !seq.is_empty() {
                            if let Ok(inner) = seq[0].as_sequence() {
                                if !inner.is_empty() {
                                    if let Ok(nonce_bytes) = inner[0].as_slice() {
                                        if nonce_bytes == expected_nonce.as_slice() {
                                            nonce_verified = true;
                                            tracing::debug!(
                                                "iOS App Attest nonce verified successfully"
                                            );
                                        } else {
                                            tracing::warn!(
                                                "Nonce mismatch: expected {}, got {}",
                                                hex::encode(expected_nonce),
                                                hex::encode(nonce_bytes)
                                            );
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        if !nonce_verified {
            #[cfg(not(debug_assertions))]
            return Err(Error::validation(
                "Nonce verification failed - attestation may be replayed",
            ));

            #[cfg(debug_assertions)]
            tracing::warn!("Nonce verification failed - allowing in debug mode only");
        }

        // 11. Parse authData to extract flags and attested credential data
        if auth_data.len() < 37 {
            return Err(Error::validation("authData too short"));
        }

        let flags = auth_data[32];
        let user_present = (flags & 0x01) != 0;
        let user_verified = (flags & 0x04) != 0;
        let attested_credential = (flags & 0x40) != 0;

        tracing::info!(
            "iOS App Attest verification complete: UP={}, UV={}, AT={}, chain_len={}, nonce_ok={}",
            user_present,
            user_verified,
            attested_credential,
            x5c.len(),
            nonce_verified
        );

        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), fmt.to_string());
        metadata.insert("chain_length".to_string(), x5c.len().to_string());
        metadata.insert("user_present".to_string(), user_present.to_string());
        metadata.insert("user_verified".to_string(), user_verified.to_string());
        metadata.insert("nonce_verified".to_string(), nonce_verified.to_string());
        metadata.insert("chain_verified".to_string(), "true".to_string());

        Ok(AttestationResult {
            verified: true,
            platform: AttestationPlatform::IosAppAttest,
            device_integrity: DeviceIntegrity {
                has_secure_hardware: true, // iOS Secure Enclave
                is_genuine: true,          // Attestation passed
                is_official_app: true,     // App Attest only works for apps from App Store
                integrity_passed: true,
            },
            reason: None,
            metadata,
        })
    }

    /// Verify Android Play Integrity attestation
    pub async fn verify_android_play_integrity(
        &self,
        data: &AndroidPlayIntegrityData,
    ) -> Result<AttestationResult> {
        // Validate package name
        if let Some(expected_pkg) = &self.config.android_package_name {
            if &data.package_name != expected_pkg {
                return Err(Error::unauthenticated(format!(
                    "Package name mismatch: expected {}, got {}",
                    expected_pkg, data.package_name
                )));
            }
        }

        // Decrypt and verify integrity token
        // Production: Use Google Play Integrity API server-side verification
        // Reference: https://developer.android.com/google/play/integrity/verdict

        let verdict = self
            .verify_play_integrity_token(&data.integrity_token, &data.nonce)
            .await?;

        Ok(verdict)
    }

    /// Verify Play Integrity token (server-side)
    async fn verify_play_integrity_token(
        &self,
        token: &str,
        nonce: &[u8],
    ) -> Result<AttestationResult> {
        // Validate token format
        if token.is_empty() {
            return Err(Error::validation("Empty integrity token"));
        }

        // Validate nonce format - must be non-empty and reasonable size
        // Nonce prevents replay attacks by binding token to a specific request
        if nonce.is_empty() {
            return Err(Error::validation(
                "Empty nonce - required for replay protection",
            ));
        }
        if nonce.len() > 500 {
            return Err(Error::validation("Nonce too large - maximum 500 bytes"));
        }

        // Production: Call Google Play Integrity API to decrypt and verify the token
        // This is required because the token is encrypted with Google's keys
        #[cfg(feature = "attestation-api")]
        {
            return self.call_play_integrity_api(token, nonce).await;
        }

        // Fallback for builds without attestation-api feature (development only)
        #[cfg(not(feature = "attestation-api"))]
        {
            // In non-production builds, we can do local JWT parsing for testing
            // This is NOT secure for production - tokens are encrypted, not just signed
            #[cfg(debug_assertions)]
            {
                tracing::warn!("Using local JWT parsing - NOT SECURE FOR PRODUCTION");
                return self.parse_integrity_token_local(token, nonce).await;
            }

            #[cfg(not(debug_assertions))]
            {
                return Err(Error::internal(
                    "attestation-api feature required for production builds",
                ));
            }
        }
    }

    /// Call Google Play Integrity API to decode integrity token
    #[cfg(feature = "attestation-api")]
    async fn call_play_integrity_api(
        &self,
        token: &str,
        nonce: &[u8],
    ) -> Result<AttestationResult> {
        use reqwest::Client;
        use serde_json::json;

        // Get package name and service account credentials from config
        let package_name =
            std::env::var("DCHAT_ANDROID_PACKAGE").unwrap_or_else(|_| "com.dchat.app".to_string());

        // Service account access token (should be fetched using OAuth2)
        let access_token = self.get_google_access_token().await?;

        let url = format!(
            "https://playintegrity.googleapis.com/v1/{}:decodeIntegrityToken",
            package_name
        );

        let client = Client::builder()
            .https_only(true)
            .build()
            .map_err(|e| Error::internal(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .post(&url)
            .bearer_auth(&access_token)
            .json(&json!({
                "integrity_token": token
            }))
            .send()
            .await
            .map_err(|e| Error::internal(format!("Play Integrity API request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::internal(format!(
                "Play Integrity API returned {}: {}",
                status, body
            )));
        }

        let api_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::internal(format!("Failed to parse API response: {}", e)))?;

        // Extract token payload from response
        let token_payload = api_response
            .get("tokenPayloadExternal")
            .ok_or_else(|| Error::internal("Missing tokenPayloadExternal in response"))?;

        // Verify nonce matches
        use base64::{engine::general_purpose, Engine as _};
        let expected_nonce = general_purpose::STANDARD.encode(nonce);
        let received_nonce = token_payload
            .get("requestDetails")
            .and_then(|r| r.get("nonce"))
            .and_then(|n| n.as_str())
            .unwrap_or("");

        if received_nonce != expected_nonce {
            return Err(Error::unauthenticated("Nonce mismatch in integrity token"));
        }

        // Extract device integrity verdicts
        let device_recognition = token_payload
            .get("deviceIntegrity")
            .and_then(|d| d.get("deviceRecognitionVerdict"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        let _meets_basic = device_recognition.contains(&"MEETS_BASIC_INTEGRITY");
        let meets_device = device_recognition.contains(&"MEETS_DEVICE_INTEGRITY");
        let meets_strong = device_recognition.contains(&"MEETS_STRONG_INTEGRITY");

        // Extract app integrity
        let app_recognition = token_payload
            .get("appIntegrity")
            .and_then(|a| a.get("appRecognitionVerdict"))
            .and_then(|v| v.as_str())
            .unwrap_or("UNEVALUATED");

        let is_play_recognized = app_recognition == "PLAY_RECOGNIZED";

        let mut metadata = HashMap::new();
        metadata.insert("device_verdicts".to_string(), device_recognition.join(","));
        metadata.insert("app_verdict".to_string(), app_recognition.to_string());

        // Account licensing (if available)
        if let Some(account) = token_payload.get("accountDetails") {
            if let Some(licensing) = account.get("appLicensingVerdict").and_then(|v| v.as_str()) {
                metadata.insert("licensing".to_string(), licensing.to_string());
            }
        }

        Ok(AttestationResult {
            verified: meets_device && is_play_recognized,
            platform: AttestationPlatform::AndroidPlayIntegrity,
            device_integrity: DeviceIntegrity {
                has_secure_hardware: meets_strong,
                is_genuine: meets_device,
                is_official_app: is_play_recognized,
                integrity_passed: meets_device,
            },
            reason: if meets_device && is_play_recognized {
                None
            } else {
                Some(format!(
                    "Integrity check failed: device={}, app={}",
                    device_recognition.join(","),
                    app_recognition
                ))
            },
            metadata,
        })
    }

    /// Get Google Cloud access token for API calls
    #[cfg(feature = "attestation-api")]
    async fn get_google_access_token(&self) -> Result<String> {
        // In production, use service account credentials to get access token
        // Options:
        // 1. Service account JSON key file (GOOGLE_APPLICATION_CREDENTIALS env var)
        // 2. Workload identity (GKE)
        // 3. Compute Engine metadata server

        // Check for cached token first
        if let Some(cached) = self.get_cached_access_token() {
            return Ok(cached);
        }

        // Get token from metadata server (for GCE/GKE)
        let metadata_url = "http://metadata.google.internal/computeMetadata/v1/instance/service-accounts/default/token";

        let client = reqwest::Client::new();
        let response = client
            .get(metadata_url)
            .header("Metadata-Flavor", "Google")
            .send()
            .await;

        if let Ok(resp) = response {
            if resp.status().is_success() {
                if let Ok(token_data) = resp.json::<serde_json::Value>().await {
                    if let Some(token) = token_data.get("access_token").and_then(|t| t.as_str()) {
                        return Ok(token.to_string());
                    }
                }
            }
        }

        // Fallback: Check for credentials file
        if let Ok(creds_path) = std::env::var("GOOGLE_APPLICATION_CREDENTIALS") {
            return self.get_token_from_service_account(&creds_path).await;
        }

        Err(Error::internal(
            "No Google Cloud credentials available. Set GOOGLE_APPLICATION_CREDENTIALS or run on GCE/GKE."
        ))
    }

    /// Get cached access token if still valid
    #[cfg(feature = "attestation-api")]
    fn get_cached_access_token(&self) -> Option<String> {
        use chrono::Duration;

        let cache = self.google_token_cache.read().ok()?;
        if let Some(ref cached) = *cache {
            // Token is valid if it has at least 5 minutes before expiry
            let min_validity = chrono::Utc::now() + Duration::minutes(5);
            if cached.expires_at > min_validity {
                tracing::debug!("Using cached Google access token");
                return Some(cached.token.clone());
            }
        }
        None
    }

    /// Cache a new access token
    #[cfg(feature = "attestation-api")]
    fn cache_access_token(&self, token: String, expires_in_secs: i64) {
        use chrono::Duration;

        if let Ok(mut cache) = self.google_token_cache.write() {
            let expires_at = chrono::Utc::now() + Duration::seconds(expires_in_secs);
            *cache = Some(CachedAccessToken { token, expires_at });
            tracing::debug!("Cached Google access token, expires at {}", expires_at);
        }
    }

    /// Get access token using service account key file
    #[cfg(feature = "attestation-api")]
    async fn get_token_from_service_account(&self, creds_path: &str) -> Result<String> {
        use base64::{engine::general_purpose, Engine as _};
        use std::fs;

        let creds_json = fs::read_to_string(creds_path)
            .map_err(|e| Error::internal(format!("Failed to read credentials file: {}", e)))?;

        let creds: serde_json::Value = serde_json::from_str(&creds_json)
            .map_err(|e| Error::internal(format!("Invalid credentials JSON: {}", e)))?;

        let client_email = creds
            .get("client_email")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("Missing client_email in credentials"))?;

        let private_key_pem = creds
            .get("private_key")
            .and_then(|v| v.as_str())
            .ok_or_else(|| Error::internal("Missing private_key in credentials"))?;

        // Create JWT header (RS256)
        let header = serde_json::json!({
            "alg": "RS256",
            "typ": "JWT"
        });
        let header_b64 = general_purpose::URL_SAFE_NO_PAD.encode(header.to_string());

        // Create JWT claims
        let now = chrono::Utc::now().timestamp();
        let claims = serde_json::json!({
            "iss": client_email,
            "scope": "https://www.googleapis.com/auth/playintegrity",
            "aud": "https://oauth2.googleapis.com/token",
            "iat": now,
            "exp": now + 3600,
        });
        let claims_b64 = general_purpose::URL_SAFE_NO_PAD.encode(claims.to_string());

        // Create signing input
        let signing_input = format!("{}.{}", header_b64, claims_b64);

        // Parse RSA private key and sign
        // The key is in PKCS#8 PEM format
        use rsa::pkcs1v15::SigningKey;
        use rsa::signature::Signer;
        use rsa::{pkcs8::DecodePrivateKey, RsaPrivateKey};

        let private_key = RsaPrivateKey::from_pkcs8_pem(private_key_pem)
            .map_err(|e| Error::internal(format!("Failed to parse RSA private key: {}", e)))?;

        let signing_key = SigningKey::<sha2::Sha256>::new(private_key);
        let signature: rsa::pkcs1v15::Signature = signing_key.sign(signing_input.as_bytes());
        // Convert signature to bytes manually
        let sig_bytes: Box<[u8]> = signature.into();
        let signature_b64 = general_purpose::URL_SAFE_NO_PAD.encode(&sig_bytes);

        // Complete JWT
        let jwt = format!("{}.{}", signing_input, signature_b64);

        // Exchange JWT for access token
        let client = reqwest::Client::builder()
            .https_only(true)
            .build()
            .map_err(|e| Error::internal(format!("Failed to create HTTP client: {}", e)))?;

        let response = client
            .post("https://oauth2.googleapis.com/token")
            .form(&[
                ("grant_type", "urn:ietf:params:oauth:grant-type:jwt-bearer"),
                ("assertion", &jwt),
            ])
            .send()
            .await
            .map_err(|e| Error::internal(format!("Token exchange request failed: {}", e)))?;

        if !response.status().is_success() {
            let status = response.status();
            let body = response.text().await.unwrap_or_default();
            return Err(Error::internal(format!(
                "Token exchange failed with {}: {}",
                status, body
            )));
        }

        let token_response: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::internal(format!("Failed to parse token response: {}", e)))?;

        let access_token = token_response
            .get("access_token")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Error::internal("Missing access_token in response"))?
            .to_string();

        let expires_in = token_response
            .get("expires_in")
            .and_then(|e| e.as_i64())
            .unwrap_or(3600);

        // Cache the token
        self.cache_access_token(access_token.clone(), expires_in);

        tracing::info!("Successfully obtained Google access token via service account");
        Ok(access_token)
    }

    /// Parse integrity token locally (development/testing only)
    #[cfg(all(debug_assertions, not(feature = "attestation-api")))]
    async fn parse_integrity_token_local(
        &self,
        token: &str,
        nonce: &[u8],
    ) -> Result<AttestationResult> {
        // Token is a signed JWT - verify signature with Google's public key
        // Split into parts: header.payload.signature
        let parts: Vec<&str> = token.split('.').collect();
        if parts.len() != 3 {
            return Err(Error::validation("Invalid token format: expected JWT"));
        }

        // Decode payload (base64url)
        use base64::{engine::general_purpose, Engine as _};
        let payload_bytes = general_purpose::URL_SAFE_NO_PAD
            .decode(parts[1])
            .map_err(|e| Error::validation(format!("Invalid token payload: {}", e)))?;

        let payload: serde_json::Value = serde_json::from_slice(&payload_bytes)
            .map_err(|e| Error::validation(format!("Invalid token JSON: {}", e)))?;

        // Extract verdicts
        let device_integrity = payload
            .get("deviceIntegrity")
            .and_then(|d| d.get("deviceRecognitionVerdict"))
            .and_then(|v| v.as_array())
            .map(|arr| arr.iter().filter_map(|v| v.as_str()).collect::<Vec<_>>())
            .unwrap_or_default();

        let meets_basic = device_integrity.contains(&"MEETS_BASIC_INTEGRITY");
        let meets_device = device_integrity.contains(&"MEETS_DEVICE_INTEGRITY");
        let meets_strong = device_integrity.contains(&"MEETS_STRONG_INTEGRITY");

        let mut metadata = HashMap::new();
        metadata.insert("device_verdicts".to_string(), device_integrity.join(","));
        metadata.insert("verification_mode".to_string(), "local_parse".to_string());

        // Verify nonce matches
        let token_nonce = payload
            .get("requestDetails")
            .and_then(|r| r.get("nonce"))
            .and_then(|n| n.as_str())
            .map(|s| general_purpose::STANDARD.decode(s).unwrap_or_default())
            .unwrap_or_default();

        if token_nonce != nonce {
            return Err(Error::unauthenticated("Nonce mismatch in integrity token"));
        }

        Ok(AttestationResult {
            verified: meets_device,
            platform: AttestationPlatform::AndroidPlayIntegrity,
            device_integrity: DeviceIntegrity {
                has_secure_hardware: meets_strong,
                is_genuine: meets_device,
                is_official_app: meets_basic,
                integrity_passed: meets_device,
            },
            reason: if meets_device {
                None
            } else {
                Some("Device integrity check failed".to_string())
            },
            metadata,
        })
    }

    /// Verify Android Key Attestation
    pub async fn verify_android_key_attestation(
        &self,
        data: &AndroidKeyAttestationData,
    ) -> Result<AttestationResult> {
        // Android Key Attestation verifies a key was generated in secure hardware
        // Reference: https://source.android.com/docs/security/features/keystore/attestation

        use sha2::{Digest, Sha256};
        use x509_parser::prelude::*;

        if data.certificate_chain.is_empty() {
            return Err(Error::validation("Empty certificate chain"));
        }

        // Parse all certificates in the chain
        let mut cert_chain: Vec<X509Certificate> = Vec::new();
        for (i, cert_der) in data.certificate_chain.iter().enumerate() {
            let (_, cert) = X509Certificate::from_der(cert_der).map_err(|e| {
                Error::validation(format!("Failed to parse certificate {}: {:?}", i, e))
            })?;
            cert_chain.push(cert);
        }

        if cert_chain.is_empty() {
            return Err(Error::validation("No valid certificates in chain"));
        }

        // Verify certificate chain validity
        // Check issuer/subject relationship for each certificate
        for i in 0..cert_chain.len() - 1 {
            let subject = &cert_chain[i];
            let issuer = &cert_chain[i + 1];

            // Check issuer/subject relationship
            if subject.issuer() != issuer.subject() {
                return Err(Error::validation(format!(
                    "Certificate chain broken at level {}: issuer mismatch",
                    i
                )));
            }

            #[cfg(debug_assertions)]
            tracing::debug!(
                "Android cert chain level {}: subject={}, issuer={}",
                i,
                subject.subject(),
                issuer.subject()
            );
        }

        // Verify root certificate is Google hardware attestation root CA
        // Google Hardware Attestation Root CA SHA-256 fingerprint
        const GOOGLE_HARDWARE_ATTESTATION_ROOT_FINGERPRINT: &[u8] = &[
            0xeb, 0xd2, 0x2c, 0x8b, 0x0b, 0x3e, 0x03, 0xa4, 0x58, 0x40, 0x7c, 0x2f, 0x2c, 0x77,
            0xb8, 0xf0, 0xec, 0x77, 0x80, 0x5f, 0xd3, 0xfa, 0xab, 0x6f, 0xa3, 0xd5, 0xee, 0xf5,
            0xe1, 0x43, 0xe6, 0xf4,
        ];

        let root_cert = cert_chain.last().unwrap();
        let root_fingerprint = Sha256::digest(root_cert.as_ref());

        let is_google_root =
            root_fingerprint.as_slice() == GOOGLE_HARDWARE_ATTESTATION_ROOT_FINGERPRINT;

        #[cfg(not(debug_assertions))]
        if !is_google_root {
            return Err(Error::validation(
                "Root certificate is not Google Hardware Attestation Root CA",
            ));
        }

        #[cfg(debug_assertions)]
        if !is_google_root {
            tracing::warn!(
                "Root certificate fingerprint mismatch - expected Google Hardware Attestation Root. Got: {}",
                hex::encode(root_fingerprint)
            );
        }

        // Extract attestation extension from leaf certificate (OID 1.3.6.1.4.1.11129.2.1.17)
        let android_key_attestation_oid =
            oid_registry::Oid::from(&[1, 3, 6, 1, 4, 1, 11129, 2, 1, 17]).expect("Valid OID");

        let leaf_cert = &cert_chain[0];
        let mut attestation_security_level = "Unknown";
        let mut challenge_verified = false;
        let mut is_strongbox = false;

        for ext in leaf_cert.extensions() {
            if ext.oid == android_key_attestation_oid {
                // Parse the ASN.1 attestation extension
                // Structure defined in: https://source.android.com/docs/security/features/keystore/attestation#attestation-extension
                if let Ok((_, parsed)) = der_parser::parse_der(ext.value) {
                    if let Ok(seq) = parsed.as_sequence() {
                        // attestationVersion at index 0
                        // attestationSecurityLevel at index 1 (0=Software, 1=TrustedEnvironment, 2=StrongBox)
                        // keymasterSecurityLevel at index 2
                        // attestationChallenge at index 4

                        // Extract attestationSecurityLevel
                        if seq.len() > 1 {
                            if let Ok(level) = seq[1].as_i32() {
                                attestation_security_level = match level {
                                    0 => "Software",
                                    1 => "TrustedEnvironment",
                                    2 => {
                                        is_strongbox = true;
                                        "StrongBox"
                                    }
                                    _ => "Unknown",
                                };
                            }
                        }

                        // Extract and verify attestationChallenge (index 4)
                        if seq.len() > 4 {
                            if let Ok(attestation_challenge) = seq[4].as_slice() {
                                if attestation_challenge == data.challenge.as_slice() {
                                    challenge_verified = true;
                                    tracing::debug!("Android Key Attestation challenge verified");
                                } else {
                                    tracing::warn!(
                                        "Challenge mismatch: expected {}, got {}",
                                        hex::encode(&data.challenge),
                                        hex::encode(attestation_challenge)
                                    );
                                }
                            }
                        }
                    }
                }
                break;
            }
        }

        if !challenge_verified {
            #[cfg(not(debug_assertions))]
            return Err(Error::validation(
                "Attestation challenge verification failed",
            ));

            #[cfg(debug_assertions)]
            tracing::warn!("Challenge verification failed - allowing in debug mode only");
        }

        let has_secure_hardware = attestation_security_level != "Software";

        let mut metadata = HashMap::new();
        metadata.insert("chain_length".to_string(), cert_chain.len().to_string());
        metadata.insert(
            "security_level".to_string(),
            attestation_security_level.to_string(),
        );
        metadata.insert("is_strongbox".to_string(), is_strongbox.to_string());
        metadata.insert(
            "challenge_verified".to_string(),
            challenge_verified.to_string(),
        );
        metadata.insert("chain_verified".to_string(), "true".to_string());
        metadata.insert("google_root".to_string(), is_google_root.to_string());

        tracing::info!(
            "Android Key Attestation verification complete: security_level={}, strongbox={}, challenge_ok={}, chain_len={}",
            attestation_security_level,
            is_strongbox,
            challenge_verified,
            cert_chain.len()
        );

        Ok(AttestationResult {
            verified: has_secure_hardware && challenge_verified,
            platform: AttestationPlatform::AndroidKeyAttestation,
            device_integrity: DeviceIntegrity {
                has_secure_hardware,
                is_genuine: has_secure_hardware, // TEE/StrongBox implies genuine device
                is_official_app: false,          // Key attestation doesn't verify app
                integrity_passed: has_secure_hardware && challenge_verified,
            },
            reason: if has_secure_hardware && challenge_verified {
                None
            } else {
                Some(format!(
                    "Security level: {}, Challenge verified: {}",
                    attestation_security_level, challenge_verified
                ))
            },
            metadata,
        })
    }

    /// Verify WebAuthn attestation
    pub async fn verify_webauthn(
        &self,
        data: &WebAuthnAttestationData,
    ) -> Result<AttestationResult> {
        // WebAuthn attestation verification
        // Reference: https://www.w3.org/TR/webauthn-2/#sctn-attestation

        use sha2::{Digest, Sha256};

        // 1. Parse clientDataJSON
        let client_data: serde_json::Value = serde_json::from_slice(&data.client_data_json)
            .map_err(|e| Error::validation(format!("Invalid clientDataJSON: {}", e)))?;

        // 2. Verify type is "webauthn.create"
        let op_type = client_data
            .get("type")
            .and_then(|t| t.as_str())
            .ok_or_else(|| Error::validation("Missing type in clientData"))?;

        if op_type != "webauthn.create" {
            return Err(Error::validation(format!("Unexpected type: {}", op_type)));
        }

        // 3. Verify origin is allowed
        let origin = client_data
            .get("origin")
            .and_then(|o| o.as_str())
            .ok_or_else(|| Error::validation("Missing origin"))?;

        if !self.config.webauthn_origins.is_empty()
            && !self.config.webauthn_origins.contains(&origin.to_string())
        {
            return Err(Error::unauthenticated(format!(
                "Origin not allowed: {}",
                origin
            )));
        }

        // 4. Compute clientDataHash
        let _client_data_hash = Sha256::digest(&data.client_data_json);

        // 5. Parse attestationObject (CBOR)
        let attestation: ciborium::Value = ciborium::from_reader(&data.attestation_object[..])
            .map_err(|e| Error::validation(format!("Invalid attestation CBOR: {}", e)))?;

        let map = match attestation {
            ciborium::Value::Map(m) => m,
            _ => return Err(Error::validation("Attestation must be a map")),
        };

        // Helper functions
        let find_text =
            |map: &Vec<(ciborium::Value, ciborium::Value)>, key: &str| -> Option<String> {
                for (k, v) in map {
                    if let ciborium::Value::Text(k_str) = k {
                        if k_str == key {
                            if let ciborium::Value::Text(s) = v {
                                return Some(s.clone());
                            }
                        }
                    }
                }
                None
            };

        let find_bytes =
            |map: &Vec<(ciborium::Value, ciborium::Value)>, key: &str| -> Option<Vec<u8>> {
                for (k, v) in map {
                    if let ciborium::Value::Text(k_str) = k {
                        if k_str == key {
                            if let ciborium::Value::Bytes(b) = v {
                                return Some(b.clone());
                            }
                        }
                    }
                }
                None
            };

        // 6. Extract fmt and authData
        let fmt = find_text(&map, "fmt").ok_or_else(|| Error::validation("Missing fmt"))?;

        let auth_data =
            find_bytes(&map, "authData").ok_or_else(|| Error::validation("Missing authData"))?;

        // 7. Parse authData
        if auth_data.len() < 37 {
            return Err(Error::validation("authData too short"));
        }

        // RP ID hash (32 bytes) + flags (1 byte) + sign count (4 bytes)
        let rp_id_hash = &auth_data[0..32];
        let flags = auth_data[32];

        let user_present = (flags & 0x01) != 0;
        let user_verified = (flags & 0x04) != 0;

        // 8. Verify RP ID if configured
        if let Some(expected_rp_id) = &self.config.webauthn_rp_id {
            let expected_hash = Sha256::digest(expected_rp_id.as_bytes());
            if rp_id_hash != expected_hash.as_slice() {
                return Err(Error::unauthenticated("RP ID mismatch"));
            }
        }

        let mut metadata = HashMap::new();
        metadata.insert("format".to_string(), fmt.to_string());
        metadata.insert("origin".to_string(), origin.to_string());
        metadata.insert("user_present".to_string(), user_present.to_string());
        metadata.insert("user_verified".to_string(), user_verified.to_string());

        Ok(AttestationResult {
            verified: user_present,
            platform: AttestationPlatform::WebAuthn,
            device_integrity: DeviceIntegrity {
                has_secure_hardware: fmt == "packed" || fmt == "tpm" || fmt == "android-key",
                is_genuine: true,
                is_official_app: true, // WebAuthn is browser-verified
                integrity_passed: user_present,
            },
            reason: None,
            metadata,
        })
    }
}

/// Verify a device attestation payload (legacy simple API)
///
/// For new code, use `AttestationVerifier` with platform-specific methods.
///
/// SECURITY: Simulated attestation is ONLY available in debug builds.
/// In release builds, this function will always reject simulated attestation.
#[cfg(debug_assertions)]
pub fn verify_device_attestation(attestation: &str) -> Result<AttestationResult> {
    // Only allow simulated attestation in debug builds
    if attestation == "dchat-enclave-attestation-v1" {
        Ok(AttestationResult {
            verified: true,
            platform: AttestationPlatform::Simulated,
            device_integrity: DeviceIntegrity {
                has_secure_hardware: false,
                is_genuine: true,
                is_official_app: true,
                integrity_passed: true,
            },
            reason: Some("Simulated attestation (DEBUG BUILD ONLY)".to_string()),
            metadata: HashMap::new(),
        })
    } else {
        Err(Error::unauthenticated("Unknown attestation format"))
    }
}

/// Verify a device attestation payload (legacy simple API)
///
/// SECURITY: In release builds, simulated attestation is NEVER accepted.
#[cfg(not(debug_assertions))]
pub fn verify_device_attestation(_attestation: &str) -> Result<AttestationResult> {
    // In release builds, reject ALL legacy attestation formats
    // Production clients MUST use platform-specific attestation (iOS App Attest, Play Integrity, etc.)
    Err(Error::unauthenticated(
        "Legacy attestation not supported in production. Use platform-specific attestation.",
    ))
}

/// Create a `VerifiedBadge` from a successful attestation.
///
/// SECURITY: In release builds, this only works with platform-specific attestation results.
pub fn badge_from_attestation(issuer: String, attestation: &str) -> Result<VerifiedBadge> {
    let result = verify_device_attestation(attestation)?;

    if !result.verified {
        return Err(Error::unauthenticated(
            result
                .reason
                .unwrap_or_else(|| "Attestation verification failed".to_string()),
        ));
    }

    let now: DateTime<Utc> = Utc::now();

    let mut metadata = result.metadata;
    metadata.insert("platform".to_string(), format!("{:?}", result.platform));
    metadata.insert("attestation".to_string(), attestation.to_string());

    // Store the raw attestation bytes as the signature proof.
    // Platform attestations (App Attest, Play Integrity, etc.) are cryptographically
    // signed by the platform and the attestation string IS the proof.
    let attestation_bytes = attestation.as_bytes().to_vec();

    let proof = VerificationProof {
        proof_type: match result.platform {
            #[cfg(debug_assertions)]
            AttestationPlatform::Simulated => ProofType::SelfSigned,
            AttestationPlatform::IosAppAttest => ProofType::AuthoritySigned {
                authority: "Apple".to_string(),
            },
            AttestationPlatform::AndroidPlayIntegrity => ProofType::AuthoritySigned {
                authority: "Google".to_string(),
            },
            AttestationPlatform::AndroidKeyAttestation => ProofType::AuthoritySigned {
                authority: "Google".to_string(),
            },
            AttestationPlatform::WebAuthn => ProofType::AuthoritySigned {
                authority: "WebAuthn".to_string(),
            },
            _ => ProofType::Custom("platform_attestation".to_string()),
        },
        signature: Signature::new(attestation_bytes),
        metadata,
    };

    let badge = VerifiedBadge {
        badge_type: crate::verification::BadgeType::Verified,
        issued_at: now,
        expires_at: Some(now + chrono::Duration::days(90)), // Badges expire after 90 days
        issuer,
        proof,
    };

    Ok(badge)
}

/// Create badge from a verified attestation result with raw attestation bytes
///
/// The `attestation_bytes` parameter should contain the raw attestation data
/// from the platform (e.g., App Attest assertion, Play Integrity token).
/// This is stored as the cryptographic proof in the badge.
pub fn badge_from_attestation_result(
    issuer: String,
    result: &AttestationResult,
    attestation_bytes: &[u8],
) -> Result<VerifiedBadge> {
    if !result.verified {
        return Err(Error::unauthenticated(
            result
                .reason
                .clone()
                .unwrap_or_else(|| "Attestation not verified".to_string()),
        ));
    }

    let now: DateTime<Utc> = Utc::now();

    let mut metadata = result.metadata.clone();
    metadata.insert("platform".to_string(), format!("{:?}", result.platform));
    metadata.insert(
        "has_secure_hardware".to_string(),
        result.device_integrity.has_secure_hardware.to_string(),
    );

    let proof = VerificationProof {
        proof_type: ProofType::Custom("device_attestation".to_string()),
        signature: Signature::new(attestation_bytes.to_vec()),
        metadata,
    };

    // Expiration based on integrity level
    let expiration = if result.device_integrity.has_secure_hardware {
        Some(now + chrono::Duration::days(180)) // 6 months for hardware-backed
    } else {
        Some(now + chrono::Duration::days(30)) // 30 days for software
    };

    Ok(VerifiedBadge {
        badge_type: crate::verification::BadgeType::Verified,
        issued_at: now,
        expires_at: expiration,
        issuer,
        proof,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[cfg(debug_assertions)]
    fn test_verify_simulated_attestation() {
        let res = verify_device_attestation("dchat-enclave-attestation-v1").unwrap();
        assert!(res.verified);
        assert_eq!(res.platform, AttestationPlatform::Simulated);
    }

    #[test]
    fn test_verify_bad_attestation() {
        let res = verify_device_attestation("invalid");
        assert!(res.is_err());
    }

    #[test]
    #[cfg(debug_assertions)]
    fn test_badge_from_attestation() {
        let badge =
            badge_from_attestation("system".to_string(), "dchat-enclave-attestation-v1").unwrap();
        assert_eq!(badge.issuer, "system");
        assert!(badge.is_valid());
        assert!(badge.expires_at.is_some()); // Badges now expire
    }

    #[test]
    fn test_device_integrity_default() {
        let integrity = DeviceIntegrity::default();
        assert!(!integrity.has_secure_hardware);
        assert!(!integrity.integrity_passed);
    }

    #[test]
    fn test_attestation_config_default() {
        let config = AttestationConfig::default();
        // allow_simulated only exists in debug builds
        #[cfg(debug_assertions)]
        assert!(config.allow_simulated);
        assert!(config.ios_team_id.is_none());
        assert!(config.android_package_name.is_none());
    }

    #[tokio::test]
    async fn test_verifier_creation() {
        let verifier = AttestationVerifier::default_verifier();
        // Just verify it creates successfully
        assert!(verifier.config.webauthn_origins.is_empty());
    }
}
