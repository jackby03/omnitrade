//! Portfolio account state tracking with simultaneous long + short support.
//! PHASE 5 BREAKING CHANGE: HashMap<(String, Side), Position> enables hedging.

use std::collections::HashMap;

use omnitrade_core::{Position, Side};

use crate::error::EngineError;
use crate::fill::OrderFill;

/// Position key: (symbol, side) tuple enables long + short simultaneously.
type PositionKey = (String, Side);

/// Tracks portfolio state: cash balance, long/short positions, and fees.
/// **BREAKING CHANGE**: `positions` now uses `HashMap<(String, Side), Position>`.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountState {
    /// Available cash balance.
    pub balance: f64,
    /// Starting balance for PnL calculation.
    pub initial_balance: f64,
    /// Open positions by (symbol, side). Supports Long and Short simultaneously.
    pub positions: HashMap<PositionKey, Position>,
    /// Running fee total paid across all fills.
    pub total_fees_paid: f64,
}

impl AccountState {
    /// Constructs a new [`AccountState`]. Errors if `initial_balance < 0.0` or NaN.
    pub fn new(initial_balance: f64) -> Result<Self, EngineError> {
        if initial_balance < 0.0 || initial_balance.is_nan() {
            return Err(EngineError::InvalidFieldValue {
                field: "initial_balance",
                reason: "initial_balance cannot be negative or NaN",
            });
        }
        Ok(Self {
            balance: initial_balance,
            initial_balance,
            positions: HashMap::new(),
            total_fees_paid: 0.0,
        })
    }

    /// Applies an execution fill. Buy -> Long position, Sell -> Short position.
    /// Errors if balance is inadequate.
    pub fn apply_fill(&mut self, fill: &OrderFill) -> Result<(), EngineError> {
        match fill.side {
            Side::Buy => self.apply_buy_fill(fill),
            Side::Sell => self.apply_sell_fill(fill),
        }
    }

    fn apply_buy_fill(&mut self, fill: &OrderFill) -> Result<(), EngineError> {
        let required = fill.notional() + fill.fee;
        if self.balance < required {
            return Err(EngineError::InsufficientBalance {
                required,
                available: self.balance,
            });
        }
        self.balance -= required;
        self.total_fees_paid += fill.fee;

        let remaining = self.offset_opposite(fill, Side::Sell);
        if remaining <= 0.0 {
            return Ok(());
        }
        let notional = remaining * fill.fill_price;
        let key = (fill.symbol.clone(), Side::Buy);
        self.positions
            .entry(key)
            .and_modify(|pos| {
                let total_notional = pos.entry_price * pos.quantity + notional;
                let total_qty = pos.quantity + remaining;
                pos.entry_price = total_notional / total_qty;
                pos.quantity = total_qty;
            })
            .or_insert_with(|| {
                Position::new(fill.symbol.clone(), Side::Buy, fill.fill_price, remaining)
            });
        Ok(())
    }

    fn apply_sell_fill(&mut self, fill: &OrderFill) -> Result<(), EngineError> {
        let net_cash = fill.notional() - fill.fee;
        if self.balance + net_cash < 0.0 {
            return Err(EngineError::InsufficientBalance {
                required: (fill.fee - fill.notional()).max(0.0),
                available: self.balance,
            });
        }
        self.balance += net_cash;
        self.total_fees_paid += fill.fee;

        let remaining = self.offset_opposite(fill, Side::Buy);
        if remaining <= 0.0 {
            return Ok(());
        }
        let notional = remaining * fill.fill_price;
        let key = (fill.symbol.clone(), Side::Sell);
        self.positions
            .entry(key)
            .and_modify(|pos| {
                let total_notional = pos.entry_price * pos.quantity + notional;
                let total_qty = pos.quantity + remaining;
                pos.entry_price = total_notional / total_qty;
                pos.quantity = total_qty;
            })
            .or_insert_with(|| {
                Position::new(fill.symbol.clone(), Side::Sell, fill.fill_price, remaining)
            });
        Ok(())
    }

    /// Verrechnet einen Fill zuerst mit der Gegenposition und gibt die Restmenge zurück,
    /// die eine Position auf der Seite des Fills eröffnet. Cash wurde bereits gebucht.
    fn offset_opposite(&mut self, fill: &OrderFill, opposite: Side) -> f64 {
        let key = (fill.symbol.clone(), opposite);
        let Some(pos) = self.positions.get_mut(&key) else {
            return fill.fill_quantity;
        };
        let closed = pos.quantity.min(fill.fill_quantity);
        pos.quantity -= closed;
        if pos.quantity <= f64::EPSILON {
            self.positions.remove(&key);
        }
        fill.fill_quantity - closed
    }

    /// Closes an open position at `price`: realizes PnL, books the proceeds
    /// (minus `fee_rate`-derived fees) into `balance` and removes the position.
    /// Returns the realized PnL; `0.0` if no position of that side was open.
    pub fn close_position_at(
        &mut self,
        symbol: &str,
        side: Side,
        price: f64,
        fee_rate: f64,
    ) -> Result<f64, EngineError> {
        let key = (symbol.to_string(), side);
        let Some(pos) = self.positions.get(&key) else {
            return Ok(0.0);
        };
        let qty = pos.quantity;
        let notional = qty * price;
        let fee = notional * fee_rate;
        let net_cash = match side {
            // Closing a long sells inventory: proceeds come in.
            Side::Buy => notional - fee,
            // Closing a short buys it back: cost goes out.
            Side::Sell => -notional - fee,
        };
        if self.balance + net_cash < 0.0 {
            return Err(EngineError::InsufficientBalance {
                required: -net_cash,
                available: self.balance,
            });
        }
        let pnl = match side {
            Side::Buy => (price - pos.entry_price) * qty,
            Side::Sell => (pos.entry_price - price) * qty,
        };
        self.balance += net_cash;
        self.total_fees_paid += fee;
        self.positions.remove(&key);
        Ok(pnl)
    }

    /// Returns a reference to a specific position (symbol, side).
    #[must_use]
    pub fn get_position(&self, symbol: &str, side: Side) -> Option<&Position> {
        self.positions.get(&(symbol.to_string(), side))
    }

    /// Returns all open positions for a symbol (both long and short).
    #[must_use]
    pub fn get_all_positions_for_symbol(&self, symbol: &str) -> Vec<&Position> {
        self.positions
            .iter()
            .filter_map(
                |((sym, _), pos)| {
                    if sym == symbol {
                        Some(pos)
                    } else {
                        None
                    }
                },
            )
            .collect()
    }

    /// Closes a position. Returns error if position doesn't exist.
    pub fn close_position(&mut self, symbol: &str, side: Side) -> Result<(), EngineError> {
        let key = (symbol.to_string(), side);
        if self.positions.remove(&key).is_none() {
            return Err(EngineError::SymbolNotFound {
                symbol: symbol.to_string(),
            });
        }
        Ok(())
    }

    /// Updates unrealized PnL for a position at current market price. Errors if not found.
    pub fn update_unrealized_pnl(
        &mut self,
        symbol: &str,
        side: Side,
        current_price: f64,
    ) -> Result<(), EngineError> {
        let key = (symbol.to_string(), side);
        if let Some(pos) = self.positions.get_mut(&key) {
            pos.mark_to_market(current_price);
            Ok(())
        } else {
            Err(EngineError::SymbolNotFound {
                symbol: symbol.to_string(),
            })
        }
    }

    /// Calculates total account equity: cash plus the marked-to-market value of all
    /// positions.
    ///
    /// `balance` is real cash (a buy pays the full price, a sell credits the proceeds),
    /// so a long adds its position value (`entry * qty + unrealized PnL`) and a short
    /// subtracts the buy-back liability (`entry * qty - unrealized PnL`).
    #[must_use]
    pub fn total_equity(&self) -> f64 {
        let positions_value: f64 = self
            .positions
            .values()
            .map(|p| match p.side {
                Side::Buy => p.notional_value() + p.unrealized_pnl,
                Side::Sell => -p.notional_value() + p.unrealized_pnl,
            })
            .sum();
        self.balance + positions_value
    }

    /// Calculates total PnL since account creation.
    #[must_use]
    pub fn total_pnl(&self) -> f64 {
        self.total_equity() - self.initial_balance
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::fill::OrderFill;

    #[test]
    fn test_phase5_sell_offsets_long_and_leaves_short() {
        let mut acc = AccountState::new(200_000.0).expect("account");
        let buy = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            1.0,
            0.0,
            "USDT".to_string(),
            1000,
            true,
        );
        let sell = OrderFill::new(
            2,
            "BTCUSDT".to_string(),
            Side::Sell,
            51_000.0,
            2.0,
            0.0,
            "USDT".to_string(),
            2000,
            false,
        );
        acc.apply_fill(&buy).expect("buy");
        acc.apply_fill(&sell).expect("sell");
        // Sell (2.0) verrechnet zuerst das Long (1.0); übrig bleibt ein Short über 1.0.
        assert!(acc.get_position("BTCUSDT", Side::Buy).is_none());
        let short = acc.get_position("BTCUSDT", Side::Sell).expect("short");
        assert!((short.quantity - 1.0).abs() < 1e-9);
        assert_eq!(acc.get_all_positions_for_symbol("BTCUSDT").len(), 1);
    }

    fn fill(id: u64, side: Side, price: f64, qty: f64) -> OrderFill {
        OrderFill::new(
            id,
            "BTCUSDT".to_string(),
            side,
            price,
            qty,
            0.0,
            "USDT".to_string(),
            id,
            false,
        )
    }

    #[test]
    fn test_sell_closes_long_without_phantom_short() {
        let mut acc = AccountState::new(100_000.0).expect("account");
        acc.apply_fill(&fill(1, Side::Buy, 100.0, 2.0))
            .expect("buy");
        acc.apply_fill(&fill(2, Side::Sell, 110.0, 2.0))
            .expect("sell");
        assert!(acc.positions.is_empty());
        assert!((acc.total_pnl() - 20.0).abs() < 1e-9);
    }

    #[test]
    fn test_buy_closes_short_partially_and_flips_remainder() {
        let mut acc = AccountState::new(100_000.0).expect("account");
        acc.apply_fill(&fill(1, Side::Sell, 100.0, 2.0))
            .expect("short");
        acc.apply_fill(&fill(2, Side::Buy, 90.0, 3.0)).expect("buy");
        assert!(acc.get_position("BTCUSDT", Side::Sell).is_none());
        let long = acc.get_position("BTCUSDT", Side::Buy).expect("long");
        assert!((long.quantity - 1.0).abs() < 1e-9);
        assert!((long.entry_price - 90.0).abs() < 1e-9);
    }

    fn unit_fill(side: Side, price: f64, qty: f64) -> OrderFill {
        OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            side,
            price,
            qty,
            0.0,
            "USDT".to_string(),
            1000,
            true,
        )
    }

    #[test]
    fn test_equity_of_open_long_includes_position_value() {
        // Arrange: 0.1 BTC @ 50_000 costs 5_000 cash, equity must stay 10_000.
        let mut acc = AccountState::new(10_000.0).expect("account");
        acc.apply_fill(&unit_fill(Side::Buy, 50_000.0, 0.1))
            .expect("buy");

        // Act & Assert
        assert!((acc.balance - 5_000.0).abs() < 1e-9);
        assert!((acc.total_equity() - 10_000.0).abs() < 1e-9);
        acc.update_unrealized_pnl("BTCUSDT", Side::Buy, 51_000.0)
            .expect("mtm");
        assert!((acc.total_equity() - 10_100.0).abs() < 1e-9);
        assert!((acc.total_pnl() - 100.0).abs() < 1e-9);
    }

    #[test]
    fn test_equity_of_open_short_excludes_sale_proceeds() {
        // Arrange: shorting 0.1 BTC @ 50_000 credits 5_000 cash but owes the coins back.
        let mut acc = AccountState::new(10_000.0).expect("account");
        acc.apply_fill(&unit_fill(Side::Sell, 50_000.0, 0.1))
            .expect("sell");

        // Act & Assert
        assert!((acc.balance - 15_000.0).abs() < 1e-9);
        assert!((acc.total_equity() - 10_000.0).abs() < 1e-9);
        acc.update_unrealized_pnl("BTCUSDT", Side::Sell, 49_000.0)
            .expect("mtm");
        assert!((acc.total_equity() - 10_100.0).abs() < 1e-9);
        acc.update_unrealized_pnl("BTCUSDT", Side::Sell, 52_000.0)
            .expect("mtm");
        assert!((acc.total_equity() - 9_800.0).abs() < 1e-9);
    }

    #[test]
    fn test_equity_equals_cash_after_flat_round_trip() {
        // Arrange
        let mut acc = AccountState::new(10_000.0).expect("account");
        acc.apply_fill(&unit_fill(Side::Buy, 50_000.0, 0.1))
            .expect("buy");
        acc.apply_fill(&unit_fill(Side::Sell, 52_000.0, 0.1))
            .expect("sell");

        // Act & Assert
        assert!(acc.positions.is_empty());
        assert!((acc.total_equity() - acc.balance).abs() < 1e-9);
        assert!((acc.total_pnl() - 200.0).abs() < 1e-9);
    }

    /// T-287/T-292: das Guthaben allein ist nach einem Kauf fast leer - die
    /// Equity darf trotzdem nicht einbrechen, solange die Position steht.
    #[test]
    fn test_equity_haelt_den_positionswert() {
        let mut acc = AccountState::new(100_000.0).expect("account");
        let buy = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            1.0,
            0.0,
            "USDT".to_string(),
            1000,
            false,
        );
        acc.apply_fill(&buy).expect("buy");
        assert_eq!(acc.balance, 50_000.0, "Nennwert ist vom Guthaben weg");
        assert_eq!(
            acc.total_equity(),
            100_000.0,
            "Equity unveraendert beim Kauf"
        );

        // Kurs faellt 10 %: Equity folgt dem Marktwert, nicht dem Guthaben.
        acc.update_unrealized_pnl("BTCUSDT", Side::Buy, 45_000.0)
            .expect("mark");
        assert_eq!(acc.total_equity(), 95_000.0);
        assert_eq!(acc.total_pnl(), -5_000.0);
    }

    /// Ein Short ist ein Mittelzufluss im Guthaben - der Gegenwert muss als
    /// NEGATIVER Positionswert gegengerechnet werden.
    #[test]
    fn test_equity_bei_short_zieht_den_positionswert_ab() {
        let mut acc = AccountState::new(100_000.0).expect("account");
        let sell = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Sell,
            50_000.0,
            1.0,
            0.0,
            "USDT".to_string(),
            1000,
            false,
        );
        acc.apply_fill(&sell).expect("sell");
        assert_eq!(acc.balance, 150_000.0, "Short bucht den Nennwert zu");
        assert_eq!(
            acc.total_equity(),
            100_000.0,
            "Equity unveraendert beim Short"
        );

        acc.update_unrealized_pnl("BTCUSDT", Side::Sell, 55_000.0)
            .expect("mark");
        assert_eq!(
            acc.total_equity(),
            95_000.0,
            "Short verliert, wenn der Kurs steigt"
        );
    }
}
