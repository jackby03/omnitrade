//! Portfolio account state tracking and PnL calculation.

use std::collections::HashMap;

use omnitrade_core::{Position, Side};

use crate::error::EngineError;
use crate::fill::OrderFill;

/// Tracks overall portfolio state, including cash balance, open positions, and fees.
#[derive(Debug, Clone, PartialEq)]
pub struct AccountState {
    /// Available cash balance.
    pub balance: f64,
    /// Starting balance for PnL calculation.
    pub initial_balance: f64,
    /// Open positions mapped by symbol.
    pub positions: HashMap<String, Position>,
    /// Running fee total paid across all fills.
    pub total_fees_paid: f64,
}

impl AccountState {
    /// Constructs a new [`AccountState`] with the given initial cash balance.
    ///
    /// # Errors
    /// Returns [`EngineError::InvalidFieldValue`] if `initial_balance < 0.0` or `initial_balance.is_nan()`.
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

    /// Applies an execution fill to the account state, updating balance, positions, and fees.
    ///
    /// # Errors
    /// Returns [`EngineError::InsufficientBalance`] if available balance is inadequate.
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

        match self.positions.get_mut(&fill.symbol) {
            None => {
                self.positions.insert(
                    fill.symbol.clone(),
                    Position::new(
                        fill.symbol.clone(),
                        Side::Buy,
                        fill.fill_price,
                        fill.fill_quantity,
                    ),
                );
            }
            Some(pos) if pos.side == Side::Buy => {
                let total_notional = pos.entry_price * pos.quantity + fill.notional();
                let total_qty = pos.quantity + fill.fill_quantity;
                pos.entry_price = total_notional / total_qty;
                pos.quantity = total_qty;
            }
            Some(pos) => {
                let closed_qty = pos.quantity.min(fill.fill_quantity);
                let pnl = (pos.entry_price - fill.fill_price) * closed_qty;
                pos.realized_pnl += pnl;
                pos.quantity -= closed_qty;
                let remaining_fill_qty = fill.fill_quantity - closed_qty;

                if pos.quantity <= 1e-12 {
                    self.positions.remove(&fill.symbol);
                }
                if remaining_fill_qty > 1e-12 {
                    self.positions.insert(
                        fill.symbol.clone(),
                        Position::new(
                            fill.symbol.clone(),
                            Side::Buy,
                            fill.fill_price,
                            remaining_fill_qty,
                        ),
                    );
                }
            }
        }

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

        match self.positions.get_mut(&fill.symbol) {
            None => {
                self.positions.insert(
                    fill.symbol.clone(),
                    Position::new(
                        fill.symbol.clone(),
                        Side::Sell,
                        fill.fill_price,
                        fill.fill_quantity,
                    ),
                );
            }
            Some(pos) if pos.side == Side::Sell => {
                let total_notional = pos.entry_price * pos.quantity + fill.notional();
                let total_qty = pos.quantity + fill.fill_quantity;
                pos.entry_price = total_notional / total_qty;
                pos.quantity = total_qty;
            }
            Some(pos) => {
                let closed_qty = pos.quantity.min(fill.fill_quantity);
                let pnl = (fill.fill_price - pos.entry_price) * closed_qty;
                pos.realized_pnl += pnl;
                pos.quantity -= closed_qty;
                let remaining_fill_qty = fill.fill_quantity - closed_qty;

                if pos.quantity <= 1e-12 {
                    self.positions.remove(&fill.symbol);
                }
                if remaining_fill_qty > 1e-12 {
                    self.positions.insert(
                        fill.symbol.clone(),
                        Position::new(
                            fill.symbol.clone(),
                            Side::Sell,
                            fill.fill_price,
                            remaining_fill_qty,
                        ),
                    );
                }
            }
        }

        Ok(())
    }

    /// Updates unrealized PnL for an open position using current market price.
    ///
    /// # Errors
    /// Returns [`EngineError::SymbolNotFound`] if symbol does not exist in `positions`.
    pub fn update_unrealized_pnl(
        &mut self,
        symbol: &str,
        current_price: f64,
    ) -> Result<(), EngineError> {
        if let Some(pos) = self.positions.get_mut(symbol) {
            pos.mark_to_market(current_price);
            Ok(())
        } else {
            Err(EngineError::SymbolNotFound {
                symbol: symbol.to_string(),
            })
        }
    }

    /// Calculates total account equity (`balance + sum of unrealized_pnl of all open positions`).
    ///
    /// Note: `realized_pnl` on individual positions is already captured directly within cash `balance`
    /// upon fill execution, so it is omitted here to prevent double-counting.
    #[must_use]
    pub fn total_equity(&self) -> f64 {
        let unrealized_sum: f64 = self.positions.values().map(|p| p.unrealized_pnl).sum();
        self.balance + unrealized_sum
    }

    /// Calculates total PnL since account creation (`total_equity() - initial_balance`).
    #[must_use]
    pub fn total_pnl(&self) -> f64 {
        self.total_equity() - self.initial_balance
    }

    /// Returns a reference to an open position if present.
    #[must_use]
    pub fn get_position(&self, symbol: &str) -> Option<&Position> {
        self.positions.get(symbol)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::Side;

    #[test]
    fn test_new_valid_and_invalid_initial_balance() {
        // Arrange & Act
        let valid_account = AccountState::new(10_000.0);
        let negative_err = AccountState::new(-100.0);
        let nan_err = AccountState::new(f64::NAN);

        // Assert
        assert!(valid_account.is_ok());
        let acc = valid_account.expect("account creation failed");
        assert_eq!(acc.balance, 10_000.0);
        assert_eq!(acc.initial_balance, 10_000.0);
        assert_eq!(acc.total_fees_paid, 0.0);
        assert!(acc.positions.is_empty());

        assert_eq!(
            negative_err,
            Err(EngineError::InvalidFieldValue {
                field: "initial_balance",
                reason: "initial_balance cannot be negative or NaN",
            })
        );
        assert_eq!(
            nan_err,
            Err(EngineError::InvalidFieldValue {
                field: "initial_balance",
                reason: "initial_balance cannot be negative or NaN",
            })
        );
    }

    #[test]
    fn test_buy_fill_deducts_notional_plus_fee() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");
        let fill = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            0.1,
            10.0,
            "USDT".to_string(),
            1000,
            true,
        );

        // Act
        let result = account.apply_fill(&fill);

        // Assert
        assert!(result.is_ok());
        assert_eq!(account.balance, 10_000.0 - (5_000.0 + 10.0));
        assert_eq!(account.total_fees_paid, 10.0);
        let pos = account.get_position("BTCUSDT").expect("position missing");
        assert_eq!(pos.side, Side::Buy);
        assert_eq!(pos.entry_price, 50_000.0);
        assert_eq!(pos.quantity, 0.1);
    }

    #[test]
    fn test_sell_fill_adds_notional_minus_fee() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");
        let fill = OrderFill::new(
            1,
            "ETHUSDT".to_string(),
            Side::Sell,
            3_000.0,
            1.0,
            5.0,
            "USDT".to_string(),
            1000,
            false,
        );

        // Act
        let result = account.apply_fill(&fill);

        // Assert
        assert!(result.is_ok());
        assert_eq!(account.balance, 10_000.0 + (3_000.0 - 5.0));
        assert_eq!(account.total_fees_paid, 5.0);
        let pos = account.get_position("ETHUSDT").expect("position missing");
        assert_eq!(pos.side, Side::Sell);
        assert_eq!(pos.entry_price, 3_000.0);
        assert_eq!(pos.quantity, 1.0);
    }

    #[test]
    fn test_buy_and_sell_roundtrip_negative_pnl_due_to_fees() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");
        let buy_fill = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            0.1,
            15.0,
            "USDT".to_string(),
            1000,
            true,
        );
        let sell_fill = OrderFill::new(
            2,
            "BTCUSDT".to_string(),
            Side::Sell,
            50_000.0,
            0.1,
            15.0,
            "USDT".to_string(),
            2000,
            false,
        );

        // Act
        account.apply_fill(&buy_fill).expect("buy fill failed");
        account.apply_fill(&sell_fill).expect("sell fill failed");

        // Assert
        assert_eq!(account.total_fees_paid, 30.0);
        assert_eq!(account.balance, 10_000.0 - 30.0);
        assert_eq!(account.total_equity(), 9_970.0);
        assert_eq!(account.total_pnl(), -30.0);
        assert!(account.get_position("BTCUSDT").is_none());
    }

    #[test]
    fn test_insufficient_balance_error() {
        // Arrange
        let mut account = AccountState::new(100.0).expect("failed to create account");
        let fill = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            0.1,
            5.0,
            "USDT".to_string(),
            1000,
            true,
        );

        // Act
        let result = account.apply_fill(&fill);

        // Assert
        assert_eq!(
            result,
            Err(EngineError::InsufficientBalance {
                required: 5_005.0,
                available: 100.0,
            })
        );
        assert_eq!(account.balance, 100.0);
    }

    #[test]
    fn test_total_equity_includes_unrealized_pnl() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");
        let fill = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            0.1,
            0.0,
            "USDT".to_string(),
            1000,
            true,
        );
        account.apply_fill(&fill).expect("buy fill failed");

        // Act
        let update_res = account.update_unrealized_pnl("BTCUSDT", 55_000.0);

        // Assert
        assert!(update_res.is_ok());
        let pos = account.get_position("BTCUSDT").expect("position missing");
        assert_eq!(pos.unrealized_pnl, 500.0);
        assert_eq!(account.balance, 5_000.0);
        assert_eq!(account.total_equity(), 5_500.0);
        assert_eq!(account.total_pnl(), -4_500.0);
    }

    #[test]
    fn test_position_removed_when_fully_closed() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");
        let buy_fill = OrderFill::new(
            1,
            "BTCUSDT".to_string(),
            Side::Buy,
            50_000.0,
            0.2,
            0.0,
            "USDT".to_string(),
            1000,
            true,
        );
        let partial_sell = OrderFill::new(
            2,
            "BTCUSDT".to_string(),
            Side::Sell,
            55_000.0,
            0.1,
            0.0,
            "USDT".to_string(),
            2000,
            false,
        );
        let final_sell = OrderFill::new(
            3,
            "BTCUSDT".to_string(),
            Side::Sell,
            55_000.0,
            0.1,
            0.0,
            "USDT".to_string(),
            3000,
            false,
        );

        // Act & Assert 1: Open position
        account.apply_fill(&buy_fill).expect("buy fill failed");
        assert_eq!(
            account
                .get_position("BTCUSDT")
                .expect("position missing")
                .quantity,
            0.2
        );

        // Act & Assert 2: Partial close
        account
            .apply_fill(&partial_sell)
            .expect("partial sell failed");
        assert_eq!(
            account
                .get_position("BTCUSDT")
                .expect("position missing")
                .quantity,
            0.1
        );

        // Act & Assert 3: Final close -> position removed
        account.apply_fill(&final_sell).expect("final sell failed");
        assert!(account.get_position("BTCUSDT").is_none());
    }

    #[test]
    fn test_update_unrealized_pnl_symbol_not_found() {
        // Arrange
        let mut account = AccountState::new(10_000.0).expect("failed to create account");

        // Act
        let res = account.update_unrealized_pnl("UNKNOWN", 100.0);

        // Assert
        assert_eq!(
            res,
            Err(EngineError::SymbolNotFound {
                symbol: "UNKNOWN".to_string()
            })
        );
    }
}
