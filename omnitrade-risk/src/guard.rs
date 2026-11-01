//! Risk guard for validating orders before execution.

use crate::error::RiskViolation;
use crate::limits::RiskLimits;
use omnitrade_core::Order;

/// Minimal account state interface for risk checking.
pub trait AccountInfo {
    /// Total account equity (balance + unrealized PnL)
    fn total_equity(&self) -> f64;

    /// Total realized and unrealized PnL
    fn total_pnl(&self) -> f64;

    /// Initial account balance
    fn initial_balance(&self) -> f64;

    /// Equity, an der der Drawdown gemessen wird. Vorgabe: die Konto-Equity.
    ///
    /// Ein Live-Lauf auf einem GETEILTEN Konto ueberschreibt das mit der Equity
    /// seines eigenen Kapitals: die Konto-Equity bewegt sich durch fremde
    /// Spuren, Ein- und Auszahlungen (31.10.2026: 140 -> 288 -> 246 USD bei
    /// 50 USD Laufkapital) und sperrt sonst jeden Auftrag.
    fn drawdown_equity(&self) -> f64 {
        self.total_equity()
    }
}

/// Validates orders against risk limits before execution.
///
/// Enforces position size, drawdown, and daily loss limits on all orders.
/// All checks are performed regardless of environment (testnet or mainnet).
pub struct RiskGuard {
    limits: RiskLimits,
    peak_equity: f64,
    daily_loss: f64,
    /// Opt-in: measure the loss limit against the equity seen at the first check
    /// instead of `AccountInfo::initial_balance`.
    track_run_baseline: bool,
    run_baseline: Option<f64>,
}

impl RiskGuard {
    /// Creates a new RiskGuard with the specified limits.
    pub fn new(limits: RiskLimits) -> Self {
        Self {
            limits,
            peak_equity: 0.0,
            daily_loss: 0.0,
            track_run_baseline: false,
            run_baseline: None,
        }
    }

    /// Measures the loss limit (`max_daily_loss`) against the account equity at the
    /// **first checked order** of this guard instead of the declared initial balance.
    ///
    /// Live trading needs this: the declared starting capital is a user input and says
    /// nothing about what the account really holds. If the account holds far more, the
    /// loss `initial - equity` stays negative and the limit never triggers.
    #[must_use]
    pub fn with_run_baseline(mut self) -> Self {
        self.track_run_baseline = true;
        self
    }

    /// Equity recorded at the first check, if run-baseline tracking is enabled.
    pub fn run_baseline(&self) -> Option<f64> {
        self.run_baseline
    }

    /// Creates a RiskGuard for testnet trading.
    pub fn testnet() -> Self {
        Self::new(RiskLimits::testnet())
    }

    /// Creates a RiskGuard for mainnet trading.
    pub fn mainnet() -> Self {
        Self::new(RiskLimits::mainnet())
    }

    /// Checks an order against all risk limits.
    ///
    /// Returns Ok(()) if the order passes all checks, or an error describing the violation.
    /// Automatically tracks peak equity for drawdown calculation.
    pub fn check_order(
        &mut self,
        order: &Order,
        account: &dyn AccountInfo,
        current_price: f64,
    ) -> Result<(), RiskViolation> {
        // Update peak equity tracking (Drawdown-Equity, siehe AccountInfo)
        let current_equity = account.total_equity();
        let dd_equity = account.drawdown_equity();
        if dd_equity > self.peak_equity {
            self.peak_equity = dd_equity;
        }
        if self.track_run_baseline && self.run_baseline.is_none() {
            self.run_baseline = Some(current_equity);
        }

        // Check position size limit
        self.check_position_size(order, current_price)?;

        // Check drawdown limit
        self.check_drawdown(dd_equity)?;

        // Check daily loss limit (simplified: using current PnL as daily loss)
        self.check_daily_loss(account)?;

        Ok(())
    }

    /// Validates order size against maximum position limit.
    fn check_position_size(&self, order: &Order, current_price: f64) -> Result<(), RiskViolation> {
        let order_size = order.quantity * current_price;
        if order_size > self.limits.max_position_size {
            return Err(RiskViolation::PositionSizeExceeded {
                order_size,
                limit: self.limits.max_position_size,
            });
        }
        Ok(())
    }

    /// Validates drawdown against maximum allowed threshold.
    fn check_drawdown(&self, current_equity: f64) -> Result<(), RiskViolation> {
        if self.peak_equity <= 0.0 {
            return Ok(());
        }

        let drawdown = ((self.peak_equity - current_equity) / self.peak_equity) * 100.0;
        if drawdown > self.limits.max_drawdown_pct {
            return Err(RiskViolation::DrawdownExceeded {
                current_pct: drawdown,
                limit_pct: self.limits.max_drawdown_pct,
            });
        }
        Ok(())
    }

    /// Validates daily loss against limit.
    fn check_daily_loss(&self, account: &dyn AccountInfo) -> Result<(), RiskViolation> {
        // Nur Verluste zählen; Gewinne (negativer Wert) dürfen nicht blockieren.
        let reference = self
            .run_baseline
            .unwrap_or_else(|| account.initial_balance());
        let current_loss = reference - account.total_equity();
        if current_loss > self.limits.max_daily_loss {
            return Err(RiskViolation::DailyLossExceeded {
                loss: current_loss,
                limit: self.limits.max_daily_loss,
            });
        }
        Ok(())
    }

    /// Resets daily loss tracking (call once per day).
    pub fn reset_daily_loss(&mut self) {
        self.daily_loss = 0.0;
    }

    /// Gets the current peak equity (for testing/monitoring).
    pub fn peak_equity(&self) -> f64 {
        self.peak_equity
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::{OrderStatus, OrderType, Side, TimeInForce};

    struct MockAccount {
        balance: f64,
        initial_balance: f64,
        total_pnl: f64,
    }

    impl AccountInfo for MockAccount {
        fn total_equity(&self) -> f64 {
            self.balance
        }

        fn total_pnl(&self) -> f64 {
            self.total_pnl
        }

        fn initial_balance(&self) -> f64 {
            self.initial_balance
        }
    }

    fn create_test_order(id: u64, quantity: f64) -> Order {
        Order {
            id,
            symbol: "BTCUSDT".to_string(),
            side: Side::Buy,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::GTC,
            price: 0.0,
            stop_price: 0.0,
            quantity,
            filled_quantity: 0.0,
            status: OrderStatus::New,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            trailing_highest_price: None,
        }
    }

    fn create_mock_account(balance: f64, initial: f64, pnl: f64) -> MockAccount {
        MockAccount {
            balance,
            initial_balance: initial,
            total_pnl: pnl,
        }
    }

    #[test]
    fn test_position_size_within_limit() {
        let mut guard = RiskGuard::testnet();
        let account = create_mock_account(10000.0, 10000.0, 0.0);
        let order = create_test_order(1, 1.0);
        // Price: 40000, order_size: 40000 < 10000 limit should fail
        let result = guard.check_order(&order, &account, 40000.0);
        assert!(matches!(
            result,
            Err(RiskViolation::PositionSizeExceeded { .. })
        ));
    }

    #[test]
    fn test_position_size_exceeds_limit() {
        let mut guard = RiskGuard::testnet();
        let account = create_mock_account(10000.0, 10000.0, 0.0);
        let order = create_test_order(2, 1.0);
        // Price: 50000, order_size: 50000 > 10000 limit should fail
        let result = guard.check_order(&order, &account, 50000.0);
        assert!(matches!(
            result,
            Err(RiskViolation::PositionSizeExceeded { .. })
        ));
    }

    #[test]
    fn test_drawdown_within_limit() {
        let mut guard = RiskGuard::testnet();
        let account = create_mock_account(8500.0, 10000.0, -1500.0);
        guard.peak_equity = 10000.0;
        let order = create_test_order(3, 1.0);
        let result = guard.check_order(&order, &account, 3000.0);
        assert!(result.is_ok());
    }

    #[test]
    fn test_profit_does_not_count_as_daily_loss() {
        let mut guard = RiskGuard::testnet();
        // Equity 20000 über Startkapital 10000: Gewinn von 10000 darf nicht blockieren.
        let account = create_mock_account(20000.0, 10000.0, 10000.0);
        guard.peak_equity = 20000.0;
        let order = create_test_order(5, 1.0);
        let result = guard.check_order(&order, &account, 3000.0);
        assert!(result.is_ok(), "Gewinn blockierte: {result:?}");
    }

    #[test]
    fn test_loss_above_daily_limit_is_rejected() {
        let mut guard = RiskGuard::testnet();
        // Verlust 2500 > max_daily_loss 2000 (Drawdown 25 % bei Peak 10000 grenzwertig, daher Peak = Equity).
        let account = create_mock_account(7500.0, 10000.0, -2500.0);
        guard.peak_equity = 7500.0;
        let order = create_test_order(6, 1.0);
        let result = guard.check_order(&order, &account, 3000.0);
        assert!(matches!(
            result,
            Err(RiskViolation::DailyLossExceeded { .. })
        ));
    }

    #[test]
    fn test_drawdown_exceeds_limit() {
        let mut guard = RiskGuard::testnet();
        let account = create_mock_account(7000.0, 10000.0, -3000.0);
        guard.peak_equity = 10000.0;
        let order = create_test_order(4, 1.0);
        let result = guard.check_order(&order, &account, 3000.0);
        assert!(matches!(
            result,
            Err(RiskViolation::DrawdownExceeded { .. })
        ));
    }

    #[test]
    fn test_peak_equity_tracking() {
        let mut guard = RiskGuard::testnet();
        assert_eq!(guard.peak_equity(), 0.0);
        let account = create_mock_account(15000.0, 15000.0, 0.0);
        let order = create_test_order(5, 0.1);
        let _ = guard.check_order(&order, &account, 50000.0);
        assert_eq!(guard.peak_equity(), 15000.0);
    }

    #[test]
    fn test_run_baseline_makes_loss_limit_independent_of_declared_initial_balance() {
        // Arrange: Konto hält 93_000, deklariertes Startkapital war nur 10_000.
        let order = create_test_order(7, 1.0);
        let start = create_mock_account(93_000.0, 10_000.0, 83_000.0);
        let later = create_mock_account(90_000.0, 10_000.0, 80_000.0);

        // Act & Assert: ohne Basis löst ein Verlust von 3_000 nie aus (initial − equity < 0) …
        let mut plain = RiskGuard::testnet();
        plain.check_order(&order, &start, 3000.0).expect("start");
        assert!(plain.check_order(&order, &later, 3000.0).is_ok());

        // … mit Laufbeginn-Basis schon (3_000 > max_daily_loss 2_000).
        let mut guard = RiskGuard::testnet().with_run_baseline();
        guard.check_order(&order, &start, 3000.0).expect("start");
        assert_eq!(guard.run_baseline(), Some(93_000.0));
        assert!(matches!(
            guard.check_order(&order, &later, 3000.0),
            Err(RiskViolation::DailyLossExceeded { .. })
        ));
    }

    #[test]
    fn test_run_baseline_is_set_once_and_gains_do_not_block() {
        // Arrange
        let order = create_test_order(8, 1.0);
        let mut guard = RiskGuard::testnet().with_run_baseline();
        assert_eq!(guard.run_baseline(), None);

        // Act
        guard
            .check_order(&order, &create_mock_account(10_000.0, 1.0, 0.0), 3000.0)
            .expect("first");
        let gain = guard.check_order(&order, &create_mock_account(15_000.0, 1.0, 0.0), 3000.0);

        // Assert
        assert!(gain.is_ok(), "{gain:?}");
        assert_eq!(
            guard.run_baseline(),
            Some(10_000.0),
            "baseline must not move"
        );
    }

    #[test]
    fn test_default_guard_has_no_run_baseline() {
        let mut guard = RiskGuard::testnet();
        let order = create_test_order(9, 1.0);
        guard
            .check_order(&order, &create_mock_account(1_000.0, 1_000.0, 0.0), 10.0)
            .expect("ok");
        assert_eq!(guard.run_baseline(), None);
    }
}
