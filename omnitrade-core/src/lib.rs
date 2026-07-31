//! # omnitrade-core
//!
//! Foundation crate for the omnitrade trading engine.
//!
//! Provides zero-allocation data structures and mathematical primitives:
//! - **Domain types**: `Side`, `OrderType`, `Candle`, `Tick`, `Order`, `Position`
//! - **RingBuffer**: Safe, generic, power-of-two circular buffer with bitmask indexing
//! - **Technical Indicators**: SMA, EMA, RSI, Volatility (rolling std dev)
//!
//! This crate is `no_std`-compatible (with the `std` feature flag) and uses
//! zero external dependencies for all business logic.

#![cfg_attr(not(feature = "std"), no_std)]

pub mod error;
pub mod indicators;
pub mod ring_buffer;
pub mod types;

pub use error::CoreError;
pub use indicators::{Ema, Rsi, Sma, Volatility};
pub use ring_buffer::RingBuffer;
pub use types::*;
