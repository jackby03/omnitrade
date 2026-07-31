# AGENTS.md — omnitrade

## Start Here

Before doing any work, read these two files in order:

1. **[.agents/AGENTS.md](.agents/AGENTS.md)** — Project-wide coding standards, SOLID rules, error handling conventions, and testing requirements.
2. **[.agents/backlog/README.md](.agents/backlog/README.md)** — Ticket orchestration system, dependency DAG, parallel execution groups, and ticket format.

## Quick Context

omnitrade is a Rust workspace for a high-performance desktop trading terminal and bot engine.

- **Architecture**: Headless-First, Actor-Based. UI is a passive subscriber only.
- **License**: AGPL-3.0-or-later. All SaaS modifications must release source.
- **Phase 1 (core)**: DONE. Phases 2–5 are pending in the backlog.

## Pre-Commit Checklist

```sh
cargo fmt --all
cargo check --workspace
cargo clippy --workspace -- -D warnings
cargo test --workspace
```
