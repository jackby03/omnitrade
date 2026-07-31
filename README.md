# omnitrade

High-performance, open-source trading terminal & bot engine built natively in Rust.

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)

## Architecture

```
omnitrade-ui (Immediate Mode GPU Workspace via egui)
      ^
      │ tokio::sync::broadcast / mpsc
      v
omnitrade-engine (Matching Engine, Paper Trading & Risk Manager)
      ├── omnitrade-script (Strategy DSL Interpreter)
      └── omnitrade-exchange (WebSocket & REST API client)
            └── omnitrade-core (Domain Types, Safe RingBuffers & Vector Math)
```

## Workspace Crates

| Crate | Purpose | Status |
|---|---|---|
| `omnitrade-core` | Domain types, RingBuffer, TA indicators | ✅ Phase 1 |
| `omnitrade-exchange` | WebSocket & REST connectors (Binance, Bybit) | 🔲 Phase 2 |
| `omnitrade-engine` | Order matching, paper trading, PnL tracking | 🔲 Phase 3 |
| `omnitrade-script` | Strategy DSL (Lexer, AST, Evaluator) | 🔲 Phase 4 |
| `omnitrade-cli` | Headless CLI for backtesting | 🔲 Phase 4 |
| `omnitrade-ui` | GPU desktop GUI (egui + egui_tiles) | 🔲 Phase 5 |

## Building

```bash
# Build the entire workspace
cargo build --workspace

# Run all tests
cargo test --workspace

# Check for lint warnings
cargo clippy --workspace -- -D warnings
```

## License

Licensed under the [GNU Affero General Public License v3.0](LICENSE).
