//! Grid-Live: Limit-Order-Leiter auf einem Spot-Konto.
//!
//! Beim Start wird eine Kauf-Leiter unter `start_price` gelegt. Wird eine Order
//! gefüllt, ersetzt eine Gegenorder eine Stufe weiter (`spacing_pct`) sie: ein
//! gefüllter Kauf wird zum Verkauf darüber, ein gefüllter Verkauf zum Kauf darunter.
//! Verkäufe entstehen so nur aus Beständen, die das Grid selbst gekauft hat.
//!
//! Jede Order läuft durch den RiskGuard. Fills werden per Polling erkannt, der
//! Not-Aus (Flag-Datei) storniert die offenen Orders des Symbols und beendet den Lauf.

use std::collections::{HashMap, HashSet};
use std::path::Path;
use std::time::Duration;

use omnitrade_core::{OrderStatus, Side};
use omnitrade_exchange::{ExchangeTrading, OrderRequest};
use omnitrade_risk::guard::RiskGuard;
use omnitrade_script::GridConfig;
use tracing::{info, warn};

use crate::kill_switch;
use crate::live_loop::{cancel_open_orders, place_order_with_kill, LiveLoopError};

/// Laufzeit-Parameter eines Grid-Laufs.
#[derive(Debug, Clone)]
pub struct GridLiveConfig<'a> {
    /// Grid-Definition (Stufen, Abstand, Größe, Startpreis).
    pub grid: &'a GridConfig,
    /// Handelssymbol (z. B. "BTCUSDT").
    pub symbol: &'a str,
    /// Startkapital als Referenz für Drawdown und Tagesverlust.
    pub initial_capital: f64,
    /// Flag-Datei des Not-Aus.
    pub kill_file: &'a Path,
    /// Abstand der Fill-Abfragen.
    pub poll_interval: Duration,
    /// Nachkommastellen für Preise (Tick-Size des Symbols).
    pub price_decimals: u32,
    /// Nachkommastellen für Mengen (Step-Size des Symbols).
    pub qty_decimals: u32,
}

/// Eine vom Grid platzierte, noch offene Stufe.
#[derive(Debug, Clone, Copy)]
struct Level {
    side: Side,
    price: f64,
    qty: f64,
}

fn round_price(value: f64, decimals: u32) -> f64 {
    let f = 10f64.powi(decimals as i32);
    (value * f).round() / f
}

/// Mengen werden abgerundet, damit nie mehr verkauft wird, als gekauft wurde.
fn floor_qty(value: f64, decimals: u32) -> f64 {
    let f = 10f64.powi(decimals as i32);
    (value * f).floor() / f
}

/// Führt das Grid aus, bis der Not-Aus gesetzt wird.
///
/// # Errors
/// Fehler bei ungültiger Konfiguration. Einzelne abgelehnte oder fehlgeschlagene
/// Orders werden geloggt und beenden den Lauf nicht.
pub async fn grid_loop(
    config: &GridLiveConfig<'_>,
    exchange_trading: impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
) -> Result<(), LiveLoopError> {
    let GridLiveConfig {
        grid,
        symbol,
        initial_capital,
        kill_file,
        poll_interval,
        price_decimals,
        qty_decimals,
    } = *config;

    if (grid.leverage - 1.0).abs() > f64::EPSILON {
        return Err(LiveLoopError::ExchangeError(
            "Grid-Live läuft auf Spot: leverage muss 1.0 sein".to_string(),
        ));
    }

    let mut levels: HashMap<u64, Level> = HashMap::new();

    for (price, qty) in grid.generate_buy_orders() {
        if kill_switch::is_engaged(kill_file) {
            cancel_open_orders(&exchange_trading, symbol).await;
            return Ok(());
        }
        let level = Level {
            side: Side::Buy,
            price: round_price(price, price_decimals),
            qty: floor_qty(qty, qty_decimals),
        };
        place_level(
            &exchange_trading,
            risk_guard,
            symbol,
            initial_capital,
            level,
            &mut levels,
            kill_file,
        )
        .await;
    }
    info!("Grid gestartet: {} Stufen aktiv", levels.len());

    let mut ticker = tokio::time::interval(poll_interval);
    loop {
        ticker.tick().await;
        if kill_switch::is_engaged(kill_file) {
            cancel_open_orders(&exchange_trading, symbol).await;
            return Ok(());
        }

        let open: HashSet<u64> = match exchange_trading.get_open_orders().await {
            Ok(orders) => orders
                .into_iter()
                .filter(|o| o.symbol == symbol)
                .map(|o| o.id)
                .collect(),
            Err(e) => {
                warn!("Offene Orders nicht abrufbar, nächster Versuch: {}", e);
                continue;
            }
        };

        let gone: Vec<u64> = levels
            .keys()
            .copied()
            .filter(|id| !open.contains(id))
            .collect();
        for id in gone {
            let Some(level) = levels.get(&id).copied() else {
                continue;
            };
            match exchange_trading.get_order(&id.to_string(), symbol).await {
                Ok(Some(order)) if order.status == OrderStatus::Filled => {
                    levels.remove(&id);
                    info!("Stufe gefüllt: {:?} @ {}", level.side, level.price);
                    let counter = counter_level(
                        level,
                        order.filled_quantity,
                        grid.spacing_pct,
                        price_decimals,
                        qty_decimals,
                    );
                    if levels
                        .values()
                        .any(|l| l.side == counter.side && l.price == counter.price)
                    {
                        warn!(
                            "Gegenstufe {:?} @ {} existiert bereits",
                            counter.side, counter.price
                        );
                        continue;
                    }
                    place_level(
                        &exchange_trading,
                        risk_guard,
                        symbol,
                        initial_capital,
                        counter,
                        &mut levels,
                        kill_file,
                    )
                    .await;
                }
                Ok(Some(order))
                    if matches!(order.status, OrderStatus::Cancelled | OrderStatus::Rejected) =>
                {
                    levels.remove(&id);
                    warn!(
                        "Stufe {:?} @ {} wurde extern beendet ({:?}), Lücke im Grid",
                        level.side, level.price, order.status
                    );
                }
                Ok(_) => {}
                Err(e) => warn!("Status von Order {} nicht abrufbar: {}", id, e),
            }
        }
    }
}

/// Berechnet die Gegenstufe zu einer gefüllten Stufe.
fn counter_level(
    filled: Level,
    filled_qty: f64,
    spacing_pct: f64,
    price_decimals: u32,
    qty_decimals: u32,
) -> Level {
    let (side, factor) = match filled.side {
        Side::Buy => (Side::Sell, 1.0 + spacing_pct),
        Side::Sell => (Side::Buy, 1.0 - spacing_pct),
    };
    let qty = if filled_qty > 0.0 {
        filled_qty
    } else {
        filled.qty
    };
    Level {
        side,
        price: round_price(filled.price * factor, price_decimals),
        qty: floor_qty(qty, qty_decimals),
    }
}

/// Platziert eine Stufe über den RiskGuard und merkt sie sich bei Erfolg.
async fn place_level(
    exchange_trading: &impl ExchangeTrading,
    risk_guard: &mut RiskGuard,
    symbol: &str,
    initial_capital: f64,
    level: Level,
    levels: &mut HashMap<u64, Level>,
    kill_file: &Path,
) {
    if level.qty <= 0.0 || level.price <= 0.0 {
        warn!("Stufe verworfen (Preis/Menge nach Rundung 0): {:?}", level);
        return;
    }
    let req = OrderRequest::limit(symbol.to_string(), level.side, level.qty, level.price);
    match place_order_with_kill(
        exchange_trading,
        risk_guard,
        &req,
        level.price,
        initial_capital,
        kill_file,
    )
    .await
    {
        Ok(order) => {
            info!(
                "Stufe platziert: {:?} {} @ {}",
                level.side, level.qty, level.price
            );
            levels.insert(order.id, level);
        }
        Err(e) => warn!(
            "Stufe {:?} @ {} nicht platziert (Lücke im Grid): {}",
            level.side, level.price, e
        ),
    }
}

#[cfg(test)]
#[path = "grid_loop_tests.rs"]
mod tests;
