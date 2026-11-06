//! Passive feed from `omnitrade live --status-file`: reads the JSON snapshot
//! (schema 1) and copies it into `UIState`. Read-only; no control channel.

use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;

use omnitrade_core::Candle;
use serde::Deserialize;

use crate::state::UIState;

/// Snapshot layout version this feed understands.
pub const SCHEMA: u32 = 1;

/// Default location relative to the omnitrade directory.
pub const DEFAULT_PATH: &str = "status/live.json";

/// One order event of the snapshot.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct OrderRow {
    pub time_ms: u64,
    pub side: String,
    pub quantity: f64,
    pub price: f64,
    pub outcome: String,
    pub detail: String,
}

/// One closed candle of the snapshot.
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct CandleRow {
    pub time_ms: u64,
    pub open: f64,
    pub high: f64,
    pub low: f64,
    pub close: f64,
    pub volume: f64,
}

/// The status document (fields the UI needs; unknown ones are ignored).
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(default)]
pub struct StatusDoc {
    pub schema: u32,
    pub state: String,
    pub exchange: String,
    pub symbol: String,
    pub network: String,
    pub script: String,
    pub initial_capital: f64,
    pub updated_ms: u64,
    pub candles_seen: u64,
    pub last_close: Option<f64>,
    pub last_signal: String,
    pub orders_placed: u64,
    pub orders_rejected: u64,
    pub net_quantity: f64,
    pub last_error: String,
    pub recent_orders: Vec<OrderRow>,
    pub recent_candles: Vec<CandleRow>,
}

/// Path from the first CLI argument, `OMNITRADE_STATUS`, or the default.
pub fn resolve_path(arg: Option<String>) -> PathBuf {
    arg.or_else(|| std::env::var("OMNITRADE_STATUS").ok())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(DEFAULT_PATH))
}

/// Reads and validates the snapshot file.
pub fn read_status(path: &Path) -> Result<StatusDoc, String> {
    let text = std::fs::read_to_string(path)
        .map_err(|e| format!("no status file ({}): {e}", path.display()))?;
    let doc: StatusDoc =
        serde_json::from_str(&text).map_err(|e| format!("status file unreadable: {e}"))?;
    if doc.schema != SCHEMA {
        return Err(format!("unknown schema {}", doc.schema));
    }
    Ok(doc)
}

/// Copies a snapshot into the UI state.
pub fn apply(state: &mut UIState, doc: StatusDoc) {
    let candles = doc
        .recent_candles
        .iter()
        .map(|c| Candle::new(c.time_ms, c.open, c.high, c.low, c.close, c.volume))
        .collect();
    state.set_candles(&doc.symbol, candles);
    // Die Statusdatei meldet den Lauf, nicht die Boersenverbindung. Die Verbindung
    // setzt allein der Marktdaten-Feed, sonst zeigt die GUI bei fehlender Datei
    // dauerhaft "getrennt", obwohl der Kurs-Stream läuft.
    state.feed_error = None;
    state.status = Some(doc);
}

/// Marks the feed as broken (missing or bad file).
pub fn apply_error(state: &mut UIState, message: String) {
    state.feed_error = Some(message);
}

/// Polls the file once per second on a background thread.
pub fn spawn_poller(path: PathBuf, state: Arc<RwLock<UIState>>) {
    std::thread::spawn(move || loop {
        let result = read_status(&path);
        {
            let mut guard = state.write().unwrap_or_else(|p| p.into_inner());
            match result {
                Ok(doc) => apply(&mut guard, doc),
                Err(e) => apply_error(&mut guard, e),
            }
        }
        std::thread::sleep(Duration::from_secs(1));
    });
}

/// `HH:MM:SS` (UTC) of a millisecond timestamp.
pub fn clock(ms: u64) -> String {
    let s = ms / 1000 % 86_400;
    format!("{:02}:{:02}:{:02}", s / 3600, s / 60 % 60, s % 60)
}

/// Lines for the positions panel.
pub fn summary_lines(doc: &StatusDoc) -> Vec<String> {
    let last = doc
        .last_close
        .map_or_else(|| "-".to_string(), |p| format!("{p:.2}"));
    let value = doc.last_close.map_or(0.0, |p| p * doc.net_quantity);
    vec![
        format!("{} · {} {}", doc.state, doc.exchange, doc.network),
        format!("{} · script {}", doc.symbol, doc.script),
        format!("last {last} · signal {}", doc.last_signal),
        format!(
            "candles {} · capital {:.0}",
            doc.candles_seen, doc.initial_capital
        ),
        format!(
            "orders {} ok / {} rejected",
            doc.orders_placed, doc.orders_rejected
        ),
        format!("net {:.8} (≈ {value:.2})", doc.net_quantity),
    ]
}

/// One console line per order, newest first.
pub fn order_lines(doc: &StatusDoc) -> Vec<String> {
    doc.recent_orders
        .iter()
        .rev()
        .map(|o| {
            let mut line = format!(
                "{} {} {} @ {:.2} {}",
                clock(o.time_ms),
                o.side,
                o.quantity,
                o.price,
                o.outcome
            );
            if !o.detail.is_empty() {
                line.push_str(&format!(" – {}", o.detail));
            }
            line
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    const JSON: &str = r#"{"schema":1,"state":"running","exchange":"okx","symbol":"BTCUSDC",
        "network":"testnet","script":"s.omni","initial_capital":500.0,"updated_ms":1,
        "candles_seen":2,"last_close":83000.0,"last_signal":"hold","orders_placed":1,
        "orders_rejected":1,"net_quantity":0.006,"last_error":"","future_field":true,
        "recent_orders":[{"time_ms":3661000,"side":"buy","quantity":0.006,"price":83000.0,
        "outcome":"filled","detail":""},{"time_ms":7322000,"side":"sell","quantity":0.006,
        "price":83100.0,"outcome":"rejected","detail":"min size"}],
        "recent_candles":[{"time_ms":1,"open":1.0,"high":2.0,"low":0.5,"close":1.5,"volume":3.0}]}"#;

    fn doc() -> StatusDoc {
        serde_json::from_str(JSON).unwrap()
    }

    #[test]
    fn test_apply_fills_state() {
        let mut state = UIState::new();
        apply(&mut state, doc());
        // Verbindungsstatus bleibt beim Marktdaten-Feed, die Statusdatei setzt ihn nicht.
        assert!(!state.connected);
        assert_eq!(state.latest_candles("BTCUSDC", 10).len(), 1);
        assert_eq!(state.latest_candles("BTCUSDC", 10)[0].close, 1.5);
        assert_eq!(state.status.as_ref().unwrap().exchange, "okx");
    }

    #[test]
    fn test_apply_replaces_candles_not_appends() {
        let mut state = UIState::new();
        apply(&mut state, doc());
        apply(&mut state, doc());
        assert_eq!(state.latest_candles("BTCUSDC", 10).len(), 1);
    }

    #[test]
    fn test_stopped_is_not_connected_and_error_marks_feed() {
        let mut state = UIState::new();
        let mut d = doc();
        d.state = "stopped".into();
        apply(&mut state, d);
        assert!(!state.connected);
        apply_error(&mut state, "weg".into());
        assert_eq!(state.feed_error.as_deref(), Some("weg"));
    }

    #[test]
    fn test_read_status_errors() {
        assert!(read_status(Path::new("does/not/exist.json")).is_err());
        let dir = std::env::temp_dir().join(format!("omni-ui-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("live.json");
        std::fs::write(&file, JSON.replace("\"schema\":1", "\"schema\":9")).unwrap();
        assert!(read_status(&file).unwrap_err().contains("schema"));
        std::fs::write(&file, JSON).unwrap();
        assert_eq!(read_status(&file).unwrap().symbol, "BTCUSDC");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn test_lines() {
        let d = doc();
        assert_eq!(clock(3_661_000), "01:01:01");
        let orders = order_lines(&d);
        assert!(orders[0].contains("sell") && orders[0].contains("min size"));
        assert!(orders[1].starts_with("01:01:01 buy"));
        assert!(summary_lines(&d)[0].contains("okx testnet"));
    }

    #[test]
    fn test_resolve_path_prefers_argument() {
        assert_eq!(resolve_path(Some("x.json".into())), PathBuf::from("x.json"));
    }
}
