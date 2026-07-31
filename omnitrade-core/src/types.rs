//! Core domain types for the omnitrade trading engine.
//!
//! All monetary values use `f64`. This is a deliberate trade-off for Phase 1:
//! `f64` provides sufficient precision for paper trading and indicator math,
//! while keeping the code simple and `no_std`-compatible.
//!
//! TODO: Evaluate fixed-point arithmetic (e.g., `rust_decimal`) for production
//! order matching where exact decimal representation matters.

/// Side of a trade or order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Side {
    Buy,
    Sell,
}

impl Side {
    /// Returns the opposite side.
    #[inline]
    pub fn opposite(self) -> Self {
        match self {
            Side::Buy => Side::Sell,
            Side::Sell => Side::Buy,
        }
    }
}

/// Type of order to be placed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderType {
    /// Execute immediately at best available price.
    Market,
    /// Execute at specified price or better.
    Limit,
    /// Trigger a market order when stop price is reached.
    StopMarket,
    /// Trigger a limit order when stop price is reached.
    StopLimit,
}

/// Time-in-force policy for an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TimeInForce {
    /// Good 'Til Cancelled — remains active until filled or manually cancelled.
    GTC,
    /// Immediate Or Cancel — fill what's possible immediately, cancel the rest.
    IOC,
    /// Fill Or Kill — must be filled entirely immediately or cancelled entirely.
    FOK,
}

/// Lifecycle status of an order.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum OrderStatus {
    /// Order has been accepted but not yet matched.
    New,
    /// Order has been partially filled.
    PartiallyFilled,
    /// Order has been completely filled.
    Filled,
    /// Order has been cancelled (by user or system).
    Cancelled,
    /// Order was rejected (insufficient margin, invalid params, etc.).
    Rejected,
}

/// OHLCV candlestick data point.
///
/// Represents a single aggregated price bar over a time interval.
#[derive(Debug, Clone, PartialEq)]
pub struct Candle {
    /// Bar open timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Opening price of the interval.
    pub open: f64,
    /// Highest price during the interval.
    pub high: f64,
    /// Lowest price during the interval.
    pub low: f64,
    /// Closing price of the interval.
    pub close: f64,
    /// Total traded volume during the interval.
    pub volume: f64,
}

impl Candle {
    /// Creates a new `Candle`.
    pub fn new(timestamp_ms: u64, open: f64, high: f64, low: f64, close: f64, volume: f64) -> Self {
        Self {
            timestamp_ms,
            open,
            high,
            low,
            close,
            volume,
        }
    }

    /// Returns the midpoint of the candle body: `(open + close) / 2`.
    #[inline]
    pub fn body_midpoint(&self) -> f64 {
        (self.open + self.close) / 2.0
    }

    /// Returns the full range of the candle: `high - low`.
    #[inline]
    pub fn range(&self) -> f64 {
        self.high - self.low
    }

    /// Returns `true` if this is a bullish (green) candle.
    #[inline]
    pub fn is_bullish(&self) -> bool {
        self.close >= self.open
    }
}

/// A single trade tick from the exchange.
#[derive(Debug, Clone, PartialEq)]
pub struct Tick {
    /// Trade timestamp in milliseconds since Unix epoch.
    pub timestamp_ms: u64,
    /// Execution price.
    pub price: f64,
    /// Executed quantity.
    pub quantity: f64,
    /// `true` if the buyer was the maker (i.e., the sell side was the aggressor).
    pub is_buyer_maker: bool,
}

impl Tick {
    /// Creates a new `Tick`.
    pub fn new(timestamp_ms: u64, price: f64, quantity: f64, is_buyer_maker: bool) -> Self {
        Self {
            timestamp_ms,
            price,
            quantity,
            is_buyer_maker,
        }
    }

    /// Returns the notional value of this tick: `price * quantity`.
    #[inline]
    pub fn notional(&self) -> f64 {
        self.price * self.quantity
    }
}

/// An order in the trading system.
#[derive(Debug, Clone, PartialEq)]
pub struct Order {
    /// Unique order identifier.
    pub id: u64,
    /// Trading pair symbol (e.g., "BTCUSDT").
    pub symbol: String,
    /// Buy or sell.
    pub side: Side,
    /// Order type (Market, Limit, etc.).
    pub order_type: OrderType,
    /// Time-in-force policy.
    pub time_in_force: TimeInForce,
    /// Limit price (relevant for Limit and StopLimit orders).
    pub price: f64,
    /// Stop/trigger price (relevant for StopMarket and StopLimit orders).
    pub stop_price: f64,
    /// Requested quantity.
    pub quantity: f64,
    /// Quantity that has been filled so far.
    pub filled_quantity: f64,
    /// Current order lifecycle status.
    pub status: OrderStatus,
    /// Timestamp when the order was created (ms since epoch).
    pub created_at_ms: u64,
    /// Timestamp when the order was last updated (ms since epoch).
    pub updated_at_ms: u64,
}

impl Order {
    /// Returns the remaining unfilled quantity.
    #[inline]
    pub fn remaining_quantity(&self) -> f64 {
        self.quantity - self.filled_quantity
    }

    /// Returns `true` if the order is in a terminal state (Filled, Cancelled, or Rejected).
    #[inline]
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.status,
            OrderStatus::Filled | OrderStatus::Cancelled | OrderStatus::Rejected
        )
    }

    /// Returns `true` if the order is still active (New or PartiallyFilled).
    #[inline]
    pub fn is_active(&self) -> bool {
        matches!(self.status, OrderStatus::New | OrderStatus::PartiallyFilled)
    }
}

/// A trading position (open or closed).
#[derive(Debug, Clone, PartialEq)]
pub struct Position {
    /// Trading pair symbol.
    pub symbol: String,
    /// Direction of the position.
    pub side: Side,
    /// Average entry price.
    pub entry_price: f64,
    /// Current position size.
    pub quantity: f64,
    /// Unrealized profit/loss at current market price.
    pub unrealized_pnl: f64,
    /// Realized profit/loss from closed portions.
    pub realized_pnl: f64,
}

impl Position {
    /// Creates a new position.
    pub fn new(symbol: String, side: Side, entry_price: f64, quantity: f64) -> Self {
        Self {
            symbol,
            side,
            entry_price,
            quantity,
            unrealized_pnl: 0.0,
            realized_pnl: 0.0,
        }
    }

    /// Returns the notional value of this position: `entry_price * quantity`.
    #[inline]
    pub fn notional_value(&self) -> f64 {
        self.entry_price * self.quantity
    }

    /// Updates unrealized PnL based on the current market price.
    pub fn mark_to_market(&mut self, current_price: f64) {
        let price_delta = current_price - self.entry_price;
        self.unrealized_pnl = match self.side {
            Side::Buy => price_delta * self.quantity,
            Side::Sell => -price_delta * self.quantity,
        };
    }

    /// Returns `true` if the position is fully closed (zero quantity).
    #[inline]
    pub fn is_closed(&self) -> bool {
        self.quantity == 0.0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn side_opposite() {
        assert_eq!(Side::Buy.opposite(), Side::Sell);
        assert_eq!(Side::Sell.opposite(), Side::Buy);
    }

    #[test]
    fn candle_properties() {
        let c = Candle::new(1_000_000, 100.0, 110.0, 90.0, 105.0, 500.0);
        assert_eq!(c.range(), 20.0);
        assert_eq!(c.body_midpoint(), 102.5);
        assert!(c.is_bullish());

        let bearish = Candle::new(1_000_000, 105.0, 110.0, 90.0, 100.0, 500.0);
        assert!(!bearish.is_bullish());
    }

    #[test]
    fn tick_notional() {
        let t = Tick::new(1_000_000, 50_000.0, 0.5, false);
        assert_eq!(t.notional(), 25_000.0);
    }

    #[test]
    fn order_remaining_and_status() {
        let order = Order {
            id: 1,
            symbol: "BTCUSDT".into(),
            side: Side::Buy,
            order_type: OrderType::Limit,
            time_in_force: TimeInForce::GTC,
            price: 50_000.0,
            stop_price: 0.0,
            quantity: 1.0,
            filled_quantity: 0.3,
            status: OrderStatus::PartiallyFilled,
            created_at_ms: 1_000_000,
            updated_at_ms: 1_000_001,
        };
        assert!((order.remaining_quantity() - 0.7).abs() < f64::EPSILON);
        assert!(order.is_active());
        assert!(!order.is_terminal());
    }

    #[test]
    fn order_terminal_states() {
        for status in [
            OrderStatus::Filled,
            OrderStatus::Cancelled,
            OrderStatus::Rejected,
        ] {
            let order = Order {
                id: 1,
                symbol: "ETHUSDT".into(),
                side: Side::Sell,
                order_type: OrderType::Market,
                time_in_force: TimeInForce::IOC,
                price: 0.0,
                stop_price: 0.0,
                quantity: 2.0,
                filled_quantity: 2.0,
                status,
                created_at_ms: 0,
                updated_at_ms: 0,
            };
            assert!(order.is_terminal());
            assert!(!order.is_active());
        }
    }

    #[test]
    fn position_mark_to_market_long() {
        let mut pos = Position::new("BTCUSDT".into(), Side::Buy, 50_000.0, 1.0);
        pos.mark_to_market(51_000.0);
        assert_eq!(pos.unrealized_pnl, 1_000.0);

        pos.mark_to_market(49_000.0);
        assert_eq!(pos.unrealized_pnl, -1_000.0);
    }

    #[test]
    fn position_mark_to_market_short() {
        let mut pos = Position::new("BTCUSDT".into(), Side::Sell, 50_000.0, 1.0);
        pos.mark_to_market(49_000.0);
        assert_eq!(pos.unrealized_pnl, 1_000.0);

        pos.mark_to_market(51_000.0);
        assert_eq!(pos.unrealized_pnl, -1_000.0);
    }

    #[test]
    fn position_notional_value() {
        let pos = Position::new("ETHUSDT".into(), Side::Buy, 3_000.0, 10.0);
        assert_eq!(pos.notional_value(), 30_000.0);
    }
}
