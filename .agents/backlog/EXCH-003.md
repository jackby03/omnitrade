# EXCH-003: Binance WebSocket Connection Manager

| Field | Value |
|---|---|
| Status | `DONE` |
| Phase | 2 |
| Crate | `omnitrade-exchange` |
| Target | `src/binance/connection.rs` |
| Depends On | `EXCH-001` |
| Blocks | `EXCH-004` |
| Complexity | L |

## Description

Resilient WebSocket connection manager for Binance streams. Handles connection lifecycle, exponential backoff reconnection, and raw message forwarding. This module is ONLY responsible for maintaining the TCP/WS connection — message parsing and routing happen in EXCH-004.

## Requirements

- Define `struct BinanceConnection` with fields:
  - `base_url: String` (default: `wss://stream.binance.com:9443/ws`)
  - `streams: Vec<String>` (e.g., `["btcusdt@kline_1m", "btcusdt@depth@100ms"]`)
  - `raw_tx: mpsc::Sender<String>` — channel for raw JSON messages
  - `state: ConnectionState` enum (`Disconnected`, `Connecting`, `Connected`, `Reconnecting`)
- Implement `async fn connect(&mut self) -> Result<(), ExchangeError>`:
  - Build combined stream URL: `{base_url}/{stream1}/{stream2}/...`
  - Open `tokio-tungstenite` WebSocket connection
  - Spawn a `tokio::task` that reads messages and sends raw strings into `raw_tx`
- Implement exponential backoff reconnection:
  - On disconnect: wait 1s, 2s, 4s, 8s, 16s, cap at 30s
  - Log each attempt via `tracing::warn!`
  - Reset backoff on successful reconnection
- Implement `async fn disconnect(&mut self) -> Result<(), ExchangeError>`
- Implement `fn add_stream(&mut self, stream: &str)`

## Acceptance Criteria

- [x] `ConnectionState` transitions are tested (Disconnected → Connecting → Connected)
- [x] Backoff calculation logic is unit-tested (1, 2, 4, 8, 16, 30, 30, 30...)
- [x] `add_stream` correctly builds the combined URL
- [x] No `.unwrap()` or `.expect()` in production code
- [x] Uses `tracing` for structured logging (not `println!`)

## Cargo Dependencies

```toml
tokio = { version = "1", features = ["rt-multi-thread", "macros", "time", "sync"] }
tokio-tungstenite = { version = "0.24", features = ["native-tls"] }
futures-util = "0.3"
tracing = "0.1"
```
