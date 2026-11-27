//! Kraken WebSocket v2 message router: parses `ohlc` and `book` channel messages.
//!
//! Candle semantics match the Binance router: only **closed** candles are emitted.
//! Kraken streams the open candle repeatedly, so a candle is released when a candle
//! with a newer `interval_begin` arrives. Snapshots only seed the pending candle.

use std::collections::HashMap;

use omnitrade_core::Candle;
use serde_json::Value;
use tokio::sync::mpsc;
use tracing::warn;

use crate::error::ExchangeError;
use crate::traits::DepthUpdate;

const CHANNEL_CAPACITY: usize = 100;

/// Message router for Kraken WebSocket v2 streams.
pub struct KrakenRouter {
    raw_rx: mpsc::Receiver<String>,
    candle_senders: HashMap<String, mpsc::Sender<Candle>>,
    depth_senders: HashMap<String, mpsc::Sender<DepthUpdate>>,
    pending: HashMap<String, Candle>,
}

impl KrakenRouter {
    /// Creates a router consuming raw JSON messages from `raw_rx`.
    pub fn new(raw_rx: mpsc::Receiver<String>) -> Self {
        Self {
            raw_rx,
            candle_senders: HashMap::new(),
            depth_senders: HashMap::new(),
            pending: HashMap::new(),
        }
    }

    /// Registers a candle channel for a symbol (e.g. `BTC/USD`).
    pub fn register_candle_channel(&mut self, symbol: &str) -> mpsc::Receiver<Candle> {
        let (tx, rx) = mpsc::channel(CHANNEL_CAPACITY);
        self.candle_senders.insert(symbol.to_uppercase(), tx);
        rx
    }

    /// Registers a depth channel for a symbol (e.g. `BTC/USD`).
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
                warn!(error = %err, "Malformed Kraken JSON");
                return Ok(());
            }
        };
        let is_snapshot = value.get("type").and_then(Value::as_str) == Some("snapshot");
        let Some(data) = value.get("data").and_then(Value::as_array) else {
            // Heartbeats, status and subscribe acks carry no data.
            if value.get("success") == Some(&Value::Bool(false)) {
                warn!(message = raw, "Kraken rejected a request");
            }
            return Ok(());
        };
        match value.get("channel").and_then(Value::as_str) {
            Some("ohlc") => {
                for item in data {
                    self.handle_ohlc(item, is_snapshot);
                }
            }
            Some("book") => {
                for item in data {
                    self.handle_book(item);
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn handle_ohlc(&mut self, item: &Value, is_snapshot: bool) {
        let Some(symbol) = item.get("symbol").and_then(Value::as_str) else {
            return;
        };
        let Some(candle) = parse_candle(item) else {
            warn!(symbol, "Unparseable Kraken ohlc entry");
            return;
        };
        let key = symbol.to_uppercase();
        match self.pending.get(&key).cloned() {
            Some(prev) if candle.timestamp_ms > prev.timestamp_ms => {
                // A newer candle began, so the previous one is closed. Snapshot history
                // is never emitted: replaying old candles would fire stale signals.
                if !is_snapshot {
                    self.emit_candle(&key, prev);
                }
                self.pending.insert(key, candle);
            }
            Some(prev) if candle.timestamp_ms < prev.timestamp_ms => {}
            _ => {
                self.pending.insert(key, candle);
            }
        }
    }

    fn emit_candle(&self, key: &str, candle: Candle) {
        if let Some(sender) = self.candle_senders.get(key) {
            if let Err(err) = sender.try_send(candle) {
                warn!(symbol = key, error = %err, "Failed to route candle message");
            }
        }
    }

    fn handle_book(&mut self, item: &Value) {
        let Some(symbol) = item.get("symbol").and_then(Value::as_str) else {
            return;
        };
        let side = |key: &str| -> Option<Vec<(f64, f64)>> {
            item.get(key)?
                .as_array()?
                .iter()
                .map(|l| Some((l.get("price")?.as_f64()?, l.get("qty")?.as_f64()?)))
                .collect()
        };
        // Kraken v2 schickt bei einseitigen Änderungen nur `bids` oder nur `asks`. Eine
        // fehlende Seite ist dann ein leeres Delta; eine vorhandene, aber kaputte Seite nicht.
        let seite = |key: &str| -> Option<Vec<(f64, f64)>> {
            match item.get(key) {
                None => Some(Vec::new()),
                Some(_) => side(key),
            }
        };
        let (Some(bids), Some(asks)) = (seite("bids"), seite("asks")) else {
            warn!(symbol, "Unparseable Kraken book entry");
            return;
        };
        let timestamp_ms = item
            .get("timestamp")
            .and_then(Value::as_str)
            .and_then(parse_rfc3339_ms)
            .unwrap_or(0);
        let key = symbol.to_uppercase();
        if let Some(sender) = self.depth_senders.get(&key) {
            if let Err(err) = sender.try_send(DepthUpdate::new(timestamp_ms, bids, asks)) {
                warn!(symbol = %key, error = %err, "Failed to route depth message");
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

fn parse_candle(item: &Value) -> Option<Candle> {
    let f = |key: &str| item.get(key)?.as_f64();
    let begin = parse_rfc3339_ms(item.get("interval_begin")?.as_str()?)?;
    Some(Candle::new(
        begin,
        f("open")?,
        f("high")?,
        f("low")?,
        f("close")?,
        f("volume")?,
    ))
}

/// Parses `YYYY-MM-DDTHH:MM:SS[.ffffff]Z` into milliseconds since the Unix epoch.
fn parse_rfc3339_ms(s: &str) -> Option<u64> {
    let s = s.strip_suffix('Z')?;
    let (date, time) = s.split_once('T')?;
    let mut d = date.split('-');
    let (y, m, day): (i64, i64, i64) = (
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
        d.next()?.parse().ok()?,
    );
    let (hms, frac) = time.split_once('.').unwrap_or((time, "0"));
    let mut t = hms.split(':');
    let (h, mi, sec): (i64, i64, i64) = (
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
        t.next()?.parse().ok()?,
    );
    let millis: i64 = format!("{frac:0<3}").get(..3)?.parse().ok()?;
    // days_from_civil (Howard Hinnant)
    let y = if m <= 2 { y - 1 } else { y };
    let era = y.div_euclid(400);
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    let days = era * 146_097 + doe - 719_468;
    let total = ((days * 24 + h) * 60 + mi) * 60 + sec;
    u64::try_from(total * 1000 + millis).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn router() -> KrakenRouter {
        let (_tx, rx) = mpsc::channel(10);
        KrakenRouter::new(rx)
    }

    fn ohlc(kind: &str, begin: &str, close: f64) -> String {
        format!(
            r#"{{"channel":"ohlc","type":"{kind}","data":[{{"symbol":"BTC/USD","open":100.0,"high":110.0,"low":90.0,"close":{close},"volume":2.5,"vwap":100.0,"trades":3,"interval_begin":"{begin}","interval":1,"timestamp":"2026-01-01T00:00:59.000000Z"}}],"timestamp":"2026-01-01T00:00:59.000000Z"}}"#
        )
    }

    #[test]
    fn parses_rfc3339_timestamps() {
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:00Z"), Some(0));
        assert_eq!(parse_rfc3339_ms("1970-01-01T00:00:01.5Z"), Some(1500));
        assert_eq!(
            parse_rfc3339_ms("2023-10-04T16:25:00.123456Z"),
            Some(1_696_436_700_123)
        );
        assert_eq!(parse_rfc3339_ms("kaputt"), None);
    }

    #[test]
    fn emits_candle_only_when_next_interval_begins() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_candle_channel("BTC/USD");

        // Act & Assert
        r.process_raw_message(&ohlc("update", "2026-01-01T00:00:00.000000Z", 101.0))
            .expect("ok");
        r.process_raw_message(&ohlc("update", "2026-01-01T00:00:00.000000Z", 102.0))
            .expect("ok");
        assert!(rx.try_recv().is_err(), "open candle must not be emitted");

        r.process_raw_message(&ohlc("update", "2026-01-01T00:01:00.000000Z", 103.0))
            .expect("ok");
        let closed = rx.try_recv().expect("previous candle closed");
        assert_eq!(closed.close, 102.0, "last update of the closed candle wins");
        assert_eq!(closed.volume, 2.5);
        assert!(rx.try_recv().is_err());
    }

    #[test]
    fn snapshot_seeds_without_emitting_history() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_candle_channel("BTC/USD");

        // Act
        r.process_raw_message(&ohlc("snapshot", "2026-01-01T00:00:00.000000Z", 100.0))
            .expect("ok");
        r.process_raw_message(&ohlc("snapshot", "2026-01-01T00:01:00.000000Z", 101.0))
            .expect("ok");

        // Assert
        assert!(rx.try_recv().is_err());
        r.process_raw_message(&ohlc("update", "2026-01-01T00:02:00.000000Z", 102.0))
            .expect("ok");
        assert_eq!(rx.try_recv().expect("closed").close, 101.0);
    }

    #[test]
    fn routes_book_updates_with_zero_quantity_levels() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_depth_channel("BTC/USD");
        let msg = r#"{"channel":"book","type":"update","data":[{"symbol":"BTC/USD","bids":[{"price":100.5,"qty":0.0}],"asks":[{"price":101.0,"qty":2.0}],"checksum":1,"timestamp":"1970-01-01T00:00:02.000000Z"}]}"#;

        // Act
        r.process_raw_message(msg).expect("ok");

        // Assert
        let d = rx.try_recv().expect("depth update");
        assert_eq!(d.bids, vec![(100.5, 0.0)]);
        assert_eq!(d.asks, vec![(101.0, 2.0)]);
        assert_eq!(d.timestamp_ms, 2000);
    }

    #[test]
    fn ignores_heartbeats_acks_garbage_and_unknown_symbols() {
        // Arrange
        let mut r = router();
        let mut rx = r.register_candle_channel("ETH/USD");

        // Act & Assert
        for raw in [
            r#"{"channel":"heartbeat"}"#,
            r#"{"method":"subscribe","success":true,"result":{}}"#,
            r#"{"method":"subscribe","success":false,"error":"x"}"#,
            "kein json",
        ] {
            assert!(r.process_raw_message(raw).is_ok());
        }
        r.process_raw_message(&ohlc("update", "2026-01-01T00:00:00.000000Z", 1.0))
            .expect("ok");
        r.process_raw_message(&ohlc("update", "2026-01-01T00:01:00.000000Z", 2.0))
            .expect("ok");
        assert!(
            rx.try_recv().is_err(),
            "BTC/USD candles must not reach ETH/USD"
        );
    }
}
