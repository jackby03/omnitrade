//! Level-2 Order Book and `OrderedFloat` price wrapper.

use crate::error::EngineError;
use std::cmp::Reverse;
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};

/// A wrapper around `f64` for total ordering and NaN rejection.
#[derive(Copy, Clone, Debug)]
struct OrderedFloat(f64);

impl OrderedFloat {
    /// Creates a new `OrderedFloat` after validating `val` is finite.
    fn new(val: f64) -> Result<Self, EngineError> {
        if val.is_nan() || val.is_infinite() {
            return Err(EngineError::InvalidFieldValue {
                field: "price",
                reason: "price must be a finite non-NaN number",
            });
        }
        Ok(Self(val))
    }

    /// Returns the underlying `f64` value.
    fn into_inner(self) -> f64 {
        self.0
    }
}

impl PartialEq for OrderedFloat {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_bits() == other.0.to_bits()
    }
}

impl Eq for OrderedFloat {}

impl Hash for OrderedFloat {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.0.to_bits().hash(state);
    }
}

impl PartialOrd for OrderedFloat {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for OrderedFloat {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.total_cmp(&other.0)
    }
}

/// A Level-2 order book maintaining sorted bids and asks.
#[derive(Debug, Clone)]
pub struct L2OrderBook {
    symbol: String,
    bids: BTreeMap<Reverse<OrderedFloat>, f64>,
    asks: BTreeMap<OrderedFloat, f64>,
    last_update_ms: u64,
}

impl L2OrderBook {
    /// Constructs a new empty `L2OrderBook` for a symbol.
    pub fn new(symbol: impl Into<String>) -> Self {
        Self {
            symbol: symbol.into(),
            bids: BTreeMap::new(),
            asks: BTreeMap::new(),
            last_update_ms: 0,
        }
    }

    /// Returns the trading pair symbol.
    pub fn symbol(&self) -> &str {
        &self.symbol
    }

    /// Returns the last update timestamp in milliseconds.
    pub fn last_update_ms(&self) -> u64 {
        self.last_update_ms
    }

    /// Sets the last update timestamp in milliseconds.
    pub fn set_last_update_ms(&mut self, timestamp_ms: u64) {
        self.last_update_ms = timestamp_ms;
    }

    /// Applies delta updates to bids and asks. Quantity of 0 removes the level.
    pub fn apply_delta(
        &mut self,
        bids: &[(f64, f64)],
        asks: &[(f64, f64)],
    ) -> Result<(), EngineError> {
        for &(price, qty) in bids {
            if qty < 0.0 || qty.is_nan() || qty.is_infinite() {
                return Err(EngineError::InvalidFieldValue {
                    field: "quantity",
                    reason: "quantity must be a finite non-negative number",
                });
            }
            let key = OrderedFloat::new(price)?;
            if qty == 0.0 {
                self.bids.remove(&Reverse(key));
            } else {
                self.bids.insert(Reverse(key), qty);
            }
        }

        for &(price, qty) in asks {
            if qty < 0.0 || qty.is_nan() || qty.is_infinite() {
                return Err(EngineError::InvalidFieldValue {
                    field: "quantity",
                    reason: "quantity must be a finite non-negative number",
                });
            }
            let key = OrderedFloat::new(price)?;
            if qty == 0.0 {
                self.asks.remove(&key);
            } else {
                self.asks.insert(key, qty);
            }
        }

        Ok(())
    }

    /// Returns the highest bid (price, quantity) if available.
    pub fn best_bid(&self) -> Option<(f64, f64)> {
        self.bids
            .iter()
            .next()
            .map(|(price, &qty)| (price.0.into_inner(), qty))
    }

    /// Returns the lowest ask (price, quantity) if available.
    pub fn best_ask(&self) -> Option<(f64, f64)> {
        self.asks
            .iter()
            .next()
            .map(|(price, &qty)| (price.into_inner(), qty))
    }

    /// Calculates the bid-ask spread (`best_ask - best_bid`).
    pub fn spread(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid, _)), Some((ask, _))) => Some(ask - bid),
            _ => None,
        }
    }

    /// Calculates the mid price (`(best_ask + best_bid) / 2`).
    pub fn mid_price(&self) -> Option<f64> {
        match (self.best_bid(), self.best_ask()) {
            (Some((bid, _)), Some((ask, _))) => Some((ask + bid) / 2.0),
            _ => None,
        }
    }

    /// Returns an iterator over bid levels (price, quantity) sorted descending.
    pub fn bids(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.bids
            .iter()
            .map(|(price, &qty)| (price.0.into_inner(), qty))
    }

    /// Returns an iterator over ask levels (price, quantity) sorted ascending.
    pub fn asks(&self) -> impl Iterator<Item = (f64, f64)> + '_ {
        self.asks
            .iter()
            .map(|(price, &qty)| (price.into_inner(), qty))
    }

    /// Returns the quantity available at a given price level across bids and asks.
    pub fn depth_at(&self, price: f64) -> Option<f64> {
        let key = OrderedFloat::new(price).ok()?;
        self.bids
            .get(&Reverse(key))
            .copied()
            .or_else(|| self.asks.get(&key).copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ordered_float_validation() {
        // Arrange & Act & Assert
        assert!(OrderedFloat::new(f64::NAN).is_err());
        assert!(OrderedFloat::new(f64::INFINITY).is_err());
        assert!(OrderedFloat::new(f64::NEG_INFINITY).is_err());
        assert!(OrderedFloat::new(100.5).is_ok());
    }

    #[test]
    fn test_empty_book_spread_and_mid() {
        // Arrange
        let book = L2OrderBook::new("BTCUSDT");

        // Act & Assert
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.spread(), None);
        assert_eq!(book.mid_price(), None);
    }

    #[test]
    fn test_bids_descending_asks_ascending() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");
        let bids = [(100.0, 1.0), (105.0, 2.0), (102.0, 0.5)];
        let asks = [(110.0, 1.5), (107.0, 3.0), (108.0, 0.8)];

        // Act
        let result = book.apply_delta(&bids, &asks);

        // Assert
        assert!(result.is_ok());
        assert_eq!(book.best_bid(), Some((105.0, 2.0)));
        assert_eq!(book.best_ask(), Some((107.0, 3.0)));
        assert_eq!(book.spread(), Some(2.0));
        assert_eq!(book.mid_price(), Some(106.0));
    }

    #[test]
    fn test_level_deletion_on_zero_qty() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");
        book.apply_delta(&[(100.0, 1.0)], &[(110.0, 1.0)])
            .expect("initial delta should succeed");

        // Act
        book.apply_delta(&[(100.0, 0.0)], &[(110.0, 0.0)])
            .expect("deletion delta should succeed");

        // Assert
        assert_eq!(book.best_bid(), None);
        assert_eq!(book.best_ask(), None);
        assert_eq!(book.depth_at(100.0), None);
        assert_eq!(book.depth_at(110.0), None);
    }

    #[test]
    fn test_depth_at() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");
        book.apply_delta(&[(100.0, 2.5)], &[(110.0, 3.5)])
            .expect("delta should succeed");

        // Act & Assert
        assert_eq!(book.depth_at(100.0), Some(2.5));
        assert_eq!(book.depth_at(110.0), Some(3.5));
        assert_eq!(book.depth_at(105.0), None);
    }

    #[test]
    fn test_invalid_qty_returns_error() {
        // Arrange
        let mut book = L2OrderBook::new("BTCUSDT");

        // Act & Assert
        assert!(book.apply_delta(&[(100.0, -1.0)], &[]).is_err());
        assert!(book.apply_delta(&[], &[(110.0, f64::NAN)]).is_err());
    }
}
