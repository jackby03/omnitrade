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
pub mod credentials;
pub mod error;
pub mod keystore;
pub mod kraken;
pub mod okx;
pub mod trading;
pub mod traits;
pub mod venue;

pub use binance::{
    fetch_depth_snapshot, fetch_klines, fetch_symbol_filters, BinanceClient, BinanceMarginClient,
    BinanceRestClient, DepthSnapshot, SymbolFilters,
};
pub use credentials::{ApiCredentials, Network};
pub use error::ExchangeError;
pub use keystore::Keystore;
pub use kraken::{KrakenClient, KrakenRestClient};
pub use okx::{OkxClient, OkxRestClient};
pub use trading::{
    Account, AccountBalance, ExchangeTrading, FillDetails, OrderRequest, PositionSnapshot,
};
pub use traits::{DepthUpdate, ExchangeInfo, ExchangeStream};
pub use venue::Exchange;
