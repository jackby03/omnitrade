//! UI state container for passive consumption of trading engine updates.

use std::collections::HashMap;

use omnitrade_core::{Candle, Position};
use omnitrade_engine::L2OrderBook;

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
}

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
        }
    }

    /// Appends a new candle snapshot for a given symbol.
    pub fn update_candle(&mut self, symbol: &str, candle: Candle) {
        self.candles
            .entry(symbol.to_string())
            .or_default()
            .push(candle);
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
}
