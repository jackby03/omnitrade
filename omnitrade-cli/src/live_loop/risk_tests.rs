//! Tests der Risiko-Helfer: Eigenkapital, Liquidationsabstand, Positionsdeckel, Mainnet-Cap.

use super::risk::*;
use super::test_support::*;
use super::*;
use omnitrade_core::Side;
use omnitrade_exchange::{Account, AccountBalance, PositionSnapshot};
use std::collections::HashMap;

#[tokio::test]
async fn bought_coins_do_not_look_like_a_drawdown_to_the_risk_guard() {
    // Arrange: 5 % Drawdown-Limit, Referenzpreis 50_000.
    let mut guard = RiskGuard::new(omnitrade_risk::RiskLimits::new(
        1_000_000_000.0,
        5.0,
        1_000_000_000.0,
    ));
    let req = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);

    // Act: erste Order bei 10_000 USDT setzt die Spitze.
    place_order_with_risk_check(
        &trading_with(10_000.0, 0.0),
        &mut guard,
        &req,
        50_000.0,
        10_000.0,
    )
    .await
    .expect("first order passes");
    // Nach einem Kauf: 5_000 USDT + 0.1 BTC (= 5_000 USDT) bleiben 10_000 Eigenkapital.
    let after_buy = place_order_with_risk_check(
        &trading_with(5_000.0, 0.1),
        &mut guard,
        &req,
        50_000.0,
        10_000.0,
    )
    .await;

    // Assert: ohne Bewertung der BTC sähe der Guard 50 % Drawdown und lehnte ab.
    assert!(after_buy.is_ok(), "{after_buy:?}");
}

#[tokio::test]
async fn real_loss_is_still_blocked_after_valuing_holdings() {
    // Arrange
    let mut guard = RiskGuard::new(omnitrade_risk::RiskLimits::new(
        1_000_000_000.0,
        5.0,
        1_000_000_000.0,
    ));
    let req = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);
    place_order_with_risk_check(
        &trading_with(10_000.0, 0.0),
        &mut guard,
        &req,
        50_000.0,
        10_000.0,
    )
    .await
    .expect("first order passes");

    // Act: Preis fällt auf 40_000, 5_000 USDT + 0.1 BTC = 9_000, also 10 % Drawdown.
    let res = place_order_with_risk_check(
        &trading_with(5_000.0, 0.1),
        &mut guard,
        &req,
        40_000.0,
        10_000.0,
    )
    .await;

    // Assert
    assert!(
        matches!(res, Err(LiveLoopError::RiskViolation(_))),
        "{res:?}"
    );
}

#[tokio::test]
async fn perpetual_drawdown_ignores_foreign_account_movements() {
    // Laufkapital 50, Konto 140 -> 288 -> 246 durch FREMDE Bewegungen (31.10.2026).
    // Die eigene Position steht bei uPnL 0.
    let mut guard = RiskGuard::new(omnitrade_risk::RiskLimits::new(
        1_000_000_000.0,
        10.0,
        1_000_000_000.0,
    ));
    let req = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);
    for konto in [140.0, 288.0, 246.0] {
        let res = place_order_guarded(
            &trading_with(konto, 0.0),
            &mut guard,
            &req,
            50_000.0,
            50.0,
            0.0,
            Some("USDT"),
            None,
            None,
            Path::new(""),
        )
        .await;
        assert!(res.is_ok(), "Konto {konto}: {res:?}");
    }
}

#[tokio::test]
async fn perpetual_drawdown_still_blocks_own_unrealized_loss() {
    // Laufkapital 50, uPnL +4 dann -3 => Spitze 54, Stand 47 = 13 % Drawdown.
    let mut guard = RiskGuard::new(omnitrade_risk::RiskLimits::new(
        1_000_000_000.0,
        10.0,
        1_000_000_000.0,
    ));
    let req = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);
    let trading = trading_with(200.0, 0.0);
    *trading.pos_upl.lock().expect("upl") = (vec![4.0, -3.0], 0);
    place_order_guarded(
        &trading,
        &mut guard,
        &req,
        50_000.0,
        50.0,
        0.0,
        Some("USDT"),
        None,
        None,
        Path::new(""),
    )
    .await
    .expect("erste Order");
    let res = place_order_guarded(
        &trading,
        &mut guard,
        &req,
        50_000.0,
        50.0,
        0.0,
        Some("USDT"),
        None,
        None,
        Path::new(""),
    )
    .await;
    assert!(
        matches!(res, Err(LiveLoopError::RiskViolation(_))),
        "{res:?}"
    );
}

pub(super) fn perp(base_qty: f64, upl: f64) -> PositionSnapshot {
    PositionSnapshot {
        is_perpetual: true,
        base_qty,
        unrealized_pnl: upl,
        ..PositionSnapshot::default()
    }
}

pub(super) fn perp_with_liq(base_qty: f64, mark: f64, liq: Option<f64>) -> PositionSnapshot {
    PositionSnapshot {
        is_perpetual: true,
        base_qty,
        unrealized_pnl: 0.0,
        liq_price: liq,
        mark_price: Some(mark),
    }
}

#[test]
fn opening_or_growing_near_liquidation_is_rejected() {
    // Arrange: Long bei Mark 50 000, Liquidation 48 000 = 4 % Abstand
    let near = perp_with_liq(0.05, 50_000.0, Some(48_000.0));
    // Act + Assert
    assert!(check_liquidation_distance(&near, Side::Buy).is_err());
}

#[test]
fn reducing_near_liquidation_stays_allowed() {
    let near = perp_with_liq(0.05, 50_000.0, Some(48_000.0));
    assert!(check_liquidation_distance(&near, Side::Sell).is_ok());
}

#[test]
fn healthy_distance_allows_growing_position() {
    // 6 % Abstand über der Schwelle
    let healthy = perp_with_liq(0.05, 50_000.0, Some(47_000.0));
    assert!(check_liquidation_distance(&healthy, Side::Buy).is_ok());
}

#[test]
fn open_position_without_liquidation_price_fails_closed() {
    let missing = perp_with_liq(0.05, 50_000.0, None);
    assert!(check_liquidation_distance(&missing, Side::Buy).is_err());
}

#[test]
fn flat_position_passes_liquidation_check() {
    let flat = perp_with_liq(0.0, 50_000.0, None);
    assert!(check_liquidation_distance(&flat, Side::Buy).is_ok());
}

#[test]
fn unrealized_loss_reduces_equity_for_perpetuals() {
    // Arrange: 1000 USDT Guthaben, offene Long-Position mit -150 USDT
    let mut account = Account {
        balances: HashMap::new(),
    };
    account.balances.insert(
        "USDT".to_string(),
        AccountBalance {
            free: 1000.0,
            locked: 0.0,
        },
    );
    // Act + Assert
    let equity = account_equity(&account, "BTC", "USDT", 50_000.0, &perp(0.05, -150.0));
    assert!((equity - 850.0).abs() < 1e-9);
}

#[test]
fn perpetual_exposure_uses_position_not_spot_balance() {
    // Arrange: Spot-BTC-Bestand 2, Perp-Position 0.05 long
    let mut account = Account {
        balances: HashMap::new(),
    };
    account.balances.insert(
        "BTC".to_string(),
        AccountBalance {
            free: 2.0,
            locked: 0.0,
        },
    );
    // Act + Assert
    assert!((held_base(&account, "BTC", &perp(0.05, 0.0)) - 0.05).abs() < 1e-12);
    assert!((held_base(&account, "BTC", &PositionSnapshot::default()) - 2.0).abs() < 1e-12);
}

#[test]
fn split_symbol_accepts_perpetual_suffix() {
    let pair = |b: &str, q: &str| Some((b.to_string(), q.to_string()));
    assert_eq!(split_symbol("BTC-USDT-SWAP"), pair("BTC", "USDT"));
    assert_eq!(split_symbol("eth-usdt-swap"), pair("ETH", "USDT"));
}

#[test]
fn split_symbol_handles_all_exchange_notations() {
    let pair = |b: &str, q: &str| Some((b.to_string(), q.to_string()));
    assert_eq!(split_symbol("BTCUSDT"), pair("BTC", "USDT"));
    assert_eq!(split_symbol("btcusdc"), pair("BTC", "USDC"));
    assert_eq!(split_symbol("ETHBTC"), pair("ETH", "BTC"));
    assert_eq!(split_symbol("BNBUSDT"), pair("BNB", "USDT"));
    assert_eq!(split_symbol("USDCUSDT"), pair("USDC", "USDT"));
    assert_eq!(split_symbol("BTC-USDT"), pair("BTC", "USDT"));
    assert_eq!(split_symbol("btc/usd"), pair("BTC", "USD"));
    assert_eq!(split_symbol("BTCEUR"), pair("BTC", "EUR"));
}

#[test]
fn split_symbol_handles_xperp_and_dated_futures() {
    let pair = |b: &str, q: &str| Some((b.to_string(), q.to_string()));
    // Regression: Live-Lauf 21.10.2026 - der RiskGuard-Pfad konnte
    // SOL-USD_UM_XPERP-310404 nicht zerlegen und lehnte jede Buy-Order ab.
    assert_eq!(split_symbol("SOL-USD_UM_XPERP-310404"), pair("SOL", "USD"));
    assert_eq!(split_symbol("sol-usd_um_xperp-310404"), pair("SOL", "USD"));
    assert_eq!(split_symbol("BTC-USD-261030"), pair("BTC", "USD"));
    // Missbildungen bleiben abgelehnt (fail closed).
    assert_eq!(split_symbol("SOL-USD_UM_XPERP-3104"), None);
    assert_eq!(split_symbol("SOL-USD_UM_XPERP-ABCD30"), None);
    assert_eq!(split_symbol("-USD_UM_XPERP-310404"), None);
}

#[test]
fn split_symbol_rejects_unsplittable_input() {
    for bad in [
        "",
        "USDT",
        "BTC-",
        "-USDT",
        "A-B-C",
        "BTCXYZ",
        "BTC/USD/EUR",
    ] {
        assert_eq!(split_symbol(bad), None, "{bad}");
    }
}

#[test]
fn pair_equity_values_base_holdings_at_price() {
    // Arrange
    let account = Account {
        balances: HashMap::from([
            (
                "USDT".to_string(),
                AccountBalance {
                    free: 5_000.0,
                    locked: 1_000.0,
                },
            ),
            (
                "BTC".to_string(),
                AccountBalance {
                    free: 0.1,
                    locked: 0.05,
                },
            ),
            (
                "ETH".to_string(),
                AccountBalance {
                    free: 99.0,
                    locked: 0.0,
                },
            ),
        ]),
    };

    // Act & Assert: fremde Assets (ETH) zählen nicht, gesperrte Bestände schon.
    assert!((pair_equity(&account, "BTC", "USDT", 50_000.0) - 13_500.0).abs() < 1e-9);
    assert!((pair_equity(&account, "SOL", "USDT", 50_000.0) - 6_000.0).abs() < 1e-9);
}

#[tokio::test]
async fn invalid_price_or_symbol_is_rejected_fail_closed() {
    // Arrange
    let mut guard = RiskGuard::testnet();
    let ok = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);
    let bad_symbol = OrderRequest::market("FOO".into(), Side::Buy, 0.0001);
    let trading = trading_with(10_000.0, 0.0);

    // Act & Assert
    for price in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        let res = place_order_with_risk_check(&trading, &mut guard, &ok, price, 1000.0).await;
        assert!(
            matches!(res, Err(LiveLoopError::RiskViolation(_))),
            "{price}"
        );
    }
    let res =
        place_order_with_risk_check(&trading, &mut guard, &bad_symbol, 50_000.0, 1000.0).await;
    assert!(
        matches!(res, Err(LiveLoopError::ExchangeError(_))),
        "{res:?}"
    );
    assert!(trading.placed.lock().expect("placed").is_empty());
}

#[tokio::test]
async fn loss_limit_uses_equity_at_run_start_not_declared_capital() {
    // Arrange: deklariertes Startkapital 10_000, das Konto hält aber 93_000.
    let limits = omnitrade_risk::RiskLimits::new(1_000_000_000.0, 100.0, 2_000.0);
    let mut guard = RiskGuard::new(limits).with_run_baseline();
    let req = OrderRequest::market("BTCUSDT".into(), Side::Buy, 0.0001);
    place_order_with_risk_check(
        &trading_with(93_000.0, 0.0),
        &mut guard,
        &req,
        50_000.0,
        10_000.0,
    )
    .await
    .expect("first order sets the baseline");

    // Act: 3_000 Verlust seit Laufbeginn (> 2_000), obwohl 90_000 weit über 10_000 liegt.
    let res = place_order_with_risk_check(
        &trading_with(90_000.0, 0.0),
        &mut guard,
        &req,
        50_000.0,
        10_000.0,
    )
    .await;

    // Assert
    assert!(
        matches!(res, Err(LiveLoopError::RiskViolation(_))),
        "{res:?}"
    );
}

#[test]
fn adopt_existing_counts_startbestand_als_laufposition() {
    // Bestand 0.2 bei Start; ohne Adopt gilt er als fremd, mit Adopt zaehlt er.
    let mut fremd = ExposureLimit::new(25.0).allow_shorts(true);
    assert!(fremd.check(Side::Buy, 0.1, 95.0, 0.2).is_ok());
    let mut eigen = ExposureLimit::new(25.0)
        .allow_shorts(true)
        .adopt_existing(true);
    // 0.2 + 0.1 = 0.3 * 95 = 28.5 > 25
    assert!(eigen.check(Side::Buy, 0.1, 95.0, 0.2).is_err());
    // Sell reduziert die uebernommene Position, kein Short.
    let mut eigen = ExposureLimit::new(25.0)
        .allow_shorts(true)
        .adopt_existing(true);
    assert!(eigen.check(Side::Sell, 0.1, 95.0, 0.2).is_ok());
}

#[test]
fn exposure_cap_limits_what_the_run_builds_up() {
    // Arrange: Deckel 200, Preis 100 → höchstens 2 Einheiten Position aus diesem Lauf.
    let mut limit = ExposureLimit::new(200.0);

    // Act & Assert
    assert!(limit.check(Side::Buy, 1.0, 100.0, 5.0).is_ok());
    assert!(
        limit.check(Side::Buy, 1.0, 100.0, 6.0).is_ok(),
        "zweite Einheit: 200"
    );
    let over = limit.check(Side::Buy, 1.0, 100.0, 7.0);
    assert!(
        matches!(&over, Err(m) if m.contains("Positionsdeckel")),
        "{over:?}"
    );
}

#[test]
fn exposure_cap_ignores_preexisting_holdings_and_never_sells_them() {
    // Arrange: das Konto hält schon 5 Einheiten, bevor der Lauf beginnt.
    let mut limit = ExposureLimit::new(150.0);

    // Act & Assert: Kaufen zählt ab dem Startbestand, nicht ab 0.
    assert!(limit.check(Side::Buy, 1.0, 100.0, 5.0).is_ok());
    // Verkaufen vor dem ersten Kauf: Lauf besitzt nichts, der Bestand bleibt tabu.
    let mut fresh = ExposureLimit::new(150.0);
    let sell = fresh.check(Side::Sell, 1.0, 100.0, 5.0);
    assert!(
        matches!(&sell, Err(m) if m.contains("übersteigt")),
        "{sell:?}"
    );
    // Nach dem Kauf (Bestand 6) darf die gekaufte Einheit zurück, nicht mehr.
    assert!(limit.check(Side::Sell, 1.0, 100.0, 6.0).is_ok());
    assert!(limit.check(Side::Sell, 2.0, 100.0, 6.0).is_err());
}

#[test]
fn exposure_cap_sell_tolerates_fee_shortfall() {
    // Arrange: 0.001 gekauft, Gebühr in der Basis → Bestand nur 0.000999.
    let mut limit = ExposureLimit::new(1_000.0);
    limit.check(Side::Buy, 0.001, 50_000.0, 1.0).expect("buy");

    // Act & Assert
    assert!(limit.check(Side::Sell, 0.001, 50_000.0, 1.000999).is_ok());
}

#[test]
fn exposure_cap_rejects_shorts_by_default() {
    // Arrange: flacher Start, Standard-Lauf (long-only, Haussatz C.2).
    let mut limit = ExposureLimit::new(200.0);

    // Act: Sell bei flacher Position = Short-Eröffnung.
    let err = limit.check(Side::Sell, 1.0, 100.0, 0.0);

    // Assert
    assert!(
        matches!(&err, Err(m) if m.contains("übersteigt")),
        "{err:?}"
    );
}

#[test]
fn exposure_cap_allows_shorts_when_enabled() {
    let mut limit = ExposureLimit::new(200.0).allow_shorts(true);

    // Short-Eröffnung: 1 @ 100 = 100 ≤ 200.
    assert!(limit.check(Side::Sell, 1.0, 100.0, 0.0).is_ok());
    // Short vergrößern: 200 ≤ 200.
    assert!(limit.check(Side::Sell, 1.0, 100.0, -1.0).is_ok());
    // Dritter Short: 300 > 200 → Deckel in beide Richtungen.
    let over = limit.check(Side::Sell, 1.0, 100.0, -2.0);
    assert!(
        matches!(&over, Err(m) if m.contains("Positionsdeckel")),
        "{over:?}"
    );

    // Buy schließt den Short (Reduktion ist immer erlaubt).
    let mut back = ExposureLimit::new(200.0).allow_shorts(true);
    back.check(Side::Sell, 1.0, 100.0, 0.0).expect("short");
    assert!(back.check(Side::Buy, 1.0, 100.0, -1.0).is_ok());
}

#[test]
fn exposure_cap_allows_flipping_own_long_into_short_when_enabled() {
    // Ein Lauf, der 1 gekauft hat, darf sie schließen und ins Short drehen (Deckel 300).
    let mut limit = ExposureLimit::new(300.0).allow_shorts(true);
    assert!(limit.check(Side::Buy, 1.0, 100.0, 0.0).is_ok());
    // Close + Short: 2 @ 100, Bestand 1 → Laufposition -1 = 100 ≤ 300.
    assert!(limit.check(Side::Sell, 2.0, 100.0, 1.0).is_ok());
    // Ohne erlaubte Shorts bliebe derselbe Flip abgelehnt.
    let mut long_only = ExposureLimit::new(300.0);
    long_only.check(Side::Buy, 1.0, 100.0, 0.0).expect("buy");
    let flip = long_only.check(Side::Sell, 2.0, 100.0, 1.0);
    assert!(
        matches!(&flip, Err(m) if m.contains("übersteigt")),
        "{flip:?}"
    );
}
