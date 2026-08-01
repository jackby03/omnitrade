//! Binance WebSocket Data Transfer Objects (DTOs) and conversion implementations.

use omnitrade_core::Candle;
use serde::{Deserialize, Serialize};

use crate::{DepthUpdate, ExchangeError};

/// Generic wrapper for Binance WebSocket stream messages.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceStreamWrapper<T> {
    /// Name of the stream (e.g., "btcusdt@kline_1m").
    pub stream: String,
    /// Payload data object.
    pub data: T,
}

/// Raw kline/candlestick payload within a Binance kline WebSocket event.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceKlineData {
    /// Kline start time in milliseconds.
    #[serde(rename = "t")]
    pub t: u64,
    /// Kline close time in milliseconds.
    #[serde(default, rename = "T")]
    pub end_time: u64,
    /// Symbol.
    #[serde(default, rename = "s")]
    pub s: String,
    /// Interval.
    #[serde(default, rename = "i")]
    pub i: String,
    /// First trade ID.
    #[serde(default, rename = "f")]
    pub f: u64,
    /// Last trade ID.
    #[serde(default, rename = "L")]
    pub last_trade_id: u64,
    /// Open price string.
    #[serde(rename = "o")]
    pub o: String,
    /// Close price string.
    #[serde(rename = "c")]
    pub c: String,
    /// High price string.
    #[serde(rename = "h")]
    pub h: String,
    /// Low price string.
    #[serde(rename = "l")]
    pub l: String,
    /// Base asset volume string.
    #[serde(rename = "v")]
    pub v: String,
    /// Number of trades.
    #[serde(default, rename = "n")]
    pub n: u64,
    /// Is this kline closed?
    #[serde(default, rename = "x")]
    pub x: bool,
    /// Quote asset volume string.
    #[serde(default, rename = "q")]
    pub q: String,
    /// Taker buy base asset volume string.
    #[serde(default, rename = "V")]
    pub taker_buy_base_volume: String,
    /// Taker buy quote asset volume string.
    #[serde(default, rename = "Q")]
    pub taker_buy_quote_volume: String,
    /// Ignore string.
    #[serde(default, rename = "B")]
    pub ignore: String,
}

/// Binance WebSocket kline event message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceKlineEvent {
    /// Event type ("kline").
    #[serde(rename = "e")]
    pub e: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: u64,
    /// Symbol (e.g., "BTCUSDT").
    #[serde(rename = "s")]
    pub s: String,
    /// Kline payload data.
    #[serde(rename = "k")]
    pub k: BinanceKlineData,
}

/// Binance WebSocket order book depth update event message.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BinanceDepthEvent {
    /// Event type ("depthUpdate").
    #[serde(rename = "e")]
    pub e: String,
    /// Event time in milliseconds.
    #[serde(rename = "E")]
    pub event_time: u64,
    /// Symbol (e.g., "BTCUSDT").
    #[serde(rename = "s")]
    pub s: String,
    /// Bids array of `[price, quantity]` string pairs.
    #[serde(rename = "b")]
    pub b: Vec<[String; 2]>,
    /// Asks array of `[price, quantity]` string pairs.
    #[serde(rename = "a")]
    pub a: Vec<[String; 2]>,
}

impl TryFrom<BinanceKlineEvent> for Candle {
    type Error = ExchangeError;

    fn try_from(event: BinanceKlineEvent) -> Result<Self, Self::Error> {
        let k = event.k;
        let open =
            k.o.parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: k.o.clone(),
                    reason: format!("failed to parse open price: {e}"),
                })?;
        let high =
            k.h.parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: k.h.clone(),
                    reason: format!("failed to parse high price: {e}"),
                })?;
        let low =
            k.l.parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: k.l.clone(),
                    reason: format!("failed to parse low price: {e}"),
                })?;
        let close =
            k.c.parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: k.c.clone(),
                    reason: format!("failed to parse close price: {e}"),
                })?;
        let volume =
            k.v.parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: k.v.clone(),
                    reason: format!("failed to parse volume: {e}"),
                })?;

        Ok(Candle::new(k.t, open, high, low, close, volume))
    }
}

impl TryFrom<BinanceDepthEvent> for DepthUpdate {
    type Error = ExchangeError;

    fn try_from(event: BinanceDepthEvent) -> Result<Self, Self::Error> {
        let mut bids = Vec::with_capacity(event.b.len());
        for level in &event.b {
            let price = level[0]
                .parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: level[0].clone(),
                    reason: format!("failed to parse bid price: {e}"),
                })?;
            let qty = level[1]
                .parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: level[1].clone(),
                    reason: format!("failed to parse bid qty: {e}"),
                })?;
            bids.push((price, qty));
        }

        let mut asks = Vec::with_capacity(event.a.len());
        for level in &event.a {
            let price = level[0]
                .parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: level[0].clone(),
                    reason: format!("failed to parse ask price: {e}"),
                })?;
            let qty = level[1]
                .parse::<f64>()
                .map_err(|e| ExchangeError::MessageParseFailed {
                    raw: level[1].clone(),
                    reason: format!("failed to parse ask qty: {e}"),
                })?;
            asks.push((price, qty));
        }

        Ok(DepthUpdate::new(event.event_time, bids, asks))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_wrapper_deserialization() {
        // Arrange
        let json = r#"{
            "stream": "btcusdt@kline_1m",
            "data": {
                "e": "kline",
                "E": 123456789,
                "s": "BTCUSDT",
                "k": {
                    "t": 123400000,
                    "o": "0.001",
                    "h": "0.005",
                    "l": "0.001",
                    "c": "0.004",
                    "v": "100",
                    "x": true
                }
            }
        }"#;

        // Act
        let wrapper: BinanceStreamWrapper<BinanceKlineEvent> =
            serde_json::from_str(json).expect("failed to deserialize BinanceStreamWrapper");

        // Assert
        assert_eq!(wrapper.stream, "btcusdt@kline_1m");
        assert_eq!(wrapper.data.s, "BTCUSDT");
        assert_eq!(wrapper.data.k.t, 123400000);
    }

    #[test]
    fn kline_event_to_candle_conversion_success() {
        // Arrange
        let json = r#"{
            "e": "kline",
            "E": 123456789,
            "s": "BTCUSDT",
            "k": {
                "t": 123400000,
                "o": "0.001",
                "h": "0.005",
                "l": "0.001",
                "c": "0.004",
                "v": "100",
                "x": true
            }
        }"#;

        // Act
        let event: BinanceKlineEvent =
            serde_json::from_str(json).expect("failed to deserialize BinanceKlineEvent");
        let candle: Candle =
            Candle::try_from(event).expect("failed to convert BinanceKlineEvent to Candle");

        // Assert
        assert_eq!(candle.timestamp_ms, 123400000);
        assert_eq!(candle.open, 0.001);
        assert_eq!(candle.high, 0.005);
        assert_eq!(candle.low, 0.001);
        assert_eq!(candle.close, 0.004);
        assert_eq!(candle.volume, 100.0);
    }

    #[test]
    fn kline_event_to_candle_conversion_failure_invalid_price() {
        // Arrange
        let json = r#"{
            "e": "kline",
            "E": 123456789,
            "s": "BTCUSDT",
            "k": {
                "t": 123400000,
                "o": "0.001",
                "h": "invalid",
                "l": "0.001",
                "c": "0.004",
                "v": "100",
                "x": true
            }
        }"#;

        // Act
        let event: BinanceKlineEvent =
            serde_json::from_str(json).expect("failed to deserialize BinanceKlineEvent");
        let result: Result<Candle, ExchangeError> = Candle::try_from(event);

        // Assert
        assert!(result.is_err());
        if let Err(ExchangeError::MessageParseFailed { raw, reason }) = result {
            assert_eq!(raw, "invalid");
            assert!(reason.contains("failed to parse high price"));
        } else {
            panic!("Expected MessageParseFailed error variant");
        }
    }

    #[test]
    fn depth_event_to_depth_update_conversion_success() {
        // Arrange
        let json = r#"{
            "e": "depthUpdate",
            "E": 123456789,
            "s": "BTCUSDT",
            "b": [["0.0024", "10"]],
            "a": [["0.0026", "100"]]
        }"#;

        // Act
        let event: BinanceDepthEvent =
            serde_json::from_str(json).expect("failed to deserialize BinanceDepthEvent");
        let depth: DepthUpdate = DepthUpdate::try_from(event)
            .expect("failed to convert BinanceDepthEvent to DepthUpdate");

        // Assert
        assert_eq!(depth.timestamp_ms, 123456789);
        assert_eq!(depth.bids, vec![(0.0024, 10.0)]);
        assert_eq!(depth.asks, vec![(0.0026, 100.0)]);
    }

    #[test]
    fn depth_event_to_depth_update_conversion_failure_invalid_qty() {
        // Arrange
        let json = r#"{
            "e": "depthUpdate",
            "E": 123456789,
            "s": "BTCUSDT",
            "b": [["0.0024", "bad_qty"]],
            "a": [["0.0026", "100"]]
        }"#;

        // Act
        let event: BinanceDepthEvent =
            serde_json::from_str(json).expect("failed to deserialize BinanceDepthEvent");
        let result: Result<DepthUpdate, ExchangeError> = DepthUpdate::try_from(event);

        // Assert
        assert!(result.is_err());
        if let Err(ExchangeError::MessageParseFailed { raw, reason }) = result {
            assert_eq!(raw, "bad_qty");
            assert!(reason.contains("failed to parse bid qty"));
        } else {
            panic!("Expected MessageParseFailed error variant");
        }
    }
}
