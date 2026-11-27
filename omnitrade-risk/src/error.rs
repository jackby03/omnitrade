//! Risk management error types.

use thiserror::Error;

/// Errors that can occur during risk management operations.
#[derive(Debug, Error)]
pub enum RiskError {
    /// Configuration error
    #[error("risk configuration error: {0}")]
    Configuration(String),

    /// Serialization/deserialization error
    #[error("serialization error: {0}")]
    Serialization(String),

    /// Internal system error
    #[error("internal risk system error: {0}")]
    Internal(String),
}

/// Risk violations that prevent order execution.
///
/// Returned as `Result<(), RiskViolation>` from risk checks.
#[derive(Debug, Clone, Error)]
pub enum RiskViolation {
    /// Order quantity exceeds maximum position size limit
    #[error("position size violation: order size {order_size} exceeds limit {limit}")]
    PositionSizeExceeded { order_size: f64, limit: f64 },

    /// Account drawdown exceeds maximum allowed threshold
    #[error("drawdown violation: current {current_pct:.2}% exceeds limit {limit_pct:.2}%")]
    DrawdownExceeded { current_pct: f64, limit_pct: f64 },

    /// Daily loss limit has been exceeded
    #[error("daily loss violation: current loss ${loss:.2} exceeds limit ${limit:.2}")]
    DailyLossExceeded { loss: f64, limit: f64 },

    /// Kill switch has been activated
    #[error("trading halted: kill switch activated")]
    KillSwitchActive,

    /// Leverage exceeds maximum allowed limit
    #[error("leverage violation: requested leverage exceeds maximum")]
    LeverageTooHigh,

    /// Liquidation price is too close to entry price
    #[error("liquidation distance violation: distance to liquidation is below minimum required")]
    LiquidationTooClose,

    /// Generic risk violation
    #[error("risk violation: {0}")]
    Other(String),
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_position_size_violation_display() {
        let violation = RiskViolation::PositionSizeExceeded {
            order_size: 1000.0,
            limit: 500.0,
        };
        assert!(violation.to_string().contains("position size violation"));
        assert!(violation.to_string().contains("1000"));
    }

    #[test]
    fn test_drawdown_violation_display() {
        let violation = RiskViolation::DrawdownExceeded {
            current_pct: 25.5,
            limit_pct: 20.0,
        };
        assert!(violation.to_string().contains("drawdown violation"));
    }

    #[test]
    fn test_kill_switch_violation() {
        let violation = RiskViolation::KillSwitchActive;
        assert!(violation.to_string().contains("kill switch"));
    }

    #[test]
    fn test_leverage_too_high_display() {
        let violation = RiskViolation::LeverageTooHigh;
        assert!(violation.to_string().contains("leverage"));
        assert!(violation.to_string().contains("exceeds"));
    }

    #[test]
    fn test_liquidation_too_close_display() {
        let violation = RiskViolation::LiquidationTooClose;
        assert!(violation.to_string().contains("liquidation"));
        assert!(violation.to_string().contains("distance"));
    }
}
