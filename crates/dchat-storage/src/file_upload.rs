//! File upload and storage system for media files

use dchat_core::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use tokio::fs;
use tokio::io::AsyncWriteExt;
use uuid::Uuid;
use sha2::{Sha256, Digest};

/// Media file type
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MediaFileType {
    Photo,
    Video,
    Audio,
    Voice,
    Document,
    Sticker,
    Animation,
    VideoNote,
}

/// Uploaded file metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UploadedFile {
    pub file_id: String,
    pub file_unique_id: String,
    pub file_type: MediaFileType,
    pub file_size: u64,
    pub file_path: String,
    pub mime_type: Option<String>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub duration: Option<u32>,
    pub checksum: String,
    pub uploaded_at: chrono::DateTime<chrono::Utc>,
}

/// File upload configuration
#[derive(Debug, Clone)]
pub struct UploadConfig {
    pub storage_path: PathBuf,
    pub max_photo_size: u64,      // 10 MB
    pub max_video_size: u64,      // 100 MB
    pub max_audio_size: u64,      // 50 MB
    pub max_document_size: u64,   // 100 MB
    pub allowed_photo_types: Vec<String>,
    pub allowed_video_types: Vec<String>,
    pub allowed_audio_types: Vec<String>,
    pub allowed_document_types: Vec<String>,
}

impl Default for UploadConfig {
    fn default() -> Self {
        Self {
            storage_path: PathBuf::from("./data/uploads"),
            max_photo_size: 10 * 1024 * 1024,      // 10 MB
            max_video_size: 100 * 1024 * 1024,     // 100 MB
            max_audio_size: 50 * 1024 * 1024,      // 50 MB
            max_document_size: 100 * 1024 * 1024,  // 100 MB
            allowed_photo_types: vec![
                "image/jpeg".to_string(),
                "image/png".to_string(),
                "image/gif".to_string(),
                "image/webp".to_string(),
            ],
            allowed_video_types: vec![
                "video/mp4".to_string(),
                "video/mpeg".to_string(),
                "video/webm".to_string(),
                "video/quicktime".to_string(),
            ],
            allowed_audio_types: vec![
                "audio/mpeg".to_string(),
                "audio/mp4".to_string(),
                "audio/ogg".to_string(),
                "audio/wav".to_string(),
                "audio/webm".to_string(),
            ],
            allowed_document_types: vec![
                "application/pdf".to_string(),
                "application/zip".to_string(),
                "text/plain".to_string(),
                "application/msword".to_string(),
                "application/vnd.openxmlformats-officedocument.wordprocessingml.document".to_string(),
            ],
        }
    }
}

/// File upload manager
pub struct FileUploadManager {
    config: UploadConfig,
}

impl FileUploadManager {
    /// Create new file upload manager
    pub fn new(config: UploadConfig) -> Self {
        Self { config }
    }

    /// Create with default configuration
    pub fn with_defaults() -> Self {
        Self::new(UploadConfig::default())
    }

    /// Initialize storage directory
    pub async fn init_storage(&self) -> Result<()> {
        fs::create_dir_all(&self.config.storage_path)
            .await
            .map_err(|e| Error::storage(format!("Failed to create storage directory: {}", e)))?;

        // Create subdirectories for each media type
        for subdir in &["photos", "videos", "audio", "voice", "documents", "stickers", "animations", "thumbnails"] {
            let path = self.config.storage_path.join(subdir);
            fs::create_dir_all(&path)
                .await
                .map_err(|e| Error::storage(format!("Failed to create subdirectory {}: {}", subdir, e)))?;
        }

        Ok(())
    }

    /// Upload a file
    pub async fn upload_file(
        &self,
        file_type: MediaFileType,
        file_data: Vec<u8>,
        mime_type: Option<String>,
        width: Option<u32>,
        height: Option<u32>,
        duration: Option<u32>,
    ) -> Result<UploadedFile> {
        // Validate file size
        let file_size = file_data.len() as u64;
        self.validate_file_size(file_type, file_size)?;

        // Validate MIME type
        if let Some(ref mime) = mime_type {
            self.validate_mime_type(file_type, mime)?;
        }

        // Generate unique file ID and checksum
        let file_id = Uuid::new_v4().to_string();
        let file_unique_id = format!("{}", Uuid::new_v4().simple());
        let checksum = self.compute_checksum(&file_data);

        // Determine file extension from MIME type
        let extension = mime_type.as_ref()
            .and_then(|m| self.get_extension_from_mime(m))
            .unwrap_or("bin");

        // Determine storage subdirectory
        let subdir = match file_type {
            MediaFileType::Photo => "photos",
            MediaFileType::Video => "videos",
            MediaFileType::Audio => "audio",
            MediaFileType::Voice => "voice",
            MediaFileType::Document => "documents",
            MediaFileType::Sticker => "stickers",
            MediaFileType::Animation => "animations",
            MediaFileType::VideoNote => "videos",
        };

        // Create file path: storage_path/subdir/file_id.ext
        let filename = format!("{}.{}", file_id, extension);
        let file_path = self.config.storage_path.join(subdir).join(&filename);

        // Write file to disk
        let mut file = fs::File::create(&file_path)
            .await
            .map_err(|e| Error::storage(format!("Failed to create file: {}", e)))?;

        file.write_all(&file_data)
            .await
            .map_err(|e| Error::storage(format!("Failed to write file: {}", e)))?;

        file.sync_all()
            .await
            .map_err(|e| Error::storage(format!("Failed to sync file: {}", e)))?;

        Ok(UploadedFile {
            file_id,
            file_unique_id,
            file_type,
            file_size,
            file_path: file_path.to_string_lossy().to_string(),
            mime_type,
            width,
            height,
            duration,
            checksum,
            uploaded_at: chrono::Utc::now(),
        })
    }

    /// Get file by ID
    pub async fn get_file(&self, file_id: &str) -> Result<Vec<u8>> {
        // Search all subdirectories for the file
        for subdir in &["photos", "videos", "audio", "voice", "documents", "stickers", "animations"] {
            let dir_path = self.config.storage_path.join(subdir);
            
            if let Ok(mut entries) = fs::read_dir(&dir_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let filename = entry.file_name();
                    let filename_str = filename.to_string_lossy();
                    
                    if filename_str.starts_with(file_id) {
                        let file_path = entry.path();
                        let file_data = fs::read(&file_path)
                            .await
                            .map_err(|e| Error::storage(format!("Failed to read file: {}", e)))?;
                        return Ok(file_data);
                    }
                }
            }
        }

        Err(Error::NotFound(format!("File not found: {}", file_id)))
    }

    /// Delete file by ID
    pub async fn delete_file(&self, file_id: &str) -> Result<()> {
        // Search all subdirectories for the file
        for subdir in &["photos", "videos", "audio", "voice", "documents", "stickers", "animations", "thumbnails"] {
            let dir_path = self.config.storage_path.join(subdir);
            
            if let Ok(mut entries) = fs::read_dir(&dir_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    let filename = entry.file_name();
                    let filename_str = filename.to_string_lossy();
                    
                    if filename_str.starts_with(file_id) {
                        let file_path = entry.path();
                        fs::remove_file(&file_path)
                            .await
                            .map_err(|e| Error::storage(format!("Failed to delete file: {}", e)))?;
                        return Ok(());
                    }
                }
            }
        }

        Err(Error::NotFound(format!("File not found: {}", file_id)))
    }

    /// Generate thumbnail for image or video
    pub async fn generate_thumbnail(
        &self,
        source_file_id: &str,
        max_width: u32,
        max_height: u32,
    ) -> Result<UploadedFile> {
        // Get original file
        let file_data = self.get_file(source_file_id).await?;

        // Generate thumbnail using image processing
        use image::GenericImageView;
        
        let thumbnail_id = format!("thumb_{}", Uuid::new_v4());
        
        let thumbnail_path = self.config.storage_path
            .join("thumbnails")
            .join(format!("{}.jpg", thumbnail_id));

        // Decode image
        let img = image::load_from_memory(&file_data)
            .map_err(|e| Error::storage(format!("Failed to decode image: {}", e)))?;
        
        // Calculate new dimensions maintaining aspect ratio
        let (orig_width, orig_height) = img.dimensions();
        let (new_width, new_height) = if orig_width > orig_height {
            let ratio = max_width as f32 / orig_width as f32;
            (max_width, (orig_height as f32 * ratio) as u32)
        } else {
            let ratio = max_height as f32 / orig_height as f32;
            ((orig_width as f32 * ratio) as u32, max_height)
        };
        
        // Resize image
        let thumbnail = img.resize(new_width, new_height, image::imageops::FilterType::Lanczos3);
        
        // Save as JPEG
        thumbnail.save(&thumbnail_path)
            .map_err(|e| Error::storage(format!("Failed to save thumbnail: {}", e)))?;
        
        // Read back the saved file for checksum
        let thumbnail_data = fs::read(&thumbnail_path)
            .await
            .map_err(|e| Error::storage(format!("Failed to read thumbnail: {}", e)))?;
        
        let checksum = self.compute_checksum(&thumbnail_data);

        Ok(UploadedFile {
            file_id: thumbnail_id.clone(),
            file_unique_id: format!("{}", Uuid::new_v4().simple()),
            file_type: MediaFileType::Photo,
            file_size: file_data.len() as u64,
            file_path: thumbnail_path.to_string_lossy().to_string(),
            mime_type: Some("image/jpeg".to_string()),
            width: Some(max_width),
            height: Some(max_height),
            duration: None,
            checksum,
            uploaded_at: chrono::Utc::now(),
        })
    }

    /// Compute SHA-256 checksum of file data
    fn compute_checksum(&self, data: &[u8]) -> String {
        let mut hasher = Sha256::new();
        hasher.update(data);
        format!("{:x}", hasher.finalize())
    }

    /// Validate file size
    fn validate_file_size(&self, file_type: MediaFileType, file_size: u64) -> Result<()> {
        let max_size = match file_type {
            MediaFileType::Photo => self.config.max_photo_size,
            MediaFileType::Video | MediaFileType::VideoNote => self.config.max_video_size,
            MediaFileType::Audio | MediaFileType::Voice => self.config.max_audio_size,
            MediaFileType::Document | MediaFileType::Sticker | MediaFileType::Animation => {
                self.config.max_document_size
            }
        };

        if file_size > max_size {
            return Err(Error::validation(format!(
                "File size {} exceeds maximum {} for {:?}",
                file_size, max_size, file_type
            )));
        }

        Ok(())
    }

    /// Validate MIME type
    fn validate_mime_type(&self, file_type: MediaFileType, mime_type: &str) -> Result<()> {
        let allowed_types = match file_type {
            MediaFileType::Photo => &self.config.allowed_photo_types,
            MediaFileType::Video | MediaFileType::VideoNote => &self.config.allowed_video_types,
            MediaFileType::Audio | MediaFileType::Voice => &self.config.allowed_audio_types,
            MediaFileType::Document | MediaFileType::Sticker | MediaFileType::Animation => {
                &self.config.allowed_document_types
            }
        };

        if !allowed_types.iter().any(|t| t == mime_type) {
            return Err(Error::validation(format!(
                "MIME type {} not allowed for {:?}",
                mime_type, file_type
            )));
        }

        Ok(())
    }

    /// Get file extension from MIME type
    fn get_extension_from_mime(&self, mime_type: &str) -> Option<&str> {
        match mime_type {
            "image/jpeg" => Some("jpg"),
            "image/png" => Some("png"),
            "image/gif" => Some("gif"),
            "image/webp" => Some("webp"),
            "video/mp4" => Some("mp4"),
            "video/mpeg" => Some("mpeg"),
            "video/webm" => Some("webm"),
            "video/quicktime" => Some("mov"),
            "audio/mpeg" => Some("mp3"),
            "audio/mp4" => Some("m4a"),
            "audio/ogg" => Some("ogg"),
            "audio/wav" => Some("wav"),
            "audio/webm" => Some("webm"),
            "application/pdf" => Some("pdf"),
            "application/zip" => Some("zip"),
            "text/plain" => Some("txt"),
            _ => None,
        }
    }

    /// Get storage statistics
    pub async fn get_storage_stats(&self) -> Result<StorageStats> {
        let mut stats = StorageStats::default();

        for subdir in &["photos", "videos", "audio", "voice", "documents", "stickers", "animations", "thumbnails"] {
            let dir_path = self.config.storage_path.join(subdir);
            
            if let Ok(mut entries) = fs::read_dir(&dir_path).await {
                while let Ok(Some(entry)) = entries.next_entry().await {
                    if let Ok(metadata) = entry.metadata().await {
                        if metadata.is_file() {
                            stats.total_files += 1;
                            stats.total_size += metadata.len();
                        }
                    }
                }
            }
        }

        Ok(stats)
    }
}

/// Storage statistics
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct StorageStats {
    pub total_files: u64,
    pub total_size: u64,
}

impl StorageStats {
    pub fn size_mb(&self) -> f64 {
        self.total_size as f64 / (1024.0 * 1024.0)
    }

    pub fn size_gb(&self) -> f64 {
        self.total_size as f64 / (1024.0 * 1024.0 * 1024.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn test_upload_manager_creation() {
        let manager = FileUploadManager::with_defaults();
        assert_eq!(manager.config.max_photo_size, 10 * 1024 * 1024);
    }

    #[tokio::test]
    async fn test_checksum_computation() {
        let manager = FileUploadManager::with_defaults();
        let data = b"test data";
        let checksum = manager.compute_checksum(data);
        assert_eq!(checksum.len(), 64); // SHA-256 produces 64 hex characters
    }

    #[tokio::test]
    async fn test_file_size_validation() {
        let manager = FileUploadManager::with_defaults();
        
        // Valid size
        assert!(manager.validate_file_size(MediaFileType::Photo, 1024 * 1024).is_ok());
        
        // Too large
        assert!(manager.validate_file_size(MediaFileType::Photo, 20 * 1024 * 1024).is_err());
    }

    #[tokio::test]
    async fn test_mime_type_validation() {
        let manager = FileUploadManager::with_defaults();
        
        // Valid MIME type
        assert!(manager.validate_mime_type(MediaFileType::Photo, "image/jpeg").is_ok());
        
        // Invalid MIME type
        assert!(manager.validate_mime_type(MediaFileType::Photo, "application/pdf").is_err());
    }

    #[tokio::test]
    async fn test_extension_from_mime() {
        let manager = FileUploadManager::with_defaults();
        
        assert_eq!(manager.get_extension_from_mime("image/jpeg"), Some("jpg"));
        assert_eq!(manager.get_extension_from_mime("video/mp4"), Some("mp4"));
        assert_eq!(manager.get_extension_from_mime("audio/mpeg"), Some("mp3"));
        assert_eq!(manager.get_extension_from_mime("unknown/type"), None);
    }
}
