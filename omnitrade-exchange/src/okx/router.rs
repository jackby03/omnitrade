//! OKX WebSocket v5 message router: `candle*` and `books` channels.
//!
//! Candle semantics match the Binance router: only **confirmed** (closed) candles are
//! emitted, each timestamp at most once.

use std::collections::HashMap;

use omnitrade_core::Candle;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::warn;

use crate::error::ExchangeError;
use crate::traits::DepthUpdate;

const CHANNEL_CAPACITY: usize = 100;

/// Message router for OKX WebSocket streams.
pub struct OkxRouter {
    raw_rx: mpsc::Receiver<String>,
    candle_senders: HashMap<String, mpsc::Sender<Candle>>,
    depth_senders: HashMap<String, mpsc::Sender<DepthUpdate>>,
    last_emitted: HashMap<String, u64>,
}

impl OkxRouter {
    /// Creates a router consuming raw JSON messages from `raw_rx`.
    pub fn new(raw_rx: mpsc::Receiver<String>) -> Self {
        Self {
            raw_rx,
            candle_senders: HashMap::new(),
            depth_senders: HashMap::new(),
            last_emitted: HashMap::new(),
        }
    }

    /// Registers a candle channel for an instrument (e.g. `BTC-USDT`).
    pub fn register_candle_channel(&mut self, symbol: &str) -> mpsc::Receiver<Candle> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        self.candle_senders.insert(symbol.to_uppercase(), tx);
        rx
    }

    /// Registers a depth channel for an instrument (e.g. `BTC-USDT`).
    pub fn register_depth_channel(&mut self, symbol: &str) -> mpsc::Receiver<DepthUpdate> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        self.depth_senders.insert(symbol.to_uppercase(), tx);
        rx
    }

    /// Processes one raw JSON message. Unknown or malformed messages are logged, not fatal.
    pub fn process_raw_message(&mut self, raw: &str) -> Result<(), ExchangeError> {
        let value: Value = match serde_json::from_str(raw) {
            Ok(v) => v,
            Err(err) => {
                warn!(error = %err, "Malformed OKX JSON");
                return Ok(());
            }
        };
        // OKX markiert das vollständige Buch (nach Abo und nach jedem Wiederverbinden) mit
        // `action: snapshot`, Änderungen mit `update`.
        let book_snapshot = value.get("action").and_then(Value::as_str) == Some("snapshot");
        if value.get("event").and_then(Value::as_str) == Some("error") {
            warn!(message = raw, "OKX rejected a request");
            return Ok(());
        }
        let (Some(arg), Some(data)) = (
            value.get("arg"),
            value.get("data").and_then(Value::as_array),
        ) else {
            return Ok(()); // subscribe acks and the like
        };
        let (Some(channel), Some(inst)) = (
            arg.get("channel").and_then(Value::as_str),
            arg.get("instId").and_then(Value::as_str),
        ) else {
            return Ok(());
        };
        let key = inst.to_uppercase();
        if channel.starts_with("candle") {
            for row in data {
                self.handle_candle(&key, row);
            }
        } else if channel == "books" || channel == "books5" {
            for item in data {
                self.handle_book(&key, item, book_snapshot);
            }
        }
        Ok(())
    }

    fn handle_candle(&mut self, key: &str, row: &Value) {
        let Some(fields) = row.as_array() else { return };
        let num = |i: usize| fields.get(i)?.as_str()?.parse::<f64>().ok();
        let confirmed = fields.get(8).and_then(Value::as_str) == Some("1");
        let ts = fields
            .first()
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok());
        let (true, Some(ts), Some(o), Some(h), Some(l), Some(c), Some(v)) =
            (confirmed, ts, num(1), num(2), num(3), num(4), num(5))
        else {
            return; // open candle or unparseable
        };
        if self.last_emitted.get(key).is_some_and(|last| *last >= ts) {
            return;
        }
        self.last_emitted.insert(key.to_string(), ts);
        if let Some(sender) = self.candle_senders.get(key) {
            if let Err(err) = sender.try_send(Candle::new(ts, o, h, l, c, v)) {
                warn!(symbol = key, error = %err, "Failed to route candle message");
            }
        }
    }

    fn handle_book(&mut self, key: &str, item: &Value, snapshot: bool) {
        let side = |name: &str| -> Option<Vec<(f64, f64)>> {
            item.get(name)?
                .as_array()?
                .iter()
                .map(|l| {
                    Some((
                        l.get(0)?.as_str()?.parse().ok()?,
                        l.get(1)?.as_str()?.parse().ok()?,
                    ))
                })
                .collect()
        };
        let (Some(bids), Some(asks)) = (side("bids"), side("asks")) else {
            warn!(symbol = key, "Unparseable OKX book entry");
            return;
        };
        let ts = item
            .get("ts")
            .and_then(Value::as_str)
            .and_then(|s| s.parse::<u64>().ok())
            .unwrap_or(0);
        if let Some(sender) = self.depth_senders.get(key) {
            let update = if snapshot {
                DepthUpdate::snapshot(ts, bids, asks)
            } else {
                DepthUpdate::new(ts, bids, asks)
            };
            if let Err(err) = sender.try_send(update) {
                warn!(symbol = key, error = %err, "Failed to route depth message");
            }
        }
    }

    /// Runs the routing loop until the raw channel closes.
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

    fn router() -> OkxRouter {
        let (_tx, rx) = mpsc::channel(10);
        OkxRouter::new(rx)
    }

    fn candle(ts: u64, close: &str, confirm: &str) -> String {
        format!(
            r#"{{"arg":{{"channel":"candle1m","instId":"BTC-USDT"}},"data":[["{ts}","100","110","90","{close}","2.5","250","250","{confirm}"]]}}"#
        )
    }

    #[test]
    fn emits_only_confirmed_candles_once() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_candle_channel("BTC-USDT");

        // Act & Assert
        r.process_raw_message(&candle(1000, "101", "0"))
            .expect("ok");
        assert!(
            rx.try_recv().is_err(),
            "unconfirmed candle must not be emitted"
        );

        r.process_raw_message(&candle(1000, "102", "1"))
            .expect("ok");
        let c = rx.try_recv().expect("confirmed candle");
        assert_eq!((c.timestamp_ms, c.close, c.volume), (1000, 102.0, 2.5));

        r.process_raw_message(&candle(1000, "103", "1"))
            .expect("ok");
        assert!(
            rx.try_recv().is_err(),
            "same timestamp is not emitted twice"
        );

        r.process_raw_message(&candle(61000, "104", "1"))
            .expect("ok");
        assert_eq!(rx.try_recv().expect("next candle").close, 104.0);
    }

    #[test]
    fn routes_book_snapshot_and_zero_size_updates() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_depth_channel("BTC-USDT");
        let msg = r#"{"arg":{"channel":"books","instId":"BTC-USDT"},"action":"update","data":[{"asks":[["101.5","0","0","0"]],"bids":[["100.5","3.2","0","2"]],"ts":"1700000000123","checksum":1}]}"#;

        // Act
        r.process_raw_message(msg).expect("ok");

        // Assert
        let d = rx.try_recv().expect("depth update");
        assert_eq!(d.bids, vec![(100.5, 3.2)]);
        assert_eq!(d.asks, vec![(101.5, 0.0)]);
        assert_eq!(d.timestamp_ms, 1_700_000_000_123);
    }

    #[test]
    fn ignores_acks_errors_garbage_and_other_instruments() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_candle_channel("ETH-USDT");

        // Act & Assert
        for raw in [
            r#"{"event":"subscribe","arg":{"channel":"candle1m","instId":"ETH-USDT"}}"#,
            r#"{"event":"error","code":"60012","msg":"bad"}"#,
            "pong",
            "kein json",
        ] {
            assert!(r.process_raw_message(raw).is_ok());
        }
        r.process_raw_message(&candle(1000, "1", "1")).expect("ok");
        assert!(rx.try_recv().is_err(), "BTC-USDT must not reach ETH-USDT");
    }
}
