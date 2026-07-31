//! Domain-specific error types for `omnitrade-core`.
//!
//! Each error variant maps to a single failure mode, following the principle
//! of strongly typed errors. All errors implement `std::error::Error` via
//! `thiserror` and are safe to propagate with the `?` operator.

use thiserror::Error;

/// Errors produced by `omnitrade-core` operations.
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum CoreError {
    /// An indicator or buffer was constructed with an invalid period.
    #[error("invalid period: {0} (must be greater than zero)")]
    InvalidPeriod(usize),

    /// A `RingBuffer` was constructed with a non-power-of-two capacity.
    #[error("invalid ring buffer capacity: {0} (must be a power of two and greater than zero)")]
    InvalidCapacity(usize),

    /// A candle was constructed with invalid OHLCV relationships.
    #[error("invalid candle: {reason}")]
    InvalidCandle {
        /// Human-readable explanation of the validation failure.
        reason: String,
    },

    /// A value was outside its expected domain (e.g., negative quantity).
    #[error("value out of range: {field} = {value} ({reason})")]
    ValueOutOfRange {
        /// Name of the field that failed validation.
        field: &'static str,
        /// The invalid value.
        value: f64,
        /// Why the value is invalid.
        reason: &'static str,
    },
}
