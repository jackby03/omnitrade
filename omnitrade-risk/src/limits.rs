//! Risk limits configuration.

use serde::{Deserialize, Serialize};

/// Risk management limits for trading.
///
/// Defines the boundaries within which trading operations are permitted.
/// All limits are enforced regardless of market environment (testnet or mainnet).
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct RiskLimits {
    /// Maximum position size (in quote currency, e.g., USDT)
    ///
    /// Any single order that would result in a position larger than this value
    /// will be rejected.
    pub max_position_size: f64,

    /// Maximum acceptable drawdown as a percentage (0-100)
    ///
    /// When current account drawdown exceeds this percentage, no new orders are permitted.
    /// Drawdown is calculated as: (peak_equity - current_equity) / peak_equity * 100
    pub max_drawdown_pct: f64,

    /// Maximum daily loss limit (in quote currency)
    ///
    /// Accumulated losses within a calendar day cannot exceed this amount.
    pub max_daily_loss: f64,
}

impl RiskLimits {
    /// Creates a new RiskLimits configuration.
    pub fn new(max_position_size: f64, max_drawdown_pct: f64, max_daily_loss: f64) -> Self {
        Self {
            max_position_size,
            max_drawdown_pct,
            max_daily_loss,
        }
    }

    /// Creates conservative testnet limits for development.
    pub fn testnet() -> Self {
        Self {
            max_position_size: 10000.0, // 10k USDT per position
            max_drawdown_pct: 25.0,     // 25% max drawdown
            max_daily_loss: 2000.0,     // 2k USDT daily loss limit
        }
    }

    /// Creates very conservative mainnet limits for production.
    pub fn mainnet() -> Self {
        Self {
            max_position_size: 5000.0, // 5k USDT per position
            max_drawdown_pct: 10.0,    // 10% max drawdown
            max_daily_loss: 1000.0,    // 1k USDT daily loss limit
        }
    }

    /// Validates that limits are reasonable (positive values).
    pub fn validate(&self) -> Result<(), String> {
        if self.max_position_size <= 0.0 {
            return Err("max_position_size must be positive".to_string());
        }
        if self.max_drawdown_pct <= 0.0 || self.max_drawdown_pct > 100.0 {
            return Err("max_drawdown_pct must be between 0 and 100".to_string());
        }
        if self.max_daily_loss <= 0.0 {
            return Err("max_daily_loss must be positive".to_string());
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_risk_limits_new() {
        let limits = RiskLimits::new(10000.0, 20.0, 2000.0);
        assert_eq!(limits.max_position_size, 10000.0);
        assert_eq!(limits.max_drawdown_pct, 20.0);
        assert_eq!(limits.max_daily_loss, 2000.0);
    }

    #[test]
    fn test_testnet_limits() {
        let limits = RiskLimits::testnet();
        assert_eq!(limits.max_position_size, 10000.0);
        assert_eq!(limits.max_drawdown_pct, 25.0);
        assert_eq!(limits.max_daily_loss, 2000.0);
    }

    #[test]
    fn test_mainnet_limits() {
        let limits = RiskLimits::mainnet();
        assert_eq!(limits.max_position_size, 5000.0);
        assert_eq!(limits.max_drawdown_pct, 10.0);
        assert_eq!(limits.max_daily_loss, 1000.0);
    }

    #[test]
    fn test_validate_positive_values() {
        let limits = RiskLimits::new(10000.0, 20.0, 2000.0);
        assert!(limits.validate().is_ok());
    }

    #[test]
    fn test_validate_zero_position_size() {
        let limits = RiskLimits::new(0.0, 20.0, 2000.0);
        assert!(limits.validate().is_err());
    }

    #[test]
    fn test_validate_invalid_drawdown_pct() {
        let limits = RiskLimits::new(10000.0, 150.0, 2000.0);
        assert!(limits.validate().is_err());
    }
}
