//! Rolling volatility (standard deviation) indicator.
//!
//! Uses Welford's online algorithm for numerically stable, O(1) incremental
//! computation of population variance and standard deviation.

use crate::CoreError;

/// Rolling standard deviation using Welford's online algorithm.
///
/// Computes the **population** standard deviation over the last `period` values.
/// This is numerically stable even for very large or very small values, avoiding
/// the catastrophic cancellation that plagues the naive "sum of squares" approach.
///
/// # Examples
///
/// ```
/// use omnitrade_core::Volatility;
///
/// let mut vol = Volatility::new(3).unwrap();
/// vol.update(2.0);
/// vol.update(4.0);
/// let v = vol.update(6.0).unwrap();
/// // Population std dev of [2, 4, 6] = sqrt(8/3) ≈ 1.6329931618...
/// assert!((v - 1.6329931618).abs() < 1e-6);
/// ```
#[derive(Debug)]
pub struct Volatility {
    period: usize,
    /// Circular buffer of values.
    buffer: Vec<f64>,
    /// Current write position.
    pos: usize,
    /// Number of values received (saturates at `period`).
    count: usize,
    /// Running mean.
    mean: f64,
    /// Running M2 (sum of squared differences from the mean).
    m2: f64,
}

impl Volatility {
    /// Creates a new volatility indicator with the given period.
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
            buffer: vec![0.0; period],
            pos: 0,
            count: 0,
            mean: 0.0,
            m2: 0.0,
        })
    }

    /// Feeds a new value and returns the current population standard deviation,
    /// or `None` if fewer than `period` values have been provided.
    ///
    /// When the buffer is full, uses an incremental update that removes the
    /// oldest value and adds the new one, maintaining O(1) per update.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        if self.count < self.period {
            // Accumulating phase: use standard Welford's add
            self.buffer[self.pos] = value;
            self.pos = (self.pos + 1) % self.period;
            self.count += 1;

            let delta = value - self.mean;
            self.mean += delta / self.count as f64;
            let delta2 = value - self.mean;
            self.m2 += delta * delta2;

            if self.count == self.period {
                let variance = self.m2 / self.count as f64;
                return Some(variance.sqrt());
            }
            return None;
        }

        // Sliding window: remove oldest, add newest
        let old_value = self.buffer[self.pos];
        self.buffer[self.pos] = value;
        self.pos = (self.pos + 1) % self.period;

        // Update mean and M2 for the removal of old_value and addition of value.
        let old_mean = self.mean;
        self.mean += (value - old_value) / self.period as f64;
        self.m2 += (value - old_value) * (value - self.mean + old_value - old_mean);

        // Guard against floating-point drift producing tiny negative M2
        if self.m2 < 0.0 {
            self.m2 = 0.0;
        }

        let variance = self.m2 / self.period as f64;
        Some(variance.sqrt())
    }

    /// Returns the current volatility (population std dev) without feeding a new value.
    pub fn value(&self) -> Option<f64> {
        if self.count == self.period {
            let variance = self.m2 / self.period as f64;
            Some(variance.sqrt())
        } else {
            None
        }
    }

    /// Returns the current variance (population) without feeding a new value.
    pub fn variance(&self) -> Option<f64> {
        if self.count == self.period {
            Some(self.m2 / self.period as f64)
        } else {
            None
        }
    }

    /// Returns the current mean of the window.
    pub fn mean(&self) -> f64 {
        self.mean
    }

    /// Returns the configured period.
    pub fn period(&self) -> usize {
        self.period
    }

    /// Resets the indicator to its initial state.
    pub fn reset(&mut self) {
        self.buffer.fill(0.0);
        self.pos = 0;
        self.count = 0;
        self.mean = 0.0;
        self.m2 = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    /// Helper: compute population std dev from a slice (reference implementation).
    fn reference_std_dev(values: &[f64]) -> f64 {
        let n = values.len() as f64;
        let mean = values.iter().sum::<f64>() / n;
        let variance = values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / n;
        variance.sqrt()
    }

    #[test]
    fn volatility_basic() {
        let mut vol = Volatility::new(3).expect("valid period");
        assert_eq!(vol.update(2.0), None);
        assert_eq!(vol.update(4.0), None);
        let v = vol.update(6.0).unwrap();
        let expected = reference_std_dev(&[2.0, 4.0, 6.0]);
        assert_relative_eq!(v, expected, epsilon = 1e-10);
    }

    #[test]
    fn volatility_sliding_window() {
        let mut vol = Volatility::new(3).expect("valid period");
        vol.update(2.0);
        vol.update(4.0);
        vol.update(6.0);

        let v = vol.update(8.0).unwrap();
        let expected = reference_std_dev(&[4.0, 6.0, 8.0]);
        assert_relative_eq!(v, expected, epsilon = 1e-10);

        let v = vol.update(10.0).unwrap();
        let expected = reference_std_dev(&[6.0, 8.0, 10.0]);
        assert_relative_eq!(v, expected, epsilon = 1e-10);
    }

    #[test]
    fn volatility_constant_values() {
        let mut vol = Volatility::new(4).expect("valid period");
        for _ in 0..3 {
            vol.update(5.0);
        }
        let v = vol.update(5.0).unwrap();
        assert_relative_eq!(v, 0.0, epsilon = 1e-15);

        let v = vol.update(5.0).unwrap();
        assert_relative_eq!(v, 0.0, epsilon = 1e-15);
    }

    #[test]
    fn volatility_period_1() {
        let mut vol = Volatility::new(1).expect("valid period");
        assert_relative_eq!(vol.update(42.0).unwrap(), 0.0);
        assert_relative_eq!(vol.update(100.0).unwrap(), 0.0);
    }

    #[test]
    fn volatility_variance() {
        let mut vol = Volatility::new(3).expect("valid period");
        vol.update(2.0);
        vol.update(4.0);
        vol.update(6.0);
        let expected_var = 8.0 / 3.0;
        assert_relative_eq!(vol.variance().unwrap(), expected_var, epsilon = 1e-10);
    }

    #[test]
    fn volatility_mean() {
        let mut vol = Volatility::new(3).expect("valid period");
        vol.update(2.0);
        vol.update(4.0);
        vol.update(6.0);
        assert_relative_eq!(vol.mean(), 4.0, epsilon = 1e-10);
    }

    #[test]
    fn volatility_long_sequence() {
        let mut vol = Volatility::new(4).expect("valid period");
        let values: Vec<f64> = (0..100).map(|i| i as f64 * 0.1 + 1000.0).collect();

        for (i, &v) in values.iter().enumerate() {
            if let Some(result) = vol.update(v) {
                let start = i.saturating_sub(3);
                let window = &values[start..=i];
                let expected = reference_std_dev(window);
                assert_relative_eq!(result, expected, epsilon = 1e-8);
            }
        }
    }

    #[test]
    fn volatility_value_peek() {
        let mut vol = Volatility::new(2).expect("valid period");
        assert_eq!(vol.value(), None);
        vol.update(3.0);
        assert_eq!(vol.value(), None);
        vol.update(5.0);
        assert!(vol.value().is_some());
    }

    #[test]
    fn volatility_reset() {
        let mut vol = Volatility::new(2).expect("valid period");
        vol.update(10.0);
        vol.update(20.0);
        assert!(vol.value().is_some());
        vol.reset();
        assert_eq!(vol.value(), None);
        assert_eq!(vol.update(5.0), None);
    }

    #[test]
    fn volatility_zero_period_returns_error() {
        let result = Volatility::new(0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CoreError::InvalidPeriod(0));
    }

    #[test]
    fn volatility_large_values_stability() {
        let mut vol = Volatility::new(3).expect("valid period");
        let base = 1e12;
        vol.update(base);
        vol.update(base + 1.0);
        let v = vol.update(base + 2.0).unwrap();
        let expected = reference_std_dev(&[base, base + 1.0, base + 2.0]);
        assert_relative_eq!(v, expected, epsilon = 1e-4);
    }

    #[test]
    fn volatility_alternating_values() {
        let mut vol = Volatility::new(4).expect("valid period");
        vol.update(10.0);
        vol.update(20.0);
        vol.update(10.0);
        let v = vol.update(20.0).unwrap();
        let expected = reference_std_dev(&[10.0, 20.0, 10.0, 20.0]);
        assert_relative_eq!(v, expected, epsilon = 1e-10);
    }
}
