---
name: run-omnitrade
description: Run, start, build, drive, click through and screenshot the omnitrade desktop app (omnitrade-ui, egui) and its CLI (omnitrade-cli validate, backtest, kill). Use when asked to run, start, launch, build, screenshot, click through, test the GUI, press or reset the NOT-AUS / kill switch, or smoke-test a strategy backtest.
---

# run-omnitrade

omnitrade ist ein Rust-Workspace: `omnitrade-ui` (egui-Desktop-GUI, reine Anzeige plus Not-Aus-Flag-Datei) und `omnitrade-cli` (validate, backtest, live, kill). Der Agent-Pfad ist ein PowerShell-Driver, der die GUI startet, sie per Screenshot festhält, den Not-Aus klickt und die Flag-Datei prüft.

**Pfade in dieser Datei sind relativ zum Repo-Root** `O:\Werkstatt\omnitrade` (dort liegt `Cargo.toml`). Getestet auf Windows 11, PowerShell 5.1 (`powershell.exe`), Rust/cargo 1.98.1.

## Agent-Pfad (zuerst lesen)

Alles läuft über `.claude/skills/run-omnitrade/driver.ps1`. Aufruf aus dem Repo-Root:

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 build
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 flow
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 cli
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 stop
```

Einzelbefehle: `start`, `shot [name]`, `killswitch`, `reset`, `status`, `fetch`.

- `flow` = `start` → Screenshot `01_start.png` → `killswitch` → `reset` → `status`. Exit 0 heißt: Flag-Datei gesetzt und danach wieder weg.
- Screenshots: `target/run-omnitrade/*.png` (`01_start.png`, `after_killswitch.png`, `after_reset.png`). Nach jedem Schritt mit `Read` ansehen, nicht nur den Exit-Code prüfen.
- Eigener Screenshot: `driver.ps1 shot mein_name` erzeugt `target/run-omnitrade/mein_name.png`.
- Erwartete Zustände:
  - `01_start.png`: Kopfzeile `verbunden`, `NOT-AUS` rot, Kerzen und Orderbuch sichtbar, Logzeile `199 historische Kerzen geladen`.
  - Nach `killswitch`: Kopfzeile `NOT-AUS GESETZT` plus Button `Zurücksetzen`; `target/run-omnitrade/omnitrade.kill` existiert.
  - Nach `reset`: Datei weg, `status` zeigt `NOT-AUS: frei`.
- Der Driver setzt `OMNITRADE_KILL_FILE` auf `target/run-omnitrade/omnitrade.kill`. Die GUI und ihre Kindprozesse schreiben damit nie in die echte Not-Aus-Datei (`%TEMP%\omnitrade.kill`) der laufenden Live-Schleifen.

## Prerequisites

Nichts installieren. Vorhanden und geprüft: `cargo` (1.98.1), `powershell.exe` 5.1. Der Driver braucht kein Python und keinen Docker. Der Kerzenabruf läuft über `Invoke-RestMethod` gegen OKX (Port 443).

## Build

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 build
```

Erwartung: `Finished \`dev\` profile`. Erststart 1m18s, danach inkrementell etwa 3 s. Ergebnis: `target/debug/omnitrade-cli.exe` und `target/debug/omnitrade-ui.exe`.

## Run: CLI (headless, kein Display nötig)

```bash
powershell -NoProfile -ExecutionPolicy Bypass -File .claude/skills/run-omnitrade/driver.ps1 cli
```

`cli` holt die letzten 100 OKX-Kerzen BTC-USDT 1H nach `target/run-omnitrade/btc_1h.csv`, validiert `sample_buy.omni` und `sample_rsi.omni` und führt zwei Backtests mit `--initial-balance 1000000` aus.

- Erwartung `validate`: `Script validation successful`, Exit 0.
- Erwartung `backtest`: Block `=== Backtest Results ===`, Exit 0. Auf diesem Stand liefert `sample_buy` einen Trade, `sample_rsi` zwei Trades.

## Run: GUI (Mensch-Pfad)

Dasselbe Binary direkt starten, ohne Flag-Datei-Umleitung. Achtung: dann greift der Not-Aus die echte Live-Datei `%TEMP%\omnitrade.kill`.

```bash
target/debug/omnitrade-ui.exe BTC-USDT testnet --exchange okx
```

## Gotchas

- **Der Not-Aus ist eine Datei, kein Prozess-Signal.** Der Button schreibt `OMNITRADE_KILL_FILE` (oder `%TEMP%\omnitrade.kill`). Laufende `omnitrade-cli live`-Schleifen beobachten diese Datei, stornieren offene Orders und stoppen. Der Driver isoliert die Datei, aber ein Lauf ohne Driver trifft die echten Live-Schleifen. Vor einem manuellen Klick `driver.ps1 status` lesen und prüfen, welche Live-Prozesse laufen.
- **Der erste Klick nach einer Mausbewegung geht bei egui oft verloren.** Der Driver klickt deshalb bis zu dreimal und prüft nach jedem Klick die Flag-Datei. Ein fester Doppelklick schaltet den Not-Aus hin und her (im Test: mehrfach `gesetzt`/`aufgehoben` im Log). Nicht zurück zum Doppelklick.
- **Klickpunkte sind relativ zur Fensterecke oben links (inklusive Titelleiste) in physischen Pixeln.** Der Driver setzt `SetProcessDPIAware`. `NOT-AUS` liegt bei (398, 42), `Zurücksetzen` bei (522, 42). Nach einer UI-Änderung per Screenshot prüfen und die Konstanten `$KillXY`/`$ResetXY` in `driver.ps1` anpassen.
- **Der Driver bringt das Fenster mit `SetForegroundWindow` nach vorn.** Das ist keine Pflicht für den Klick, schadet aber nicht.
- **Der Reset-Klick ist ein Fallback-Fall.** Einmal kam der GUI-Klick nicht durch. Der Driver meldet dann `WARNUNG: GUI-Klick auf den Reset-Knopf ohne Wirkung` und hebt die Flag-Datei über `omnitrade-cli kill --reset` auf. Das ist dieselbe Datei und dasselbe Protokoll. Das Log der GUI zeigt in dem Fall keinen Eintrag `Not-Aus aufgehoben`.
- **Die Live-Statusanzeige ist eine Momentaufnahme.** Die GUI liest `status/live.json` (Standard `status/live.json` relativ zum Start-Verzeichnis, Überschreibung über `OMNITRADE_STATUS`). `running` heißt nur, dass die Datei so geschrieben wurde. Ob der Prozess lebt, zeigt `Get-Process omnitrade-cli`.
- **Backtest-Standardguthaben 10 000 reicht nicht.** Die Standardgröße (1 BTC bei etwa 81 000 USD) liefert `InsufficientBalance { required: 80928.55, available: 10000.0 }`. Deshalb `--initial-balance 1000000` setzen. Geprüft am 31.10.2026.
- **Indikator-Backtests laufen auf diesem Stand.** `sample_rsi.omni` mit `rsi(14)` läuft durch, weil der Warmup-Fix `e63ef02` im HEAD enthalten ist. Eine ältere Notiz zum Warmup-Bug gilt nur für einen früheren Stand.
- **OKX ist von diesem Host aus erreichbar.** REST (`/api/v5/market/history-candles`) und WebSocket (Kopfzeile `verbunden`, Live-Orderbuch) liefen im Test. Die früher beschriebene Sperre gilt für den Linux-Container, nicht für diesen Host.
- **Der Konto-Hinweis `Konto-Anzeige gibt es nur für Binance (gewählt: okx)` ist erwartet.** Mit `--exchange okx` gibt es keine Kontoanzeige. Binance wurde auf diesem Host nicht geprüft.

## Troubleshooting

| Symptom | Ursache | Fix |
|---|---|---|
| `kein GUI-Fenster nach 40 s` | GUI startet nicht oder stürzt ab | `target/run-omnitrade/ui.err.log` und `ui.log` lesen |
| `Flag-Datei fehlt nach Klick auf NOT-AUS` | Klick kam nicht an | erneut `killswitch` (wiederholt bis zu dreimal); Koordinaten per Screenshot prüfen |
| `WARNUNG: GUI-Klick auf den Reset-Knopf ohne Wirkung` | egui hat den Klick verschluckt | Fallback greift automatisch; danach `status` prüfen |
| `Flag-Datei existiert auch nach CLI-Reset noch` | Datei gesperrt oder anderer Pfad | `OMNITRADE_KILL_FILE` prüfen, Datei von Hand löschen, dann `status` |
| `InsufficientBalance` im Backtest | Standardguthaben zu klein | `--initial-balance 1000000` |
| `Konto-Anzeige gibt es nur für Binance` | erwartet bei `--exchange okx` | ignorieren |

## Stand

- Geprüft am 31.10.2026 auf Windows 11 (Host, keine Session-Container): `driver.ps1 build` (Erststart 1m18s, inkrementell 2,8 s), `flow` mit Screenshot und Flag-Prüfung, `status`, `stop`, `cli` (Kerzen-Abruf, zwei `validate`, zwei `backtest`), Standardguthaben-Fehler.
- Nicht geprüft: Live-Handel mit echten Schlüsseln, Binance, macOS, Linux.
- Linux-Container: `driver.sh` (Xvfb, xdotool, ImageMagick) stammt aus einem früheren Lauf im Linux-Container und ist auf diesem Host nicht gelaufen. Seine Koordinaten und Gotchas gelten dort, nicht hier.
