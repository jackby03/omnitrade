//! Kraken Spot connector: WebSocket v2 market data and read-only account access.
//!
//! Kraken Spot has no testnet, so order placement is deliberately not implemented
//! (see [`rest::KrakenRestClient`]).

pub mod client;
pub mod klines;
pub mod rest;
pub mod router;
pub mod signing;

pub use client::KrakenClient;
pub use rest::KrakenRestClient;
pub use router::KrakenRouter;
pub use signing::KrakenSigner;
