//! Leverage-specific risk guard for margin/leveraged trading.

use crate::error::RiskViolation;
use omnitrade_core::Order;

/// Risk guard for leveraged trading positions.
///
/// Validates that leveraged orders do not exceed maximum leverage
/// and maintain adequate liquidation distance cushion.
/// This guard is separate from and complementary to the standard RiskGuard.
pub struct LeverageGuard {
    /// Maximum allowed leverage multiplier (e.g., 10.0 for 10x)
    pub max_leverage: f64,
    /// Minimum required distance from entry to liquidation price (e.g., 0.05 for 5%)
    pub min_liquidation_distance: f64,
}

impl LeverageGuard {
    /// Creates a new LeverageGuard with specified limits.
    pub fn new(max_leverage: f64, min_liquidation_distance: f64) -> Self {
        Self {
            max_leverage,
            min_liquidation_distance,
        }
    }

    /// Creates a LeverageGuard for testnet trading.
    pub fn testnet() -> Self {
        Self {
            max_leverage: 10.0,
            min_liquidation_distance: 0.05,
        }
    }

    /// Creates a LeverageGuard for mainnet trading.
    pub fn mainnet() -> Self {
        Self {
            max_leverage: 5.0,
            min_liquidation_distance: 0.10,
        }
    }

    /// Checks a leveraged order against leverage limits.
    ///
    /// Validates:
    /// 1. Leverage does not exceed maximum allowed leverage
    /// 2. Liquidation distance is sufficient (>= min_liquidation_distance)
    ///
    /// Returns Ok(()) if the order passes all checks, or an error describing the violation.
    /// This method is async for compatibility with tokio-based trading systems.
    ///
    /// # Arguments
    /// * `order` - The order being checked
    /// * `leverage` - The leverage multiplier for this order (e.g., 5.0 for 5x)
    /// * `entry_price` - The price at which the position is being entered
    pub async fn check_order(
        &self,
        _order: &Order,
        leverage: f64,
        _entry_price: f64,
    ) -> Result<(), RiskViolation> {
        // Check 1: Leverage does not exceed maximum
        if leverage > self.max_leverage {
            return Err(RiskViolation::LeverageTooHigh);
        }

        // Check 2: Leverage is positive (should be >= 1.0)
        if leverage < 1.0 {
            return Err(RiskViolation::LeverageTooHigh);
        }

        // Check 3: Liquidation distance is adequate
        let liquidation_distance = self.calculate_liquidation_distance(leverage);
        if liquidation_distance < self.min_liquidation_distance {
            return Err(RiskViolation::LiquidationTooClose);
        }

        Ok(())
    }

    /// Calculates the distance from entry to liquidation price as a fraction.
    ///
    /// For isolated linear positions with leverage L the initial margin is used up
    /// after an adverse move of roughly 1/L of the entry price:
    /// distance = 1 / L
    ///
    /// Examples:
    /// - 10x leverage: 1 / 10 = 0.1 (10% adverse move)
    /// - 5x leverage: 1 / 5 = 0.2 (20% adverse move)
    /// - 1x leverage: 1 / 1 = 1.0 (no liquidation within the price range)
    fn calculate_liquidation_distance(&self, leverage: f64) -> f64 {
        1.0 / leverage
    }

    /// Validates leverage limits configuration.
    ///
    /// Returns Ok(()) if configuration is valid, or an error string if invalid.
    pub fn validate(&self) -> Result<(), String> {
        if self.max_leverage <= 1.0 {
            return Err("max_leverage must be greater than 1.0".to_string());
        }
        if self.min_liquidation_distance <= 0.0 || self.min_liquidation_distance >= 1.0 {
            return Err("min_liquidation_distance must be between 0 and 1".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use omnitrade_core::{OrderStatus, OrderType, Side, TimeInForce};

    fn create_test_order(id: u64, side: Side) -> Order {
        Order {
            id,
            symbol: "BTCUSDT".to_string(),
            side,
            order_type: OrderType::Market,
            time_in_force: TimeInForce::GTC,
            price: 0.0,
            stop_price: 0.0,
            quantity: 1.0,
            filled_quantity: 0.0,
            status: OrderStatus::New,
            created_at_ms: 1000,
            updated_at_ms: 1000,
            trailing_highest_price: None,
        }
    }

    #[tokio::test]
    async fn test_leverage_within_limit() {
        let guard = LeverageGuard::testnet();
        let order = create_test_order(1, Side::Buy);
        let result = guard.check_order(&order, 5.0, 50000.0).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_leverage_exceeds_limit() {
        let guard = LeverageGuard::testnet();
        let order = create_test_order(1, Side::Buy);
        let result = guard.check_order(&order, 15.0, 50000.0).await;
        assert!(matches!(result, Err(RiskViolation::LeverageTooHigh)));
    }

    #[tokio::test]
    async fn test_leverage_negative_rejected() {
        let guard = LeverageGuard::testnet();
        let order = create_test_order(1, Side::Buy);
        let result = guard.check_order(&order, 0.5, 50000.0).await;
        assert!(matches!(result, Err(RiskViolation::LeverageTooHigh)));
    }

    #[tokio::test]
    async fn test_liquidation_distance_sufficient() {
        let guard = LeverageGuard::testnet();
        let order = create_test_order(1, Side::Buy);
        // With 10x leverage: distance = 1 / 10 = 0.1 (10%)
        // Min requirement: 0.05 (5%)
        let result = guard.check_order(&order, 10.0, 50000.0).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_liquidation_distance_too_close() {
        let guard = LeverageGuard::new(10.0, 0.20); // 20% min distance
        let order = create_test_order(1, Side::Buy);
        // With 10x leverage: distance = 1 / 10 = 0.1 (10%)
        // Min requirement: 0.20 (20%)
        let result = guard.check_order(&order, 10.0, 50000.0).await;
        assert!(matches!(result, Err(RiskViolation::LiquidationTooClose)));
    }

    #[test]
    fn test_liquidation_distance_calculation_high_leverage() {
        let guard = LeverageGuard::testnet();
        // With 10x leverage
        let distance = guard.calculate_liquidation_distance(10.0);
        assert_eq!(distance, 0.1);
    }

    #[test]
    fn test_liquidation_distance_calculation_low_leverage() {
        let guard = LeverageGuard::testnet();
        // With 2x leverage
        let distance = guard.calculate_liquidation_distance(2.0);
        assert_eq!(distance, 0.5);
    }

    #[test]
    fn test_liquidation_distance_calculation_edge_case() {
        let guard = LeverageGuard::testnet();
        // With 1.01x leverage (barely above 1x)
        let distance = guard.calculate_liquidation_distance(1.01);
        assert!((distance - 0.990_099).abs() < 0.001);
    }

    #[test]
    fn test_validate_valid_config() {
        let guard = LeverageGuard::testnet();
        assert!(guard.validate().is_ok());
    }

    #[test]
    fn test_validate_mainnet_config() {
        let guard = LeverageGuard::mainnet();
        assert!(guard.validate().is_ok());
    }

    #[test]
    fn test_validate_invalid_max_leverage_too_low() {
        let guard = LeverageGuard::new(1.0, 0.05);
        assert!(guard.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_liquidation_distance_too_high() {
        let guard = LeverageGuard::new(10.0, 1.5);
        assert!(guard.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_liquidation_distance_zero() {
        let guard = LeverageGuard::new(10.0, 0.0);
        assert!(guard.validate().is_err());
    }
}
