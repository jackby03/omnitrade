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
//! **Phase 2 (In Progress)**: `EXCH-001` core traits and errors completed.

pub mod error;
pub mod traits;

pub use error::ExchangeError;
pub use traits::{DepthUpdate, ExchangeInfo, ExchangeStream};
