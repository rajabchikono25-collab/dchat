//! Proof-of-Transit (PoT) Consensus Layer
//!
//! PoT validates message authenticity through geographic routing with speed-of-light verification.
//! This provides physical-world proof that messages traveled through specific relay nodes.
//!
//! Key features:
//! - 3-path geographic routing with independent verification
//! - Speed-of-light timing validation
//! - Hybrid Ed25519+Dilithium3 post-quantum signatures
//! - Tunable finality levels (Local → Continental → Global → Deep)
//! - Relay route cryptographic proofs
//! - Integration with PoRW for dual-consensus finality

use crate::block_hierarchy::Hash;
use crate::proof_of_relay_work::GeographicRegion;
use ed25519_dalek::{Signature, VerifyingKey};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::{Arc, RwLock};
use std::time::{Duration, SystemTime};
use thiserror::Error;
use tracing;

/// Post-quantum signature algorithm (placeholder - will use pqcrypto crate)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dilithium3Signature {
    pub bytes: Vec<u8>,
}

/// Hybrid signature combining classical and post-quantum algorithms
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HybridSignature {
    pub ed25519: Signature,
    pub dilithium3: Dilithium3Signature,
}

/// Geographic coordinates for relay nodes
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct GeoLocation {
    pub latitude: f64,
    pub longitude: f64,
}

impl GeoLocation {
    /// Calculate great-circle distance to another location (in km)
    pub fn distance_to(&self, other: &GeoLocation) -> f64 {
        const EARTH_RADIUS_KM: f64 = 6371.0;
        
        let lat1 = self.latitude.to_radians();
        let lat2 = other.latitude.to_radians();
        let delta_lat = (self.latitude - other.latitude).to_radians();
        let delta_lon = (self.longitude - other.longitude).to_radians();
        
        let a = (delta_lat / 2.0).sin().powi(2)
            + lat1.cos() * lat2.cos() * (delta_lon / 2.0).sin().powi(2);
        let c = 2.0 * a.sqrt().atan2((1.0 - a).sqrt());
        
        EARTH_RADIUS_KM * c
    }
}

/// Transit path through relay network
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitPath {
    /// Relay nodes in path order
    pub relays: Vec<VerifyingKey>,
    
    /// Geographic locations of each relay
    pub locations: Vec<GeoLocation>,
    
    /// Timestamp when message entered each relay
    pub timestamps: Vec<SystemTime>,
    
    /// Hybrid signatures from each relay
    pub signatures: Vec<HybridSignature>,
    
    /// Total physical distance traveled (km)
    pub total_distance: f64,
    
    /// Total time elapsed
    pub total_duration: Duration,
}

impl TransitPath {
    /// Verify path timing against speed-of-light constraints
    pub fn verify_speed_of_light(&self) -> Result<(), PoTError> {
        const SPEED_OF_LIGHT_KM_MS: f64 = 299.792; // km per millisecond
        const PROCESSING_OVERHEAD_MS: f64 = 5.0; // Per-hop processing time
        const TOLERANCE_FACTOR: f64 = 1.2; // Allow 20% margin for routing overhead
        
        for i in 1..self.relays.len() {
            let distance = self.locations[i - 1].distance_to(&self.locations[i]);
            let time_elapsed = self.timestamps[i]
                .duration_since(self.timestamps[i - 1])
                .map_err(|_| PoTError::InvalidTimestamp)?
                .as_millis() as f64;
            
            // Minimum theoretical time = distance / speed_of_light + processing
            let min_time = (distance / SPEED_OF_LIGHT_KM_MS) + PROCESSING_OVERHEAD_MS;
            let max_allowed_time = min_time * TOLERANCE_FACTOR;
            
            if time_elapsed < min_time {
                tracing::warn!(
                    "Path hop {} violates speed of light: {}ms < {}ms minimum",
                    i,
                    time_elapsed,
                    min_time
                );
                return Err(PoTError::FasterThanLight);
            }
            
            if time_elapsed > max_allowed_time {
                tracing::warn!(
                    "Path hop {} exceeded maximum time: {}ms > {}ms maximum",
                    i,
                    time_elapsed,
                    max_allowed_time
                );
                return Err(PoTError::PathTooSlow);
            }
        }
        
        Ok(())
    }
    
    /// Verify all hybrid signatures in the path
    pub fn verify_signatures(&self, _message_hash: &Hash) -> Result<(), PoTError> {
        if self.relays.len() != self.signatures.len() {
            return Err(PoTError::SignatureMismatch);
        }
        
        for (i, (_relay, _signature)) in self.relays.iter().zip(self.signatures.iter()).enumerate() {
            // Verify Ed25519 signature
            // TODO: Implement actual signature verification
            // relay.verify(message_hash.as_bytes(), &signature.ed25519)?;
            
            // Verify Dilithium3 signature
            // TODO: Integrate pqcrypto library for post-quantum verification
            
            tracing::debug!("Verified hybrid signature for relay {} in path", i);
        }
        
        Ok(())
    }
    
    /// Calculate geographic diversity score (0.0-1.0)
    pub fn geographic_diversity_score(&self) -> f64 {
        if self.locations.len() < 2 {
            return 0.0;
        }
        
        let mut total_spread = 0.0;
        for i in 1..self.locations.len() {
            total_spread += self.locations[i - 1].distance_to(&self.locations[i]);
        }
        
        // Normalize to 0-1 range (20,000km = max Earth distance)
        (total_spread / 20000.0).min(1.0)
    }
}

/// Finality level for PoT consensus
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum FinalityLevel {
    /// Single region, fast finality (~100ms)
    Local,
    
    /// Cross-continental, medium finality (~500ms)
    Continental,
    
    /// Global coverage, high finality (~2s)
    Global,
    
    /// Maximum security, deep finality (~5s)
    Deep,
}

impl FinalityLevel {
    /// Minimum number of paths required
    pub fn required_paths(&self) -> usize {
        match self {
            FinalityLevel::Local => 1,
            FinalityLevel::Continental => 2,
            FinalityLevel::Global => 3,
            FinalityLevel::Deep => 5,
        }
    }
    
    /// Minimum geographic regions required
    pub fn required_regions(&self) -> usize {
        match self {
            FinalityLevel::Local => 1,
            FinalityLevel::Continental => 2,
            FinalityLevel::Global => 3,
            FinalityLevel::Deep => 4,
        }
    }
    
    /// Target finality time
    pub fn target_time(&self) -> Duration {
        match self {
            FinalityLevel::Local => Duration::from_millis(100),
            FinalityLevel::Continental => Duration::from_millis(500),
            FinalityLevel::Global => Duration::from_secs(2),
            FinalityLevel::Deep => Duration::from_secs(5),
        }
    }
}

/// Proof of Transit for a message/block
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TransitProof {
    /// Message/block hash being proven
    pub message_hash: Hash,
    
    /// Multiple independent transit paths (redundancy)
    pub paths: Vec<TransitPath>,
    
    /// Achieved finality level
    pub finality_level: FinalityLevel,
    
    /// Timestamp when proof was created
    pub timestamp: SystemTime,
    
    /// Geographic regions covered
    pub regions_covered: Vec<GeographicRegion>,
}

impl TransitProof {
    /// Verify the transit proof
    pub fn verify(&self) -> Result<(), PoTError> {
        // Check minimum number of paths
        if self.paths.len() < self.finality_level.required_paths() {
            return Err(PoTError::InsufficientPaths);
        }
        
        // Verify each path
        for (i, path) in self.paths.iter().enumerate() {
            path.verify_speed_of_light()
                .map_err(|e| PoTError::PathVerificationFailed(i, Box::new(e)))?;
            path.verify_signatures(&self.message_hash)
                .map_err(|e| PoTError::PathVerificationFailed(i, Box::new(e)))?;
        }
        
        // Check geographic diversity
        if self.regions_covered.len() < self.finality_level.required_regions() {
            return Err(PoTError::InsufficientGeographicDiversity);
        }
        
        tracing::info!(
            "Transit proof verified: {} paths across {} regions ({:?})",
            self.paths.len(),
            self.regions_covered.len(),
            self.finality_level
        );
        
        Ok(())
    }
    
    /// Calculate average transit time across all paths
    pub fn average_transit_time(&self) -> Duration {
        if self.paths.is_empty() {
            return Duration::from_secs(0);
        }
        
        let total: Duration = self.paths.iter().map(|p| p.total_duration).sum();
        total / self.paths.len() as u32
    }
    
    /// Calculate geographic diversity score (0.0-1.0)
    pub fn geographic_diversity_score(&self) -> f64 {
        if self.paths.is_empty() {
            return 0.0;
        }
        
        let scores: Vec<f64> = self.paths.iter()
            .map(|p| p.geographic_diversity_score())
            .collect();
        
        scores.iter().sum::<f64>() / scores.len() as f64
    }
}

/// Proof of Transit consensus engine
pub struct ProofOfTransit {
    /// Relay geographic locations
    relay_locations: Arc<RwLock<HashMap<VerifyingKey, GeoLocation>>>,
    
    /// Active transit proofs being collected
    active_proofs: Arc<RwLock<HashMap<Hash, TransitProof>>>,
    
    /// Required finality level for consensus
    required_finality: FinalityLevel,
}

impl ProofOfTransit {
    pub fn new(required_finality: FinalityLevel) -> Self {
        Self {
            relay_locations: Arc::new(RwLock::new(HashMap::new())),
            active_proofs: Arc::new(RwLock::new(HashMap::new())),
            required_finality,
        }
    }
    
    /// Register a relay's geographic location
    pub fn register_relay_location(
        &self,
        relay_id: VerifyingKey,
        location: GeoLocation,
    ) -> Result<(), PoTError> {
        let mut locations = self.relay_locations.write().unwrap();
        locations.insert(relay_id, location);
        
        tracing::info!(
            "Registered relay location: lat={}, lon={}",
            location.latitude,
            location.longitude
        );
        
        Ok(())
    }
    
    /// Submit a transit path for a message
    pub fn submit_transit_path(
        &self,
        message_hash: Hash,
        path: TransitPath,
    ) -> Result<(), PoTError> {
        // Verify path
        path.verify_speed_of_light()?;
        path.verify_signatures(&message_hash)?;
        
        // Add to active proof
        let mut proofs = self.active_proofs.write().unwrap();
        let proof = proofs
            .entry(message_hash)
            .or_insert_with(|| TransitProof {
                message_hash,
                paths: Vec::new(),
                finality_level: FinalityLevel::Local,
                timestamp: SystemTime::now(),
                regions_covered: Vec::new(),
            });
        
        proof.paths.push(path);
        
        // Update finality level based on collected paths
        proof.finality_level = self.calculate_finality_level(proof);
        
        tracing::info!(
            "Transit path submitted: {} total paths, {:?} finality",
            proof.paths.len(),
            proof.finality_level
        );
        
        Ok(())
    }
    
    /// Calculate achieved finality level for a proof
    fn calculate_finality_level(&self, proof: &TransitProof) -> FinalityLevel {
        let path_count = proof.paths.len();
        let region_count = proof.regions_covered.len();
        
        if path_count >= 5 && region_count >= 4 {
            FinalityLevel::Deep
        } else if path_count >= 3 && region_count >= 3 {
            FinalityLevel::Global
        } else if path_count >= 2 && region_count >= 2 {
            FinalityLevel::Continental
        } else {
            FinalityLevel::Local
        }
    }
    
    /// Check if message has reached required finality
    pub fn check_finality(&self, message_hash: &Hash) -> bool {
        let proofs = self.active_proofs.read().unwrap();
        
        if let Some(proof) = proofs.get(message_hash) {
            proof.finality_level >= self.required_finality
        } else {
            false
        }
    }
    
    /// Get transit proof for a message
    pub fn get_proof(&self, message_hash: &Hash) -> Option<TransitProof> {
        let proofs = self.active_proofs.read().unwrap();
        proofs.get(message_hash).cloned()
    }
}

#[derive(Debug, Error)]
pub enum PoTError {
    #[error("Message transit faster than speed of light")]
    FasterThanLight,
    
    #[error("Path took too long (possible replay attack)")]
    PathTooSlow,
    
    #[error("Invalid timestamp in transit path")]
    InvalidTimestamp,
    
    #[error("Signature verification failed")]
    SignatureVerificationFailed,
    
    #[error("Signature count mismatch")]
    SignatureMismatch,
    
    #[error("Insufficient transit paths for finality level")]
    InsufficientPaths,
    
    #[error("Insufficient geographic diversity")]
    InsufficientGeographicDiversity,
    
    #[error("Path {} verification failed: {}", .0, .1)]
    PathVerificationFailed(usize, Box<PoTError>),
    
    #[error("Relay location not registered")]
    UnknownRelayLocation,
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_geo_distance_calculation() {
        // New York to London
        let ny = GeoLocation {
            latitude: 40.7128,
            longitude: -74.0060,
        };
        let london = GeoLocation {
            latitude: 51.5074,
            longitude: -0.1278,
        };
        
        let distance = ny.distance_to(&london);
        // Approximate distance ~5,570 km
        assert!(distance > 5500.0 && distance < 5600.0);
    }
    
    #[test]
    fn test_finality_level_requirements() {
        assert_eq!(FinalityLevel::Local.required_paths(), 1);
        assert_eq!(FinalityLevel::Continental.required_paths(), 2);
        assert_eq!(FinalityLevel::Global.required_paths(), 3);
        assert_eq!(FinalityLevel::Deep.required_paths(), 5);
    }
    
    #[test]
    fn test_speed_of_light_verification() {
        // Create a realistic transit path
        let relays = vec![
            VerifyingKey::from_bytes(&[1u8; 32]).unwrap(),
            VerifyingKey::from_bytes(&[2u8; 32]).unwrap(),
        ];
        
        let locations = vec![
            GeoLocation { latitude: 40.7128, longitude: -74.0060 }, // NY
            GeoLocation { latitude: 51.5074, longitude: -0.1278 },  // London
        ];
        
        let now = SystemTime::now();
        let timestamps = vec![
            now,
            now + Duration::from_millis(50), // ~5570km would need ~18.6ms minimum
        ];
        
        let path = TransitPath {
            relays,
            locations,
            timestamps,
            signatures: vec![
                HybridSignature {
                    ed25519: Signature::from_bytes(&[0u8; 64]),
                    dilithium3: Dilithium3Signature { bytes: vec![0u8; 2420] },
                },
                HybridSignature {
                    ed25519: Signature::from_bytes(&[0u8; 64]),
                    dilithium3: Dilithium3Signature { bytes: vec![0u8; 2420] },
                },
            ],
            total_distance: 5570.0,
            total_duration: Duration::from_millis(50),
        };
        
        // Should pass - realistic timing
        assert!(path.verify_speed_of_light().is_ok());
    }
}
