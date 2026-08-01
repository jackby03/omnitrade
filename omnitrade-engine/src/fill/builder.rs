//! Fluent builder implementation for [`OrderFill`].

use omnitrade_core::Side;

use crate::error::EngineError;
use crate::fill::OrderFill;

/// Builder for constructing [`OrderFill`] instances with field validation.
#[derive(Debug, Clone)]
pub struct OrderFillBuilder {
    order_id: u64,
    symbol: String,
    side: Side,
    fill_price: f64,
    fill_quantity: f64,
    fee: f64,
    fee_asset: String,
    timestamp_ms: u64,
    is_maker: bool,
}

impl OrderFillBuilder {
    /// Creates a new `OrderFillBuilder` with default initial values.
    pub fn new() -> Self {
        Self {
            order_id: 0,
            symbol: String::new(),
            side: Side::Buy,
            fill_price: 0.0,
            fill_quantity: 0.0,
            fee: 0.0,
            fee_asset: String::new(),
            timestamp_ms: 0,
            is_maker: false,
        }
    }

    /// Sets the order ID.
    pub fn order_id(mut self, id: u64) -> Self {
        self.order_id = id;
        self
    }

    /// Sets the symbol.
    pub fn symbol(mut self, symbol: impl Into<String>) -> Self {
        self.symbol = symbol.into();
        self
    }

    /// Sets the order side.
    pub fn side(mut self, side: Side) -> Self {
        self.side = side;
        self
    }

    /// Sets the execution fill price.
    pub fn fill_price(mut self, price: f64) -> Self {
        self.fill_price = price;
        self
    }

    /// Sets the execution fill quantity.
    pub fn fill_quantity(mut self, quantity: f64) -> Self {
        self.fill_quantity = quantity;
        self
    }

    /// Sets the fee amount and asset symbol.
    pub fn fee(mut self, fee: f64, fee_asset: impl Into<String>) -> Self {
        self.fee = fee;
        self.fee_asset = fee_asset.into();
        self
    }

    /// Sets the execution timestamp in milliseconds.
    pub fn timestamp_ms(mut self, timestamp_ms: u64) -> Self {
        self.timestamp_ms = timestamp_ms;
        self
    }

    /// Sets whether the fill was a maker trade.
    pub fn is_maker(mut self, is_maker: bool) -> Self {
        self.is_maker = is_maker;
        self
    }

    /// Validates and builds the [`OrderFill`].
    ///
    /// # Errors
    /// Returns [`EngineError::InvalidFieldValue`] if field values fail validation.
    pub fn build(self) -> Result<OrderFill, EngineError> {
        OrderFill::try_new(
            self.order_id,
            self.symbol,
            self.side,
            self.fill_price,
            self.fill_quantity,
            self.fee,
            self.fee_asset,
            self.timestamp_ms,
            self.is_maker,
        )
    }
}

impl Default for OrderFillBuilder {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_fill_builder_success() {
        // Arrange & Act
        let fill = OrderFillBuilder::new()
            .order_id(200)
            .symbol("SOLUSDT")
            .side(Side::Buy)
            .fill_price(150.0)
            .fill_quantity(10.0)
            .fee(1.5, "USDT")
            .timestamp_ms(1_700_000_000)
            .is_maker(true)
            .build();

        // Assert
        assert!(fill.is_ok());
        let fill = fill.unwrap();
        assert_eq!(fill.order_id, 200);
        assert_eq!(fill.symbol, "SOLUSDT");
        assert_eq!(fill.notional(), 1500.0);
        assert_eq!(fill.net_notional(), 1498.5);
    }
}
