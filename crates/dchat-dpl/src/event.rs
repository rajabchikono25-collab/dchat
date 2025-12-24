//! Event emission for DPL programs

use crate::serde::DplSerialize;
#[cfg(not(feature = "std"))]
use alloc::vec::Vec;

/// Emit an event to the runtime
pub fn emit_event<T: DplSerialize>(event: &T) {
    let mut buf = Vec::new();
    if event.serialize(&mut buf).is_ok() {
        let discriminator = compute_discriminator(core::any::type_name::<T>());
        #[cfg(feature = "std")]
        {
            let _ = (discriminator, buf);
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
    fn emit(&self)
    where
        Self: Sized,
    {
        emit_event(self);
    }
}

/// Macro to emit an event
///
/// # Example
/// ```ignore
/// emit!(CounterIncremented {
///     counter: counter_key,
///     new_value: counter.value,
/// });
/// ```
#[macro_export]
macro_rules! emit {
    ($event:expr) => {
        $crate::event::emit_event(&$event)
    };
}
