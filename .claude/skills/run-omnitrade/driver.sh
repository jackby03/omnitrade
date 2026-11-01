#!/usr/bin/env bash
# Treiber fuer omnitrade im LINUX-CONTAINER (nicht auf dem Windows-Host; dort: driver.ps1).
# Auf diesem Stand nicht gelaufen. GUI unter Xvfb (Start, Screenshot, Klicks per xdotool)
# und CLI-Smoke (validate, backtest). Aufruf aus beliebigem Verzeichnis:
#   .claude/skills/run-omnitrade/driver.sh <befehl>
# Befehle: build | flow | start | shot [name] | killswitch | reset | status | stop | cli | fetch
set -euo pipefail

SKILL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "$SKILL_DIR/../../.." && pwd)"
RUN="$ROOT/target/run-omnitrade"
BIN="$ROOT/target/debug"
DISPLAY_NUM=":99"
GUI_SYMBOL="${GUI_SYMBOL:-BTC-USDT}"
GUI_EXCHANGE="${GUI_EXCHANGE:-okx}"
# Klickpunkte relativ zum Fenster (Fenster bei 0,0, 1280x800). Layout-abhaengig.
KILL_XY=(381 11)    # Button "NOT-AUS" in der Kopfzeile
RESET_XY=(504 11)   # Button "Zurücksetzen", erscheint nur nach NOT-AUS

mkdir -p "$RUN"
export DISPLAY="$DISPLAY_NUM"
export OMNITRADE_KILL_FILE="$RUN/omnitrade.kill"

die() { echo "FEHLER: $*" >&2; exit 1; }

pid_alive() { [ -f "$1" ] && kill -0 "$(cat "$1")" 2>/dev/null; }

cmd_build() {
  cd "$ROOT"
  cargo build --workspace
}

cmd_xvfb() {
  if [ -e "/tmp/.X11-unix/X${DISPLAY_NUM#:}" ]; then
    echo "Xvfb auf $DISPLAY_NUM laeuft bereits"
    return
  fi
  Xvfb "$DISPLAY_NUM" -screen 0 1600x1000x24 >"$RUN/xvfb.log" 2>&1 &
  echo $! >"$RUN/xvfb.pid"
  sleep 2
  echo "Xvfb gestartet (PID $(cat "$RUN/xvfb.pid"))"
}

window_id() { xdotool search --name omnitrade 2>/dev/null | head -1; }

wait_window() {
  for _ in $(seq 1 40); do
    [ -n "$(window_id)" ] && return 0
    sleep 1
  done
  die "kein GUI-Fenster nach 40 s – siehe $RUN/ui.log"
}

cmd_start() {
  [ -x "$BIN/omnitrade-ui" ] || die "omnitrade-ui fehlt – zuerst: driver.sh build"
  cmd_xvfb
  if pid_alive "$RUN/ui.pid"; then
    echo "GUI laeuft bereits (PID $(cat "$RUN/ui.pid"))"
    return
  fi
  rm -f "$OMNITRADE_KILL_FILE"
  # Eigene Redirects inkl. stdin: sonst haelt der Hintergrundprozess die stdout-Pipe
  # des Aufrufers offen und der Driver-Aufruf kehrt nie zurueck.
  cd "$ROOT"
  "$BIN/omnitrade-ui" "$GUI_SYMBOL" testnet --exchange "$GUI_EXCHANGE" \
    >"$RUN/ui.log" 2>&1 </dev/null &
  echo $! >"$RUN/ui.pid"
  wait_window
  sleep 8   # Kerzen-Laden abwarten
  pid_alive "$RUN/ui.pid" || die "GUI beendet sich – siehe $RUN/ui.log"
  echo "GUI gestartet (PID $(cat "$RUN/ui.pid"), Fenster $(window_id))"
}

click_in_window() {
  local wid
  wid="$(window_id)"
  [ -n "$wid" ] || die "kein GUI-Fenster – zuerst: driver.sh start"
  xdotool mousemove --window "$wid" "$1" "$2" click 1
  sleep 2
}

cmd_shot() {
  local name="${1:-screen}"
  [ -n "$(window_id)" ] || die "kein GUI-Fenster"
  import -window root "$RUN/$name.png"
  echo "$RUN/$name.png"
}

cmd_killswitch() {
  click_in_window "${KILL_XY[@]}"
  if [ -f "$OMNITRADE_KILL_FILE" ]; then
    echo "OK: NOT-AUS gesetzt, Flag-Datei $OMNITRADE_KILL_FILE"
  else
    die "Flag-Datei fehlt nach Klick auf NOT-AUS"
  fi
  cmd_shot after_killswitch
}

cmd_reset() {
  click_in_window "${RESET_XY[@]}"
  if [ -f "$OMNITRADE_KILL_FILE" ]; then
    die "Flag-Datei existiert noch nach Zurücksetzen"
  fi
  echo "OK: NOT-AUS zurueckgesetzt"
  cmd_shot after_reset
}

cmd_status() {
  pid_alive "$RUN/xvfb.pid" && echo "Xvfb: laeuft" || echo "Xvfb: aus"
  pid_alive "$RUN/ui.pid" && echo "GUI: laeuft (PID $(cat "$RUN/ui.pid"))" || echo "GUI: aus"
  if [ -f "$OMNITRADE_KILL_FILE" ]; then echo "NOT-AUS: gesetzt"; else echo "NOT-AUS: frei"; fi
}

cmd_stop() {
  pid_alive "$RUN/ui.pid" && kill "$(cat "$RUN/ui.pid")" || true
  pid_alive "$RUN/xvfb.pid" && kill "$(cat "$RUN/xvfb.pid")" || true
  rm -f "$RUN/ui.pid" "$RUN/xvfb.pid"
  echo "gestoppt"
}

cmd_fetch() {
  # OKX-Kerzen (REST, Port 443 erreichbar). Binance ist aus diesem Netz gesperrt (HTTP 451).
  local raw="$RUN/okx_raw.json" csv="$RUN/btc_1h.csv"
  curl -s -m 20 "https://www.okx.com/api/v5/market/history-candles?instId=BTC-USDT&bar=1H&limit=100" -o "$raw"
  python3 -I -c "
import json
d = json.load(open('$raw'))
if d.get('code') != '0' or not d.get('data'):
    raise SystemExit('OKX-Antwort ungueltig: ' + str(d.get('msg')))
rows = sorted(d['data'], key=lambda r: int(r[0]))
with open('$csv', 'w') as f:
    f.write('timestamp_ms,open,high,low,close,volume\n')
    for r in rows:
        f.write(f'{r[0]},{r[1]},{r[2]},{r[3]},{r[4]},{r[5]}\n')
print('rows', len(rows), '->', '$csv')
"
}

cmd_cli() {
  local cli="$BIN/omnitrade-cli"
  [ -x "$cli" ] || die "omnitrade-cli fehlt – zuerst: driver.sh build"
  cmd_fetch
  "$cli" validate --script "$SKILL_DIR/sample_buy.omni"
  "$cli" validate --script "$SKILL_DIR/sample_rsi.omni"
  "$cli" backtest --script "$SKILL_DIR/sample_buy.omni" --data "$RUN/btc_1h.csv" --initial-balance 1000000
}

cmd_flow() {
  cmd_start
  cmd_shot 01_start
  cmd_killswitch
  cmd_reset
  cmd_status
}

case "${1:-help}" in
  build) cmd_build ;;
  flow) cmd_flow ;;
  start) cmd_start ;;
  shot) cmd_shot "${2:-}" ;;
  killswitch) cmd_killswitch ;;
  reset) cmd_reset ;;
  status) cmd_status ;;
  stop) cmd_stop ;;
  fetch) cmd_fetch ;;
  cli) cmd_cli ;;
  *) echo "Befehle: build | flow | start | shot [name] | killswitch | reset | status | stop | cli | fetch"; exit 2 ;;
esac
