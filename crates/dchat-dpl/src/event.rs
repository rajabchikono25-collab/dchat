//! Event emission for DPL programs

use crate::serde::DplSerialize;

/// Emit an event to the runtime
pub fn emit_event<T: DplSerialize>(event: &T) {
    // Serialize the event
    let mut buffer = [0u8; 1024];
    if let Ok(len) = event.serialize(&mut buffer) {
        // Compute discriminator (first 8 bytes of blake3 hash of type name)
        let discriminator = compute_discriminator(core::any::type_name::<T>());

        // In production, this would call the VM syscall to emit the event
        // For now, we just log it
        #[cfg(feature = "std")]
        {
            let _ = (discriminator, &buffer[..len]);
        }
    }
}

/// Compute event discriminator from type name
fn compute_discriminator(type_name: &str) -> [u8; 8] {
    let hash = blake3::hash(type_name.as_bytes());
    let mut discriminator = [0u8; 8];
    discriminator.copy_from_slice(&hash.as_bytes()[..8]);
    discriminator
}

/// Trait for event types
pub trait Event: DplSerialize {
    /// Get the event discriminator
    fn discriminator() -> [u8; 8];

    /// Emit this event
    fn emit(&self) {
        emit_event(self);
    }
}
