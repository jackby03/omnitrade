//! OKX connector: WebSocket v5 market data and REST trading (demo or production).
//!
//! `Network::Testnet` selects OKX **demo trading** (separate demo API keys, REST header
//! `x-simulated-trading: 1`, `wspap` WebSocket hosts).

pub mod client;
pub mod instrument;
pub mod klines;
pub mod rest;
pub mod router;
pub mod signing;
pub mod swap;
pub mod swap_client;

pub use client::OkxClient;
pub use instrument::{InstType, Instrument};
pub use rest::OkxRestClient;
pub use router::OkxRouter;
pub use signing::OkxSigner;
