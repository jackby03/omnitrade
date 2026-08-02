# omnitrade — Coding Standards & Agent Orchestration

## Task Backlog

All engineering tickets live in `.agents/backlog/`. Read [backlog/README.md](backlog/README.md) for the orchestration system, dependency DAG, and parallel execution groups.

### Quick Reference

| Phase | Tickets | Crate | Status |
|---|---|---|---|
| 1 | `CORE-001` → `CORE-004` | `omnitrade-core` | ✅ DONE |
| 2 | `EXCH-001` → `EXCH-005` | `omnitrade-exchange` | ✅ DONE |
| 3 | `ENG-001` → `ENG-006` | `omnitrade-engine` | ✅ DONE |
| 4 | `SCR-001` → `SCR-005` | `omnitrade-script` | 🔲 TODO |
| 4 | `CLI-001` → `CLI-002` | `omnitrade-cli` | 🔲 TODO |
| 5 | `UI-001` → `UI-004` | `omnitrade-ui` | 🔲 TODO |

### Assigning a Ticket to a Worker Agent

```
System Role: You are a specialized, single-task Rust agent working on the `omnitrade` workspace.

Strict Rules:
1. Read and follow `.agents/AGENTS.md` for project-wide coding standards.
2. Only modify files listed in your ticket's Target field.
3. Keep each file under 250 lines of production code.
4. Include comprehensive unit tests (#[cfg(test)]) using Arrange-Act-Assert.
5. NO panic!, .unwrap(), .expect(), or unsafe in production code.
   (.expect() is ONLY allowed in tests with a descriptive message.)
6. Return Result<T, E> for all fallible operations.

Task: Complete ticket {TICKET_ID}.
Target Crate: {CRATE}
Target File: {TARGET}

Requirements:
{PASTE FROM TICKET FILE}
```

---

## Architecture

- **Headless-First**: The engine runs completely independently of the UI.
- **Zero UI Contention**: UI is a passive subscriber consuming state snapshots via channels.
- **Async Actor Model**: Prefer message passing (mpsc/broadcast) over `Arc<Mutex<T>>`.
- **Cross-Platform Keystores**: Use `keyring` crate. No OS-specific DPAPI/Keychain calls.

## File Organization

- Maximum file length: **250 lines per ticket output**, **300–400 lines general**. Files exceeding **500 lines are prohibited**.
- Break large modules into directories (`indicators/mod.rs`, `rsi.rs`, `sma.rs`).
- Single Responsibility Principle: each struct/file has exactly one reason to change.

## SOLID in Rust

- **SRP**: Don't mix networking, state evaluation, and UI rendering in one struct.
- **OCP**: Use traits for behavior expansion (e.g., `ExchangeStream` for Binance/Bybit).
- **LSP**: All trait implementations must fulfill the behavioral contract without panicking.
- **ISP**: Favor small, composable traits over large monolithic interfaces.
- **DIP**: High-level logic depends on trait abstractions, not concrete implementations.

## Error Handling

- **ABSOLUTELY NO `.unwrap()` OR `.expect()` IN PRODUCTION CODE.** Only in tests/init.
- Propagate errors with `?` and `Result<T, E>`.
- Define domain-specific error enums per crate using `thiserror`.

## Memory Safety

- Zero `unsafe` blocks in business logic.
- Minimize `.clone()`. Prefer `&[T]` and `&str` over `Vec<T>` / `String` in hot paths.

## Function Design

- Keep functions short (10–30 lines target).
- Naming: `as_` for cheap refs, `to_` for costly conversions, `into_` for ownership.
- Use `new()` or Builder pattern for struct construction.
- Add `///` doc comments for all public items.

## Testing

- Inline `#[cfg(test)] mod tests` in every file.
- Use Arrange-Act-Assert pattern.
- All code must pass `cargo check`, `cargo fmt`, `cargo clippy -- -D warnings`.
