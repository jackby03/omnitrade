//! # omnitrade-exchange
//!
//! WebSocket and REST connectors for cryptocurrency exchanges.
//!
//! Provides the network ingestion layer for omnitrade, connecting
//! to exchange APIs (Binance, Bybit) via async WebSocket streams with
//! exponential backoff auto-reconnection.
//!
//! ## Architecture
//!
//! All exchange connectors implement the [`ExchangeStream`] trait, enabling
//! the engine to consume market data without knowing which exchange is
//! providing it (Dependency Inversion Principle).
//!
//! ## Status
//!
//! **Phase 2 (Done)**: `EXCH-001` through `EXCH-005` completed.

pub mod binance;
pub mod error;
pub mod traits;

pub use binance::BinanceClient;
pub use error::ExchangeError;
pub use traits::{DepthUpdate, ExchangeInfo, ExchangeStream};
