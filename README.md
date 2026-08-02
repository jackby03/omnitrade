# omnitrade

High-performance trading terminal and bot engine, built natively in Rust.

[![License: AGPL-3.0](https://img.shields.io/badge/License-AGPL--3.0-blue.svg)](LICENSE)
[![Ko-fi](https://img.shields.io/badge/Support-Ko--fi-FF5E5B?logo=kofi&logoColor=white)](https://ko-fi.com/jackby03)

## Overview

omnitrade is a headless-first, actor-based trading platform. The engine runs independently of any UI; desktop clients are passive subscribers consuming state snapshots via channels.

- **Cross-exchange** — unified WebSocket and REST connectors for Binance, Bybit, and beyond.
- **Paper trading** — built-in matching engine and risk manager for backtesting and simulation.
- **Strategy DSL** — scriptable strategies with a custom lexer, AST, and evaluator.
- **GPU desktop** — immediate-mode GUI via `egui`, rendered in a tiled workspace.

## Crates

| Crate | Description |
|---|---|
| `omnitrade-core` | Domain types, ring buffer, technical indicators |
| `omnitrade-exchange` | Async exchange connectors (WebSocket + REST) |
| `omnitrade-engine` | Order matching, paper trading, PnL tracking |
| `omnitrade-script` | Strategy DSL interpreter |
| `omnitrade-cli` | Headless CLI for backtesting and live trading |
| `omnitrade-ui` | Desktop GUI (egui + egui_tiles) |

## Quick Start

```bash
git clone https://github.com/jackby03/omnitrade.git
cd omnitrade

cargo build --workspace
cargo test --workspace
cargo clippy --workspace -- -D warnings
```

## License

[AGPL-3.0-or-later](LICENSE). All SaaS modifications must release source.
