//! # omnitrade-engine
//!
//! Order matching engine, paper trading simulation, and portfolio state.
//!
//! This crate contains the core trading logic:
//! - **MatchingEngine**: Local order book with simulated execution
//! - **Account**: Realized/unrealized PnL tracking and margin management
//! - **Risk Manager**: Position limits, drawdown guards, and fee models
//!
//! The engine operates completely headless — it has no dependency on the UI
//! crate and communicates via message channels (`tokio::sync::broadcast/mpsc`).

pub mod error;
pub mod fill;

pub use error::EngineError;
pub use fill::{OrderFill, OrderFillBuilder};
