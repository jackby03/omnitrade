# EXCH-002: Binance WebSocket DTOs

| Field | Value |
|---|---|
| Status | `TODO` |
| Phase | 2 |
| Crate | `omnitrade-exchange` |
| Target | `src/binance/dto.rs` |
| Depends On | `EXCH-001` |
| Blocks | `EXCH-004` |
| Complexity | M |

## Description

Serde Data Transfer Objects for Binance WebSocket stream payloads. These structs map directly to Binance's JSON wire format and provide `TryFrom` conversions into omnitrade-core domain types.

## Requirements

- Define `BinanceStreamWrapper<T>` — outer envelope: `{ stream: String, data: T }`
- Define `BinanceKlineEvent`:
  ```json
  { "e": "kline", "E": 123456789, "s": "BTCUSDT", "k": {
      "t": 123400000, "o": "0.001", "h": "0.005", "l": "0.001",
      "c": "0.004", "v": "100", "x": true
  }}
  ```
- Define `BinanceDepthEvent`:
  ```json
  { "e": "depthUpdate", "E": 123456789, "s": "BTCUSDT",
    "b": [["0.0024","10"]], "a": [["0.0026","100"]] }
  ```
- Implement `TryFrom<BinanceKlineEvent> for Candle` — parse string prices to `f64`, return `ExchangeError::MessageParseFailed` on failure.
- Implement `TryFrom<BinanceDepthEvent> for DepthUpdate` — same error handling.
- Use `#[serde(rename = "...")]` for field mapping.

## Acceptance Criteria

- [ ] `BinanceKlineEvent` deserializes from a raw JSON string matching Binance docs
- [ ] `BinanceDepthEvent` deserializes from a raw JSON string matching Binance docs
- [ ] `TryFrom<BinanceKlineEvent>` produces a valid `Candle` with correct field mapping
- [ ] `TryFrom<BinanceDepthEvent>` produces a valid `DepthUpdate`
- [ ] Malformed JSON returns `Err`, not a panic
- [ ] Unit tests use static `&str` payloads (no network calls)

## Cargo Dependencies

```toml
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```
