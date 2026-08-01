//! Binance WebSocket streaming client implementation.

use async_trait::async_trait;
use omnitrade_core::Candle;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;

use crate::binance::connection::BinanceConnection;
use crate::binance::router::BinanceRouter;
use crate::error::ExchangeError;
use crate::traits::{DepthUpdate, ExchangeInfo, ExchangeStream};

const RAW_CHANNEL_CAPACITY: usize = 100;

/// Unified Binance client for streaming market data via WebSocket connections.
pub struct BinanceClient {
    connection: BinanceConnection,
    router: Option<BinanceRouter>,
    router_task: Option<JoinHandle<Result<(), ExchangeError>>>,
    is_connected: bool,
}

impl BinanceClient {
    /// Creates a new `BinanceClient` with default connection parameters.
    pub fn new() -> Self {
        let (raw_tx, raw_rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);
        let connection = BinanceConnection::new(raw_tx);
        let router = BinanceRouter::new(raw_rx);

        Self {
            connection,
            router: Some(router),
            router_task: None,
            is_connected: false,
        }
    }

    /// Creates a new `BinanceClient` with a custom base URL endpoint.
    pub fn with_base_url(base_url: impl Into<String>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);
        let connection = BinanceConnection::with_base_url(base_url, raw_tx);
        let router = BinanceRouter::new(raw_rx);

        Self {
            connection,
            router: Some(router),
            router_task: None,
            is_connected: false,
        }
    }

    /// Returns a reference to the internal `BinanceConnection`.
    pub fn connection(&self) -> &BinanceConnection {
        &self.connection
    }
}

impl Default for BinanceClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExchangeStream for BinanceClient {
    async fn connect(&mut self) -> Result<(), ExchangeError> {
        if self.is_connected || self.router.is_none() {
            return Err(ExchangeError::ConnectionFailed {
                url: self.connection.streams().join(","),
                reason: "client is already connected".to_string(),
            });
        }

        self.connection.connect().await?;

        if let Some(mut router) = self.router.take() {
            let handle = tokio::spawn(async move { router.run().await });
            self.router_task = Some(handle);
        }

        self.is_connected = true;
        Ok(())
    }

    /// Subscribes to real-time candlestick updates for a symbol and interval.
    ///
    /// **Note**: Must be called *before* calling [`connect()`](Self::connect).
    /// Attempting to subscribe after connecting returns [`ExchangeError::SubscriptionFailed`].
    async fn subscribe_candles(
        &mut self,
        symbol: &str,
        interval: &str,
    ) -> Result<mpsc::Receiver<Candle>, ExchangeError> {
        let stream_topic = format!("{}@kline_{}", symbol.to_lowercase(), interval);
        self.connection.add_stream(stream_topic);

        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.to_string(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;

        Ok(router.register_candle_channel(symbol))
    }

    /// Subscribes to real-time order book depth updates for a symbol.
    ///
    /// **Note**: Must be called *before* calling [`connect()`](Self::connect).
    /// Attempting to subscribe after connecting returns [`ExchangeError::SubscriptionFailed`].
    async fn subscribe_depth(
        &mut self,
        symbol: &str,
    ) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError> {
        let stream_topic = format!("{}@depth@100ms", symbol.to_lowercase());
        self.connection.add_stream(stream_topic);

        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.to_string(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;

        Ok(router.register_depth_channel(symbol))
    }

    async fn disconnect(&mut self) -> Result<(), ExchangeError> {
        self.connection.disconnect().await?;
        self.is_connected = false;

        if let Some(task) = self.router_task.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), task).await;
        }

        Ok(())
    }
}

impl ExchangeInfo for BinanceClient {
    fn name(&self) -> &str {
        "Binance"
    }

    fn supported_intervals(&self) -> &[&str] {
        &["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_exchange_info_methods() {
        // Arrange
        let client = BinanceClient::new();

        // Act
        let name = client.name();
        let intervals = client.supported_intervals();

        // Assert
        assert_eq!(name, "Binance");
        assert_eq!(
            intervals,
            &["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d"]
        );
    }

    #[test]
    fn test_exchange_stream_object_safety() {
        // Arrange
        let client = BinanceClient::new();

        // Act
        let boxed: Box<dyn ExchangeStream> = Box::new(client);

        // Assert
        drop(boxed);
    }

    #[tokio::test]
    async fn test_stream_registration() {
        // Arrange
        let mut client = BinanceClient::new();

        // Act
        let candle_res = client.subscribe_candles("BTCUSDT", "1m").await;
        let depth_res = client.subscribe_depth("ethusdt").await;

        // Assert
        assert!(candle_res.is_ok(), "subscribe_candles should succeed");
        assert!(depth_res.is_ok(), "subscribe_depth should succeed");

        let streams = client.connection().streams();
        assert_eq!(streams.len(), 2);
        assert_eq!(streams[0], "btcusdt@kline_1m");
        assert_eq!(streams[1], "ethusdt@depth@100ms");
    }

    #[tokio::test]
    async fn test_subscribe_after_connect_fails() {
        // Arrange
        let mut client = BinanceClient::with_base_url("wss://invalid.host");
        client.router.take();

        // Act
        let res = client.subscribe_candles("BTCUSDT", "1m").await;

        // Assert
        assert!(res.is_err());
        if let Err(ExchangeError::SubscriptionFailed { symbol, reason }) = res {
            assert_eq!(symbol, "BTCUSDT");
            assert!(reason.contains("cannot subscribe"));
        } else {
            panic!("Expected SubscriptionFailed error variant");
        }
    }

    #[tokio::test]
    async fn test_disconnect_cleans_up_task() {
        // Arrange
        let mut client = BinanceClient::new();

        // Act
        let disconnect_res = client.disconnect().await;

        // Assert
        assert!(disconnect_res.is_ok());
    }

    #[tokio::test]
    async fn test_reconnect_returns_error() {
        // Arrange
        let mut client = BinanceClient::new();
        client.is_connected = true;

        // Act
        let res = client.connect().await;

        // Assert
        assert!(res.is_err());
        if let Err(ExchangeError::ConnectionFailed { reason, .. }) = res {
            assert!(reason.contains("already connected"));
        } else {
            panic!("Expected ConnectionFailed error");
        }
    }

    #[tokio::test]
    async fn test_mock_end_to_end_flow() {
        // Arrange
        let (raw_tx, raw_rx) = mpsc::channel(10);
        let mut router = BinanceRouter::new(raw_rx);
        let mut candle_rx = router.register_candle_channel("BTCUSDT");

        let sample_kline_json = r#"{
            "e": "kline",
            "E": 1700000000000,
            "s": "BTCUSDT",
            "k": {
                "t": 1700000000000,
                "T": 1700000059999,
                "s": "BTCUSDT",
                "i": "1m",
                "f": 1,
                "L": 10,
                "o": "50000.0",
                "c": "50100.0",
                "h": "50200.0",
                "l": "49900.0",
                "v": "15.5",
                "n": 10,
                "x": true,
                "q": "775000.0",
                "V": "8.0",
                "Q": "400000.0",
                "B": "0"
            }
        }"#;

        // Act
        raw_tx.send(sample_kline_json.to_string()).await.unwrap();
        drop(raw_tx); // close sender so router loop terminates

        router.run().await.unwrap();
        let candle = candle_rx.recv().await.unwrap();

        // Assert
        assert_eq!(candle.timestamp_ms, 1700000000000);
        assert_eq!(candle.open, 50000.0);
        assert_eq!(candle.close, 50100.0);
    }
}
