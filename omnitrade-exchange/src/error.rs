//! Domain-specific error types for exchange connectors.

use omnitrade_core::CoreError;
use thiserror::Error;

/// Errors produced by exchange connections and streaming feeds.
#[derive(Debug, Error, Clone, PartialEq)]
pub enum ExchangeError {
    /// Connection to the exchange WebSocket or REST API failed.
    #[error("connection failed to '{url}': {reason}")]
    ConnectionFailed {
        /// Target URL or endpoint.
        url: String,
        /// Reason for failure.
        reason: String,
    },

    /// Failed to parse a message received from the exchange stream.
    #[error("failed to parse message: {reason} (raw payload: '{raw}')")]
    MessageParseFailed {
        /// Raw message payload string.
        raw: String,
        /// Parsing error details.
        reason: String,
    },

    /// Subscription to a symbol or stream failed.
    #[error("subscription to symbol '{symbol}' failed: {reason}")]
    SubscriptionFailed {
        /// Target symbol (e.g. BTCUSDT).
        symbol: String,
        /// Reason for failure.
        reason: String,
    },

    /// Channel sender or receiver was closed unexpectedly.
    #[error("internal event channel closed unexpectedly")]
    ChannelClosed,

    /// Operation timed out.
    #[error("operation timed out after {duration_ms} ms")]
    Timeout {
        /// Timeout duration in milliseconds.
        duration_ms: u64,
    },

    /// Error propagated from `omnitrade-core`.
    #[error(transparent)]
    Core(#[from] CoreError),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn connection_failed_display() {
        // Arrange
        let err = ExchangeError::ConnectionFailed {
            url: "wss://stream.binance.com".into(),
            reason: "DNS lookup failed".into(),
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(
            formatted,
            "connection failed to 'wss://stream.binance.com': DNS lookup failed"
        );
    }

    #[test]
    fn message_parse_failed_display() {
        // Arrange
        let err = ExchangeError::MessageParseFailed {
            raw: "{bad json}".into(),
            reason: "expected key at line 1".into(),
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(
            formatted,
            "failed to parse message: expected key at line 1 (raw payload: '{bad json}')"
        );
    }

    #[test]
    fn subscription_failed_display() {
        // Arrange
        let err = ExchangeError::SubscriptionFailed {
            symbol: "INVALID".into(),
            reason: "symbol not found".into(),
        };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(
            formatted,
            "subscription to symbol 'INVALID' failed: symbol not found"
        );
    }

    #[test]
    fn channel_closed_display() {
        // Arrange
        let err = ExchangeError::ChannelClosed;

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "internal event channel closed unexpectedly");
    }

    #[test]
    fn timeout_display() {
        // Arrange
        let err = ExchangeError::Timeout { duration_ms: 5000 };

        // Act
        let formatted = format!("{err}");

        // Assert
        assert_eq!(formatted, "operation timed out after 5000 ms");
    }

    #[test]
    fn core_error_transparent_propagation() {
        // Arrange
        let core_err = CoreError::InvalidPeriod(0);

        // Act
        let exch_err: ExchangeError = core_err.into();

        // Assert
        assert_eq!(
            format!("{exch_err}"),
            "invalid period: 0 (must be greater than zero)"
        );
    }
}
