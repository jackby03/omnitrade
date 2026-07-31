# omnitrade — Coding Standards

## Architecture
- **Headless-First**: The engine runs completely independently of the UI.
- **Zero UI Contention**: UI is a passive subscriber consuming state snapshots via channels.
- **Async Actor Model**: Prefer message passing (mpsc/broadcast) over `Arc<Mutex<T>>`.
- **Cross-Platform Keystores**: Use `keyring` crate. No OS-specific DPAPI/Keychain calls.

## File Organization
- Maximum file length: **300–400 lines**. Files exceeding **500 lines are prohibited**.
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
