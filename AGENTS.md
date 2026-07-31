# AGENTS.md — AI Agent Coding Standards for omnitrade

## Project Overview

omnitrade is a free, open-source, high-performance desktop trading terminal and automated bot engine written entirely in Rust. It targets quantitative traders and developers demanding sub-millisecond execution, zero-garbage-collection pauses, and native GPU rendering without Electron.

## System Architecture

omnitrade follows a strict **Headless-First** and **Actor-Based** architecture. The core trading engine runs independently from the visual workspace, communicating via asynchronous channels to ensure UI rendering never affects order execution or market data processing.

```
omnitrade-ui              (Immediate-Mode GPU Workspace — egui + egui_tiles)
    ^                                         |
    |           tokio::sync::broadcast / mpsc  |
    |                                         v
omnitrade-engine          (Matching Engine, Paper Trading & Risk Manager)
    |                        ^
    v                        |
omnitrade-script            omnitrade-exchange
(Strategy DSL)              (WebSocket/REST Streamers)
    |                        |
    +------------+-----------+
                 v
         omnitrade-core     (Domain Types, Safe RingBuffer & TA Indicators)
```

**Data flow arrows point upward**: the UI is ONLY a passive subscriber. The engine MUST run headless with zero UI dependencies.

## Workspace Structure (Expected Files)

```
omnitrade/
├── Cargo.toml                       # Workspace manifest (resolver = "2")
├── LICENSE                           # AGPL-3.0-or-later
├── AGENTS.md                         # This file
├── omnitrade-core/                   # PHASE 1 — Foundation (IMPLEMENTED)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                    # Re-exports all core types
│       ├── error.rs                  # CoreError enum (thiserror)
│       ├── types.rs                  # Candle, Tick, Order, Position, Side, OrderType
│       ├── ring_buffer.rs            # Safe power-of-two circular buffer
│       └── indicators/
│           ├── mod.rs
│           ├── sma.rs                # Simple Moving Average (O(1) running sum)
│           ├── ema.rs                # Exponential Moving Average (Wilder smoothing)
│           ├── rsi.rs                # Relative Strength Index
│           └── volatility.rs         # Rolling std dev (Welford's algorithm)
├── omnitrade-exchange/               # PHASE 2 — Live Data (TODO)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── traits.rs                 # ExchangeStream, MarketDataStream traits
│       └── binance/                  # Binance WebSocket + REST connector
├── omnitrade-engine/                 # PHASE 3 — Paper Trading (TODO)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── matching.rs               # In-memory order book & simulated execution
│       └── account.rs                # Realized/Unrealized PnL, margin tracking
├── omnitrade-script/                 # PHASE 4 — Strategy DSL (TODO)
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs
│       ├── ast.rs                    # AST node definitions
│       ├── lexer.rs                  # Tokenizer
│       ├── parser.rs                 # AST construction
│       └── evaluator.rs              # Strategy evaluation engine
├── omnitrade-cli/                    # PHASE 4 — Headless Runner (TODO)
│   ├── Cargo.toml
│   └── src/
│       └── main.rs                   # Backtesting & automated test entrypoint
└── omnitrade-ui/                     # PHASE 5 — Desktop GUI (TODO)
    ├── Cargo.toml
    └── src/
        ├── main.rs
        └── widgets/                  # Charts, depth panels, order forms
```

## Development Phases & Gatekeeper Criteria

| Phase | Crate | Goal | Gatekeeper |
|-------|-------|------|------------|
| 1 | `omnitrade-core` | Zero-allocation data structures & TA math | 100% unit test coverage vs reference datasets |
| 2 | `omnitrade-exchange` | Resilient WebSocket/REST ingestion | 12-hour continuous feed test, no leaks or disconnects |
| 3 | `omnitrade-engine` | Paper trading & risk simulation | Automated balance reconciliation under simulated volatility |
| 4 | `omnitrade-script` + `omnitrade-cli` | Strategy DSL & backtesting | 1M candles processed < 1s in release mode |
| 5 | `omnitrade-ui` | Immediate-mode GPU workspace | Stable 60-144 FPS with active feed, zero engine delays |

## Engineering Guarantees

1. **Safe Rust First**: Zero `unsafe` blocks in core business logic.
2. **No UI Contention**: UI never executes trading logic; acts solely as a passive view consuming state snapshots.
3. **Cross-Platform Credentials**: API keys encrypted via OS-native keystores (`keyring` crate: Windows Credential Manager, macOS Keychain, Linux Secret Service).
4. **License**: AGPL-3.0-or-later — all SaaS/hosting modifications MUST release source back to the community.

---

## Coding Standards

### 1. Modular File Sizes (Strict Limit)

- **Maximum file length: 300–400 lines.** Files exceeding 500 lines are PROHIBITED.
- Break large modules into directories (e.g., `indicators/mod.rs`, `rsi.rs`, `sma.rs`).
- **Single Responsibility Principle (SRP)**: Each struct or file must have exactly one reason to change.

### 2. SOLID Principles in Rust Idioms

- **S (SRP)**: Keep structs focused. Do NOT mix networking, state evaluation, and UI rendering in one struct.
- **O (OCP)**: Use traits for behavior expansion (e.g., `ExchangeStream` trait for Binance, Bybit).
- **L (LSP)**: All trait implementations MUST fulfill the behavioral contract without panicking.
- **I (ISP)**: Favor small, composable traits (e.g., `MarketDataStream` and `OrderExecutor`) over large monolithic interfaces.
- **D (DIP)**: High-level logic (`omnitrade-engine`) MUST depend on trait abstractions, not specific exchange implementations.

### 3. Error Handling & Memory Safety

- **ABSOLUTELY NO `.unwrap()` OR `.expect()` IN PRODUCTION CODE.** Propagate errors using `?` and `Result<T, E>`. `.expect()` is ONLY allowed in unit tests or initialization routines with clear rationale.
- **Strongly typed errors**: Define domain-specific error enums per crate using `thiserror`.
- **Zero Unsafe Code**: Do NOT write `unsafe` blocks. Use standard library safe abstractions.
- **Minimize allocations**: Avoid unnecessary `.clone()`. Prefer slicing (`&[T]`) and string references (`&str`) over heap allocations (`Vec<T>`, `String`) in hot execution paths.

### 4. Function Design & Conventions

- Keep functions short and single-purpose (target: 10–30 lines).
- **Naming conventions**:
  - `as_` — cheap reference conversions
  - `to_` — costly conversions
  - `into_` — ownership-consuming operations
- **Constructor patterns**: Use `new()` or the Builder pattern for complex initializations.
- **Documentation**: Add `///` doc comments for ALL public traits, structs, enums, and functions.

### 5. Testing Requirements

- Include inline `#[cfg(test)] mod tests` in every file using Arrange-Act-Assert pattern.
- Cover core logic AND edge cases (empty inputs, boundary values, error paths).
- Ensure all generated code passes:
  ```
  cargo check
  cargo fmt
  cargo clippy -- -D warnings
  ```

### 6. Workspace Conventions

- All crates use `edition.workspace = true`, `license.workspace = true`, `rust-version.workspace = true`, `repository.workspace = true`.
- Re-export key types in `lib.rs` for each crate.
- Use `#[cfg(feature = "std")]` gates for `no_std` compatibility where applicable.

---

## Before Committing

Run these checks and fix ALL issues before marking work complete:

```sh
cargo fmt --all
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```
