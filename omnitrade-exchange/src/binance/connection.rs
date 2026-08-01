//! Binance WebSocket connection manager with automatic reconnection support.

use crate::error::ExchangeError;
use futures_util::StreamExt;
use std::time::Duration;
use tokio::sync::{mpsc, oneshot};
use tracing::{info, warn};

/// Connection states for the Binance WebSocket client.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ConnectionState {
    /// Connection is closed or idle.
    Disconnected,
    /// Establishing connection for the first time.
    Connecting,
    /// Connection established and active.
    Connected,
    /// Re-establishing connection following a failure.
    Reconnecting,
}

/// WebSocket connection manager for Binance market data streams.
#[derive(Debug)]
pub struct BinanceConnection {
    base_url: String,
    streams: Vec<String>,
    raw_tx: mpsc::Sender<String>,
    state: ConnectionState,
    shutdown_tx: Option<oneshot::Sender<()>>,
}

impl BinanceConnection {
    /// Default Binance WebSocket stream endpoint base URL.
    pub const DEFAULT_BASE_URL: &'static str = "wss://stream.binance.com:9443/ws";

    /// Creates a new `BinanceConnection` with default base URL.
    pub fn new(raw_tx: mpsc::Sender<String>) -> Self {
        Self {
            base_url: Self::DEFAULT_BASE_URL.to_string(),
            streams: Vec::new(),
            raw_tx,
            state: ConnectionState::Disconnected,
            shutdown_tx: None,
        }
    }

    /// Creates a new `BinanceConnection` with a custom base URL.
    pub fn with_base_url(base_url: impl Into<String>, raw_tx: mpsc::Sender<String>) -> Self {
        Self {
            base_url: base_url.into(),
            streams: Vec::new(),
            raw_tx,
            state: ConnectionState::Disconnected,
            shutdown_tx: None,
        }
    }

    /// Adds a stream subscription topic (e.g., "btcusdt@kline_1m").
    pub fn add_stream(&mut self, stream: impl Into<String>) {
        self.streams.push(stream.into());
    }

    /// Returns the current state of the connection.
    pub fn state(&self) -> ConnectionState {
        self.state
    }

    /// Returns the list of subscribed stream topics.
    pub fn streams(&self) -> &[String] {
        &self.streams
    }

    /// Returns the base URL for the WebSocket endpoint.
    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    /// Builds the combined WebSocket URL.
    ///
    /// Formats as `{base_url}/{stream1}/{stream2}/...`
    pub fn build_url(&self) -> String {
        let base = self.base_url.trim_end_matches('/');
        if self.streams.is_empty() {
            base.to_string()
        } else {
            format!("{base}/{}", self.streams.join("/"))
        }
    }

    /// Connects to the Binance WebSocket server.
    pub async fn connect(&mut self) -> Result<(), ExchangeError> {
        self.state = ConnectionState::Connecting;
        let url = self.build_url();

        info!("Connecting to Binance WebSocket endpoint at '{url}'");

        let (ws_stream, _) = match tokio_tungstenite::connect_async(&url).await {
            Ok(res) => res,
            Err(err) => {
                warn!("Failed to connect to '{url}': {err}");
                self.state = ConnectionState::Disconnected;
                return Err(ExchangeError::ConnectionFailed {
                    url,
                    reason: err.to_string(),
                });
            }
        };

        info!("Successfully connected to Binance WebSocket endpoint at '{url}'");
        self.state = ConnectionState::Connected;

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        let raw_tx = self.raw_tx.clone();

        tokio::spawn(async move {
            let (_write, mut read) = ws_stream.split();
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        info!("Shutdown signal received; closing WebSocket receiver task");
                        break;
                    }
                    msg_res = read.next() => {
                        match msg_res {
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Text(text))) => {
                                if raw_tx.send(text.to_string()).await.is_err() {
                                    warn!("Channel receiver dropped; stopping WebSocket reader");
                                    break;
                                }
                            }
                            Some(Ok(tokio_tungstenite::tungstenite::Message::Close(_))) => {
                                info!("WebSocket closed by remote peer");
                                break;
                            }
                            Some(Ok(_)) => {}
                            Some(Err(err)) => {
                                warn!("WebSocket read error: {err}");
                                break;
                            }
                            None => {
                                info!("WebSocket stream ended");
                                break;
                            }
                        }
                    }
                }
            }
        });

        Ok(())
    }

    /// Connects with exponential backoff retries.
    ///
    /// On failure, waits 1s, 2s, 4s, 8s, 16s, capped at 30s per attempt.
    pub async fn connect_with_retry(&mut self, max_attempts: u32) -> Result<(), ExchangeError> {
        let mut attempt = 0;
        loop {
            if attempt > 0 {
                self.state = ConnectionState::Reconnecting;
                let backoff = calculate_backoff(attempt - 1);
                warn!(
                    "Reconnection attempt {}/{} failed previously. Retrying in {}s...",
                    attempt,
                    max_attempts,
                    backoff.as_secs()
                );
                tokio::time::sleep(backoff).await;
            }

            match self.connect().await {
                Ok(()) => return Ok(()),
                Err(err) => {
                    attempt += 1;
                    if attempt >= max_attempts {
                        warn!("Maximum reconnection attempts ({max_attempts}) reached. Giving up.");
                        self.state = ConnectionState::Disconnected;
                        return Err(err);
                    }
                }
            }
        }
    }

    /// Gracefully disconnects the WebSocket stream.
    pub async fn disconnect(&mut self) -> Result<(), ExchangeError> {
        if let Some(shutdown_tx) = self.shutdown_tx.take() {
            let _ = shutdown_tx.send(());
        }
        self.state = ConnectionState::Disconnected;
        info!("Disconnected Binance WebSocket connection");
        Ok(())
    }
}

/// Calculates exponential backoff duration for reconnection attempts.
///
/// Backoff sequence: 1s, 2s, 4s, 8s, 16s, capped at 30s.
pub fn calculate_backoff(attempt: u32) -> Duration {
    let secs = 1u64.checked_shl(attempt).unwrap_or(u64::MAX);
    let capped = secs.min(30);
    Duration::from_secs(capped)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_initialization() {
        // Arrange
        let (tx, _rx) = mpsc::channel(32);

        // Act
        let conn = BinanceConnection::new(tx);

        // Assert
        assert_eq!(conn.state(), ConnectionState::Disconnected);
        assert_eq!(conn.base_url(), BinanceConnection::DEFAULT_BASE_URL);
        assert!(conn.streams().is_empty());
    }

    #[test]
    fn test_add_stream_and_build_url() {
        // Arrange
        let (tx, _rx) = mpsc::channel(32);
        let mut conn = BinanceConnection::new(tx);

        // Act
        conn.add_stream("btcusdt@kline_1m");
        conn.add_stream("btcusdt@depth@100ms");
        let url = conn.build_url();

        // Assert
        assert_eq!(conn.streams().len(), 2);
        assert_eq!(
            url,
            "wss://stream.binance.com:9443/ws/btcusdt@kline_1m/btcusdt@depth@100ms"
        );
    }

    #[test]
    fn test_build_url_no_streams() {
        // Arrange
        let (tx, _rx) = mpsc::channel(32);
        let conn = BinanceConnection::with_base_url("wss://stream.binance.com:9443/ws/", tx);

        // Act
        let url = conn.build_url();

        // Assert
        assert_eq!(url, "wss://stream.binance.com:9443/ws");
    }

    #[test]
    fn test_backoff_sequence() {
        // Arrange & Act & Assert
        let expected = [1, 2, 4, 8, 16, 30, 30, 30];
        for (i, &exp) in expected.iter().enumerate() {
            let duration = calculate_backoff(i as u32);
            assert_eq!(duration.as_secs(), exp, "backoff for attempt {} failed", i);
        }

        // Test large attempt overflow protection
        assert_eq!(calculate_backoff(100).as_secs(), 30);
    }

    #[test]
    fn test_connection_state_enum() {
        // Arrange
        let state = ConnectionState::Connecting;

        // Act & Assert
        assert_eq!(state, ConnectionState::Connecting);
        assert_ne!(state, ConnectionState::Connected);
    }

    #[tokio::test]
    async fn test_disconnect() {
        // Arrange
        let (tx, _rx) = mpsc::channel(32);
        let mut conn = BinanceConnection::new(tx);
        conn.state = ConnectionState::Connected;

        // Act
        let result = conn.disconnect().await;

        // Assert
        assert!(result.is_ok());
        assert_eq!(conn.state(), ConnectionState::Disconnected);
    }
}
