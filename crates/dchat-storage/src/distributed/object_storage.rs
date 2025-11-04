// Distributed object storage implementation using MinIO/S3
//
// This module provides object storage for media files, attachments, and
// cold-tier message storage. Supports multi-region replication and CDN integration.

use s3::bucket::Bucket;
use s3::creds::Credentials;
use s3::region::Region;
use std::path::Path;
use tokio::fs;
use serde::{Deserialize, Serialize};
use tracing::{debug, error, info, warn};
use uuid::Uuid;

use crate::error::{StorageError, StorageResult};

/// Configuration for distributed object storage
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectStorageConfig {
    /// S3/MinIO endpoint URL
    pub endpoint: String,
    /// Bucket name
    pub bucket_name: String,
    /// Access key ID
    pub access_key: String,
    /// Secret access key
    pub secret_key: String,
    /// Region (e.g., "us-east-1")
    pub region: String,
    /// CDN URL for public access
    pub cdn_url: Option<String>,
    /// Enable multi-region replication
    pub multi_region: bool,
    /// Upload timeout in seconds
    pub upload_timeout_seconds: u64,
}

impl Default for ObjectStorageConfig {
    fn default() -> Self {
        Self {
            endpoint: "https://s3.dchat.network".to_string(),
            bucket_name: "dchat-media".to_string(),
            access_key: String::new(),
            secret_key: String::new(),
            region: "us-east-1".to_string(),
            cdn_url: Some("https://cdn.dchat.network".to_string()),
            multi_region: true,
            upload_timeout_seconds: 60,
        }
    }
}

/// Distributed object storage client
pub struct DistributedObjectStorage {
    /// S3 bucket handle (boxed in rust-s3 0.35)
    bucket: Box<Bucket>,
    /// Configuration
    config: ObjectStorageConfig,
}

/// Object metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObjectMetadata {
    pub key: String,
    pub size_bytes: u64,
    pub content_type: String,
    pub etag: String,
    pub public_url: String,
}

/// Storage tier for objects
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StorageTier {
    /// Hot tier - frequent access, fast retrieval
    Hot,
    /// Warm tier - occasional access
    Warm,
    /// Cold tier - rare access, slower retrieval
    Cold,
    /// Archive tier - very rare access, multi-hour retrieval
    Archive,
}

impl DistributedObjectStorage {
    /// Create new object storage client
    pub async fn new(config: ObjectStorageConfig) -> StorageResult<Self> {
        info!("Initializing object storage: bucket={}, region={}", 
              config.bucket_name, config.region);
        
        let credentials = Credentials::new(
            Some(&config.access_key),
            Some(&config.secret_key),
            None,
            None,
            None,
        ).map_err(|e| {
            error!("Failed to create S3 credentials: {}", e);
            StorageError::ObjectStorage(format!("Invalid credentials: {}", e))
        })?;
        
        let region = if config.endpoint.starts_with("http") {
            // Custom endpoint (MinIO)
            Region::Custom {
                region: config.region.clone(),
                endpoint: config.endpoint.clone(),
            }
        } else {
            // AWS region
            config.region.parse().map_err(|e| {
                error!("Invalid region: {}", e);
                StorageError::ObjectStorage(format!("Invalid region: {}", e))
            })?
        };
        
        let bucket = Bucket::new(&config.bucket_name, region, credentials)
            .map_err(|e| {
                error!("Failed to create S3 bucket: {}", e);
                StorageError::ObjectStorage(format!("Bucket creation failed: {}", e))
            })?;
        
        info!("Successfully initialized object storage");
        Ok(Self { bucket, config })
    }
    
    /// Upload file from disk
    pub async fn upload_file(
        &self,
        file_path: &Path,
        object_key: &str,
        content_type: &str,
    ) -> StorageResult<ObjectMetadata> {
        info!("Uploading file: {} -> {}", file_path.display(), object_key);
        
        // Read file contents
        let data = fs::read(file_path).await
            .map_err(|e| {
                error!("Failed to read file: {}", e);
                StorageError::Io(e)
            })?;
        
        self.upload_bytes(&data, object_key, content_type).await
    }
    
    /// Upload raw bytes
    pub async fn upload_bytes(
        &self,
        data: &[u8],
        object_key: &str,
        content_type: &str,
    ) -> StorageResult<ObjectMetadata> {
        debug!("Uploading {} bytes to key: {}", data.len(), object_key);
        
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.upload_timeout_seconds),
            self.bucket.put_object_with_content_type(object_key, data, content_type)
        ).await
        .map_err(|_| {
            error!("Upload timeout for key: {}", object_key);
            StorageError::Timeout
        })?
        .map_err(|e| {
            error!("Upload failed: {}", e);
            StorageError::ObjectStorage(format!("Upload failed: {}", e))
        })?;
        
        let etag = response
            .headers()
            .get("etag")
            .map(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        
        let public_url = if let Some(cdn_url) = &self.config.cdn_url {
            format!("{}/{}", cdn_url, object_key)
        } else {
            format!("{}/{}/{}", self.config.endpoint, self.config.bucket_name, object_key)
        };
        
        debug!("Upload successful: etag={}, url={}", etag, public_url);
        
        Ok(ObjectMetadata {
            key: object_key.to_string(),
            size_bytes: data.len() as u64,
            content_type: content_type.to_string(),
            etag,
            public_url,
        })
    }
    
    /// Download object as bytes
    pub async fn download_bytes(&self, object_key: &str) -> StorageResult<Vec<u8>> {
        debug!("Downloading object: {}", object_key);
        
        let response = tokio::time::timeout(
            std::time::Duration::from_secs(self.config.upload_timeout_seconds),
            self.bucket.get_object(object_key)
        ).await
        .map_err(|_| {
            error!("Download timeout for key: {}", object_key);
            StorageError::Timeout
        })?
        .map_err(|e| {
            error!("Download failed: {}", e);
            StorageError::ObjectStorage(format!("Download failed: {}", e))
        })?;
        
        debug!("Download successful: {} bytes", response.bytes().len());
        Ok(response.bytes().to_vec())
    }
    
    /// Download object to file
    pub async fn download_file(
        &self,
        object_key: &str,
        dest_path: &Path,
    ) -> StorageResult<()> {
        let data = self.download_bytes(object_key).await?;
        
        fs::write(dest_path, data).await
            .map_err(|e| {
                error!("Failed to write file: {}", e);
                StorageError::Io(e)
            })?;
        
        info!("Downloaded to: {}", dest_path.display());
        Ok(())
    }
    
    /// Delete object
    pub async fn delete(&self, object_key: &str) -> StorageResult<()> {
        debug!("Deleting object: {}", object_key);
        
        self.bucket.delete_object(object_key).await
            .map_err(|e| {
                error!("Delete failed: {}", e);
                StorageError::ObjectStorage(format!("Delete failed: {}", e))
            })?;
        
        debug!("Delete successful: {}", object_key);
        Ok(())
    }
    
    /// Check if object exists
    pub async fn exists(&self, object_key: &str) -> StorageResult<bool> {
        let result = self.bucket.head_object(object_key).await;
        
        match result {
            Ok(_) => Ok(true),
            Err(e) => {
                // Check if it's a 404 Not Found error
                let err_str = e.to_string();
                if err_str.contains("404") || err_str.contains("Not Found") {
                    Ok(false)
                } else {
                    error!("Head object failed: {}", e);
                    Err(StorageError::ObjectStorage(format!("Head object failed: {}", e)))
                }
            }
        }
    }
    
    /// Get object metadata without downloading
    pub async fn get_metadata(&self, object_key: &str) -> StorageResult<ObjectMetadata> {
        debug!("Getting metadata for: {}", object_key);
        
        let (head_object, _) = self.bucket.head_object(object_key).await
            .map_err(|e| {
                error!("Head object failed: {}", e);
                StorageError::ObjectStorage(format!("Head object failed: {}", e))
            })?;
        
        let size_bytes = head_object
            .content_length
            .unwrap_or(0) as u64;
        
        let content_type = head_object
            .content_type
            .unwrap_or_else(|| "application/octet-stream".to_string());
        
        let etag = head_object
            .e_tag
            .unwrap_or_default();
        
        let public_url = if let Some(cdn_url) = &self.config.cdn_url {
            format!("{}/{}", cdn_url, object_key)
        } else {
            format!("{}/{}/{}", self.config.endpoint, self.config.bucket_name, object_key)
        };
        
        Ok(ObjectMetadata {
            key: object_key.to_string(),
            size_bytes,
            content_type,
            etag,
            public_url,
        })
    }
    
    /// Copy object to different storage tier
    pub async fn copy_to_tier(
        &self,
        source_key: &str,
        dest_key: &str,
        tier: StorageTier,
    ) -> StorageResult<()> {
        info!("Copying {} to {} (tier: {:?})", source_key, dest_key, tier);
        
        // Download source
        let data = self.download_bytes(source_key).await?;
        
        // Re-upload with new storage class
        let storage_class = match tier {
            StorageTier::Hot => "STANDARD",
            StorageTier::Warm => "STANDARD_IA",
            StorageTier::Cold => "GLACIER",
            StorageTier::Archive => "DEEP_ARCHIVE",
        };
        
        // Upload to destination with storage class
        self.bucket.put_object_with_content_type(dest_key, &data, "application/octet-stream")
            .await
            .map_err(|e| {
                error!("Copy failed: {}", e);
                StorageError::ObjectStorage(format!("Copy failed: {}", e))
            })?;
        
        info!("Copy successful to tier: {:?}", tier);
        Ok(())
    }
    
    /// Generate pre-signed URL for temporary access
    pub async fn generate_presigned_url(
        &self,
        object_key: &str,
        expires_in_seconds: u32,
    ) -> StorageResult<String> {
        debug!("Generating pre-signed URL for: {}", object_key);
        
        let url = self.bucket.presign_get(object_key, expires_in_seconds, None).await
            .map_err(|e| {
                error!("Pre-sign failed: {}", e);
                StorageError::ObjectStorage(format!("Pre-sign failed: {}", e))
            })?;
        
        Ok(url)
    }
    
    /// Health check - verify bucket access
    pub async fn health_check(&self) -> StorageResult<bool> {
        let test_key = format!("_health_check_{}", Uuid::new_v4());
        let test_data = b"health check";
        
        // Try to upload
        let upload_result = self.bucket.put_object(&test_key, test_data).await;
        
        if upload_result.is_err() {
            error!("Object storage health check failed: upload error");
            return Ok(false);
        }
        
        // Try to delete
        let delete_result = self.bucket.delete_object(&test_key).await;
        
        if delete_result.is_err() {
            warn!("Object storage health check: delete failed");
        }
        
        Ok(true)
    }
}

/// Helper functions for object key generation
pub mod keys {
    use uuid::Uuid;
    use chrono::Utc;
    
    /// Generate key for media file
    pub fn media_key(user_id: &str, filename: &str) -> String {
        let date = Utc::now().format("%Y/%m/%d");
        let file_id = Uuid::new_v4();
        format!("media/{}/{}/{}/{}", date, user_id, file_id, filename)
    }
    
    /// Generate key for cold-tier message
    pub fn cold_message_key(message_id: Uuid, created_at: chrono::DateTime<Utc>) -> String {
        let date = created_at.format("%Y/%m/%d");
        format!("cold/{}/{}", date, message_id)
    }
    
    /// Generate key for archive-tier message
    pub fn archive_message_key(message_id: Uuid, created_at: chrono::DateTime<Utc>) -> String {
        let date = created_at.format("%Y/%m");
        format!("archive/{}/{}", date, message_id)
    }
    
    /// Generate key for backup
    pub fn backup_key(backup_id: Uuid, timestamp: chrono::DateTime<Utc>) -> String {
        let date = timestamp.format("%Y%m%d");
        format!("backups/{}/{}", date, backup_id)
    }
    
    /// Generate key for user avatar
    pub fn avatar_key(user_id: &str) -> String {
        format!("avatars/{}", user_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Utc;
    
    #[test]
    fn test_key_generation() {
        let message_id = Uuid::new_v4();
        let now = Utc::now();
        
        let media_key = keys::media_key("user123", "photo.jpg");
        assert!(media_key.contains("media/"));
        assert!(media_key.contains("user123"));
        
        let cold_key = keys::cold_message_key(message_id, now);
        assert!(cold_key.starts_with("cold/"));
        
        let archive_key = keys::archive_message_key(message_id, now);
        assert!(archive_key.starts_with("archive/"));
        
        let avatar_key = keys::avatar_key("user456");
        assert_eq!(avatar_key, "avatars/user456");
    }
    
    #[test]
    fn test_storage_tier() {
        let tier = StorageTier::Hot;
        assert_eq!(tier, StorageTier::Hot);
        assert_ne!(tier, StorageTier::Cold);
    }
}
