//! Live status snapshot for passive observers.
//!
//! `omnitrade live --status-file PATH` keeps a small JSON document up to date
//! while the loop runs. Observers (e.g. a dashboard) only read the file; there
//! is no socket and no control channel. Each write goes to `PATH.tmp` first and
//! is then renamed over `PATH`, so a reader never sees a half-written file.

use omnitrade_core::Candle;
use serde::Serialize;
use std::collections::VecDeque;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Number of recent order events kept in the snapshot.
const MAX_ORDERS: usize = 20;

/// Number of recent closed candles kept in the snapshot (for charts).
const MAX_CANDLES: usize = 300;

/// Current version of the snapshot layout. Bump on incompatible changes.
pub const SCHEMA: u32 = 1;

/// One order attempt as seen by the live loop.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct OrderEvent {
    pub time_ms: u64,
    pub side: String,
    pub quantity: f64,
    pub price: f64,
    /// "filled", "placed" or "rejected".
    pub outcome: String,
    /// Rejection or exchange message, empty if accepted.
    pub detail: String,
}

/// One closed candle as written to the snapshot.
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct CandleRow {
    pub time_ms: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// The document written to the status file.
#[derive(Debug, Clone, Serialize)]
pub struct LiveStatus {
    pub schema: u32,
    /// "starting", "running", "stopped" or "error".
    pub state: String,
    pub exchange: String,
    pub symbol: String,
    pub network: String,
    pub script: String,
    pub initial_capital: f64,
    pub pid: u32,
    pub started_ms: u64,
    pub updated_ms: u64,
    pub candles_seen: u64,
    pub last_close: Option<f64>,
    pub last_candle_ms: Option<u64>,
    /// "buy", "sell" or "hold".
    pub last_signal: String,
    pub orders_placed: u64,
    pub orders_rejected: u64,
    /// Net quantity from accepted orders (buy positive, sell negative).
    pub net_quantity: f64,
    pub last_error: String,
    pub recent_orders: VecDeque<OrderEvent>,
    /// Last closed candles, oldest first (additive field, schema unchanged).
    pub recent_candles: VecDeque<CandleRow>,
}

fn now_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// Holds the snapshot and writes it to disk. Without a path it only tracks
/// state in memory, so the live loop needs no special casing.
pub struct StatusWriter {
    path: Option<PathBuf>,
    status: LiveStatus,
}

impl StatusWriter {
    pub fn new(
        path: Option<PathBuf>,
        exchange: &str,
        symbol: &str,
        network: &str,
        script: &str,
        initial_capital: f64,
    ) -> Self {
        let now = now_ms();
        Self {
            path,
            status: LiveStatus {
                schema: SCHEMA,
                state: "starting".to_string(),
                exchange: exchange.to_string(),
                symbol: symbol.to_string(),
                network: network.to_string(),
                script: script.to_string(),
                initial_capital,
                pid: std::process::id(),
                started_ms: now,
                updated_ms: now,
                candles_seen: 0,
                last_close: None,
                last_candle_ms: None,
                last_signal: "hold".to_string(),
                orders_placed: 0,
                orders_rejected: 0,
                net_quantity: 0.0,
                last_error: String::new(),
                recent_orders: VecDeque::new(),
                recent_candles: VecDeque::new(),
            },
        }
    }

    #[cfg(test)]
    pub fn status(&self) -> &LiveStatus {
        &self.status
    }

    pub fn set_state(&mut self, state: &str) {
        self.status.state = state.to_string();
    }

    pub fn set_error(&mut self, message: &str) {
        self.status.state = "error".to_string();
        self.status.last_error = message.to_string();
    }

    pub fn record_candle(&mut self, candle: &Candle, signal: &str) {
        self.status.state = "running".to_string();
        self.status.candles_seen += 1;
        self.status.last_close = Some(candle.close);
        self.status.last_candle_ms = Some(candle.timestamp_ms);
        self.status.last_signal = signal.to_string();
        if self.status.recent_candles.len() == MAX_CANDLES {
            self.status.recent_candles.pop_front();
        }
        self.status.recent_candles.push_back(CandleRow {
            time_ms: candle.timestamp_ms,
            open: candle.open,
            high: candle.high,
            low: candle.low,
            close: candle.close,
            volume: candle.volume,
        });
    }

    /// Setzt die Nettoposition auf den Wert der BOERSE.
    ///
    /// Der eigene Zaehler in [`Self::record_order`] ist nur die Zwischenanzeige
    /// zwischen Auftrag und naechster Kerze: eine Position kann auch ohne diesen
    /// Lauf entstehen oder vergehen (Handschluss, Stop, zweiter Prozess). Die
    /// Boerse ist die Wahrheit, der Zaehler die Schaetzung.
    pub fn set_net_quantity(&mut self, quantity: f64) {
        self.status.net_quantity = quantity;
    }

    pub fn record_order(
        &mut self,
        side: &str,
        quantity: f64,
        price: f64,
        outcome: &str,
        detail: &str,
    ) {
        if outcome == "rejected" {
            self.status.orders_rejected += 1;
            self.status.last_error = detail.to_string();
        } else {
            self.status.orders_placed += 1;
            let sign = if side == "sell" { -1.0 } else { 1.0 };
            self.status.net_quantity += sign * quantity;
        }
        if self.status.recent_orders.len() == MAX_ORDERS {
            self.status.recent_orders.pop_front();
        }
        self.status.recent_orders.push_back(OrderEvent {
            time_ms: now_ms(),
            side: side.to_string(),
            quantity,
            price,
            outcome: outcome.to_string(),
            detail: detail.to_string(),
        });
    }

    /// Writes the snapshot atomically. A failing write must never stop
    /// trading, so callers log the error and carry on.
    pub fn flush(&mut self) -> io::Result<()> {
        self.status.updated_ms = now_ms();
        let Some(path) = &self.path else {
            return Ok(());
        };
        write_atomic(path, &self.status)
    }
}

fn write_atomic(path: &Path, status: &LiveStatus) -> io::Result<()> {
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        fs::create_dir_all(dir)?;
    }
    let json = serde_json::to_vec_pretty(status).map_err(io::Error::other)?;
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, json)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path(name: &str) -> PathBuf {
        std::env::temp_dir()
            .join(format!("omnitrade-status-{}-{}", std::process::id(), name))
            .join("live.json")
    }

    #[test]
    fn test_flush_writes_readable_json() {
        let path = temp_path("flush");
        let mut w = StatusWriter::new(
            Some(path.clone()),
            "okx",
            "BTCUSDT",
            "testnet",
            "s.omni",
            1000.0,
        );
        w.record_candle(
            &Candle::new(1_700_000_000_000, 59900.0, 60100.0, 59800.0, 60000.0, 2.5),
            "buy",
        );
        w.record_order("buy", 1.0, 60000.0, "filled", "");
        w.flush().unwrap();

        let text = fs::read_to_string(&path).unwrap();
        let value: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(value["schema"], SCHEMA);
        assert_eq!(value["state"], "running");
        assert_eq!(value["symbol"], "BTCUSDT");
        assert_eq!(value["exchange"], "okx");
        assert_eq!(value["candles_seen"], 1);
        assert_eq!(value["orders_placed"], 1);
        assert_eq!(value["net_quantity"], 1.0);
        assert_eq!(value["recent_orders"][0]["outcome"], "filled");
        assert_eq!(value["recent_candles"][0]["close"], 60000.0);
        assert_eq!(value["recent_candles"][0]["time_ms"], 1_700_000_000_000u64);
        assert!(!path.with_extension("json.tmp").exists());
        fs::remove_dir_all(path.parent().unwrap()).ok();
    }

    #[test]
    fn test_rejections_count_separately_and_keep_position() {
        let mut w = StatusWriter::new(None, "okx", "BTCUSDT", "testnet", "s.omni", 1000.0);
        w.record_order("sell", 2.0, 100.0, "filled", "");
        w.record_order("buy", 1.0, 100.0, "rejected", "max position");
        let s = w.status();
        assert_eq!(s.orders_placed, 1);
        assert_eq!(s.orders_rejected, 1);
        assert_eq!(s.net_quantity, -2.0);
        assert_eq!(s.last_error, "max position");
    }

    #[test]
    fn test_recent_orders_are_capped() {
        let mut w = StatusWriter::new(None, "okx", "BTCUSDT", "testnet", "s.omni", 1000.0);
        for i in 0..(MAX_ORDERS + 5) {
            w.record_order("buy", 1.0, i as f64, "filled", "");
        }
        let s = w.status();
        assert_eq!(s.recent_orders.len(), MAX_ORDERS);
        assert_eq!(s.recent_orders.front().unwrap().price, 5.0);
    }

    #[test]
    fn test_recent_candles_are_capped() {
        let mut w = StatusWriter::new(None, "okx", "BTCUSDT", "testnet", "s.omni", 500.0);
        for i in 0..(MAX_CANDLES + 5) {
            let p = i as f64;
            w.record_candle(&Candle::new(i as u64, p, p, p, p, 1.0), "hold");
        }
        let s = w.status();
        assert_eq!(s.recent_candles.len(), MAX_CANDLES);
        assert_eq!(s.recent_candles.front().unwrap().time_ms, 5);
    }

    #[test]
    fn test_without_path_flush_is_noop() {
        let mut w = StatusWriter::new(None, "okx", "BTCUSDT", "testnet", "s.omni", 1000.0);
        assert!(w.flush().is_ok());
    }

    #[test]
    fn test_error_sets_state() {
        let mut w = StatusWriter::new(None, "okx", "BTCUSDT", "testnet", "s.omni", 1000.0);
        w.set_error("ws closed");
        assert_eq!(w.status().state, "error");
        assert_eq!(w.status().last_error, "ws closed");
    }
}
