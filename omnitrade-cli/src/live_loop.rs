//! Live trading event loop for executing strategies against real-time market data.
//!
//! Connects to an exchange WebSocket stream, subscribes to candle updates,
//! evaluates a strategy script against each candle, checks risk limits,
//! and places orders via the exchange trading interface.

use omnitrade_core::{Candle, OrderStatus, Side, TimeInForce};
use omnitrade_exchange::{
    ExchangeStream, ExchangeTrading, Network, OrderRequest, PositionSnapshot,
};
use omnitrade_risk::guard::RiskGuard;
use omnitrade_script::{compile_and_run_extended, EvaluatorState};
use std::fs;
use std::path::Path;
use std::time::Duration;
use thiserror::Error;
use tracing::{error, info, trace, warn};

use crate::kill_switch;
use crate::status::StatusWriter;

mod risk;
#[cfg(test)]
mod risk_tests;
#[cfg(test)]
mod risk_tests_lang;
#[cfg(test)]
mod risk_tests_mainnet;
#[cfg(test)]
mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use risk::{
    account_equity, bewertungs_waehrung, check_liquidation_distance, held_base,
    realisiert_beim_schliessen, split_symbol, valid_mainnet_cap, ExposureLimit, LongOnlyGuard,
};

/// Maximale Anzahl Kerzen, die für die Skript-Auswertung vorgehalten werden.
const MAX_CANDLE_BUFFER: usize = 500;

/// Ohne Kerze gilt ein Kanal nach so vielen Intervallen als still.
const FUNKSTILLE_INTERVALLE: u64 = 3;
/// Untergrenze, damit ein 1m-Kanal nicht bei jedem Aussetzer nachlaedt.
const FUNKSTILLE_MINDEST_SEKUNDEN: u64 = 60;

/// Aktuelle Zeit in Millisekunden seit der Unix-Epoche.
fn jetzt_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

/// Sekunden eines Intervalls (`1m` -> 60, `4h` -> 14400).
pub(crate) fn intervall_sekunden(interval: &str) -> u64 {
    let (zahl, einheit) = interval.split_at(interval.len().saturating_sub(1));
    let n: u64 = zahl.parse().unwrap_or(1).max(1);
    match einheit {
        "m" | "M" => n * 60,
        "h" | "H" => n * 3_600,
        "d" | "D" => n * 86_400,
        "w" | "W" => n * 604_800,
        _ => n,
    }
}

/// Schweigt der Kanal so lange, dass die Schleife blind handelt?
pub(crate) fn kanal_still(letzte_kerze_ms: u64, jetzt_ms: u64, intervall_s: u64) -> bool {
    let grenze_s = (intervall_s * FUNKSTILLE_INTERVALLE).max(FUNKSTILLE_MINDEST_SEKUNDEN);
    jetzt_ms.saturating_sub(letzte_kerze_ms) > grenze_s * 1_000
}

/// Baut den Kerzenpuffer nach einer Funkstille aus REST-Kerzen neu auf.
///
/// Die REST-Kerzen bringen den Bestand auf Stand; Kerzen, die der Puffer bereits
/// juenger kennt (der Kanal lief wieder an), bleiben erhalten. Zeitstempel sind
/// eindeutig, der Puffer bleibt bei [`MAX_CANDLE_BUFFER`] gedeckelt.
pub(crate) fn puffer_aus_rest(alt: &[Candle], neu: Vec<Candle>) -> Vec<Candle> {
    let mut alle = neu;
    match alle.last().map(|c| c.timestamp_ms) {
        Some(letzte) => alle.extend(alt.iter().filter(|c| c.timestamp_ms > letzte).cloned()),
        None => alle = alt.to_vec(),
    }
    alle.sort_by_key(|c| c.timestamp_ms);
    alle.dedup_by_key(|c| c.timestamp_ms);
    if alle.len() > MAX_CANDLE_BUFFER {
        let ueber = alle.len() - MAX_CANDLE_BUFFER;
        alle.drain(..ueber);
    }
    alle
}

/// Quote-Währung, deren Bestand als Konto-Equity für den RiskGuard dient.
/// Quote-Währungen für Symbole ohne Trennzeichen (`BTCUSDT`), längere zuerst, damit
/// z. B. `USDT` vor `USD` und `BNB` erkannt wird.
const KNOWN_QUOTES: [&str; 10] = [
    "FDUSD", "USDT", "USDC", "BUSD", "TUSD", "USD", "EUR", "BTC", "ETH", "BNB",
];

/// Errors that can occur during live trading loop execution.
#[derive(Debug, Error)]
pub enum LiveLoopError {
    /// File I/O error reading the strategy script.
    #[error("Script file error: {0}")]
    ScriptFileError(#[from] std::io::Error),

    /// Exchange connection or trading error.
    #[error("Exchange error: {0}")]
    ExchangeError(String),

    /// Risk guard rejection.
    #[error("Risk guard rejection: {0}")]
    RiskViolation(String),

    /// Network validation error.
    #[error("Network validation error: {0}")]
    NetworkError(String),
}

/// Schreibt den Status-Snapshot; ein fehlgeschlagenes Schreiben stoppt den Handel nicht.
fn flush_status(status: &mut StatusWriter) {
    if let Err(e) = status.flush() {
        warn!("Status-Datei konnte nicht geschrieben werden: {}", e);
    }
}

fn signal_name(signal: omnitrade_script::Signal) -> &'static str {
    match signal {
        omnitrade_script::Signal::Buy => "buy",
        omnitrade_script::Signal::Sell => "sell",
        omnitrade_script::Signal::Hold => "hold",
    }
}

/// Laufzeit-Parameter eines Live-Laufs.
#[derive(Debug, Clone)]
pub struct LiveConfig<'a> {
    /// Pfad zum .omni-Strategieskript.
    pub script_path: &'a str,
    /// Handelssymbol (z. B. "BTCUSDT").
    pub symbol: &'a str,
    /// Netzwerk (Testnet oder Mainnet).
    pub network: Network,
    /// Startkapital als Referenz für Drawdown und Tagesverlust.
    pub initial_capital: f64,
    /// Menge je Market-Order (Basiswährung).
    pub order_quantity: f64,
    /// Flag-Datei des Not-Aus; ist sie gesetzt, stoppt die Schleife.
    pub kill_file: &'a Path,
    /// Deckel für die Position, die dieser Lauf selbst aufbaut (Quote-Währung).
    /// Auf Mainnet Pflicht und höchstens [`MAINNET_HARD_CAP`].
    pub max_notional: Option<f64>,
    /// Kerzenlänge, auf der die Strategie läuft (z. B. "1m", "1h").
    pub interval: &'a str,
    /// Erlaubt Kurzpositionen; der Deckel gilt dann in beide Richtungen.
    pub allow_shorts: bool,
    /// Bereits geschlossene Kerzen, die die Indikatoren vorwärmen. Leer, wenn die Börse
    /// keine Historie liefert.
    pub warmup: &'a [Candle],
}

/// Fest verdrahteter Höchstwert des Deckels auf Mainnet (Quote-Währung, z. B. USD).
/// Er lässt sich nur per Code-Änderung anheben, nie per Argument.
pub const MAINNET_HARD_CAP: f64 = 100.0;

/// Main async actor-style loop for live trading.
///
/// # Returns
/// `Ok(())` if loop completes gracefully, or `Err(LiveLoopError)` on fatal error.
///
/// # Error Handling
/// Non-fatal errors (individual order rejections, risk violations) are logged but
/// do not halt the loop. The loop continues monitoring for new signals.
pub async fn live_loop(
    config: &LiveConfig<'_>,
    // Abrechnungswaehrung des Instruments (`settleCcy`, z. B. USDC bei XPERP).
    // `None` = die Quote des Symbols gilt (Spot bzw. Konto in der Quote-Waehrung).
    settle_currency: Option<&str>,
    mut exchange_stream: impl ExchangeStream,
    exchange_trading: impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    status: &mut StatusWriter,
) -> Result<(), LiveLoopError> {
    let LiveConfig {
        script_path,
        symbol,
        network,
        initial_capital,
        order_quantity,
        kill_file,
        max_notional,
        interval,
        warmup,
        allow_shorts,
    } = *config;

    // Mainnet nur mit gültigem Deckel (testnet-first, echtes Geld braucht eine Grenze).
    if network == Network::Mainnet && !valid_mainnet_cap(max_notional) {
        return Err(LiveLoopError::NetworkError(format!(
            "Mainnet trading requires a max notional between 0 and {MAINNET_HARD_CAP}"
        )));
    }
    if let Some(cap) = max_notional {
        if !cap.is_finite() || cap <= 0.0 {
            return Err(LiveLoopError::NetworkError(format!(
                "Invalid max notional: {cap}"
            )));
        }
    }
    // OMNITRADE_ADOPT_POSITION=1: die bei Start offene Position gehoert dem Lauf.
    let adopt = std::env::var("OMNITRADE_ADOPT_POSITION").is_ok_and(|v| v == "1");
    if adopt {
        warn!("OMNITRADE_ADOPT_POSITION=1: bestehende Position wird als Laufposition uebernommen");
    }
    let mut exposure = max_notional.map(|cap| {
        ExposureLimit::new(cap)
            .allow_shorts(allow_shorts)
            .adopt_existing(adopt)
    });
    // Long-only gilt AUCH ohne Deckel: die Bremse haengt nicht an --max-notional.
    let mut long_only = LongOnlyGuard::new().adopt_existing(adopt);
    // Zustand der Skript-Auswertung (Trailing-Stop-Hoch etc.) über alle Kerzen.
    let mut script_state = EvaluatorState::new(0.0);
    // Spot: Bestand der Basiswaehrung vor dem ersten Auftrag dieses Laufs.
    let mut spot_start: Option<f64> = None;
    // Perpetual: realisierter PnL des Laufs und der Positionsstand der letzten Kerze.
    let mut run_realized = 0.0_f64;
    let mut perp_zuvor: Option<PositionSnapshot> = None;
    if allow_shorts {
        warn!(
            "Shorts aktiviert: Sell-Signale dürfen Kurzpositionen eröffnen \
             (Positionsdeckel in beide Richtungen)"
        );
    }

    // Load and validate script file
    let script_content = fs::read_to_string(script_path)?;

    if order_quantity <= 0.0 {
        return Err(LiveLoopError::ExchangeError(
            "Order-Menge muss positiv sein".to_string(),
        ));
    }

    // Abonnieren muss VOR connect() geschehen: connect() übergibt den Router
    // an die Verbindung, danach scheitert subscribe_candles.
    let mut candle_rx = exchange_stream
        .subscribe_candles(symbol, interval)
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))?;

    exchange_stream
        .connect()
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))?;
    info!("Verbunden mit der Boerse ({:?})", network);

    // Candle buffer for script evaluation (auf MAX_CANDLE_BUFFER begrenzt)
    let mut candle_buffer: Vec<Candle> = warmup.to_vec();
    for candle in &candle_buffer {
        status.record_candle(candle, "hold");
    }
    flush_status(status);

    // Funkstille-Waechter: ein WebSocket, der nur noch schweigt, sieht wie ein
    // laufender Prozess aus. Am 21.10.2026 stand die Schleife nach einem Lesefehler
    // 5,5 h still (Statusdatei eingefroren, keine Order, kein Fehler).
    let intervall_s = intervall_sekunden(interval);
    // Die Vorwaerm-Kerzen sind Historie: der Waechter zaehlt ab Schleifenstart,
    // sonst laedt jeder Start sofort unnoetig aus REST nach.
    let mut letzte_kerze_ms = jetzt_ms();
    let mut rest_nachgeladen = false;

    // Not-Aus wird bei jeder Kerze und im Sekundentakt geprüft, damit er auch
    // bei ruhigem Markt wirkt.
    let mut kill_check = tokio::time::interval(Duration::from_secs(1));

    // Main event loop: process each candle as it arrives
    loop {
        let candle = tokio::select! {
            received = candle_rx.recv() => match received {
                Some(candle) => candle,
                None => break,
            },
            _ = kill_check.tick() => {
                if kill_switch::is_engaged(kill_file) {
                    return halt_trading(&exchange_trading, symbol, &mut exchange_stream).await;
                }
                if !kanal_still(letzte_kerze_ms, jetzt_ms(), intervall_s) {
                    rest_nachgeladen = false;
                    continue;
                }
                if rest_nachgeladen {
                    continue;
                }
                rest_nachgeladen = true;
                match exchange_stream
                    .recent_candles(symbol, interval, MAX_CANDLE_BUFFER)
                    .await
                {
                    Ok(kerzen) if !kerzen.is_empty() => {
                        let vorher = candle_buffer.len();
                        candle_buffer = puffer_aus_rest(&candle_buffer, kerzen);
                        letzte_kerze_ms = candle_buffer
                            .last()
                            .map_or(letzte_kerze_ms, |c| c.timestamp_ms);
                        warn!(
                            "Kerzen-Kanal still - Puffer aus REST neu aufgebaut ({} -> {} Kerzen)",
                            vorher,
                            candle_buffer.len()
                        );
                    }
                    Ok(_) => warn!("Kerzen-Kanal still und REST lieferte keine Kerzen"),
                    Err(e) => warn!("Kerzen-Kanal still, REST-Nachladen fehlgeschlagen: {e}"),
                }
                continue;
            }
        };
        if kill_switch::is_engaged(kill_file) {
            return halt_trading(&exchange_trading, symbol, &mut exchange_stream).await;
        }
        // Eine Kerze kann in der Vorwärmung und im Stream zugleich erscheinen.
        if candle_buffer
            .last()
            .is_some_and(|last| candle.timestamp_ms <= last.timestamp_ms)
        {
            continue;
        }
        letzte_kerze_ms = candle.timestamp_ms;
        rest_nachgeladen = false;
        candle_buffer.push(candle.clone());
        if candle_buffer.len() > MAX_CANDLE_BUFFER {
            let overflow = candle_buffer.len() - MAX_CANDLE_BUFFER;
            candle_buffer.drain(..overflow);
        }
        // Das Buch folgt der BOERSE: eine Position kann auch ohne diesen Lauf
        // entstehen oder vergehen (Handschluss, Stop, ein zweiter Prozess). Der
        // eigene Zaehler in record_order ist nur die Zwischenanzeige bis zur
        // naechsten Kerze; scheitert der Abruf, bleibt der alte Wert stehen und
        // wird geloggt - nie stillschweigend auf 0 gesetzt.
        // Die signierte Boersenposition entscheidet ueber die Richtung des Auftrags.
        // Scheitert der Abruf, bleibt das Buch stehen und dieser Lauf ordert nichts.
        // Spot-Snapshots kennen keinen Bestand (Standard: 0). Dort zaehlt der Laufbestand
        // aus dem Kontostand, abzueglich des Vorbestands.
        let (net_position, is_spot) = match exchange_trading.position_snapshot(symbol).await {
            Ok(snapshot) if snapshot.is_perpetual => {
                if let Some(vorher) = perp_zuvor.as_ref() {
                    run_realized += realisiert_beim_schliessen(vorher, &snapshot);
                }
                perp_zuvor = Some(snapshot);
                status.set_net_quantity(snapshot.base_qty);
                (Some(snapshot.base_qty), false)
            }
            Ok(_) => {
                match spot_run_net(&exchange_trading, symbol, adopt, &mut spot_start).await {
                    Ok(net) => {
                        status.set_net_quantity(net);
                        (Some(net), true)
                    }
                    Err(err) => {
                        warn!("Spot-Bestand nicht abrufbar ({err}) - Buch bleibt auf dem letzten Stand");
                        (None, true)
                    }
                }
            }
            Err(err) => {
                warn!("Position nicht abrufbar ({err}) - Buch bleibt auf dem letzten Stand");
                (None, false)
            }
        };

        trace!("Evaluating candle: {:?}", candle);

        // Evaluate strategy script against accumulated candles
        let intent =
            match compile_and_run_extended(&script_content, &candle_buffer, &mut script_state) {
                Ok(intent) => intent,
                Err(e) => {
                    warn!("Script evaluation error for {}: {}", symbol, e);
                    status.record_candle(&candle, "hold");
                    flush_status(status);
                    continue;
                }
            };
        let signal = intent.signal;
        status.record_candle(&candle, signal_name(signal));
        flush_status(status);

        // Skip if no signal
        if signal == omnitrade_script::Signal::Hold {
            continue;
        }
        // Stop-Loss, Take-Profit und Trailing-Stop werden im Live-Modus nicht ausgeführt.
        // Statt sie stillschweigend zu verwerfen, wird der Auftrag explizit abgelehnt.
        if intent.stop_loss.is_some()
            || intent.take_profit.is_some()
            || intent.trailing_pct.is_some()
        {
            warn!(
                "Risiko-Direktive (stop_loss/take_profit/trailing_stop) im Live-Modus nicht unterstützt - Auftrag abgelehnt"
            );
            continue;
        }

        let Some(net) = net_position else {
            warn!("Keine Position bekannt - Auftrag wird ausgelassen (fail closed)");
            continue;
        };
        let Some(side) = order_side(signal, net, allow_shorts) else {
            continue;
        };
        // Spot: ein Verkauf geht nie ueber den Laufbestand hinaus. Die Kauf-Gebuehr wird in
        // der Basiswaehrung abgezogen; Vorbestand bleibt unangetastet.
        let qty = if is_spot && side == Side::Sell && net > 0.0 {
            order_quantity.min(net)
        } else {
            order_quantity
        };

        let order_req = OrderRequest::market(symbol.to_string(), side, qty);
        info!("Placing order: {:?}", order_req);

        // Place order and check risk guard
        match place_order_guarded(
            &exchange_trading,
            risk_guard,
            &order_req,
            candle.close,
            initial_capital,
            run_realized,
            settle_currency,
            exposure.as_mut(),
            if allow_shorts {
                None
            } else {
                Some(&mut long_only)
            },
            kill_file,
        )
        .await
        {
            Ok(order) => {
                info!("Order filled: {:?}", order);
                let side_name = if side == Side::Buy { "buy" } else { "sell" };
                status.record_order(side_name, qty, candle.close, "placed", "");
                flush_status(status);
            }
            Err(e) => {
                warn!("Order rejected by RiskGuard: {}", e);
                let side_name = if side == Side::Buy { "buy" } else { "sell" };
                status.record_order(side_name, qty, candle.close, "rejected", &e.to_string());
                flush_status(status);
                // Continue loop; do not halt on single order failure
            }
        }
    }

    // Graceful disconnect
    exchange_stream
        .disconnect()
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))?;

    Ok(())
}

/// Not-Aus: storniert offene Orders des Symbols und trennt die Verbindung.
///
/// Bestände werden bewusst nicht verkauft.
mod orders;
pub(crate) use orders::{
    cancel_open_orders, order_side, place_order_guarded, place_order_with_kill,
    place_order_with_risk_check,
};
use orders::{halt_trading, spot_run_net};
