//! Technical analysis indicators for streaming price data.
//!
//! All indicators follow the same incremental interface:
//! - `new(period)` — create a new indicator with the given lookback period
//! - `update(value) -> Option<f64>` — feed a new value and get the result
//!   (returns `None` until enough data has been accumulated)
//! - `value() -> Option<f64>` — peek at the current value without updating
//!
//! Indicators use the [`RingBuffer`](crate::RingBuffer) internally for O(1) updates.

mod ema;
mod rsi;
mod sma;
mod volatility;

pub use ema::Ema;
pub use rsi::Rsi;
pub use sma::Sma;
pub use volatility::Volatility;
