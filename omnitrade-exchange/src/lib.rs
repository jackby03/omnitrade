//! # omnitrade-exchange
//!
//! WebSocket and REST connectors for cryptocurrency exchanges.
//!
//! This crate provides the network ingestion layer for omnitrade, connecting
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
//! **Phase 2** — Not yet implemented. This is a placeholder crate.
