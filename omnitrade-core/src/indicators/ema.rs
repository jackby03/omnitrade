//! Exponential Moving Average (EMA) indicator.
//!
//! The EMA gives more weight to recent values using a smoothing multiplier.
//! It is seeded with the SMA of the first `period` values for accuracy.

use crate::CoreError;

/// Exponential Moving Average with configurable period.
///
/// Uses the standard smoothing formula:
/// ```text
/// multiplier = 2.0 / (period + 1)
/// EMA_today = (value - EMA_yesterday) * multiplier + EMA_yesterday
/// ```
///
/// The first EMA value is seeded from the SMA of the first `period` values.
///
/// # Examples
///
/// ```
/// use omnitrade_core::Ema;
///
/// let mut ema = Ema::new(3).unwrap();
/// ema.update(2.0);
/// ema.update(4.0);
/// let first = ema.update(6.0); // Seeded from SMA(2,4,6) = 4.0
/// assert!(first.is_some());
/// ```
#[derive(Debug)]
pub struct Ema {
    period: usize,
    multiplier: f64,
    /// Accumulates the sum of the first `period` values to compute the seed SMA.
    seed_sum: f64,
    /// Number of values received so far.
    count: usize,
    /// Current EMA value (valid after `count >= period`).
    current: f64,
    /// Whether the EMA has been seeded.
    seeded: bool,
}

impl Ema {
    /// Creates a new EMA indicator with the given period.
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
            multiplier: 2.0 / (period as f64 + 1.0),
            seed_sum: 0.0,
            count: 0,
            current: 0.0,
            seeded: false,
        })
    }

    /// Feeds a new value and returns the current EMA, or `None` if fewer
    /// than `period` values have been provided.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        self.count += 1;

        if !self.seeded {
            self.seed_sum += value;
            if self.count == self.period {
                // Seed with SMA
                self.current = self.seed_sum / self.period as f64;
                self.seeded = true;
                return Some(self.current);
            }
            return None;
        }

        // Standard EMA formula
        self.current = (value - self.current) * self.multiplier + self.current;
        Some(self.current)
    }

    /// Returns the current EMA value without feeding a new data point.
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

    /// Returns the smoothing multiplier.
    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    /// Resets the indicator to its initial state.
    pub fn reset(&mut self) {
        self.seed_sum = 0.0;
        self.count = 0;
        self.current = 0.0;
        self.seeded = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn ema_multiplier() {
        let ema = Ema::new(10).expect("valid period");
        assert_relative_eq!(ema.multiplier(), 2.0 / 11.0);

        let ema3 = Ema::new(3).expect("valid period");
        assert_relative_eq!(ema3.multiplier(), 0.5);
    }

    #[test]
    fn ema_seed_from_sma() {
        let mut ema = Ema::new(3).expect("valid period");
        assert_eq!(ema.update(2.0), None);
        assert_eq!(ema.update(4.0), None);
        // Seed = SMA(2, 4, 6) = 4.0
        let seed = ema.update(6.0).unwrap();
        assert_relative_eq!(seed, 4.0);
    }

    #[test]
    fn ema_subsequent_values() {
        let mut ema = Ema::new(3).expect("valid period");
        ema.update(2.0);
        ema.update(4.0);
        ema.update(6.0); // seed = 4.0, multiplier = 0.5

        // EMA = (8 - 4) * 0.5 + 4 = 6.0
        assert_relative_eq!(ema.update(8.0).unwrap(), 6.0);
        // EMA = (10 - 6) * 0.5 + 6 = 8.0
        assert_relative_eq!(ema.update(10.0).unwrap(), 8.0);
    }

    #[test]
    fn ema_period_1() {
        // With period=1, multiplier = 2/2 = 1.0, so EMA = value
        let mut ema = Ema::new(1).expect("valid period");
        assert_relative_eq!(ema.update(42.0).unwrap(), 42.0);
        assert_relative_eq!(ema.update(100.0).unwrap(), 100.0);
    }

    #[test]
    fn ema_constant_values() {
        let mut ema = Ema::new(5).expect("valid period");
        for _ in 0..4 {
            ema.update(10.0);
        }
        assert_relative_eq!(ema.update(10.0).unwrap(), 10.0);
        assert_relative_eq!(ema.update(10.0).unwrap(), 10.0);
        assert_relative_eq!(ema.update(10.0).unwrap(), 10.0);
    }

    #[test]
    fn ema_value_peek() {
        let mut ema = Ema::new(2).expect("valid period");
        assert_eq!(ema.value(), None);
        ema.update(3.0);
        assert_eq!(ema.value(), None);
        ema.update(5.0);
        assert!(ema.value().is_some());
    }

    #[test]
    fn ema_reset() {
        let mut ema = Ema::new(2).expect("valid period");
        ema.update(10.0);
        ema.update(20.0);
        assert!(ema.value().is_some());
        ema.reset();
        assert_eq!(ema.value(), None);
        assert_eq!(ema.update(5.0), None);
    }

    #[test]
    fn ema_known_sequence() {
        // Verify EMA(10) against a manually computed sequence.
        let prices = [
            22.27, 22.19, 22.08, 22.17, 22.18, 22.13, 22.23, 22.43, 22.24, 22.29, 22.15,
        ];
        let mut ema = Ema::new(10).expect("valid period");

        let mut last_val = None;
        for p in prices {
            last_val = ema.update(p);
        }

        let multiplier = 2.0 / 11.0;
        let seed = 22.221;
        let expected = (22.15 - seed) * multiplier + seed;
        assert_relative_eq!(last_val.unwrap(), expected, epsilon = 1e-10);
    }

    #[test]
    fn ema_zero_period_returns_error() {
        let result = Ema::new(0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CoreError::InvalidPeriod(0));
    }
}
