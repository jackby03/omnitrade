# EXCH-004: Binance Message Router

| Field | Value |
|---|---|
| Status | `TODO` |
| Phase | 2 |
| Crate | `omnitrade-exchange` |
| Target | `src/binance/router.rs` |
| Depends On | `EXCH-002`, `EXCH-003` |
| Blocks | `EXCH-005` |
| Complexity | M |

## Description

Message routing layer that receives raw JSON strings from the `BinanceConnection`, deserializes them using the DTOs from EXCH-002, converts them to domain types, and dispatches to the appropriate typed channels.

## Requirements

- Define `struct BinanceRouter` with fields:
  - `raw_rx: mpsc::Receiver<String>` — receives raw messages from `BinanceConnection`
  - `candle_senders: HashMap<String, mpsc::Sender<Candle>>` — per-symbol candle channels
  - `depth_senders: HashMap<String, mpsc::Sender<DepthUpdate>>` — per-symbol depth channels
- Implement `async fn run(&mut self) -> Result<(), ExchangeError>`:
  - Loop over `raw_rx`, deserialize each message
  - Route to the appropriate sender based on event type (`kline` vs `depthUpdate`)
  - On parse failure: log with `tracing::warn!` and continue (don't crash)
- Implement `fn register_candle_channel(&mut self, symbol: &str) -> mpsc::Receiver<Candle>`
- Implement `fn register_depth_channel(&mut self, symbol: &str) -> mpsc::Receiver<DepthUpdate>`

## Acceptance Criteria

- [ ] Valid kline JSON is routed to the correct candle sender
- [ ] Valid depth JSON is routed to the correct depth sender
- [ ] Unknown event types are logged and skipped (no panic)
- [ ] Malformed JSON is logged and skipped (no panic)
- [ ] Channel registration returns a working `Receiver`
- [ ] Unit tests use static JSON payloads (no network)

## Cargo Dependencies

```toml
serde_json = "1"
tracing = "0.1"
```
