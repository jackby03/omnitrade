//! Auftragsplatzierung im Live-Lauf: Risikopruefung, Not-Aus und Uebermittlung.

use omnitrade_core::Side;
use omnitrade_exchange::{ExchangeTrading, OrderRequest};
use omnitrade_risk::guard::RiskGuard;

use super::*;

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
    place_order_with_kill(
        exchange_trading,
        risk_guard,
        order_req,
        current_price,
        initial_capital,
        Path::new(""),
    )
    .await
}

/// Wie [`place_order_with_risk_check`], prüft aber den übergebenen Not-Aus unmittelbar vor
/// der Übermittlung. Der Grid-Lauf nutzt das, damit eine Leiter den Not-Aus nicht überholt.
pub(crate) async fn place_order_with_kill(
    exchange_trading: &impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    order_req: &OrderRequest,
    current_price: f64,
    initial_capital: f64,
    kill_file: &Path,
) -> Result<omnitrade_core::Order, LiveLoopError> {
    place_order_guarded(
        exchange_trading,
        risk_guard,
        order_req,
        current_price,
        initial_capital,
        0.0,
        None,
        None,
        None,
        kill_file,
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
    // Bereits realisierter PnL dieses Laufs (Abrechnungswaehrung), siehe realisiert_beim_schliessen.
    run_realized: f64,
    // Abrechnungswaehrung des Instruments; `None` = Quote des Symbols.
    settle_currency: Option<&str>,
    exposure: Option<&mut ExposureLimit>,
    long_only: Option<&mut LongOnlyGuard>,
    kill_file: &Path,
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
            initial_capital + run_realized + position.unrealized_pnl
        } else {
            equity
        },
    };

    // Ein Auftrag, der die Position nur verkleinert, senkt das Risiko. An Verlust- oder
    // Drawdown-Grenzen zu scheitern würde ein verlierendes Engagement festhalten.
    let verkleinert = (order_req.side == Side::Sell && held > 0.0
        || order_req.side == Side::Buy && held < 0.0)
        && order_req.quantity <= held.abs() * 1.01;
    if !verkleinert {
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
    }

    // Die Abrufe oben können lange dauern. Der Not-Aus, der währenddessen gesetzt wurde,
    // greift hier noch vor der Übermittlung (die Schleife prüft ihn nur im Sekundentakt).
    if kill_switch::is_engaged(kill_file) {
        return Err(LiveLoopError::RiskViolation(
            "Not-Aus gesetzt - Auftrag nicht übermittelt".to_string(),
        ));
    }

    // Risk check passed; submit order to exchange
    exchange_trading
        .place_order(order_req)
        .await
        .map_err(|e| LiveLoopError::ExchangeError(e.to_string()))
}

/// Laufbestand bei Spot: Kontostand der Basiswaehrung minus Vorbestand. Der Vorbestand
/// wird beim ersten Abruf festgehalten, vor dem ersten Auftrag dieses Laufs. Mit `adopt`
/// gehoert der gesamte Bestand dem Lauf.
pub(super) async fn spot_run_net(
    exchange: &impl ExchangeTrading,
    symbol: &str,
    adopt: bool,
    start: &mut Option<f64>,
) -> Result<f64, String> {
    let (base, _quote) = split_symbol(symbol)
        .ok_or_else(|| format!("Symbol {symbol} laesst sich nicht zerlegen"))?;
    let balances = exchange.get_balance().await.map_err(|e| e.to_string())?;
    let held = held_base(&balances, &base, &PositionSnapshot::default());
    let vorbestand = *start.get_or_insert(if adopt { 0.0 } else { held });
    Ok(held - vorbestand)
}

/// Richtung des Auftrags aus Signal und signierter Boersenposition (`net`: > 0 long,
/// < 0 short, 0 flat). Wie im Backtest: kein Nachkauf, ein Gegensignal schliesst die
/// offene Position, ein Verkauf bei flat eroeffnet nur mit erlaubten Shorts.
pub(crate) fn order_side(
    signal: omnitrade_script::Signal,
    net: f64,
    allow_shorts: bool,
) -> Option<Side> {
    match signal {
        omnitrade_script::Signal::Buy if net > 0.0 => None,
        omnitrade_script::Signal::Buy => Some(Side::Buy),
        omnitrade_script::Signal::Sell if net < 0.0 => None,
        omnitrade_script::Signal::Sell if net == 0.0 && !allow_shorts => None,
        omnitrade_script::Signal::Sell => Some(Side::Sell),
        omnitrade_script::Signal::Hold => None,
    }
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

pub(super) async fn halt_trading(
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
