---
name: Backlog-Ticket
about: Atomares Ticket im Format von .agents/backlog/README.md
title: "[ID]: Titel"
labels: []
---

| Feld | Wert |
|---|---|
| Status | TODO |
| Phase | 2-5 |
| Crate | omnitrade-xxx |
| Target | src/path.rs |
| Depends On | TICKET-IDs oder "keine" |
| Blocks | TICKET-IDs oder "keine" |
| Complexity | S (< 100 LOC) / M (100-200 LOC) / L (200-250 LOC) |

## Description

<!-- Ziel des Tickets in zwei bis drei Sätzen. -->

## Requirements

<!-- Konkrete, prüfbare Anforderungen. -->

## Acceptance Criteria

- [ ] `cargo fmt`, `cargo check`, `cargo clippy -- -D warnings` und `cargo test` bestehen
- [ ] Nur die Datei aus `Target` wurde geändert
- [ ] Neue Tests vorhanden (Arrange-Act-Assert)

## Cargo Dependencies

<!-- Neue Abhängigkeiten mit Version, oder "keine". -->
