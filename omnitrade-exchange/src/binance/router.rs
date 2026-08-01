//! Binance message router for deserializing and routing WebSocket payloads.

use std::collections::HashMap;

use omnitrade_core::Candle;
use tokio::sync::mpsc;
use tracing::warn;

use crate::binance::dto::{BinanceDepthEvent, BinanceKlineEvent};
use crate::{DepthUpdate, ExchangeError};

const CHANNEL_CAPACITY: usize = 100;

/// Message router for Binance WebSocket streams.
pub struct BinanceRouter {
    raw_rx: mpsc::Receiver<String>,
    candle_senders: HashMap<String, mpsc::Sender<Candle>>,
    depth_senders: HashMap<String, mpsc::Sender<DepthUpdate>>,
}

impl BinanceRouter {
    /// Creates a new `BinanceRouter` consuming raw JSON messages from `raw_rx`.
    pub fn new(raw_rx: mpsc::Receiver<String>) -> Self {
        Self {
            raw_rx,
            candle_senders: HashMap::new(),
            depth_senders: HashMap::new(),
        }
    }

    /// Registers a candle channel for a symbol and returns its receiver.
    pub fn register_candle_channel(&mut self, symbol: &str) -> mpsc::Receiver<Candle> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        self.candle_senders.insert(symbol.to_uppercase(), tx);
        rx
    }

    /// Registers a depth update channel for a symbol and returns its receiver.
    pub fn register_depth_channel(&mut self, symbol: &str) -> mpsc::Receiver<DepthUpdate> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        self.depth_senders.insert(symbol.to_uppercase(), tx);
        rx
    }

    /// Processes a single raw JSON message string.
    pub fn process_raw_message(&mut self, raw: &str) -> Result<(), ExchangeError> {
        let value: serde_json::Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(err) => {
                warn!(error = %err, raw = raw, "Received malformed JSON string");
                return Ok(());
            }
        };

        let event_obj = match value.get("data").filter(|d| d.is_object()) {
            Some(d) => d,
            None => &value,
        };

        let event_type = match event_obj.get("e").and_then(|e| e.as_str()) {
            Some(e) => e,
            None => {
                warn!(raw = raw, "Unrecognized or missing event type 'e'");
                return Ok(());
            }
        };

        match event_type {
            "kline" => {
                let kline_evt: BinanceKlineEvent = match serde_json::from_value(event_obj.clone()) {
                    Ok(evt) => evt,
                    Err(err) => {
                        warn!(error = %err, raw = raw, "Failed to deserialize BinanceKlineEvent");
                        return Ok(());
                    }
                };
                let symbol = kline_evt.s.to_uppercase();
                let candle = Candle::try_from(kline_evt)?;
                if let Some(sender) = self.candle_senders.get(&symbol) {
                    if let Err(err) = sender.try_send(candle) {
                        warn!(symbol = %symbol, error = %err, "Failed to route candle message");
                    }
                }
            }
            "depthUpdate" => {
                let depth_evt: BinanceDepthEvent = match serde_json::from_value(event_obj.clone()) {
                    Ok(evt) => evt,
                    Err(err) => {
                        warn!(error = %err, raw = raw, "Failed to deserialize BinanceDepthEvent");
                        return Ok(());
                    }
                };
                let symbol = depth_evt.s.to_uppercase();
                let depth = DepthUpdate::try_from(depth_evt)?;
                if let Some(sender) = self.depth_senders.get(&symbol) {
                    if let Err(err) = sender.try_send(depth) {
                        warn!(symbol = %symbol, error = %err, "Failed to route depth message");
                    }
                }
            }
            unknown => {
                warn!(event_type = unknown, "Received unknown event type");
            }
        }

        Ok(())
    }

    /// Runs the message routing loop until raw_rx channel is closed.
    pub async fn run(&mut self) -> Result<(), ExchangeError> {
        while let Some(raw) = self.raw_rx.recv().await {
            if let Err(err) = self.process_raw_message(&raw) {
                warn!(error = %err, "Failed to process raw message");
            }
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const KLINE_JSON: &str = r#"{
        "e": "kline", "E": 123456789, "s": "BTCUSDT",
        "k": {
            "t": 123400000, "T": 123459999, "s": "BTCUSDT", "i": "1m",
            "f": 100, "L": 200, "o": "50000.0", "c": "50100.0",
            "h": "50200.0", "l": "49900.0", "v": "10.5", "n": 100,
            "x": true, "q": "525000.0", "V": "5.0", "Q": "250000.0", "B": "0"
        }
    }"#;

    const WRAPPED_DEPTH_JSON: &str = r#"{
        "stream": "btcusdt@depth@100ms",
        "data": {
            "e": "depthUpdate", "E": 123456789, "s": "BTCUSDT",
            "b": [["50000.0", "1.0"]], "a": [["50100.0", "2.0"]]
        }
    }"#;

    #[test]
    fn channel_registration_and_kline_routing() {
        // Arrange
        let (_tx, rx) = mpsc::channel(10);
        let mut router = BinanceRouter::new(rx);
        let mut candle_rx = router.register_candle_channel("btcusdt");

        // Act
        let result = router.process_raw_message(KLINE_JSON);

        // Assert
        assert!(result.is_ok());
        let candle = candle_rx
            .try_recv()
            .expect("should receive candle on registered channel");
        assert_eq!(candle.timestamp_ms, 123400000);
        assert_eq!(candle.close, 50100.0);
    }

    #[test]
    fn wrapped_depth_routing() {
        // Arrange
        let (_tx, rx) = mpsc::channel(10);
        let mut router = BinanceRouter::new(rx);
        let mut depth_rx = router.register_depth_channel("BTCUSDT");

        // Act
        let result = router.process_raw_message(WRAPPED_DEPTH_JSON);

        // Assert
        assert!(result.is_ok());
        let depth = depth_rx
            .try_recv()
            .expect("should receive depth update on registered channel");
        assert_eq!(depth.timestamp_ms, 123456789);
        assert_eq!(depth.bids.len(), 1);
        assert_eq!(depth.asks.len(), 1);
    }

    #[test]
    fn unknown_event_and_malformed_json_skipped() {
        // Arrange
        let (_tx, rx) = mpsc::channel(10);
        let mut router = BinanceRouter::new(rx);
        let unknown_json = r#"{"e": "aggTrade", "s": "BTCUSDT"}"#;
        let malformed_json = r#"{"e": "kline", invalid"#;

        // Act
        let res_unknown = router.process_raw_message(unknown_json);
        let res_malformed = router.process_raw_message(malformed_json);

        // Assert
        assert!(res_unknown.is_ok());
        assert!(res_malformed.is_ok());
    }

    #[tokio::test]
    async fn run_loop_processes_messages_until_closed() {
        // Arrange
        let (tx, rx) = mpsc::channel(10);
        let mut router = BinanceRouter::new(rx);
        let mut candle_rx = router.register_candle_channel("BTCUSDT");

        // Act
        tx.send(KLINE_JSON.to_string())
            .await
            .expect("failed to send raw message");
        drop(tx);

        let run_res = router.run().await;

        // Assert
        assert!(run_res.is_ok());
        let candle = candle_rx
            .try_recv()
            .expect("should have received candle before router shutdown");
        assert_eq!(candle.timestamp_ms, 123400000);
    }
}
