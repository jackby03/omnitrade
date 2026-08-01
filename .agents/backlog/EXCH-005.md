# EXCH-005: Exchange Crate Wiring & BinanceClient Facade

| Field | Value |
|---|---|
| Status | `DONE` |
| Phase | 2 |
| Crate | `omnitrade-exchange` |
| Target | `src/lib.rs`, `src/binance/mod.rs` |
| Depends On | `EXCH-001`, `EXCH-002`, `EXCH-003`, `EXCH-004` |
| Blocks | `ENG-004` |
| Complexity | M |

## Description

Wire all exchange modules together and implement the `BinanceClient` facade that implements the `ExchangeStream` trait by composing `BinanceConnection` + `BinanceRouter`.

## Requirements

### `src/binance/mod.rs`
- Re-export `BinanceClient`, `BinanceConnection`, `BinanceRouter`
- Module declarations for `client`, `connection`, `dto`, `router`

### `src/binance/client.rs`
- Define `struct BinanceClient` that composes:
  - `BinanceConnection` (from EXCH-003)
  - `BinanceRouter` (from EXCH-004)
- Implement `ExchangeStream` for `BinanceClient`:
  - `connect()` → start connection + spawn router task
  - `subscribe_candles()` → register channel on router + add stream to connection
  - `subscribe_depth()` → register channel on router + add stream to connection
  - `disconnect()` → stop connection and router
- Implement `ExchangeInfo` for `BinanceClient`:
  - `name() → "Binance"`
  - `supported_intervals() → &["1m", "3m", "5m", "15m", "30m", "1h", "4h", "1d"]`

### `src/lib.rs`
- Module declarations and public re-exports:
  - `pub use error::ExchangeError`
  - `pub use traits::{ExchangeStream, ExchangeInfo, DepthUpdate}`
  - `pub use binance::BinanceClient`

## Acceptance Criteria

- [x] `BinanceClient` implements `ExchangeStream`
- [x] `BinanceClient` implements `ExchangeInfo`
- [x] `lib.rs` compiles and re-exports all public types
- [x] All public items have `///` doc comments

## Cargo Dependencies

No new dependencies — uses those from EXCH-001 through EXCH-004.
