//! UI state container for passive consumption of trading engine updates.

use std::collections::{HashMap, VecDeque};

use omnitrade_core::{Candle, Order, Position};
use omnitrade_engine::L2OrderBook;

use crate::status_feed::StatusDoc;

/// Thread-safe state container storing passive UI snapshots of engine data.
#[derive(Debug, Clone)]
pub struct UIState {
    /// Candle history mapped by symbol.
    pub candles: HashMap<String, Vec<Candle>>,
    /// Latest L2 order book snapshot mapped by symbol.
    pub orderbook: HashMap<String, L2OrderBook>,
    /// Active open positions mapped by symbol.
    pub positions: HashMap<String, Position>,
    /// Current total account balance.
    pub account_balance: f64,
    /// Total cumulative profit and loss.
    pub total_pnl: f64,
    /// Connection status indicator.
    pub connected: bool,
    /// Account balances `(asset, free, locked)`, sorted by asset (non-zero only).
    pub balances: Vec<(String, f64, f64)>,
    /// Open orders reported by the exchange.
    pub open_orders: Vec<Order>,
    /// Recent log lines for the console panel (oldest first).
    pub log: VecDeque<String>,
    /// Latest snapshot from `omnitrade live --status-file`, if readable.
    pub status: Option<StatusDoc>,
    /// Why the status feed is not delivering (missing or bad file).
    pub feed_error: Option<String>,
}

/// Maximum number of retained console log lines.
pub const MAX_LOG_LINES: usize = 500;

impl UIState {
    /// Creates a new, empty `UIState`.
    pub fn new() -> Self {
        Self {
            candles: HashMap::new(),
            orderbook: HashMap::new(),
            positions: HashMap::new(),
            account_balance: 0.0,
            total_pnl: 0.0,
            connected: false,
            balances: Vec::new(),
            open_orders: Vec::new(),
            log: VecDeque::new(),
            status: None,
            feed_error: None,
        }
    }

    /// Appends a candle, or replaces the last one if it has the same timestamp
    /// (exchanges stream the still-open candle repeatedly).
    pub fn update_candle(&mut self, symbol: &str, candle: Candle) {
        let series = self.candles.entry(symbol.to_string()).or_default();
        match series.last_mut() {
            Some(last) if last.timestamp_ms == candle.timestamp_ms => *last = candle,
            _ => series.push(candle),
        }
    }

    /// Stores a fresh account snapshot; `account_balance` becomes the USDT total.
    pub fn set_account_snapshot(&mut self, balances: Vec<(String, f64, f64)>, orders: Vec<Order>) {
        self.account_balance = balances
            .iter()
            .find(|(asset, _, _)| asset == "USDT")
            .map_or(0.0, |(_, free, locked)| free + locked);
        self.balances = balances;
        self.open_orders = orders;
    }

    /// Replaces the whole candle series of a symbol (e.g. after loading history).
    pub fn set_candles(&mut self, symbol: &str, candles: Vec<Candle>) {
        self.candles.insert(symbol.to_string(), candles);
    }

    /// Applies a depth delta to the symbol's order book (creating it if missing).
    pub fn apply_depth(
        &mut self,
        symbol: &str,
        timestamp_ms: u64,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> Result<(), omnitrade_engine::EngineError> {
        let book = self
            .orderbook
            .entry(symbol.to_string())
            .or_insert_with(|| L2OrderBook::new(symbol));
        book.apply_delta(bids, asks)?;
        book.set_last_update_ms(timestamp_ms);
        Ok(())
    }

    /// Ersetzt das Buch eines Symbols durch einen vollständigen Stand (Snapshot mit Zeitstempel).
    /// Stufen, die im Snapshot fehlen, sind danach weg.
    pub fn replace_depth(
        &mut self,
        symbol: &str,
        timestamp_ms: u64,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> Result<(), omnitrade_engine::EngineError> {
        let mut book = L2OrderBook::new(symbol);
        book.apply_delta(bids, asks)?;
        book.set_last_update_ms(timestamp_ms);
        self.orderbook.insert(symbol.to_string(), book);
        Ok(())
    }

    /// Replaces the order book of a symbol with a full snapshot.
    pub fn set_orderbook_snapshot(
        &mut self,
        symbol: &str,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> Result<(), omnitrade_engine::EngineError> {
        let mut book = L2OrderBook::new(symbol);
        book.apply_delta(bids, asks)?;
        self.orderbook.insert(symbol.to_string(), book);
        Ok(())
    }

    /// Appends a console log line, dropping the oldest beyond [`MAX_LOG_LINES`].
    pub fn push_log(&mut self, line: impl Into<String>) {
        if self.log.len() >= MAX_LOG_LINES {
            self.log.pop_front();
        }
        self.log.push_back(line.into());
    }

    /// Updates or inserts the L2 order book snapshot for a given symbol.
    pub fn update_orderbook(&mut self, symbol: &str, book: L2OrderBook) {
        self.orderbook.insert(symbol.to_string(), book);
    }

    /// Updates account metrics (balance and total PnL).
    pub fn update_account(&mut self, balance: f64, pnl: f64) {
        self.account_balance = balance;
        self.total_pnl = pnl;
    }

    /// Returns at most `count` latest candles for the given symbol.
    pub fn latest_candles(&self, symbol: &str, count: usize) -> &[Candle] {
        match self.candles.get(symbol) {
            Some(vec) => {
                let start = vec.len().saturating_sub(count);
                &vec[start..]
            }
            None => &[],
        }
    }

    /// Returns `true` if connected to engine/exchanges.
    pub fn is_connected(&self) -> bool {
        self.connected
    }

    /// Sets connection status.
    pub fn set_connected(&mut self, connected: bool) {
        self.connected = connected;
    }
}

impl Default for UIState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dummy_candle(timestamp_ms: u64, close: f64) -> Candle {
        Candle {
            timestamp_ms,
            open: close,
            high: close,
            low: close,
            close,
            volume: 1.0,
        }
    }

    #[test]
    fn test_update_candle_appends() {
        // Arrange
        let mut state = UIState::new();
        let candle1 = dummy_candle(1000, 100.0);
        let candle2 = dummy_candle(2000, 105.0);

        // Act
        state.update_candle("BTCUSDT", candle1);
        state.update_candle("BTCUSDT", candle2);

        // Assert
        let candles = state.latest_candles("BTCUSDT", 10);
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp_ms, 1000);
        assert_eq!(candles[1].timestamp_ms, 2000);
    }

    #[test]
    fn test_latest_candles_limits_count() {
        // Arrange
        let mut state = UIState::new();
        for i in 1..=5 {
            state.update_candle("ETHUSDT", dummy_candle(i * 1000, i as f64));
        }

        // Act
        let latest_3 = state.latest_candles("ETHUSDT", 3);

        // Assert
        assert_eq!(latest_3.len(), 3);
        assert_eq!(latest_3[0].timestamp_ms, 3000);
        assert_eq!(latest_3[1].timestamp_ms, 4000);
        assert_eq!(latest_3[2].timestamp_ms, 5000);
    }

    #[test]
    fn test_latest_candles_unknown_symbol() {
        // Arrange
        let state = UIState::new();

        // Act
        let candles = state.latest_candles("UNKNOWN", 5);

        // Assert
        assert!(candles.is_empty());
    }

    #[test]
    fn test_ui_state_send_and_sync() {
        // Arrange & Act & Assert
        fn assert_send_sync<T: Send + Sync>() {}
        assert_send_sync::<UIState>();
    }

    #[test]
    fn test_update_orderbook_and_account() {
        // Arrange
        let mut state = UIState::new();
        let book = L2OrderBook::new("BTCUSDT");

        // Act
        state.update_orderbook("BTCUSDT", book);
        state.update_account(10000.0, 250.5);
        state.set_connected(true);

        // Assert
        assert!(state.orderbook.contains_key("BTCUSDT"));
        assert_eq!(state.account_balance, 10000.0);
        assert_eq!(state.total_pnl, 250.5);
        assert!(state.is_connected());
    }

    #[test]
    fn test_update_candle_replaces_open_candle_with_same_timestamp() {
        // Arrange
        let mut state = UIState::new();
        state.update_candle("BTCUSDT", dummy_candle(1000, 100.0));

        // Act
        state.update_candle("BTCUSDT", dummy_candle(1000, 101.0));
        state.update_candle("BTCUSDT", dummy_candle(2000, 102.0));

        // Assert
        let candles = state.latest_candles("BTCUSDT", 10);
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].close, 101.0);
        assert_eq!(candles[1].close, 102.0);
    }

    #[test]
    fn test_apply_depth_creates_book_and_removes_zero_levels() {
        // Arrange
        let mut state = UIState::new();

        // Act
        state
            .apply_depth("BTCUSDT", 5, &[(100.0, 1.0)], &[(101.0, 2.0)])
            .expect("valid delta");
        state
            .apply_depth("BTCUSDT", 6, &[(100.0, 0.0)], &[])
            .expect("valid delta");

        // Assert
        let book = &state.orderbook["BTCUSDT"];
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), Some((101.0, 2.0)));
        assert_eq!(book.last_update_ms(), 6);
    }

    #[test]
    fn test_push_log_is_bounded() {
        // Arrange
        let mut state = UIState::new();

        // Act
        for i in 0..(MAX_LOG_LINES + 10) {
            state.push_log(format!("line {i}"));
        }

        // Assert
        assert_eq!(state.log.len(), MAX_LOG_LINES);
        assert_eq!(state.log.front().map(String::as_str), Some("line 10"));
    }

    #[test]
    fn test_set_candles_replaces_series() {
        // Arrange
        let mut state = UIState::new();
        state.update_candle("BTCUSDT", dummy_candle(1000, 1.0));

        // Act
        state.set_candles(
            "BTCUSDT",
            vec![dummy_candle(2000, 2.0), dummy_candle(3000, 3.0)],
        );

        // Assert
        let candles = state.latest_candles("BTCUSDT", 10);
        assert_eq!(candles.len(), 2);
        assert_eq!(candles[0].timestamp_ms, 2000);
    }

    #[test]
    fn test_set_account_snapshot_derives_usdt_balance() {
        // Arrange
        let mut state = UIState::new();

        // Act
        state.set_account_snapshot(
            vec![("BTC".into(), 1.0, 0.0), ("USDT".into(), 90.0, 10.0)],
            Vec::new(),
        );

        // Assert
        assert_eq!(state.account_balance, 100.0);
        assert_eq!(state.balances.len(), 2);
    }

    #[test]
    fn test_set_orderbook_snapshot_replaces_book() {
        // Arrange
        let mut state = UIState::new();
        state
            .apply_depth("BTCUSDT", 1, &[(1.0, 1.0)], &[(2.0, 1.0)])
            .expect("valid");

        // Act
        state
            .set_orderbook_snapshot("BTCUSDT", &[(10.0, 1.0)], &[(11.0, 2.0)])
            .expect("valid");

        // Assert
        let book = &state.orderbook["BTCUSDT"];
        assert_eq!(book.best_bid(), Some((10.0, 1.0)));
        assert_eq!(book.best_ask(), Some((11.0, 2.0)));
    }
}
