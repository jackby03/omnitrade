//! Execution fill types representing order matches.

pub mod builder;

pub use builder::OrderFillBuilder;
use omnitrade_core::Side;

use crate::error::EngineError;

/// Represents a completed (or partial) order execution event.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderFill {
    /// Unique identifier of the filled order.
    pub order_id: u64,
    /// Trading pair symbol.
    pub symbol: String,
    /// Order side (Buy or Sell).
    pub side: Side,
    /// Actual execution price after slippage.
    pub fill_price: f64,
    /// Quantity executed in this fill.
    pub fill_quantity: f64,
    /// Fee charged for execution.
    pub fee: f64,
    /// Asset symbol used for fee payment.
    pub fee_asset: String,
    /// Timestamp of fill event in milliseconds.
    pub timestamp_ms: u64,
    /// `true` if order was liquidity maker, `false` if taker.
    pub is_maker: bool,
}

impl OrderFill {
    /// Constructs a new [`OrderFill`].
    ///
    /// **Note**: For validated construction, prefer [`try_new`](Self::try_new) or [`OrderFillBuilder`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        order_id: u64,
        symbol: String,
        side: Side,
        fill_price: f64,
        fill_quantity: f64,
        fee: f64,
        fee_asset: String,
        timestamp_ms: u64,
        is_maker: bool,
    ) -> Self {
        Self {
            order_id,
            symbol,
            side,
            fill_price,
            fill_quantity,
            fee,
            fee_asset,
            timestamp_ms,
            is_maker,
        }
    }

    /// Validates fields and constructs a new [`OrderFill`].
    ///
    /// # Errors
    /// Returns [`EngineError::InvalidFieldValue`] if `fill_price <= 0.0`,
    /// `fill_quantity <= 0.0`, `fee < 0.0`, or `timestamp_ms == 0`.
    #[allow(clippy::too_many_arguments)]
    pub fn try_new(
        order_id: u64,
        symbol: String,
        side: Side,
        fill_price: f64,
        fill_quantity: f64,
        fee: f64,
        fee_asset: String,
        timestamp_ms: u64,
        is_maker: bool,
    ) -> Result<Self, EngineError> {
        if fill_price <= 0.0 {
            return Err(EngineError::InvalidFieldValue {
                field: "fill_price",
                reason: "fill_price must be positive",
            });
        }
        if fill_quantity <= 0.0 {
            return Err(EngineError::InvalidFieldValue {
                field: "fill_quantity",
                reason: "fill_quantity must be positive",
            });
        }
        if fee < 0.0 {
            return Err(EngineError::InvalidFieldValue {
                field: "fee",
                reason: "fee cannot be negative",
            });
        }
        if timestamp_ms == 0 {
            return Err(EngineError::InvalidFieldValue {
                field: "timestamp_ms",
                reason: "timestamp_ms must be non-zero",
            });
        }

        Ok(Self {
            order_id,
            symbol,
            side,
            fill_price,
            fill_quantity,
            fee,
            fee_asset,
            timestamp_ms,
            is_maker,
        })
    }

    /// Returns gross notional value of the fill (`fill_price * fill_quantity`).
    #[must_use]
    pub fn notional(&self) -> f64 {
        self.fill_price * self.fill_quantity
    }

    /// Returns net notional value of the fill (`notional() - fee`).
    #[must_use]
    pub fn net_notional(&self) -> f64 {
        self.notional() - self.fee
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_fill_new_and_notional() {
        // Arrange
        let fill = OrderFill::new(
            101,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            2.0,
            15.0,
            "USDT".to_string(),
            1_700_000_000,
            true,
        );

        // Act
        let notional = fill.notional();

        // Assert
        assert_eq!(notional, 100_000.0);
    }

    #[test]
    fn test_order_fill_net_notional() {
        // Arrange
        let fill = OrderFill::new(
            102,
            "ETHUSDT".to_string(),
            Side::Sell,
            3_000.0,
            5.0,
            25.0,
            "USDT".to_string(),
            1_700_000_100,
            false,
        );

        // Act
        let net = fill.net_notional();

        // Assert
        assert_eq!(net, 14_975.0);
    }

    #[test]
    fn test_try_new_validation() {
        // Act & Assert: Invalid price
        let res = OrderFill::try_new(
            1,
            "BTCUSDT".into(),
            Side::Buy,
            0.0,
            1.0,
            0.0,
            "USDT".into(),
            1000,
            false,
        );
        assert_eq!(
            res,
            Err(EngineError::InvalidFieldValue {
                field: "fill_price",
                reason: "fill_price must be positive",
            })
        );

        // Act & Assert: Invalid quantity
        let res = OrderFill::try_new(
            1,
            "BTCUSDT".into(),
            Side::Buy,
            50000.0,
            -1.0,
            0.0,
            "USDT".into(),
            1000,
            false,
        );
        assert_eq!(
            res,
            Err(EngineError::InvalidFieldValue {
                field: "fill_quantity",
                reason: "fill_quantity must be positive",
            })
        );

        // Act & Assert: Invalid timestamp
        let res = OrderFill::try_new(
            1,
            "BTCUSDT".into(),
            Side::Buy,
            50000.0,
            1.0,
            0.0,
            "USDT".into(),
            0,
            false,
        );
        assert_eq!(
            res,
            Err(EngineError::InvalidFieldValue {
                field: "timestamp_ms",
                reason: "timestamp_ms must be non-zero",
            })
        );

        // Act & Assert: Valid
        let res = OrderFill::try_new(
            1,
            "BTCUSDT".into(),
            Side::Buy,
            50000.0,
            1.0,
            10.0,
            "USDT".into(),
            1000,
            true,
        );
        assert!(res.is_ok());
    }
}
