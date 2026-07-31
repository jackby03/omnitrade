# EXCH-001: Exchange Traits & Error Types

| Field | Value |
|---|---|
| Status | `DONE` |
| Phase | 2 |
| Crate | `omnitrade-exchange` |
| Target | `src/error.rs`, `src/traits.rs` |
| Depends On | `CORE-001`, `CORE-002` |
| Blocks | `EXCH-002`, `EXCH-003`, `EXCH-004` |
| Complexity | M |

## Description

Define the foundational traits and error types for all exchange connectors. This establishes the abstraction layer that decouples the engine from specific exchange implementations (Dependency Inversion Principle).

## Requirements

### `src/error.rs`
- Create `ExchangeError` enum using `thiserror`:
  - `ConnectionFailed { url: String, reason: String }`
  - `MessageParseFailed { raw: String, reason: String }`
  - `SubscriptionFailed { symbol: String, reason: String }`
  - `ChannelClosed`
  - `Timeout { duration_ms: u64 }`
  - `Core(#[from] omnitrade_core::CoreError)` — transparent propagation

### `src/traits.rs`
- Create `DepthUpdate` struct: `{ timestamp_ms: u64, bids: Vec<(f64, f64)>, asks: Vec<(f64, f64)> }`
- Create `async trait ExchangeStream` with methods:
  - `async fn connect(&mut self) -> Result<(), ExchangeError>`
  - `async fn subscribe_candles(&mut self, symbol: &str, interval: &str) -> Result<mpsc::Receiver<Candle>, ExchangeError>`
  - `async fn subscribe_depth(&mut self, symbol: &str) -> Result<mpsc::Receiver<DepthUpdate>, ExchangeError>`
  - `async fn disconnect(&mut self) -> Result<(), ExchangeError>`
- Create `trait ExchangeInfo` (non-async, sync metadata):
  - `fn name(&self) -> &str`
  - `fn supported_intervals(&self) -> &[&str]`

## Acceptance Criteria

- [x] `ExchangeError` implements `std::error::Error` and `Display`
- [x] `ExchangeStream` is object-safe and usable as `Box<dyn ExchangeStream>`
- [x] `DepthUpdate` derives `Debug`, `Clone`, `PartialEq`
- [x] Unit tests verify error `Display` formatting
- [x] All items have `///` doc comments
- [x] File stays under 250 lines

## Cargo Dependencies

```toml
[dependencies]
omnitrade-core = { path = "../omnitrade-core" }
thiserror = "2"
tokio = { version = "1", features = ["sync"] }
async-trait = "0.1"
```
