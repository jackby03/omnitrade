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
    account_equity, check_liquidation_distance, held_base, split_symbol, valid_mainnet_cap,
    ExposureLimit,
};

/// Maximale Anzahl Kerzen, die für die Skript-Auswertung vorgehalten werden.
const MAX_CANDLE_BUFFER: usize = 500;

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
    let mut exposure = max_notional.map(ExposureLimit::new);

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
    info!("Verbunden mit Binance ({:?})", network);

    // Candle buffer for script evaluation (auf MAX_CANDLE_BUFFER begrenzt)
    let mut candle_buffer: Vec<Candle> = warmup.to_vec();
    for candle in &candle_buffer {
        status.record_candle(candle, "hold");
    }
    flush_status(status);

    // Not-Aus wird bei jeder Kerze und im Sekundentakt geprüft, damit er auch
    // bei ruhigem Markt wirkt.
    let mut kill_check = tokio::time::interval(Duration::from_secs(1));
    // Long-only wie im Backtest: dieser Lauf hält höchstens eine Long-Position.
    let mut in_position = false;

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
        candle_buffer.push(candle.clone());
        if candle_buffer.len() > MAX_CANDLE_BUFFER {
            let overflow = candle_buffer.len() - MAX_CANDLE_BUFFER;
            candle_buffer.drain(..overflow);
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

        // Determine order side from signal. Verkauf nur bei offener Position, Kauf nur bei flat;
        // sonst würde der Risk Guard jede Verkaufsorder ohne Position ablehnen.
        let side = match signal {
            omnitrade_script::Signal::Buy if in_position => continue,
            omnitrade_script::Signal::Buy => Side::Buy,
            omnitrade_script::Signal::Sell if !in_position => continue,
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
            exposure.as_mut(),
        )
        .await
        {
            Ok(order) => {
                info!("Order filled: {:?}", order);
                let side_name = if side == Side::Buy { "buy" } else { "sell" };
                in_position = side == Side::Buy;
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
    )
    .await
}

/// Wie [`place_order_with_risk_check`], zusätzlich mit optionalem Positionsdeckel.
pub(crate) async fn place_order_guarded(
    exchange_trading: &impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    order_req: &OrderRequest,
    current_price: f64,
    initial_capital: f64,
    exposure: Option<&mut ExposureLimit>,
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
    let equity = account_equity(&balances, &base, &quote, current_price, &position);

    if let Some(limit) = exposure {
        let held = held_base(&balances, &base, &position);
        limit
            .check(order_req.side, order_req.quantity, current_price, held)
            .map_err(LiveLoopError::RiskViolation)?;
    }
    let account = SimpleAccount {
        initial: initial_capital,
        current: equity,
        pnl: equity - initial_capital,
    };

    // Check risk guard
    risk_guard
        .check_order(&mock_order, &account, current_price)
        .map_err(|e| LiveLoopError::RiskViolation(e.to_string()))?;

    // Risk check passed; submit order to exchange
    exchange_trading
        .place_order(order_req)
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))
}
