//! Live trading event loop for executing strategies against real-time market data.
//!
//! Connects to an exchange WebSocket stream, subscribes to candle updates,
//! evaluates a strategy script against each candle, checks risk limits,
//! and places orders via the exchange trading interface.

use omnitrade_core::{Candle, OrderStatus, Side, TimeInForce};
use omnitrade_exchange::{ExchangeStream, ExchangeTrading, Network, OrderRequest};
use omnitrade_risk::guard::RiskGuard;
use omnitrade_script::compile_and_run;
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
mod test_support;
#[cfg(test)]
mod tests;

pub(crate) use risk::{
    account_equity, bewertungs_waehrung, check_liquidation_distance, held_base, split_symbol,
    valid_mainnet_cap, ExposureLimit, LongOnlyGuard,
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
    let mut long_only = LongOnlyGuard::new();
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
        match exchange_trading.position_snapshot(symbol).await {
            Ok(snapshot) => status.set_net_quantity(snapshot.base_qty),
            Err(err) => {
                warn!("Position nicht abrufbar ({err}) - Buch bleibt auf dem letzten Stand")
            }
        }

        trace!("Evaluating candle: {:?}", candle);

        // Evaluate strategy script against accumulated candles
        let signal = match compile_and_run(&script_content, &candle_buffer) {
            Ok(sig) => sig,
            Err(e) => {
                warn!("Script evaluation error for {}: {}", symbol, e);
                status.record_candle(&candle, "hold");
                flush_status(status);
                continue;
            }
        };
        status.record_candle(&candle, signal_name(signal));
        flush_status(status);

        // Skip if no signal
        if signal == omnitrade_script::Signal::Hold {
            continue;
        }

        // Determine order side from signal
        let side = match signal {
            omnitrade_script::Signal::Buy => Side::Buy,
            omnitrade_script::Signal::Sell => Side::Sell,
            omnitrade_script::Signal::Hold => continue,
        };

        let order_req = OrderRequest::market(symbol.to_string(), side, order_quantity);
        info!("Placing order: {:?}", order_req);

        // Place order and check risk guard
        match place_order_guarded(
            &exchange_trading,
            risk_guard,
            &order_req,
            candle.close,
            initial_capital,
            settle_currency,
            exposure.as_mut(),
            if allow_shorts {
                None
            } else {
                Some(&mut long_only)
            },
        )
        .await
        {
            Ok(order) => {
                info!("Order filled: {:?}", order);
                let side_name = if side == Side::Buy { "buy" } else { "sell" };
                status.record_order(side_name, order_quantity, candle.close, "placed", "");
                flush_status(status);
            }
            Err(e) => {
                warn!("Order rejected by RiskGuard: {}", e);
                let side_name = if side == Side::Buy { "buy" } else { "sell" };
                status.record_order(
                    side_name,
                    order_quantity,
                    candle.close,
                    "rejected",
                    &e.to_string(),
                );
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

/// Storniert alle offenen Orders des Symbols. Fremde Symbole bleiben unberührt.
///
/// Einzelne Fehler werden geloggt und brechen den Vorgang nicht ab.
pub(crate) async fn cancel_open_orders(exchange_trading: &impl ExchangeTrading, symbol: &str) {
    error!("Not-Aus ausgelöst: keine neuen Orders, storniere offene Orders");
    match exchange_trading.get_open_orders().await {
        Ok(orders) => {
            for order in orders.into_iter().filter(|o| o.symbol == symbol) {
                if let Err(e) = exchange_trading
                    .cancel_order(&order.id.to_string(), &order.symbol)
                    .await
                {
                    warn!("Stornierung von Order {} fehlgeschlagen: {}", order.id, e);
                }
            }
        }
        Err(e) => warn!("Offene Orders nicht abrufbar: {}", e),
    }
}

/// Not-Aus: storniert offene Orders des Symbols und trennt die Verbindung.
///
/// Bestände werden bewusst nicht verkauft.
async fn halt_trading(
    exchange_trading: &impl ExchangeTrading,
    symbol: &str,
    exchange_stream: &mut impl ExchangeStream,
) -> Result<(), LiveLoopError> {
    cancel_open_orders(exchange_trading, symbol).await;
    exchange_stream
        .disconnect()
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))
}

/// Helper: Place order after passing risk guard checks.
///
/// Creates a mock Order from the request, checks it against RiskGuard,
/// and if approved, submits to the exchange.
pub(crate) async fn place_order_with_risk_check(
    exchange_trading: &impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    order_req: &OrderRequest,
    current_price: f64,
    initial_capital: f64,
) -> Result<omnitrade_core::Order, LiveLoopError> {
    place_order_guarded(
        exchange_trading,
        risk_guard,
        order_req,
        current_price,
        initial_capital,
        None,
        None,
        None,
    )
    .await
}

/// Wie [`place_order_with_risk_check`], zusätzlich mit optionalem Positionsdeckel.
///
/// Acht Parameter, weil der Risikopfad alles braucht, was die Schleife ohnehin
/// hält (Konto, Deckel, Long-only, Startkapital, Abrechnungswaehrung). Ein
/// Struktur-Umbau waere hier Umbau ohne Gewinn - die Funktion hat zwei Aufrufer.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn place_order_guarded(
    exchange_trading: &impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    order_req: &OrderRequest,
    current_price: f64,
    initial_capital: f64,
    // Abrechnungswaehrung des Instruments; `None` = Quote des Symbols.
    settle_currency: Option<&str>,
    exposure: Option<&mut ExposureLimit>,
    long_only: Option<&mut LongOnlyGuard>,
) -> Result<omnitrade_core::Order, LiveLoopError> {
    // Create mock order for risk checking
    let mock_order = omnitrade_core::Order {
        id: 0,
        symbol: order_req.symbol.clone(),
        side: order_req.side,
        order_type: order_req.order_type.clone(),
        time_in_force: TimeInForce::GTC,
        price: order_req.price.unwrap_or(current_price),
        stop_price: 0.0,
        quantity: order_req.quantity,
        filled_quantity: 0.0,
        status: OrderStatus::New,
        created_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        updated_at_ms: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64,
        trailing_highest_price: None,
    };

    // Minimal AccountInfo implementation for risk checking
    struct SimpleAccount {
        initial: f64,
        current: f64,
        pnl: f64,
        /// Equity des LAUF-Kapitals: deklariertes Startkapital + uPnL der eigenen
        /// Position. Das Konto ist geteilt und taugt nicht als Drawdown-Basis.
        run_equity: f64,
    }

    impl omnitrade_risk::guard::AccountInfo for SimpleAccount {
        fn total_equity(&self) -> f64 {
            self.current
        }

        fn total_pnl(&self) -> f64 {
            self.pnl
        }

        fn initial_balance(&self) -> f64 {
            self.initial
        }

        fn drawdown_equity(&self) -> f64 {
            self.run_equity
        }
    }

    // Echter Kontostand statt fester Startsumme, sonst bleiben Drawdown- und
    // Tagesverlust-Limits wirkungslos. Schlägt der Abruf fehl oder lässt sich das
    // Symbol nicht zerlegen, wird die Order abgelehnt (fail closed).
    if !current_price.is_finite() || current_price <= 0.0 {
        return Err(LiveLoopError::RiskViolation(format!(
            "ungültiger Preis {current_price} für die Eigenkapital-Bewertung"
        )));
    }
    let (base, quote) = split_symbol(&order_req.symbol).ok_or_else(|| {
        LiveLoopError::ExchangeError(format!(
            "Symbol {} lässt sich nicht in Basis/Quote zerlegen",
            order_req.symbol
        ))
    })?;
    let balances = exchange_trading
        .get_balance()
        .await
        .map_err(|e| LiveLoopError::ExchangeError(format!("Kontostand: {e}")))?;
    let position = exchange_trading
        .position_snapshot(&order_req.symbol)
        .await
        .map_err(|e| LiveLoopError::ExchangeError(format!("Position: {e}")))?;
    check_liquidation_distance(&position, order_req.side).map_err(LiveLoopError::RiskViolation)?;
    // Bewertungswaehrung: bei XPERP traegt das Konto USDC, das Symbol sagt "USD".
    // Ohne diese Aufloesung findet pair_equity keinen Bestand, die Equity faellt
    // auf den uPnL zusammen und der Drawdown schiesst ins Millionenprozent.
    let waehrung = bewertungs_waehrung(&balances, &quote, settle_currency);
    let equity = account_equity(&balances, &base, waehrung, current_price, &position);

    let held = held_base(&balances, &base, &position);
    // Long-only zuerst: sie gilt unabhaengig davon, ob ein Deckel gesetzt ist.
    if let Some(guard) = long_only {
        guard
            .check(order_req.side, order_req.quantity, held)
            .map_err(LiveLoopError::RiskViolation)?;
    }
    if let Some(limit) = exposure {
        limit
            .check(order_req.side, order_req.quantity, current_price, held)
            .map_err(LiveLoopError::RiskViolation)?;
    }
    let account = SimpleAccount {
        initial: initial_capital,
        current: equity,
        pnl: equity - initial_capital,
        // Perpetuals (mit Abrechnungswaehrung) teilen sich das Konto mit anderen Spuren: dort zaehlt nur das
        // Laufkapital plus der eigene uPnL. Spot bleibt bei der Paar-Equity (der
        // Wertverlust der gekauften Basis ist dort der Verlust des Laufs).
        run_equity: if position.is_perpetual && settle_currency.is_some() {
            initial_capital + position.unrealized_pnl
        } else {
            equity
        },
    };

    // Check risk guard
    risk_guard
        .check_order(&mock_order, &account, current_price)
        .map_err(|e| {
            // Der Guard misst das KONTO (alle Spuren auf demselben Konto), nicht das
            // Kapital dieses Laufs. Bei einer Ablehnung gehoeren Spitze, Basis und
            // aktuelle Equity ins Log - sonst bleibt die Ursache (Abbuchung, andere
            // Spur, uPnL) hinterher nicht mehr feststellbar.
            tracing::warn!(
                peak_equity = risk_guard.peak_equity(),
                run_baseline = ?risk_guard.run_baseline(),
                equity = equity,
                declared_initial = initial_capital,
                "RiskGuard lehnt ab: {e}"
            );
            LiveLoopError::RiskViolation(e.to_string())
        })?;

    // Risk check passed; submit order to exchange
    exchange_trading
        .place_order(order_req)
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))
}
