//! Domain-specific error types for `omnitrade-engine`.

/// Errors produced by `omnitrade-engine` operations.
#[derive(Debug, thiserror::Error, Clone, PartialEq)]
pub enum EngineError {
    /// Insufficient account balance to execute an operation.
    #[error("insufficient balance: required {required}, available {available}")]
    InsufficientBalance {
        /// Amount required for the operation.
        required: f64,
        /// Amount currently available.
        available: f64,
    },

    /// The specified order ID was not found.
    #[error("order not found: {order_id}")]
    OrderNotFound {
        /// The missing order ID.
        order_id: u64,
    },

    /// An order size was invalid or outside allowed limits.
    #[error("invalid order size {size}: {reason}")]
    InvalidOrderSize {
        /// The invalid size value.
        size: f64,
        /// Reason for validation failure.
        reason: &'static str,
    },

    /// A struct or method parameter failed validation.
    #[error("invalid field '{field}': {reason}")]
    InvalidFieldValue {
        /// Name of the field.
        field: &'static str,
        /// Reason for validation failure.
        reason: &'static str,
    },

    /// The requested trading pair symbol was not found.
    #[error("symbol not found: {symbol}")]
    SymbolNotFound {
        /// The missing symbol name.
        symbol: String,
    },

    /// The account is locked and cannot perform trading actions.
    #[error("account locked: {reason}")]
    AccountLocked {
        /// Reason for account lock.
        reason: String,
    },

    /// Transparent propagation of underlying core errors.
    #[error(transparent)]
    Core(#[from] omnitrade_core::CoreError),
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::CoreError;

    #[test]
    fn test_insufficient_balance_display() {
        // Arrange
        let err = EngineError::InsufficientBalance {
            required: 100.0,
            available: 50.0,
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(
            formatted,
            "insufficient balance: required 100, available 50"
        );
    }

    #[test]
    fn test_order_not_found_display() {
        // Arrange
        let err = EngineError::OrderNotFound { order_id: 42 };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "order not found: 42");
    }

    #[test]
    fn test_invalid_order_size_display() {
        // Arrange
        let err = EngineError::InvalidOrderSize {
            size: -1.5,
            reason: "size must be positive",
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "invalid order size -1.5: size must be positive");
    }

    #[test]
    fn test_symbol_not_found_display() {
        // Arrange
        let err = EngineError::SymbolNotFound {
            symbol: "BTCUSDT".to_string(),
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "symbol not found: BTCUSDT");
    }

    #[test]
    fn test_account_locked_display() {
        // Arrange
        let err = EngineError::AccountLocked {
            reason: "risk limit breached".to_string(),
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "account locked: risk limit breached");
    }

    #[test]
    fn test_invalid_field_value_display() {
        // Arrange
        let err = EngineError::InvalidFieldValue {
            field: "timestamp_ms",
            reason: "timestamp_ms must be non-zero",
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(
            formatted,
            "invalid field 'timestamp_ms': timestamp_ms must be non-zero"
        );
    }

    #[test]
    fn test_core_error_conversion() {
        // Arrange
        let core_err = CoreError::InvalidPeriod(0);

        // Act
        let engine_err: EngineError = core_err.into();

        // Assert
        assert_eq!(engine_err, EngineError::Core(CoreError::InvalidPeriod(0)));
        assert_eq!(
            format!("{engine_err}"),
            "invalid period: 0 (must be greater than zero)"
        );
    }
}
