//! Binance exchange connector implementations.

pub mod client;
pub mod connection;
pub mod dto;
pub mod filters;
pub mod klines;
pub mod margin;
pub mod rest;
pub mod router;
pub mod signing;

pub use client::BinanceClient;
pub use connection::{BinanceConnection, ConnectionState};
pub use dto::{BinanceDepthEvent, BinanceKlineEvent, BinanceStreamWrapper};
pub use filters::{fetch_symbol_filters, SymbolFilters};
pub use klines::{fetch_depth_snapshot, fetch_klines, DepthSnapshot};
pub use margin::BinanceMarginClient;
pub use rest::BinanceRestClient;
pub use router::BinanceRouter;
pub use signing::BinanceSigner;
