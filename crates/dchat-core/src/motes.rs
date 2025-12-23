//! Motes: The canonical smallest unit of DCHAT currency
//!
//! This module defines the authoritative monetary unit for the dchat system.
//! All balances, fees, stakes, and rewards are denominated in motes.
//!
//! # Unit Definition
//! - 1 DCHAT = 100,000,000 motes (8 decimal places)
//! - Motes are the atomic unit - all on-chain values use motes
//! - Display values should convert using the helpers in this module
//!
//! # Usage
//! ```rust
//! use dchat_core::motes::{Motes, MOTES_PER_DCHAT, dchat_to_motes, motes_to_dchat};
//!
//! let stake: Motes = dchat_to_motes(10_000); // 10,000 DCHAT in motes
//! let dchat_value = motes_to_dchat(stake);   // Back to 10,000.0 DCHAT
//! ```

use std::fmt;

/// The atomic monetary unit for DCHAT.
///
/// All on-chain balances, fees, stakes, and rewards are stored in motes.
/// 1 DCHAT = 100,000,000 motes (8 decimal places).
///
/// This is a type alias to ensure type safety while maintaining zero-cost abstraction.
pub type Motes = u64;

/// Number of motes in 1 DCHAT token.
///
/// DCHAT uses 8 decimal places:
/// - 1 DCHAT = 100,000,000 motes
/// - 0.00000001 DCHAT = 1 mote (smallest transferable unit)
pub const MOTES_PER_DCHAT: u64 = 100_000_000;

/// Number of decimal places for DCHAT.
pub const DCHAT_DECIMALS: u8 = 8;

/// Maximum possible supply in motes.
/// 100 billion DCHAT = 10^10 DCHAT * 10^8 motes = 10^18 motes.
/// This fits comfortably in u64 (max ~1.84 * 10^19).
pub const MAX_SUPPLY_MOTES: u64 = 100_000_000_000 * MOTES_PER_DCHAT;

// ============================================================================
// Conversion Functions
// ============================================================================

/// Convert whole DCHAT tokens to motes.
///
/// # Panics
/// Panics in debug mode if overflow would occur.
///
/// # Examples
/// ```
/// use dchat_core::motes::dchat_to_motes;
/// assert_eq!(dchat_to_motes(1), 100_000_000);
/// assert_eq!(dchat_to_motes(10_000), 1_000_000_000_000);
/// ```
#[inline]
pub const fn dchat_to_motes(dchat: u64) -> Motes {
    dchat.saturating_mul(MOTES_PER_DCHAT)
}

/// Convert motes to DCHAT as a floating-point value (for display only).
///
/// **Warning**: Do not use for arithmetic - use motes directly for all calculations.
///
/// # Examples
/// ```
/// use dchat_core::motes::motes_to_dchat;
/// assert_eq!(motes_to_dchat(100_000_000), 1.0);
/// assert_eq!(motes_to_dchat(150_000_000), 1.5);
/// ```
#[inline]
pub fn motes_to_dchat(motes: Motes) -> f64 {
    motes as f64 / MOTES_PER_DCHAT as f64
}

/// Convert motes to whole DCHAT (truncated, for integer display).
///
/// # Examples
/// ```
/// use dchat_core::motes::motes_to_whole_dchat;
/// assert_eq!(motes_to_whole_dchat(150_000_000), 1); // 1.5 DCHAT -> 1
/// assert_eq!(motes_to_whole_dchat(99_999_999), 0);  // < 1 DCHAT -> 0
/// ```
#[inline]
pub const fn motes_to_whole_dchat(motes: Motes) -> u64 {
    motes / MOTES_PER_DCHAT
}

/// Get the fractional motes (remainder after whole DCHAT extraction).
///
/// # Examples
/// ```
/// use dchat_core::motes::motes_fractional;
/// assert_eq!(motes_fractional(150_000_000), 50_000_000); // 0.5 DCHAT remainder
/// ```
#[inline]
pub const fn motes_fractional(motes: Motes) -> u64 {
    motes % MOTES_PER_DCHAT
}

// ============================================================================
// Checked Arithmetic
// ============================================================================

/// Add motes with overflow checking.
///
/// Returns `None` if overflow would occur.
#[inline]
pub const fn motes_add(a: Motes, b: Motes) -> Option<Motes> {
    a.checked_add(b)
}

/// Subtract motes with underflow checking.
///
/// Returns `None` if underflow would occur.
#[inline]
pub const fn motes_sub(a: Motes, b: Motes) -> Option<Motes> {
    a.checked_sub(b)
}

/// Multiply motes with overflow checking.
///
/// Returns `None` if overflow would occur.
#[inline]
pub const fn motes_mul(a: Motes, b: u64) -> Option<Motes> {
    a.checked_mul(b)
}

/// Divide motes (returns 0 if divisor is 0).
#[inline]
pub const fn motes_div(a: Motes, b: u64) -> Motes {
    if b == 0 {
        0
    } else {
        a / b
    }
}

// ============================================================================
// Formatting
// ============================================================================

/// Format motes as a human-readable DCHAT string.
///
/// # Examples
/// ```
/// use dchat_core::motes::format_motes;
/// assert_eq!(format_motes(100_000_000), "1.00000000 DCHAT");
/// assert_eq!(format_motes(150_500_000), "1.50500000 DCHAT");
/// assert_eq!(format_motes(1), "0.00000001 DCHAT");
/// ```
pub fn format_motes(motes: Motes) -> String {
    let whole = motes / MOTES_PER_DCHAT;
    let frac = motes % MOTES_PER_DCHAT;
    format!("{}.{:08} DCHAT", whole, frac)
}

/// Format motes as a compact string (removes trailing zeros).
///
/// # Examples
/// ```
/// use dchat_core::motes::format_motes_compact;
/// assert_eq!(format_motes_compact(100_000_000), "1 DCHAT");
/// assert_eq!(format_motes_compact(150_000_000), "1.5 DCHAT");
/// assert_eq!(format_motes_compact(123_456_789), "1.23456789 DCHAT");
/// ```
pub fn format_motes_compact(motes: Motes) -> String {
    let whole = motes / MOTES_PER_DCHAT;
    let frac = motes % MOTES_PER_DCHAT;

    if frac == 0 {
        format!("{} DCHAT", whole)
    } else {
        // Format with 8 decimals, then trim trailing zeros
        let frac_str = format!("{:08}", frac);
        let trimmed = frac_str.trim_end_matches('0');
        format!("{}.{} DCHAT", whole, trimmed)
    }
}

/// Parse a DCHAT string into motes.
///
/// Accepts formats:
/// - "1" or "1.0" -> 100_000_000 motes
/// - "1.5" -> 150_000_000 motes
/// - "0.00000001" -> 1 mote
///
/// # Errors
/// Returns `None` if the string is not a valid DCHAT amount.
pub fn parse_dchat(s: &str) -> Option<Motes> {
    // Remove " DCHAT" suffix if present
    let s = s
        .trim()
        .trim_end_matches(" DCHAT")
        .trim_end_matches("DCHAT")
        .trim();

    if s.is_empty() {
        return None;
    }

    if let Some(dot_pos) = s.find('.') {
        // Has decimal part
        let (whole_str, frac_str) = s.split_at(dot_pos);
        let frac_str = &frac_str[1..]; // Skip the dot

        // Parse whole part
        let whole: u64 = if whole_str.is_empty() {
            0
        } else {
            whole_str.parse().ok()?
        };

        // Parse fractional part (pad or truncate to 8 digits)
        if frac_str.len() > 8 {
            // Too many decimal places
            return None;
        }

        let frac_padded = format!("{:0<8}", frac_str);
        let frac: u64 = frac_padded.parse().ok()?;

        // Combine
        whole.checked_mul(MOTES_PER_DCHAT)?.checked_add(frac)
    } else {
        // No decimal part - whole DCHAT only
        let whole: u64 = s.parse().ok()?;
        whole.checked_mul(MOTES_PER_DCHAT)
    }
}

// ============================================================================
// Display wrapper
// ============================================================================

/// A wrapper type for displaying motes with nice formatting.
pub struct DisplayMotes(pub Motes);

impl fmt::Display for DisplayMotes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let whole = self.0 / MOTES_PER_DCHAT;
        let frac = self.0 % MOTES_PER_DCHAT;

        if frac == 0 {
            write!(f, "{} DCHAT", whole)
        } else {
            let frac_str = format!("{:08}", frac);
            let trimmed = frac_str.trim_end_matches('0');
            write!(f, "{}.{} DCHAT", whole, trimmed)
        }
    }
}

impl fmt::Debug for DisplayMotes {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "Motes({})", self.0)
    }
}

// ============================================================================
// Tests
// ============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_constants() {
        assert_eq!(MOTES_PER_DCHAT, 100_000_000);
        assert_eq!(DCHAT_DECIMALS, 8);
        assert_eq!(MAX_SUPPLY_MOTES, 100_000_000_000_000_000_000); // 10^20
    }

    #[test]
    fn test_dchat_to_motes() {
        assert_eq!(dchat_to_motes(0), 0);
        assert_eq!(dchat_to_motes(1), 100_000_000);
        assert_eq!(dchat_to_motes(10), 1_000_000_000);
        assert_eq!(dchat_to_motes(10_000), 1_000_000_000_000);
    }

    #[test]
    fn test_motes_to_dchat() {
        assert_eq!(motes_to_dchat(0), 0.0);
        assert_eq!(motes_to_dchat(100_000_000), 1.0);
        assert_eq!(motes_to_dchat(150_000_000), 1.5);
        assert_eq!(motes_to_dchat(1), 0.00000001);
    }

    #[test]
    fn test_motes_to_whole_dchat() {
        assert_eq!(motes_to_whole_dchat(0), 0);
        assert_eq!(motes_to_whole_dchat(99_999_999), 0);
        assert_eq!(motes_to_whole_dchat(100_000_000), 1);
        assert_eq!(motes_to_whole_dchat(250_000_000), 2);
    }

    #[test]
    fn test_motes_fractional() {
        assert_eq!(motes_fractional(0), 0);
        assert_eq!(motes_fractional(100_000_000), 0);
        assert_eq!(motes_fractional(150_000_000), 50_000_000);
        assert_eq!(motes_fractional(123_456_789), 23_456_789);
    }

    #[test]
    fn test_format_motes() {
        assert_eq!(format_motes(0), "0.00000000 DCHAT");
        assert_eq!(format_motes(1), "0.00000001 DCHAT");
        assert_eq!(format_motes(100_000_000), "1.00000000 DCHAT");
        assert_eq!(format_motes(123_456_789), "1.23456789 DCHAT");
    }

    #[test]
    fn test_format_motes_compact() {
        assert_eq!(format_motes_compact(0), "0 DCHAT");
        assert_eq!(format_motes_compact(100_000_000), "1 DCHAT");
        assert_eq!(format_motes_compact(150_000_000), "1.5 DCHAT");
        assert_eq!(format_motes_compact(123_456_789), "1.23456789 DCHAT");
        assert_eq!(format_motes_compact(100_100_000), "1.001 DCHAT");
    }

    #[test]
    fn test_parse_dchat() {
        assert_eq!(parse_dchat("1"), Some(100_000_000));
        assert_eq!(parse_dchat("1.0"), Some(100_000_000));
        assert_eq!(parse_dchat("1.5"), Some(150_000_000));
        assert_eq!(parse_dchat("0.00000001"), Some(1));
        assert_eq!(parse_dchat("10000"), Some(1_000_000_000_000));
        assert_eq!(parse_dchat("1 DCHAT"), Some(100_000_000));
        assert_eq!(parse_dchat("1.5 DCHAT"), Some(150_000_000));

        // Invalid inputs
        assert_eq!(parse_dchat(""), None);
        assert_eq!(parse_dchat("abc"), None);
        assert_eq!(parse_dchat("1.000000001"), None); // Too many decimals
    }

    #[test]
    fn test_checked_arithmetic() {
        assert_eq!(motes_add(100, 50), Some(150));
        assert_eq!(motes_add(u64::MAX, 1), None);

        assert_eq!(motes_sub(100, 50), Some(50));
        assert_eq!(motes_sub(50, 100), None);

        assert_eq!(motes_mul(100, 5), Some(500));
        assert_eq!(motes_mul(u64::MAX, 2), None);

        assert_eq!(motes_div(100, 5), 20);
        assert_eq!(motes_div(100, 0), 0);
    }

    #[test]
    fn test_display_motes() {
        assert_eq!(format!("{}", DisplayMotes(100_000_000)), "1 DCHAT");
        assert_eq!(format!("{}", DisplayMotes(150_000_000)), "1.5 DCHAT");
        assert_eq!(
            format!("{:?}", DisplayMotes(100_000_000)),
            "Motes(100000000)"
        );
    }
}
