//! Relative Strength Index (RSI) indicator.
//!
//! Implements Wilder's smoothing method (a special case of exponential moving
//! average) to compute the RSI from a stream of prices.
//!
//! RSI values are bounded in `[0.0, 100.0]`:
//! - **RSI > 70**: Traditionally considered overbought
//! - **RSI < 30**: Traditionally considered oversold

use crate::CoreError;

/// Relative Strength Index using Wilder's smoothing.
///
/// The RSI is computed as:
/// ```text
/// RS = avg_gain / avg_loss
/// RSI = 100 - (100 / (1 + RS))
/// ```
///
/// The first RSI value requires `period + 1` data points (to compute `period`
/// price changes). Subsequent values use Wilder's smoothing:
/// ```text
/// avg_gain = (prev_avg_gain * (period - 1) + current_gain) / period
/// avg_loss = (prev_avg_loss * (period - 1) + current_loss) / period
/// ```
///
/// # Examples
///
/// ```
/// use omnitrade_core::Rsi;
///
/// let mut rsi = Rsi::new(14).unwrap();
/// // Feed 15 prices to get the first RSI value
/// let prices = [44.0, 44.34, 44.09, 43.61, 44.33, 44.83, 45.10,
///               45.42, 45.84, 46.08, 45.89, 46.03, 45.61, 46.28, 46.28];
/// let mut result = None;
/// for p in prices {
///     result = rsi.update(p);
/// }
/// assert!(result.is_some());
/// ```
#[derive(Debug)]
pub struct Rsi {
    period: usize,
    /// Previous price (needed to compute the change).
    prev_price: f64,
    /// Number of prices received.
    count: usize,
    /// Running average gain (Wilder's smoothing).
    avg_gain: f64,
    /// Running average loss (Wilder's smoothing).
    avg_loss: f64,
    /// Accumulated gains during the seed phase.
    seed_gains: f64,
    /// Accumulated losses during the seed phase.
    seed_losses: f64,
    /// Whether the initial seed phase is complete.
    seeded: bool,
    /// Current RSI value.
    current: f64,
}

impl Rsi {
    /// Creates a new RSI indicator with the given period.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidPeriod`] if `period` is zero.
    pub fn new(period: usize) -> Result<Self, CoreError> {
        if period == 0 {
            return Err(CoreError::InvalidPeriod(period));
        }
        Ok(Self {
            period,
            prev_price: 0.0,
            count: 0,
            avg_gain: 0.0,
            avg_loss: 0.0,
            seed_gains: 0.0,
            seed_losses: 0.0,
            seeded: false,
            current: 0.0,
        })
    }

    /// Creates a new RSI with the standard period of 14.
    pub fn default_period() -> Self {
        // SAFETY: 14 is always valid — this is an infallible convenience constructor.
        Self::new(14).expect("default RSI period of 14 is always valid")
    }

    /// Feeds a new price and returns the current RSI in `[0.0, 100.0]`,
    /// or `None` if fewer than `period + 1` prices have been provided.
    pub fn update(&mut self, price: f64) -> Option<f64> {
        self.count += 1;

        // First price — just store it, no change to compute.
        if self.count == 1 {
            self.prev_price = price;
            return None;
        }

        let change = price - self.prev_price;
        self.prev_price = price;

        let gain = if change > 0.0 { change } else { 0.0 };
        let loss = if change < 0.0 { -change } else { 0.0 };

        if !self.seeded {
            self.seed_gains += gain;
            self.seed_losses += loss;

            // We need `period` changes, which requires `period + 1` prices.
            if self.count == self.period + 1 {
                self.avg_gain = self.seed_gains / self.period as f64;
                self.avg_loss = self.seed_losses / self.period as f64;
                self.seeded = true;
                self.current = self.compute_rsi();
                return Some(self.current);
            }
            return None;
        }

        // Wilder's smoothing
        self.avg_gain = (self.avg_gain * (self.period as f64 - 1.0) + gain) / self.period as f64;
        self.avg_loss = (self.avg_loss * (self.period as f64 - 1.0) + loss) / self.period as f64;
        self.current = self.compute_rsi();
        Some(self.current)
    }

    /// Computes RSI from current avg_gain and avg_loss.
    #[inline]
    fn compute_rsi(&self) -> f64 {
        if self.avg_loss == 0.0 {
            if self.avg_gain == 0.0 {
                // No movement at all — RSI is neutral
                return 50.0;
            }
            // All gains, no losses
            return 100.0;
        }
        let rs = self.avg_gain / self.avg_loss;
        100.0 - (100.0 / (1.0 + rs))
    }

    /// Returns the current RSI value without feeding a new data point.
    pub fn value(&self) -> Option<f64> {
        if self.seeded {
            Some(self.current)
        } else {
            None
        }
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Resets the indicator to its initial state.
    pub fn reset(&mut self) {
        self.prev_price = 0.0;
        self.count = 0;
        self.avg_gain = 0.0;
        self.avg_loss = 0.0;
        self.seed_gains = 0.0;
        self.seed_losses = 0.0;
        self.seeded = false;
        self.current = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn rsi_needs_period_plus_one_prices() {
        let mut rsi = Rsi::new(3).expect("valid period");
        assert_eq!(rsi.update(10.0), None);
        assert_eq!(rsi.update(11.0), None);
        assert_eq!(rsi.update(12.0), None);
        assert!(rsi.update(13.0).is_some());
    }

    #[test]
    fn rsi_all_gains() {
        let mut rsi = Rsi::new(3).expect("valid period");
        rsi.update(10.0);
        rsi.update(11.0);
        rsi.update(12.0);
        let val = rsi.update(13.0).unwrap();
        assert_relative_eq!(val, 100.0);
    }

    #[test]
    fn rsi_all_losses() {
        let mut rsi = Rsi::new(3).expect("valid period");
        rsi.update(13.0);
        rsi.update(12.0);
        rsi.update(11.0);
        let val = rsi.update(10.0).unwrap();
        assert_relative_eq!(val, 0.0);
    }

    #[test]
    fn rsi_flat_market() {
        let mut rsi = Rsi::new(3).expect("valid period");
        rsi.update(50.0);
        rsi.update(50.0);
        rsi.update(50.0);
        let val = rsi.update(50.0).unwrap();
        assert_relative_eq!(val, 50.0);
    }

    #[test]
    fn rsi_equal_gains_and_losses() {
        let mut rsi = Rsi::new(4).expect("valid period");
        rsi.update(10.0);
        rsi.update(11.0);
        rsi.update(10.0);
        rsi.update(11.0);
        let val = rsi.update(10.0).unwrap();
        assert_relative_eq!(val, 50.0);
    }

    #[test]
    fn rsi_bounded_0_to_100() {
        let mut rsi = Rsi::new(5).expect("valid period");
        let prices = [50.0, 55.0, 48.0, 52.0, 47.0, 53.0, 46.0, 54.0, 45.0, 55.0];
        for p in prices {
            if let Some(val) = rsi.update(p) {
                assert!(val >= 0.0 && val <= 100.0, "RSI out of bounds: {val}");
            }
        }
    }

    #[test]
    fn rsi_wilder_smoothing() {
        let mut rsi = Rsi::new(3).expect("valid period");
        rsi.update(10.0);
        rsi.update(12.0);
        rsi.update(11.0);
        rsi.update(13.0);
        assert_relative_eq!(rsi.value().unwrap(), 80.0);

        let val = rsi.update(12.0).unwrap();
        let expected_rs = (8.0 / 9.0) / (5.0 / 9.0);
        let expected_rsi = 100.0 - 100.0 / (1.0 + expected_rs);
        assert_relative_eq!(val, expected_rsi, epsilon = 1e-10);
    }

    #[test]
    fn rsi_known_14_period_sequence() {
        let prices = [
            44.34, 44.09, 44.15, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84, 46.08, 45.89, 46.03,
            45.61, 46.28, 46.28,
        ];
        let mut rsi = Rsi::new(14).expect("valid period");
        let mut result = None;
        for p in prices {
            result = rsi.update(p);
        }
        let val = result.unwrap();
        assert!(val > 70.0 && val < 71.0, "RSI(14) ≈ 70.46, got {val}");
    }

    #[test]
    fn rsi_value_peek() {
        let mut rsi = Rsi::new(2).expect("valid period");
        assert_eq!(rsi.value(), None);
        rsi.update(10.0);
        rsi.update(11.0);
        assert_eq!(rsi.value(), None);
        rsi.update(12.0);
        assert!(rsi.value().is_some());
    }

    #[test]
    fn rsi_reset() {
        let mut rsi = Rsi::new(2).expect("valid period");
        rsi.update(10.0);
        rsi.update(11.0);
        rsi.update(12.0);
        assert!(rsi.value().is_some());
        rsi.reset();
        assert_eq!(rsi.value(), None);
        assert_eq!(rsi.update(10.0), None);
    }

    #[test]
    fn rsi_zero_period_returns_error() {
        let result = Rsi::new(0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CoreError::InvalidPeriod(0));
    }

    #[test]
    fn rsi_default_period() {
        let rsi = Rsi::default_period();
        assert_eq!(rsi.period(), 14);
    }
}
