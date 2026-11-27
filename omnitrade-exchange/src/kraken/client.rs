//! Kraken WebSocket v2 streaming client (public market data only).

use async_trait::async_trait;
use futures_util::{SinkExt, StreamExt};
use omnitrade_core::Candle;
use serde_json::json;
use tokio::sync::{mpsc, oneshot};
use tokio::task::JoinHandle;
use tokio_tungstenite::tungstenite::Message;
use tracing::{info, warn};

use crate::error::ExchangeError;
use crate::kraken::router::KrakenRouter;
use crate::traits::{DepthUpdate, ExchangeInfo, ExchangeStream};

const RAW_CHANNEL_CAPACITY: usize = 100;
const BOOK_DEPTH: u32 = 25;

/// Kraken WebSocket v2 endpoint for public market data.
pub const WS_URL: &str = "wss://ws.kraken.com/v2";

/// Maps an interval string (`1m`, `1h`, ...) to Kraken's minute count.
pub(crate) fn interval_minutes(interval: &str) -> Option<u32> {
    match interval {
        "1m" => Some(1),
        "5m" => Some(5),
        "15m" => Some(15),
        "30m" => Some(30),
        "1h" => Some(60),
        "4h" => Some(240),
        "1d" => Some(1440),
        "1w" => Some(10080),
        _ => None,
    }
}

/// Kraken symbols are `BASE/QUOTE` pairs (e.g. `BTC/USD`).
fn validate_symbol(symbol: &str) -> Result<String, ExchangeError> {
    let upper = symbol.to_uppercase();
    match upper.split_once('/') {
        Some((base, quote)) if !base.is_empty() && !quote.is_empty() && !quote.contains('/') => {
            Ok(upper)
        }
        _ => Err(ExchangeError::SubscriptionFailed {
            symbol: symbol.to_string(),
            reason: "Kraken-Symbole haben die Form BASE/QUOTE, z. B. BTC/USD".to_string(),
        }),
    }
}

/// Streaming client for Kraken Spot market data.
pub struct KrakenClient {
    url: String,
    subscriptions: Vec<String>,
    router: Option<KrakenRouter>,
    raw_tx: mpsc::Sender<String>,
    router_task: Option<JoinHandle<Result<(), ExchangeError>>>,
    shutdown_tx: Option<oneshot::Sender<()>>,
    is_connected: bool,
}

impl KrakenClient {
    /// Creates a client for the production WebSocket endpoint.
    pub fn new() -> Self {
        Self::with_url(WS_URL)
    }

    /// Creates a client for a custom endpoint (tests).
    pub fn with_url(url: impl Into<String>) -> Self {
        let (raw_tx, raw_rx) = mpsc::channel(RAW_CHANNEL_CAPACITY);
        Self {
            url: url.into(),
            subscriptions: Vec::new(),
            router: Some(KrakenRouter::new(raw_rx)),
            raw_tx,
            router_task: None,
            shutdown_tx: None,
            is_connected: false,
        }
    }

    /// Subscribe messages that are sent right after connecting.
    pub fn subscriptions(&self) -> &[String] {
        &self.subscriptions
    }
}

impl Default for KrakenClient {
    fn default() -> Self {
        Self::new()
    }
}

#[async_trait]
impl ExchangeStream for KrakenClient {
    async fn connect(&mut self) -> Result<(), ExchangeError> {
        if self.is_connected || self.router.is_none() {
            return Err(ExchangeError::ConnectionFailed {
                url: self.url.clone(),
                reason: "client is already connected".to_string(),
            });
        }

        let (ws, _) = tokio_tungstenite::connect_async(&self.url)
            .await
            .map_err(|e| ExchangeError::ConnectionFailed {
                url: self.url.clone(),
                reason: e.to_string(),
            })?;
        info!("Connected to Kraken WebSocket at '{}'", self.url);
        let (mut write, mut read) = ws.split();

        for sub in &self.subscriptions {
            write
                .send(Message::Text(sub.clone().into()))
                .await
                .map_err(|e| ExchangeError::ConnectionFailed {
                    url: self.url.clone(),
                    reason: format!("subscribe failed: {e}"),
                })?;
        }

        let (shutdown_tx, mut shutdown_rx) = oneshot::channel();
        self.shutdown_tx = Some(shutdown_tx);
        let raw_tx = self.raw_tx.clone();
        tokio::spawn(async move {
            loop {
                tokio::select! {
                    _ = &mut shutdown_rx => {
                        let _ = write.close().await;
                        break;
                    }
                    msg = read.next() => match msg {
                        Some(Ok(Message::Text(text))) => {
                            if raw_tx.send(text.to_string()).await.is_err() {
                                break;
                            }
                        }
                        Some(Ok(Message::Ping(payload))) => {
                            let _ = write.send(Message::Pong(payload)).await;
                        }
                        Some(Ok(Message::Close(_))) | None => {
                            info!("Kraken WebSocket closed");
                            break;
                        }
                        Some(Ok(_)) => {}
                        Some(Err(err)) => {
                            warn!("Kraken WebSocket read error: {err}");
                            break;
                        }
                    }
                }
            }
        });

        if let Some(mut router) = self.router.take() {
            self.router_task = Some(tokio::spawn(async move { router.run().await }));
        }
        self.is_connected = true;
        Ok(())
    }

    /// Must be called *before* [`connect()`](Self::connect). The stream only emits
    /// **closed** candles, like the Binance client.
    async fn subscribe_candles(
        &mut self,
        symbol: &str,
        interval: &str,
    ) -> Result<mpsc::Receiver<Candle>, ExchangeError> {
        let symbol = validate_symbol(symbol)?;
        let minutes =
            interval_minutes(interval).ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.clone(),
                reason: format!("Intervall '{interval}' wird von Kraken nicht unterstützt"),
            })?;
        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.clone(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;
        let rx = router.register_candle_channel(&symbol);
        self.subscriptions.push(
            json!({"method": "subscribe", "params": {"channel": "ohlc", "symbol": [symbol], "interval": minutes}})
                .to_string(),
        );
        Ok(rx)
    }

    /// Must be called *before* [`connect()`](Self::connect). The first message per symbol
    /// is a snapshot, later ones are deltas (quantity 0 removes a level).
    async fn subscribe_depth(
        &mut self,
        symbol: &str,
    ) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError> {
        let symbol = validate_symbol(symbol)?;
        let router = self
            .router
            .as_mut()
            .ok_or_else(|| ExchangeError::SubscriptionFailed {
                symbol: symbol.clone(),
                reason: "cannot subscribe after connection has been established".to_string(),
            })?;
        let rx = router.register_depth_channel(&symbol);
        self.subscriptions.push(
            json!({"method": "subscribe", "params": {"channel": "book", "symbol": [symbol], "depth": BOOK_DEPTH}})
                .to_string(),
        );
        Ok(rx)
    }

    async fn disconnect(&mut self) -> Result<(), ExchangeError> {
        if let Some(tx) = self.shutdown_tx.take() {
            let _ = tx.send(());
        }
        self.is_connected = false;
        if let Some(task) = self.router_task.take() {
            let _ = tokio::time::timeout(std::time::Duration::from_millis(500), task).await;
        }
        Ok(())
    }
}

impl ExchangeInfo for KrakenClient {
    fn name(&self) -> &str {
        "Kraken"
    }

    fn supported_intervals(&self) -> &[&str] {
        &["1m", "5m", "15m", "30m", "1h", "4h", "1d", "1w"]
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn subscribe_builds_v2_messages() {
        // Arrange
        let mut client = KrakenClient::new();

        // Act
        client
            .subscribe_candles("btc/usd", "1h")
            .await
            .expect("candles");
        client.subscribe_depth("ETH/USD").await.expect("depth");

        // Assert
        let subs = client.subscriptions();
        assert_eq!(subs.len(), 2);
        let first: serde_json::Value = serde_json::from_str(&subs[0]).expect("json");
        assert_eq!(first["method"], "subscribe");
        assert_eq!(first["params"]["channel"], "ohlc");
        assert_eq!(first["params"]["symbol"][0], "BTC/USD");
        assert_eq!(first["params"]["interval"], 60);
        let second: serde_json::Value = serde_json::from_str(&subs[1]).expect("json");
        assert_eq!(second["params"]["channel"], "book");
        assert_eq!(second["params"]["depth"], BOOK_DEPTH);
    }

    #[tokio::test]
    async fn rejects_bad_symbol_and_interval() {
        // Arrange
        let mut client = KrakenClient::new();

        // Act & Assert
        assert!(client.subscribe_candles("BTCUSD", "1m").await.is_err());
        assert!(client.subscribe_depth("/USD").await.is_err());
        assert!(client.subscribe_candles("BTC/USD", "7m").await.is_err());
        assert!(client.subscriptions().is_empty());
    }

    #[tokio::test]
    async fn double_connect_is_rejected_and_exchange_info_is_correct() {
        // Arrange
        let mut client = KrakenClient::with_url("ws://127.0.0.1:1");
        client.router.take();

        // Act
        let res = client.connect().await;

        // Assert
        assert!(res.is_err());
        assert_eq!(client.name(), "Kraken");
        assert!(client.supported_intervals().contains(&"1m"));
    }
}
