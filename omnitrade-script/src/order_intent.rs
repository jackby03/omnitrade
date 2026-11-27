//! Order Intent and Evaluator State for extended trading signals.
//!
//! Defines order-level directives (Take-Profit, Stop-Loss, Trailing-Stop)
//! alongside base signal, with persistent state across candles.

use std::collections::HashMap;

use crate::ast::Signal;

/// Represents complete trading intent with signal and risk management parameters.
#[derive(Debug, Clone, PartialEq)]
pub struct OrderIntent {
    /// Base trading signal (Buy/Sell/Hold).
    pub signal: Signal,
    /// Stop-Loss price level (if set).
    pub stop_loss: Option<f64>,
    /// Take-Profit price level (if set).
    pub take_profit: Option<f64>,
    /// Trailing stop percentage (e.g., 0.05 for 5%).
    pub trailing_pct: Option<f64>,
    /// Stop-loss relativ zum Einstieg (z. B. 0.03 = 3 %), seitenrichtig ausgewertet.
    pub stop_loss_pct: Option<f64>,
    /// Take-profit relativ zum Einstieg (z. B. 0.05 = 5 %), seitenrichtig ausgewertet.
    pub take_profit_pct: Option<f64>,
}

impl Default for OrderIntent {
    fn default() -> Self {
        Self {
            signal: Signal::Hold,
            stop_loss: None,
            take_profit: None,
            trailing_pct: None,
            stop_loss_pct: None,
            take_profit_pct: None,
        }
    }
}

impl OrderIntent {
    /// Creates a new OrderIntent with a signal and no risk parameters.
    pub fn new(signal: Signal) -> Self {
        Self {
            signal,
            stop_loss: None,
            take_profit: None,
            trailing_pct: None,
            stop_loss_pct: None,
            take_profit_pct: None,
        }
    }

    /// Sets stop-loss price level.
    pub fn with_stop_loss(mut self, price: f64) -> Self {
        self.stop_loss = Some(price);
        self
    }

    /// Sets take-profit price level.
    pub fn with_take_profit(mut self, price: f64) -> Self {
        self.take_profit = Some(price);
        self
    }

    /// Sets trailing stop percentage (e.g., 0.05 for 5%).
    pub fn with_trailing_pct(mut self, pct: f64) -> Self {
        self.trailing_pct = Some(pct);
        self
    }
}

/// Persistent state for evaluating trailing stops across candle sequences.
///
/// Tracks highest price seen to calculate trailing stop triggers.
#[derive(Debug, Clone, PartialEq)]
pub struct EvaluatorState {
    /// Highest price observed so far (for trailing stop calculations).
    pub highest_price_seen: f64,
    /// Reference price for current trailing stop (calculated from highest).
    pub trailing_stop_ref: Option<f64>,
    /// Variable bindings after the last evaluated candle (read back by `prev()`).
    pub variables: HashMap<String, f64>,
}

impl EvaluatorState {
    /// Creates a new EvaluatorState initialized with the first candle's close.
    pub fn new(initial_price: f64) -> Self {
        Self {
            highest_price_seen: initial_price,
            trailing_stop_ref: None,
            variables: HashMap::new(),
        }
    }

    /// Updates state with a new candle's close price.
    /// Returns true if trailing stop is triggered (current < reference).
    pub fn update_and_check_trailing(&mut self, current_price: f64, trailing_pct: f64) -> bool {
        // Update highest price seen
        if current_price > self.highest_price_seen {
            self.highest_price_seen = current_price;
        }

        // Calculate trailing stop reference from highest
        let threshold = self.highest_price_seen * (1.0 - trailing_pct);

        // Check if triggered
        if current_price < threshold {
            return true;
        }

        // Update reference for next check
        self.trailing_stop_ref = Some(threshold);
        false
    }

    /// Resets highest price for a new trading sequence.
    pub fn reset_with_price(&mut self, initial_price: f64) {
        self.highest_price_seen = initial_price;
        self.trailing_stop_ref = None;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_order_intent_creation() {
        let intent = OrderIntent::new(Signal::Buy)
            .with_stop_loss(100.0)
            .with_take_profit(120.0);

        assert_eq!(intent.signal, Signal::Buy);
        assert_eq!(intent.stop_loss, Some(100.0));
        assert_eq!(intent.take_profit, Some(120.0));
        assert_eq!(intent.trailing_pct, None);
    }

    #[test]
    fn test_evaluator_state_creation() {
        let state = EvaluatorState::new(100.0);
        assert_eq!(state.highest_price_seen, 100.0);
        assert_eq!(state.trailing_stop_ref, None);
    }

    #[test]
    fn test_trailing_stop_triggered() {
        let mut state = EvaluatorState::new(100.0);
        state.highest_price_seen = 110.0;

        // With 5% trailing stop, threshold is 110 * 0.95 = 104.5
        let triggered = state.update_and_check_trailing(104.0, 0.05);

        assert!(triggered);
    }

    #[test]
    fn test_trailing_stop_not_triggered() {
        let mut state = EvaluatorState::new(100.0);
        state.highest_price_seen = 110.0;

        let triggered = state.update_and_check_trailing(105.0, 0.05);

        assert!(!triggered);
    }

    #[test]
    fn test_evaluator_state_cloneable() {
        let state = EvaluatorState::new(100.0);
        let cloned = state.clone();
        assert_eq!(state, cloned);
    }
}
