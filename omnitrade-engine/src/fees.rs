//! Fee schedule calculation and slippage modeling for engine execution.

use omnitrade_core::Side;

use crate::EngineError;

/// Fee schedule for trading pairs, specifying maker and taker fee rates.
#[derive(Debug, Clone, PartialEq)]
pub struct FeeSchedule {
    /// Maker fee rate as a fraction (e.g., 0.001 for 0.1%).
    pub maker_rate: f64,
    /// Taker fee rate as a fraction (e.g., 0.001 for 0.1%).
    pub taker_rate: f64,
    /// Currency asset in which fees are settled (e.g., "USDT").
    pub fee_asset: String,
}

impl FeeSchedule {
    /// Creates a new `FeeSchedule` after validating fee rates.
    pub fn new(
        maker_rate: f64,
        taker_rate: f64,
        fee_asset: impl Into<String>,
    ) -> Result<Self, EngineError> {
        if !(0.0..1.0).contains(&maker_rate) {
            return Err(EngineError::InvalidFieldValue {
                field: "maker_rate",
                reason: "maker rate must be in range [0.0, 1.0)",
            });
        }
        if !(0.0..1.0).contains(&taker_rate) {
            return Err(EngineError::InvalidFieldValue {
                field: "taker_rate",
                reason: "taker rate must be in range [0.0, 1.0)",
            });
        }

        Ok(Self {
            maker_rate,
            taker_rate,
            fee_asset: fee_asset.into(),
        })
    }

    /// Computes the fee amount for a given trade notional value.
    #[must_use]
    pub fn compute_fee(&self, notional: f64, is_maker: bool) -> f64 {
        let rate = if is_maker {
            self.maker_rate
        } else {
            self.taker_rate
        };
        notional * rate
    }
}

/// Slippage model representing fixed basis points price impact.
#[derive(Debug, Clone, PartialEq)]
pub struct SlippageModel {
    /// Base slippage in basis points (1 bps = 0.0001 = 0.01%).
    pub base_bps: f64,
}

impl SlippageModel {
    /// Creates a new `SlippageModel` after validating basis points.
    pub fn new(base_bps: f64) -> Result<Self, EngineError> {
        if base_bps < 0.0 {
            return Err(EngineError::InvalidFieldValue {
                field: "base_bps",
                reason: "base bps must be non-negative",
            });
        }

        Ok(Self { base_bps })
    }

    /// Applies slippage adjustment to an execution price based on order side.
    #[must_use]
    pub fn apply_slippage(&self, price: f64, side: Side) -> f64 {
        let factor = self.base_bps / 10000.0;
        match side {
            Side::Buy => price * (1.0 + factor),
            Side::Sell => price * (1.0 - factor),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fee_schedule_creation_and_compute_fee() {
        // Arrange
        let schedule =
            FeeSchedule::new(0.001, 0.002, "USDT").expect("FeeSchedule creation should succeed");

        // Act
        let maker_fee = schedule.compute_fee(10_000.0, true);
        let taker_fee = schedule.compute_fee(10_000.0, false);

        // Assert
        assert_eq!(schedule.fee_asset, "USDT");
        assert!((maker_fee - 10.0).abs() < f64::EPSILON);
        assert!((taker_fee - 20.0).abs() < f64::EPSILON);
    }

    #[test]
    fn fee_schedule_invalid_rates_err() {
        // Arrange & Act & Assert
        assert!(matches!(
            FeeSchedule::new(-0.001, 0.001, "USDT"),
            Err(EngineError::InvalidFieldValue {
                field: "maker_rate",
                ..
            })
        ));
        assert!(matches!(
            FeeSchedule::new(0.001, -0.001, "USDT"),
            Err(EngineError::InvalidFieldValue {
                field: "taker_rate",
                ..
            })
        ));
        assert!(matches!(
            FeeSchedule::new(1.0, 0.001, "USDT"),
            Err(EngineError::InvalidFieldValue {
                field: "maker_rate",
                ..
            })
        ));
        assert!(matches!(
            FeeSchedule::new(0.001, 1.5, "USDT"),
            Err(EngineError::InvalidFieldValue {
                field: "taker_rate",
                ..
            })
        ));
    }

    #[test]
    fn slippage_model_creation_and_application() {
        // Arrange
        let model = SlippageModel::new(10.0).expect("valid slippage model");
        let price = 100.0;

        // Act
        let buy_price = model.apply_slippage(price, Side::Buy);
        let sell_price = model.apply_slippage(price, Side::Sell);

        // Assert
        assert_eq!(model.base_bps, 10.0);
        assert!((buy_price - 100.1).abs() < f64::EPSILON);
        assert!((sell_price - 99.9).abs() < f64::EPSILON);
    }

    #[test]
    fn slippage_model_negative_bps_err() {
        // Arrange & Act & Assert
        assert!(matches!(
            SlippageModel::new(-5.0),
            Err(EngineError::InvalidFieldValue {
                field: "base_bps",
                ..
            })
        ));
    }

    #[test]
    fn zero_slippage_returns_original_price() {
        // Arrange
        let model = SlippageModel::new(0.0).expect("valid slippage model");
        let price = 50_000.0;

        // Act & Assert
        assert_eq!(model.apply_slippage(price, Side::Buy), price);
        assert_eq!(model.apply_slippage(price, Side::Sell), price);
    }
}
