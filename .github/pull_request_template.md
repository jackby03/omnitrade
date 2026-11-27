## Ticket

<!-- Backlog-ID aus .agents/backlog/ (z. B. ENG-004) oder Issue-Nummer -->

## Änderung

<!-- Was ändert sich, und warum. -->

## Prüfliste (aus .agents/AGENTS.md)

- [ ] `cargo fmt --all -- --check` besteht
- [ ] `cargo check --workspace` besteht
- [ ] `cargo clippy --workspace -- -D warnings` besteht
- [ ] `cargo test --workspace` besteht
- [ ] Keine `.unwrap()`, `.expect()`, `panic!` oder `unsafe` im Produktionscode (nur in Tests)
- [ ] Jede geänderte Datei bleibt unter 250 Zeilen Produktionscode (Obergrenze 500)
- [ ] Nur Dateien geändert, die im Ticket als `Target` stehen
- [ ] Neue Tests mit `#[cfg(test)]` und Arrange-Act-Assert

## Handel

<!-- Nur ausfüllen, wenn der Pull Request Handelslogik berührt. Sonst "nicht betroffen". -->
