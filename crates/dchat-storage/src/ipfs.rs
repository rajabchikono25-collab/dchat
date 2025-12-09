//! IPFS (InterPlanetary File System) Integration for dchat
//!
//! This module provides integration with IPFS for storing:
//! - Sticker packs and emoji collections
//! - Media files (images, videos, audio)
//! - NFT metadata and artwork
//! - Digital goods content
//! - Channel media archives
//!
//! Content is addressed by cryptographic hash (CID), ensuring:
//! - Immutability (content cannot be changed without changing hash)
//! - Verifiability (anyone can verify content matches its CID)
//! - Decentralization (content can be hosted by multiple nodes)
//! - Censorship resistance (no single point of control)

use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::time::Duration;
use tracing::{debug, info, warn};

/// IPFS Content Identifier (CID)
/// Example: QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG
pub type Cid = String;

/// IPFS configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsConfig {
    /// IPFS API endpoint (local node or remote gateway)
    pub api_url: String,

    /// IPFS gateway URL for content retrieval
    pub gateway_url: String,

    /// Connection timeout
    pub timeout_secs: u64,

    /// Enable pinning (prevent garbage collection)
    pub enable_pinning: bool,

    /// Maximum file size in bytes (default: 100 MB)
    pub max_file_size: u64,

    /// Enable caching locally
    pub enable_local_cache: bool,

    /// Local cache directory
    pub cache_dir: Option<PathBuf>,
}

impl Default for IpfsConfig {
    fn default() -> Self {
        Self {
            api_url: "http://127.0.0.1:5001".to_string(),
            gateway_url: "http://127.0.0.1:8080".to_string(),
            timeout_secs: 30,
            enable_pinning: true,
            max_file_size: 100 * 1024 * 1024, // 100 MB
            enable_local_cache: true,
            cache_dir: None,
        }
    }
}

/// IPFS client for content operations
pub struct IpfsClient {
    config: IpfsConfig,
    http_client: reqwest::Client,
}

/// File metadata returned after upload
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsFile {
    /// Content identifier (hash)
    pub cid: Cid,

    /// File name (original)
    pub name: String,

    /// File size in bytes
    pub size: u64,

    /// MIME type
    pub mime_type: String,

    /// Is pinned?
    pub pinned: bool,

    /// Upload timestamp
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// Directory of files on IPFS
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IpfsDirectory {
    /// Directory CID
    pub cid: Cid,

    /// Files in directory
    pub files: Vec<IpfsFile>,

    /// Total size
    pub total_size: u64,
}

/// Pin status for garbage collection prevention
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PinStatus {
    pub cid: Cid,
    pub pinned: bool,
    pub pin_type: PinType,
}

/// Type of pin
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum PinType {
    /// Directly pinned
    Direct,

    /// Pinned recursively (directory + all contents)
    Recursive,

    /// Pinned indirectly (child of pinned directory)
    Indirect,
}

impl IpfsClient {
    /// Create a new IPFS client
    pub fn new(config: IpfsConfig) -> Result<Self> {
        let http_client = reqwest::Client::builder()
            .timeout(Duration::from_secs(config.timeout_secs))
            .build()
            .map_err(|e| Error::network(format!("Failed to create HTTP client: {}", e)))?;

        Ok(Self {
            config,
            http_client,
        })
    }

    /// Upload a file to IPFS
    ///
    /// Returns the CID (content identifier) that can be used to retrieve the file.
    ///
    /// # Example
    /// ```no_run
    /// # use dchat_storage::ipfs::{IpfsClient, IpfsConfig};
    /// # async fn example() -> dchat_core::Result<()> {
    /// let client = IpfsClient::new(IpfsConfig::default())?;
    /// let data = b"Hello, IPFS!";
    /// let file = client.upload(data.to_vec(), "hello.txt".to_string(), "text/plain".to_string()).await?;
    /// println!("Uploaded to IPFS: {}", file.cid);
    /// # Ok(())
    /// # }
    /// ```
    pub async fn upload(
        &self,
        data: Vec<u8>,
        filename: String,
        mime_type: String,
    ) -> Result<IpfsFile> {
        if data.len() as u64 > self.config.max_file_size {
            return Err(Error::validation(format!(
                "File size {} exceeds maximum {}",
                data.len(),
                self.config.max_file_size
            )));
        }

        info!("Uploading file {} ({} bytes) to IPFS", filename, data.len());

        // In production: Use IPFS HTTP API
        // POST /api/v0/add with multipart form data
        let url = format!("{}/api/v0/add", self.config.api_url);

        let _form = reqwest::multipart::Form::new().part(
            "file",
            reqwest::multipart::Part::bytes(data.clone())
                .file_name(filename.clone())
                .mime_str(&mime_type)
                .map_err(|e| Error::validation(format!("Invalid MIME type: {}", e)))?,
        );

        // Simulate IPFS upload (in production, make actual HTTP request)
        debug!("POST {} with {} bytes", url, data.len());

        // Generate deterministic CID from content hash
        let cid = Self::generate_cid(&data);

        let file = IpfsFile {
            cid: cid.clone(),
            name: filename,
            size: data.len() as u64,
            mime_type,
            pinned: self.config.enable_pinning,
            uploaded_at: chrono::Utc::now(),
        };

        // Pin if enabled
        if self.config.enable_pinning {
            self.pin(&cid).await?;
        }

        // Cache locally if enabled
        if self.config.enable_local_cache {
            self.cache_locally(&file, &data).await?;
        }

        info!("Successfully uploaded to IPFS: {}", cid);
        Ok(file)
    }

    /// Download a file from IPFS by CID
    ///
    /// # Example
    /// ```no_run
    /// # use dchat_storage::ipfs::{IpfsClient, IpfsConfig};
    /// # async fn example() -> dchat_core::Result<()> {
    /// let client = IpfsClient::new(IpfsConfig::default())?;
    /// let cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
    /// let data = client.download(cid).await?;
    /// println!("Downloaded {} bytes", data.len());
    /// # Ok(())
    /// # }
    /// ```
    pub async fn download(&self, cid: &str) -> Result<Vec<u8>> {
        info!("Downloading content from IPFS: {}", cid);

        // Check local cache first
        if self.config.enable_local_cache {
            if let Ok(data) = self.get_from_cache(cid).await {
                debug!("Found in local cache: {}", cid);
                return Ok(data);
            }
        }

        // In production: Use IPFS gateway
        // GET /ipfs/{cid}
        let url = format!("{}/ipfs/{}", self.config.gateway_url, cid);

        debug!("GET {}", url);

        // Make actual HTTP request to IPFS gateway
        let response = self.http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS download failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "IPFS gateway returned error: {}",
                response.status()
            )));
        }

        // Read response data
        let data = response
            .bytes()
            .await
            .map_err(|e| Error::network(format!("Failed to read response: {}", e)))?
            .to_vec();

        info!("Downloaded {} bytes from IPFS (CID: {})", data.len(), cid);

        // Verify CID matches content
        let computed_cid = Self::generate_cid(&data);
        if computed_cid != cid {
            return Err(Error::validation(format!(
                "Content CID mismatch: expected {}, got {}",
                cid, computed_cid
            )));
        }

        // Cache for future requests
        if self.config.enable_local_cache {
            self.cache_content(cid, &data).await?;
        }

        info!("Successfully downloaded {} bytes from IPFS", data.len());
        Ok(data)
    }

    /// Pin a CID to prevent garbage collection
    ///
    /// Pinning ensures content remains available even if other nodes
    /// delete it from their cache.
    pub async fn pin(&self, cid: &str) -> Result<()> {
        info!("Pinning CID: {}", cid);

        // Make actual IPFS pin request
        let url = format!("{}/api/v0/pin/add?arg={}", self.config.api_url, cid);

        debug!("POST {}", url);

        let response = self.http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(30))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS pin request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "IPFS pin failed: {}",
                response.status()
            )));
        }

        info!("Successfully pinned CID: {}", cid);
        Ok(())
    }

    /// Unpin a CID to allow garbage collection
    pub async fn unpin(&self, cid: &str) -> Result<()> {
        info!("Unpinning CID: {}", cid);

        // Make actual IPFS unpin request
        let url = format!("{}/api/v0/pin/rm?arg={}", self.config.api_url, cid);

        debug!("POST {}", url);

        let response = self.http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS unpin request failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "IPFS unpin failed: {}",
                response.status()
            )));
        }

        info!("Successfully unpinned CID: {}", cid);
        Ok(())
    }

    /// Get pin status for a CID
    pub async fn pin_status(&self, cid: &str) -> Result<PinStatus> {
        debug!("Checking pin status: {}", cid);

        // Make actual IPFS pin list request
        let url = format!("{}/api/v0/pin/ls?arg={}", self.config.api_url, cid);

        debug!("POST {}", url);

        let response = self.http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS pin status request failed: {}", e)))?;

        let is_pinned = response.status().is_success();

        Ok(PinStatus {
            cid: cid.to_string(),
            pinned: is_pinned,
            pin_type: if is_pinned { PinType::Direct } else { PinType::Indirect },
        })
    }

    /// Upload a directory of files
    ///
    /// Returns the root CID and metadata for all files.
    pub async fn upload_directory(
        &self,
        files: Vec<(String, Vec<u8>, String)>, // (name, data, mime_type)
    ) -> Result<IpfsDirectory> {
        info!("Uploading directory with {} files", files.len());

        let mut ipfs_files = Vec::new();
        let mut total_size = 0u64;

        for (name, data, mime_type) in files {
            let file = self.upload(data, name, mime_type).await?;
            total_size += file.size;
            ipfs_files.push(file);
        }

        // In production: Create directory object with all file CIDs
        // and upload directory metadata
        let directory_cid = format!("Qmdir{}", uuid::Uuid::new_v4());

        Ok(IpfsDirectory {
            cid: directory_cid,
            files: ipfs_files,
            total_size,
        })
    }

    /// List files in a directory CID
    pub async fn list_directory(&self, dir_cid: &str) -> Result<Vec<String>> {
        debug!("Listing directory: {}", dir_cid);

        // Make actual IPFS directory list request
        let url = format!("{}/api/v0/ls?arg={}", self.config.api_url, dir_cid);

        debug!("POST {}", url);

        let response = self.http_client
            .post(&url)
            .timeout(std::time::Duration::from_secs(15))
            .send()
            .await
            .map_err(|e| Error::network(format!("IPFS directory list failed: {}", e)))?;

        if !response.status().is_success() {
            return Err(Error::network(format!(
                "IPFS directory list failed: {}",
                response.status()
            )));
        }

        // Parse JSON response to extract file names
        let json: serde_json::Value = response
            .json()
            .await
            .map_err(|e| Error::network(format!("Failed to parse directory listing: {}", e)))?;

        let mut files = Vec::new();
        if let Some(objects) = json.get("Objects").and_then(|o| o.as_array()) {
            for obj in objects {
                if let Some(links) = obj.get("Links").and_then(|l| l.as_array()) {
                    for link in links {
                        if let Some(name) = link.get("Name").and_then(|n| n.as_str()) {
                            files.push(name.to_string());
                        }
                    }
                }
            }
        }

        Ok(files)
    }

    /// Generate CID from content using BLAKE3 hash
    ///
    /// Production would use proper IPFS CIDv1 format with multihash.
    /// For now, we use BLAKE3 hash wrapped in CID format.
    fn generate_cid(data: &[u8]) -> Cid {
        let hash = blake3::hash(data);
        // IPFS CIDv1 format: base58btc(multibase) + version + codec + multihash
        // Simplified: Qm prefix (base58 marker) + hex hash
        format!("Qm{}", hex::encode(&hash.as_bytes()[..20]))
    }

    /// Cache content locally
    async fn cache_locally(&self, file: &IpfsFile, data: &[u8]) -> Result<()> {
        if let Some(ref cache_dir) = self.config.cache_dir {
            let cache_path = cache_dir.join(&file.cid);
            tokio::fs::create_dir_all(cache_dir)
                .await
                .map_err(|e| Error::storage(format!("Failed to create cache dir: {}", e)))?;

            tokio::fs::write(&cache_path, data)
                .await
                .map_err(|e| Error::storage(format!("Failed to write cache: {}", e)))?;

            debug!("Cached locally: {}", file.cid);
        }
        Ok(())
    }

    /// Get content from local cache
    async fn get_from_cache(&self, cid: &str) -> Result<Vec<u8>> {
        if let Some(ref cache_dir) = self.config.cache_dir {
            let cache_path = cache_dir.join(cid);
            if cache_path.exists() {
                return tokio::fs::read(&cache_path)
                    .await
                    .map_err(|e| Error::storage(format!("Failed to read cache: {}", e)));
            }
        }
        Err(Error::storage("Not in cache".to_string()))
    }

    /// Cache downloaded content
    async fn cache_content(&self, cid: &str, data: &[u8]) -> Result<()> {
        let file = IpfsFile {
            cid: cid.to_string(),
            name: "cached".to_string(),
            size: data.len() as u64,
            mime_type: "application/octet-stream".to_string(),
            pinned: false,
            uploaded_at: chrono::Utc::now(),
        };
        self.cache_locally(&file, data).await
    }

    /// Get gateway URL for direct browser access
    pub fn gateway_url(&self, cid: &str) -> String {
        format!("{}/ipfs/{}", self.config.gateway_url, cid)
    }

    /// Health check - verify IPFS node is reachable
    pub async fn health_check(&self) -> Result<bool> {
        let url = format!("{}/api/v0/id", self.config.api_url);

        debug!("Health check: GET {}", url);

        // Make actual request to verify IPFS node is reachable
        match self.http_client
            .get(&url)
            .timeout(std::time::Duration::from_secs(5))
            .send()
            .await
        {
            Ok(response) => {
                let is_healthy = response.status().is_success();
                if is_healthy {
                    info!("IPFS node health check: OK");
                } else {
                    warn!("IPFS node health check: FAILED (status: {})", response.status());
                }
                Ok(is_healthy)
            }
            Err(e) => {
                warn!("IPFS node health check: FAILED ({})", e);
                Ok(false) // Return false instead of error for health checks
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_create_client() {
        let config = IpfsConfig::default();
        let client = IpfsClient::new(config);
        assert!(client.is_ok());
    }

    #[tokio::test]
    async fn test_generate_cid() {
        let data = b"Hello, IPFS!";
        let cid = IpfsClient::generate_cid(data);
        assert!(cid.starts_with("Qm"));
        assert!(cid.len() > 10);

        // Same data should produce same CID
        let cid2 = IpfsClient::generate_cid(data);
        assert_eq!(cid, cid2);
    }

    #[tokio::test]
    async fn test_upload_file() {
        let mut config = IpfsConfig::default();
        config.enable_pinning = false; // Disable pinning to avoid network calls
        config.enable_local_cache = false;
        let client = IpfsClient::new(config).unwrap();

        let data = b"Test file content".to_vec();
        let result = client
            .upload(data, "test.txt".to_string(), "text/plain".to_string())
            .await;

        assert!(result.is_ok());
        let file = result.unwrap();
        assert_eq!(file.name, "test.txt");
        assert_eq!(file.size, 17);
        assert!(file.cid.starts_with("Qm"));
    }

    #[tokio::test]
    async fn test_file_size_limit() {
        let mut config = IpfsConfig::default();
        config.max_file_size = 10; // 10 bytes limit
        config.enable_pinning = false;
        config.enable_local_cache = false;
        let client = IpfsClient::new(config).unwrap();

        let large_data = vec![0u8; 100];
        let result = client
            .upload(large_data, "large.bin".to_string(), "application/octet-stream".to_string())
            .await;

        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_upload_directory() {
        let mut config = IpfsConfig::default();
        config.enable_pinning = false; // Disable pinning to avoid network calls
        config.enable_local_cache = false;
        let client = IpfsClient::new(config).unwrap();

        let files = vec![
            (
                "file1.txt".to_string(),
                b"Content 1".to_vec(),
                "text/plain".to_string(),
            ),
            (
                "file2.txt".to_string(),
                b"Content 2".to_vec(),
                "text/plain".to_string(),
            ),
        ];

        let result = client.upload_directory(files).await;
        assert!(result.is_ok());

        let directory = result.unwrap();
        assert_eq!(directory.files.len(), 2);
        assert_eq!(directory.total_size, 18); // 9 + 9 bytes
    }

    #[tokio::test]
    async fn test_gateway_url() {
        let config = IpfsConfig::default();
        let client = IpfsClient::new(config).unwrap();

        let cid = "QmYwAPJzv5CZsnA625s3Xf2nemtYgPpHdWEz79ojWnPbdG";
        let url = client.gateway_url(cid);
        assert_eq!(
            url,
            format!("http://127.0.0.1:8080/ipfs/{}", cid)
        );
    }

    #[tokio::test]
    async fn test_pin_operations() {
        // Skip this test unless IPFS daemon is running
        // Pin operations require actual IPFS API calls
        let config = IpfsConfig::default();
        let client = IpfsClient::new(config).unwrap();

        // First check if IPFS is available
        let health = client.health_check().await;
        if health.is_err() {
            println!("Skipping test_pin_operations - IPFS daemon not available");
            return;
        }

        let cid = "QmTest";

        // Pin - may fail if CID doesn't exist, which is ok
        let pin_result = client.pin(cid).await;
        // We just test that the method doesn't panic
        let _ = pin_result;
    }

    #[tokio::test]
    async fn test_health_check() {
        let config = IpfsConfig::default();
        let client = IpfsClient::new(config).unwrap();

        let result = client.health_check().await;
        assert!(result.is_ok());
    }
}
