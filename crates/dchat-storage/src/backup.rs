//! Encrypted backup and restore

use dchat_core::error::{Error, Result};
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

/// Encrypted backup metadata
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackupMetadata {
    /// Backup version
    pub version: String,

    /// Creation timestamp
    pub created_at: i64,

    /// User ID
    pub user_id: String,

    /// Backup size in bytes
    pub size: usize,

    /// Encryption algorithm
    pub encryption: String,

    /// Checksum for integrity
    pub checksum: String,
}

/// Encrypted backup
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EncryptedBackup {
    /// Backup metadata
    pub metadata: BackupMetadata,

    /// Encrypted data
    pub encrypted_data: Vec<u8>,

    /// Encryption nonce
    pub nonce: Vec<u8>,
}

impl EncryptedBackup {
    /// Create a new encrypted backup
    pub fn new(user_id: String, plaintext: Vec<u8>, encryption_key: &[u8]) -> Result<Self> {
        // In a real implementation, this would:
        // 1. Use ChaCha20-Poly1305 or similar AEAD
        // 2. Generate random nonce
        // 3. Encrypt the data
        // 4. Calculate checksum

        let mut nonce = vec![0u8; 12];
        use rand::RngCore;
        rand::thread_rng().fill_bytes(&mut nonce);

        let encrypted_data = Self::encrypt(&plaintext, encryption_key)?;
        let checksum = blake3::hash(&plaintext).to_hex().to_string();

        let metadata = BackupMetadata {
            version: env!("CARGO_PKG_VERSION").to_string(),
            created_at: chrono::Utc::now().timestamp(),
            user_id,
            size: plaintext.len(),
            encryption: "ChaCha20-Poly1305".to_string(),
            checksum,
        };

        Ok(Self {
            metadata,
            encrypted_data,
            nonce,
        })
    }

    /// Decrypt backup
    pub fn decrypt(&self, encryption_key: &[u8]) -> Result<Vec<u8>> {
        Self::do_decrypt(&self.encrypted_data, encryption_key, &self.nonce)
    }

    /// Verify backup integrity
    pub fn verify(&self, decrypted_data: &[u8]) -> bool {
        let checksum = blake3::hash(decrypted_data).to_hex().to_string();
        checksum == self.metadata.checksum
    }

    fn encrypt(plaintext: &[u8], key: &[u8]) -> Result<Vec<u8>> {
        use chacha20poly1305::aead::generic_array::GenericArray;
        use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, KeyInit};

        if key.len() != 32 {
            return Err(Error::crypto("Key must be 32 bytes"));
        }

        let cipher = ChaCha20Poly1305::new_from_slice(key)
            .map_err(|e| Error::crypto(format!("Invalid key: {}", e)))?;

        let nonce = GenericArray::from_slice(&[0u8; 12]); // Will be replaced with random nonce

        let mut buffer = plaintext.to_vec();
        cipher
            .encrypt_in_place(nonce, b"", &mut buffer)
            .map_err(|e| Error::crypto(format!("Encryption failed: {}", e)))?;

        Ok(buffer)
    }

    fn do_decrypt(ciphertext: &[u8], key: &[u8], nonce: &[u8]) -> Result<Vec<u8>> {
        use chacha20poly1305::aead::generic_array::GenericArray;
        use chacha20poly1305::{AeadInPlace, ChaCha20Poly1305, KeyInit};

        if key.len() != 32 {
            return Err(Error::crypto("Key must be 32 bytes"));
        }

        if nonce.len() != 12 {
            return Err(Error::crypto("Nonce must be 12 bytes"));
        }

        let cipher = ChaCha20Poly1305::new_from_slice(key)
            .map_err(|e| Error::crypto(format!("Invalid key: {}", e)))?;

        let nonce = GenericArray::from_slice(nonce);

        let mut buffer = ciphertext.to_vec();
        cipher
            .decrypt_in_place(nonce, b"", &mut buffer)
            .map_err(|e| Error::crypto(format!("Decryption failed: {}", e)))?;

        Ok(buffer)
    }
}

/// Backup manager
pub struct BackupManager {
    /// Backup directory
    backup_dir: PathBuf,

    /// Maximum number of backups to retain
    max_backups: usize,
}

impl BackupManager {
    pub fn new(backup_dir: PathBuf, max_backups: usize) -> Self {
        Self {
            backup_dir,
            max_backups,
        }
    }

    /// Create a backup
    pub async fn create_backup(
        &self,
        user_id: String,
        data: Vec<u8>,
        encryption_key: &[u8],
    ) -> Result<PathBuf> {
        let backup = EncryptedBackup::new(user_id.clone(), data, encryption_key)?;

        // Generate backup filename
        let timestamp = chrono::Utc::now().timestamp();
        let filename = format!("backup_{}_{}. dchat", user_id, timestamp);
        let backup_path = self.backup_dir.join(filename);

        // Serialize and write backup
        let serialized = bincode::serialize(&backup)
            .map_err(|e| Error::storage(format!("Failed to serialize backup: {}", e)))?;

        tokio::fs::create_dir_all(&self.backup_dir)
            .await
            .map_err(|e| Error::storage(format!("Failed to create backup directory: {}", e)))?;

        tokio::fs::write(&backup_path, serialized)
            .await
            .map_err(|e| Error::storage(format!("Failed to write backup: {}", e)))?;

        tracing::info!("Created backup: {:?}", backup_path);

        // Cleanup old backups
        self.cleanup_old_backups().await?;

        Ok(backup_path)
    }

    /// Restore from backup
    pub async fn restore_backup(
        &self,
        backup_path: PathBuf,
        encryption_key: &[u8],
    ) -> Result<Vec<u8>> {
        let data = tokio::fs::read(&backup_path)
            .await
            .map_err(|e| Error::storage(format!("Failed to read backup: {}", e)))?;

        let backup: EncryptedBackup = bincode::deserialize(&data)
            .map_err(|e| Error::storage(format!("Failed to deserialize backup: {}", e)))?;

        let decrypted = backup.decrypt(encryption_key)?;

        if !backup.verify(&decrypted) {
            return Err(Error::storage("Backup integrity check failed".to_string()));
        }

        tracing::info!("Restored backup: {:?}", backup_path);

        Ok(decrypted)
    }

    /// List available backups
    pub async fn list_backups(&self) -> Result<Vec<BackupMetadata>> {
        let mut entries = tokio::fs::read_dir(&self.backup_dir)
            .await
            .map_err(|e| Error::storage(format!("Failed to read backup directory: {}", e)))?;

        let mut backups = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| Error::storage(e.to_string()))?
        {
            let path = entry.path();

            if path.extension().and_then(|s| s.to_str()) == Some("dchat") {
                if let Ok(data) = tokio::fs::read(&path).await {
                    if let Ok(backup) = bincode::deserialize::<EncryptedBackup>(&data) {
                        backups.push(backup.metadata);
                    }
                }
            }
        }

        // Sort by creation time, newest first
        backups.sort_by(|a, b| b.created_at.cmp(&a.created_at));

        Ok(backups)
    }

    /// Cleanup old backups
    async fn cleanup_old_backups(&self) -> Result<()> {
        let backups = self.list_backups().await?;

        if backups.len() > self.max_backups {
            // Remove oldest backups
            for backup in backups.iter().skip(self.max_backups) {
                let filename = format!("backup_{}_{}.dchat", backup.user_id, backup.created_at);
                let path = self.backup_dir.join(filename);

                if let Err(e) = tokio::fs::remove_file(&path).await {
                    tracing::warn!("Failed to remove old backup {:?}: {}", path, e);
                }
            }
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn test_encrypted_backup() {
        let user_id = "user123".to_string();
        let data = b"sensitive data".to_vec();
        let key = b"encryption_key_32_bytes_long____";

        let backup = EncryptedBackup::new(user_id, data.clone(), key);
        assert!(backup.is_ok());

        let backup = backup.unwrap();
        let decrypted = backup.decrypt(key);
        assert!(decrypted.is_ok());

        let decrypted = decrypted.unwrap();
        assert!(backup.verify(&decrypted));
    }

    #[tokio::test]
    async fn test_backup_manager() {
        let dir = tempdir().unwrap();
        let manager = BackupManager::new(dir.path().to_path_buf(), 3);

        let user_id = "user123".to_string();
        let data = b"test backup data".to_vec();
        let key = b"encryption_key_32_bytes_long____";

        let result = manager.create_backup(user_id, data.clone(), key).await;
        assert!(result.is_ok());

        let backup_path = result.unwrap();
        let restored = manager.restore_backup(backup_path, key).await;
        assert!(restored.is_ok());
        assert_eq!(restored.unwrap(), data);
    }
}
