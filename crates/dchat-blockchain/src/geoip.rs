//! GeoIP Database Integration
//!
//! Provides IP-to-location mapping using MaxMind GeoIP2 database for:
//! - Relay node geographic verification
//! - Eclipse attack prevention (ASN diversity)
//! - Geographic diversity scoring in consensus
//! - Regional quorum requirements
//!
//! Uses MaxMind GeoLite2 City database (free, updated monthly)

use maxminddb::{geoip2, MaxMindDBError, Reader};
use serde::{Deserialize, Serialize};
use std::net::IpAddr;
use std::path::Path;
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum GeoIPError {
    #[error("MaxMind database error: {0}")]
    DatabaseError(#[from] MaxMindDBError),

    #[error("IP address not found in database: {0}")]
    AddressNotFound(String),

    #[error("Invalid IP address: {0}")]
    InvalidAddress(String),

    #[error("Database file not found: {0}")]
    DatabaseNotFound(String),
}

pub type Result<T> = std::result::Result<T, GeoIPError>;

/// Geographic location from GeoIP lookup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeoLocation {
    pub ip: IpAddr,
    pub latitude: f64,
    pub longitude: f64,
    pub city: Option<String>,
    pub country: String,
    pub country_code: String,
    pub continent: String,
    pub continent_code: String,
    pub timezone: Option<String>,
    pub asn: Option<u32>,
    pub asn_organization: Option<String>,
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

    /// Check if in same continent
    pub fn same_continent(&self, other: &GeoLocation) -> bool {
        self.continent_code == other.continent_code
    }

    /// Check if in same ASN (Autonomous System Number)
    pub fn same_asn(&self, other: &GeoLocation) -> bool {
        match (self.asn, other.asn) {
            (Some(a), Some(b)) => a == b,
            _ => false,
        }
    }
}

/// GeoIP database manager
pub struct GeoIPManager {
    reader: Arc<Reader<Vec<u8>>>,
}

impl GeoIPManager {
    /// Load GeoIP database from file
    pub fn new<P: AsRef<Path>>(database_path: P) -> Result<Self> {
        let reader = Reader::open_readfile(database_path.as_ref()).map_err(|e| {
            GeoIPError::DatabaseNotFound(format!("{:?}: {}", database_path.as_ref(), e))
        })?;

        Ok(Self {
            reader: Arc::new(reader),
        })
    }

    /// Create with default database path
    pub fn with_default_path() -> Result<Self> {
        // Look for database in common locations
        let possible_paths = [
            "/usr/share/GeoIP/GeoLite2-City.mmdb",
            "/var/lib/GeoIP/GeoLite2-City.mmdb",
            "./data/GeoLite2-City.mmdb",
            "../data/GeoLite2-City.mmdb",
        ];

        for path in &possible_paths {
            if Path::new(path).exists() {
                return Self::new(path);
            }
        }

        Err(GeoIPError::DatabaseNotFound(
            "GeoLite2-City.mmdb not found in standard locations".to_string(),
        ))
    }

    /// Lookup IP address
    pub fn lookup(&self, ip: IpAddr) -> Result<GeoLocation> {
        let city: geoip2::City = self
            .reader
            .lookup(ip)
            .map_err(|e| GeoIPError::AddressNotFound(format!("{}: {}", ip, e)))?;

        let location = city
            .location
            .ok_or_else(|| GeoIPError::AddressNotFound(format!("No location data for {}", ip)))?;

        let country = city
            .country
            .ok_or_else(|| GeoIPError::AddressNotFound(format!("No country data for {}", ip)))?;

        let continent = city
            .continent
            .ok_or_else(|| GeoIPError::AddressNotFound(format!("No continent data for {}", ip)))?;

        Ok(GeoLocation {
            ip,
            latitude: location.latitude.unwrap_or(0.0),
            longitude: location.longitude.unwrap_or(0.0),
            city: city
                .city
                .and_then(|c| c.names)
                .and_then(|n| n.get("en").map(|s| s.to_string())),
            country: country
                .names
                .and_then(|n| n.get("en").map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown".to_string()),
            country_code: country.iso_code.unwrap_or("XX").to_string(),
            continent: continent
                .names
                .and_then(|n| n.get("en").map(|s| s.to_string()))
                .unwrap_or_else(|| "Unknown".to_string()),
            continent_code: continent.code.unwrap_or("XX").to_string(),
            timezone: location.time_zone.map(|s| s.to_string()),
            asn: None, // Requires separate ASN database
            asn_organization: None,
        })
    }

    /// Batch lookup multiple IPs
    pub fn lookup_batch(&self, ips: &[IpAddr]) -> Vec<Result<GeoLocation>> {
        ips.iter().map(|ip| self.lookup(*ip)).collect()
    }

    /// Calculate geographic diversity score for a set of IPs (0.0-1.0)
    pub fn calculate_diversity_score(&self, ips: &[IpAddr]) -> f64 {
        if ips.len() < 2 {
            return 0.0;
        }

        let locations: Vec<GeoLocation> =
            ips.iter().filter_map(|ip| self.lookup(*ip).ok()).collect();

        if locations.len() < 2 {
            return 0.0;
        }

        // Count unique continents
        let mut continents = std::collections::HashSet::new();
        for loc in &locations {
            continents.insert(loc.continent_code.clone());
        }
        let continent_diversity = continents.len() as f64 / 6.0; // 6 inhabited continents

        // Count unique countries
        let mut countries = std::collections::HashSet::new();
        for loc in &locations {
            countries.insert(loc.country_code.clone());
        }
        let country_diversity = (countries.len() as f64 / locations.len() as f64).min(1.0);

        // Calculate average pairwise distance
        let mut total_distance = 0.0;
        let mut pair_count = 0;
        for i in 0..locations.len() {
            for j in (i + 1)..locations.len() {
                total_distance += locations[i].distance_to(&locations[j]);
                pair_count += 1;
            }
        }
        let avg_distance = total_distance / pair_count as f64;
        let distance_score = (avg_distance / 10000.0).min(1.0); // Normalize to 10,000km max

        // Weighted combination
        continent_diversity * 0.4 + country_diversity * 0.3 + distance_score * 0.3
    }

    /// Check if geographic distribution meets consensus requirements
    pub fn verify_geographic_quorum(&self, ips: &[IpAddr]) -> Result<GeographicQuorum> {
        let locations: Vec<GeoLocation> =
            ips.iter().filter_map(|ip| self.lookup(*ip).ok()).collect();

        if locations.is_empty() {
            return Err(GeoIPError::AddressNotFound(
                "No valid locations found".to_string(),
            ));
        }

        // Count nodes per continent
        let mut continent_counts = std::collections::HashMap::new();
        for loc in &locations {
            *continent_counts
                .entry(loc.continent_code.clone())
                .or_insert(0) += 1;
        }

        let unique_continents = continent_counts.len();
        let total_nodes = locations.len();

        // Check for excessive concentration
        let max_per_continent = continent_counts.values().max().unwrap_or(&0);
        let concentration_ratio = (*max_per_continent as f64) / (total_nodes as f64);

        Ok(GeographicQuorum {
            total_nodes,
            unique_continents,
            continent_counts,
            max_continent_concentration: concentration_ratio,
            meets_minimum_diversity: unique_continents >= 3,
            meets_concentration_limit: concentration_ratio <= 0.4,
        })
    }
}

/// Geographic quorum analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeographicQuorum {
    pub total_nodes: usize,
    pub unique_continents: usize,
    pub continent_counts: std::collections::HashMap<String, usize>,
    pub max_continent_concentration: f64,
    pub meets_minimum_diversity: bool,
    pub meets_concentration_limit: bool,
}

impl GeographicQuorum {
    /// Check if quorum is valid for consensus
    pub fn is_valid(&self) -> bool {
        self.meets_minimum_diversity && self.meets_concentration_limit
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    #[test]
    fn test_distance_calculation() {
        let new_york = GeoLocation {
            ip: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            latitude: 40.7128,
            longitude: -74.0060,
            city: Some("New York".to_string()),
            country: "United States".to_string(),
            country_code: "US".to_string(),
            continent: "North America".to_string(),
            continent_code: "NA".to_string(),
            timezone: Some("America/New_York".to_string()),
            asn: None,
            asn_organization: None,
        };

        let london = GeoLocation {
            ip: IpAddr::V4(Ipv4Addr::new(8, 8, 4, 4)),
            latitude: 51.5074,
            longitude: -0.1278,
            city: Some("London".to_string()),
            country: "United Kingdom".to_string(),
            country_code: "GB".to_string(),
            continent: "Europe".to_string(),
            continent_code: "EU".to_string(),
            timezone: Some("Europe/London".to_string()),
            asn: None,
            asn_organization: None,
        };

        let distance = new_york.distance_to(&london);

        // NYC to London is approximately 5,570 km
        assert!(distance > 5500.0 && distance < 5600.0);
    }

    #[test]
    fn test_same_continent() {
        let us_loc = GeoLocation {
            ip: IpAddr::V4(Ipv4Addr::new(8, 8, 8, 8)),
            latitude: 40.0,
            longitude: -74.0,
            city: None,
            country: "United States".to_string(),
            country_code: "US".to_string(),
            continent: "North America".to_string(),
            continent_code: "NA".to_string(),
            timezone: None,
            asn: None,
            asn_organization: None,
        };

        let ca_loc = GeoLocation {
            ip: IpAddr::V4(Ipv4Addr::new(1, 1, 1, 1)),
            latitude: 45.0,
            longitude: -75.0,
            city: None,
            country: "Canada".to_string(),
            country_code: "CA".to_string(),
            continent: "North America".to_string(),
            continent_code: "NA".to_string(),
            timezone: None,
            asn: None,
            asn_organization: None,
        };

        assert!(us_loc.same_continent(&ca_loc));
    }
}
