//! Multi-device identity management

use chrono::{DateTime, Utc};
use dchat_core::error::{Error, Result};
use dchat_core::types::{PublicKey, UserId};
use dchat_crypto::keys::KeyPair;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Device identifier
pub type DeviceId = String;

/// Represents a device associated with an identity
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Device {
    pub device_id: DeviceId,
    pub device_name: String,
    pub device_type: DeviceType,
    pub public_key: PublicKey,
    pub added_at: DateTime<Utc>,
    pub last_seen: DateTime<Utc>,
    pub trusted: bool,
}

/// Types of devices
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum DeviceType {
    Desktop,
    Mobile,
    Web,
    Server,
    Other(String),
}

impl Device {
    /// Create a new device
    pub fn new(
        device_id: DeviceId,
        device_name: String,
        device_type: DeviceType,
        keypair: &KeyPair,
    ) -> Self {
        let now = Utc::now();

        Self {
            device_id,
            device_name,
            device_type,
            public_key: keypair.public_key().to_core_public_key(),
            added_at: now,
            last_seen: now,
            trusted: false,
        }
    }

    /// Update last seen timestamp
    pub fn update_last_seen(&mut self) {
        self.last_seen = Utc::now();
    }

    /// Mark device as trusted
    pub fn set_trusted(&mut self, trusted: bool) {
        self.trusted = trusted;
    }

    /// Check if device has been inactive for a given duration
    pub fn is_inactive(&self, hours: i64) -> bool {
        let duration = Utc::now() - self.last_seen;
        duration.num_hours() > hours
    }
}

/// Manages devices for an identity
pub struct DeviceManager {
    user_devices: HashMap<UserId, Vec<Device>>,
    device_index: HashMap<String, (UserId, usize)>,
}

impl DeviceManager {
    /// Create a new device manager
    pub fn new() -> Self {
        Self {
            user_devices: HashMap::new(),
            device_index: HashMap::new(),
        }
    }

    /// Add a device to a user's identity
    pub fn add_device(&mut self, user_id: UserId, device: Device) -> Result<()> {
        // Check if device ID already exists
        if self.device_index.contains_key(&device.device_id) {
            return Err(Error::identity("Device ID already registered"));
        }

        // Get or create device list for user
        let devices = self.user_devices.entry(user_id.clone()).or_default();

        // Add device
        let device_id = device.device_id.clone();
        let index = devices.len();
        devices.push(device);

        // Update index
        self.device_index.insert(device_id, (user_id, index));

        Ok(())
    }

    /// Get all devices for a user
    pub fn get_devices(&self, user_id: &UserId) -> Vec<&Device> {
        self.user_devices
            .get(user_id)
            .map(|devices| devices.iter().collect())
            .unwrap_or_default()
    }

    /// Get a specific device
    pub fn get_device(&self, device_id: &str) -> Option<&Device> {
        self.device_index
            .get(device_id)
            .and_then(|(user_id, index)| {
                self.user_devices
                    .get(user_id)
                    .and_then(|devices| devices.get(*index))
            })
    }

    /// Get mutable device
    pub fn get_device_mut(&mut self, device_id: &str) -> Option<&mut Device> {
        if let Some((user_id, index)) = self.device_index.get(device_id).cloned() {
            self.user_devices
                .get_mut(&user_id)
                .and_then(|devices| devices.get_mut(index))
        } else {
            None
        }
    }

    /// Update device last seen
    pub fn update_device_activity(&mut self, device_id: &str) -> Result<()> {
        let device = self
            .get_device_mut(device_id)
            .ok_or_else(|| Error::identity("Device not found"))?;

        device.update_last_seen();
        Ok(())
    }

    /// Trust a device
    pub fn trust_device(&mut self, device_id: &str) -> Result<()> {
        let device = self
            .get_device_mut(device_id)
            .ok_or_else(|| Error::identity("Device not found"))?;

        device.set_trusted(true);
        Ok(())
    }

    /// Revoke a device
    pub fn revoke_device(&mut self, device_id: &str) -> Result<Device> {
        let (user_id, index) = self
            .device_index
            .remove(device_id)
            .ok_or_else(|| Error::identity("Device not found"))?;

        let devices = self
            .user_devices
            .get_mut(&user_id)
            .ok_or_else(|| Error::identity("User not found"))?;

        let device = devices.remove(index);

        // Update indices for remaining devices
        for (idx, dev) in devices.iter().enumerate().skip(index) {
            self.device_index
                .insert(dev.device_id.clone(), (user_id.clone(), idx));
        }

        Ok(device)
    }

    /// Get trusted devices for a user
    pub fn get_trusted_devices(&self, user_id: &UserId) -> Vec<&Device> {
        self.user_devices
            .get(user_id)
            .map(|devices| devices.iter().filter(|d| d.trusted).collect())
            .unwrap_or_default()
    }

    /// Check if user has any trusted devices
    pub fn has_trusted_devices(&self, user_id: &UserId) -> bool {
        self.user_devices
            .get(user_id)
            .map(|devices| devices.iter().any(|d| d.trusted))
            .unwrap_or(false)
    }

    /// Get device count for user
    pub fn device_count(&self, user_id: &UserId) -> usize {
        self.user_devices
            .get(user_id)
            .map(|devices| devices.len())
            .unwrap_or(0)
    }

    /// Remove inactive devices
    pub fn cleanup_inactive_devices(&mut self, hours: i64) -> Vec<(UserId, Device)> {
        let mut removed = Vec::new();

        for (user_id, devices) in self.user_devices.iter_mut() {
            let mut indices_to_remove = Vec::new();

            for (idx, device) in devices.iter().enumerate() {
                if device.is_inactive(hours) {
                    indices_to_remove.push(idx);
                }
            }

            // Remove in reverse order to maintain indices
            for idx in indices_to_remove.into_iter().rev() {
                let device = devices.remove(idx);
                self.device_index.remove(&device.device_id);
                removed.push((user_id.clone(), device));
            }
        }

        removed
    }
}

impl Default for DeviceManager {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use dchat_crypto::keys::KeyPair;

    #[test]
    fn test_device_creation() {
        let keypair = KeyPair::generate();
        let device = Device::new(
            "device1".to_string(),
            "Alice's Laptop".to_string(),
            DeviceType::Desktop,
            &keypair,
        );

        assert_eq!(device.device_id, "device1");
        assert_eq!(device.device_name, "Alice's Laptop");
        assert_eq!(device.trusted, false);
    }

    #[test]
    fn test_device_manager() {
        let mut manager = DeviceManager::new();
        let user_id = UserId::new();

        // Add device
        let keypair = KeyPair::generate();
        let device = Device::new(
            "device1".to_string(),
            "Device 1".to_string(),
            DeviceType::Desktop,
            &keypair,
        );

        assert!(manager.add_device(user_id.clone(), device).is_ok());

        // Get devices
        let devices = manager.get_devices(&user_id);
        assert_eq!(devices.len(), 1);

        // Trust device
        assert!(manager.trust_device("device1").is_ok());
        assert!(manager.has_trusted_devices(&user_id));

        // Get device
        let device = manager.get_device("device1");
        assert!(device.is_some());
        assert!(device.unwrap().trusted);
    }

    #[test]
    fn test_device_revocation() {
        let mut manager = DeviceManager::new();
        let user_id = UserId::new();

        let keypair = KeyPair::generate();
        let device = Device::new(
            "device1".to_string(),
            "Device 1".to_string(),
            DeviceType::Desktop,
            &keypair,
        );

        manager.add_device(user_id.clone(), device).unwrap();
        assert_eq!(manager.device_count(&user_id), 1);

        // Revoke device
        assert!(manager.revoke_device("device1").is_ok());
        assert_eq!(manager.device_count(&user_id), 0);
        assert!(manager.get_device("device1").is_none());
    }
}
