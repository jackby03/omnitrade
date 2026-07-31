//! Simple Moving Average (SMA) indicator.
//!
//! Computes the arithmetic mean of the last `period` values using a running
//! sum for O(1) incremental updates.

use crate::CoreError;

/// Simple Moving Average with O(1) incremental updates.
///
/// Maintains a running sum and a circular buffer of the last `period` values.
/// When a new value is pushed and the buffer is full, the oldest value is
/// subtracted from the sum and the new value is added.
///
/// # Examples
///
/// ```
/// use omnitrade_core::Sma;
///
/// let mut sma = Sma::new(3).unwrap();
/// assert_eq!(sma.update(1.0), None);
/// assert_eq!(sma.update(2.0), None);
/// assert_eq!(sma.update(3.0), Some(2.0)); // (1+2+3)/3 = 2.0
/// assert_eq!(sma.update(4.0), Some(3.0)); // (2+3+4)/3 = 3.0
/// ```
#[derive(Debug)]
pub struct Sma {
    period: usize,
    /// Circular buffer storing the last `period` values.
    /// We use a Vec + index instead of RingBuffer since period isn't const.
    buffer: Vec<f64>,
    /// Current write position in the circular buffer.
    pos: usize,
    /// Number of values received so far (saturates at `period`).
    count: usize,
    /// Running sum of elements in the buffer.
    sum: f64,
}

impl Sma {
    /// Creates a new SMA indicator with the given period.
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
            sum: 0.0,
        })
    }

    /// Feeds a new value and returns the current SMA, or `None` if fewer
    /// than `period` values have been provided.
    pub fn update(&mut self, value: f64) -> Option<f64> {
        if self.count >= self.period {
            // Subtract the value being overwritten
            self.sum -= self.buffer[self.pos];
        }

        self.buffer[self.pos] = value;
        self.sum += value;
        self.pos = (self.pos + 1) % self.period;

        if self.count < self.period {
            self.count += 1;
        }

        if self.count == self.period {
            Some(self.sum / self.period as f64)
        } else {
            None
        }
    }

    /// Returns the current SMA value without feeding a new data point.
    pub fn value(&self) -> Option<f64> {
        if self.count == self.period {
            Some(self.sum / self.period as f64)
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
        self.buffer.fill(0.0);
        self.pos = 0;
        self.count = 0;
        self.sum = 0.0;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use approx::assert_relative_eq;

    #[test]
    fn sma_basic() {
        let mut sma = Sma::new(3).expect("valid period");
        assert_eq!(sma.update(1.0), None);
        assert_eq!(sma.update(2.0), None);
        assert_relative_eq!(sma.update(3.0).unwrap(), 2.0);
        assert_relative_eq!(sma.update(4.0).unwrap(), 3.0); // (2+3+4)/3
        assert_relative_eq!(sma.update(5.0).unwrap(), 4.0); // (3+4+5)/3
    }

    #[test]
    fn sma_period_1() {
        let mut sma = Sma::new(1).expect("valid period");
        assert_relative_eq!(sma.update(42.0).unwrap(), 42.0);
        assert_relative_eq!(sma.update(100.0).unwrap(), 100.0);
    }

    #[test]
    fn sma_constant_values() {
        let mut sma = Sma::new(5).expect("valid period");
        for i in 0..4 {
            assert_eq!(sma.update(10.0), None, "step {i}");
        }
        assert_relative_eq!(sma.update(10.0).unwrap(), 10.0);
        assert_relative_eq!(sma.update(10.0).unwrap(), 10.0);
    }

    #[test]
    fn sma_value_peek() {
        let mut sma = Sma::new(2).expect("valid period");
        assert_eq!(sma.value(), None);
        sma.update(3.0);
        assert_eq!(sma.value(), None);
        sma.update(5.0);
        assert_relative_eq!(sma.value().unwrap(), 4.0);
    }

    #[test]
    fn sma_reset() {
        let mut sma = Sma::new(2).expect("valid period");
        sma.update(10.0);
        sma.update(20.0);
        assert!(sma.value().is_some());
        sma.reset();
        assert_eq!(sma.value(), None);
        assert_eq!(sma.update(5.0), None);
    }

    #[test]
    fn sma_known_sequence() {
        // Prices: 44, 44.34, 44.09, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84
        // SMA(5): -, -, -, -, 44.074, 44.242, 44.392, 44.658, 45.044
        let prices = [44.0, 44.34, 44.09, 43.61, 44.33, 44.83, 45.10, 45.42, 45.84];
        let expected = [44.074, 44.24, 44.392, 44.658, 45.104];

        let mut sma = Sma::new(5).expect("valid period");
        let mut results = Vec::new();
        for p in prices {
            if let Some(v) = sma.update(p) {
                results.push(v);
            }
        }

        assert_eq!(results.len(), expected.len());
        for (got, want) in results.iter().zip(expected.iter()) {
            assert_relative_eq!(got, want, epsilon = 1e-10);
        }
    }

    #[test]
    fn sma_zero_period_returns_error() {
        let result = Sma::new(0);
        assert!(result.is_err());
        assert_eq!(result.unwrap_err(), CoreError::InvalidPeriod(0));
    }

    #[test]
    fn sma_numerical_stability() {
        // Test with very large values to check for floating-point drift
        let mut sma = Sma::new(3).expect("valid period");
        let big = 1e15;
        sma.update(big);
        sma.update(big + 1.0);
        let result = sma.update(big + 2.0).unwrap();
        assert_relative_eq!(result, big + 1.0, epsilon = 1e-6);
    }
}
