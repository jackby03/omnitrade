//! Core traits and abstractions for exchange communications.

use async_trait::async_trait;
use omnitrade_core::Candle;
use tokio::sync::mpsc;

use crate::ExchangeError;

/// Level 2 order book depth snapshot update.
#[derive(Debug, Clone, PartialEq)]
pub struct DepthUpdate {
    /// Update timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Vector of bid price levels: `(price, quantity)`.
    pub bids: Vec<(f64, f64)>,
    /// Vector of ask price levels: `(price, quantity)`.
    pub asks: Vec<(f64, f64)>,
}

impl DepthUpdate {
    /// Creates a new `DepthUpdate` instance.
    pub fn new(timestamp_ms: u64, bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)>) -> Self {
        Self {
            timestamp_ms,
            bids,
            asks,
        }
    }
}

/// Asynchronous stream client for receiving real-time market data from an exchange.
#[async_trait]
pub trait ExchangeStream: Send + Sync {
    /// Connects to the exchange WebSocket feed.
    async fn connect(&mut self) -> Result<(), ExchangeError>;

    /// Subscribes to real-time candlestick updates for a symbol and interval.
    async fn subscribe_candles(
        &mut self,
        symbol: &str,
        interval: &str,
    ) -> Result<mpsc::Receiver<Candle>, ExchangeError>;

    /// Subscribes to real-time order book depth updates for a symbol.
    async fn subscribe_depth(
        &mut self,
        symbol: &str,
    ) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError>;

    /// Gracefully disconnects from the exchange feed.
    async fn disconnect(&mut self) -> Result<(), ExchangeError>;
}

/// Synchronous metadata and feature capabilities provider for an exchange connector.
pub trait ExchangeInfo: Send + Sync {
    /// Returns the human-readable exchange name (e.g. "Binance", "Bybit").
    fn name(&self) -> &str;

    /// Returns a slice of supported candle interval strings (e.g. `["1m", "5m", "1h"]`).
    fn supported_intervals(&self) -> &[&str];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn depth_update_construction_and_equality() {
        // Arrange
        let timestamp_ms = 1_700_000_000_000;
        let bids = vec![(50000.0, 1.5), (49990.0, 2.0)];
        let asks = vec![(50010.0, 0.8), (50020.0, 3.1)];

        // Act
        let depth = DepthUpdate::new(timestamp_ms, bids.clone(), asks.clone());

        // Assert
        assert_eq!(depth.timestamp_ms, timestamp_ms);
        assert_eq!(depth.bids, bids);
        assert_eq!(depth.asks, asks);
    }
}
