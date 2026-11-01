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

fn perp(base_qty: f64, upl: f64) -> PositionSnapshot {
    PositionSnapshot {
        is_perpetual: true,
        base_qty,
        unrealized_pnl: upl,
        ..PositionSnapshot::default()
    }
}

fn perp_with_liq(base_qty: f64, mark: f64, liq: Option<f64>) -> PositionSnapshot {
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

#[test]
fn mainnet_cap_validation() {
    assert!(valid_mainnet_cap(Some(20.0)));
    assert!(valid_mainnet_cap(Some(MAINNET_HARD_CAP)));
    for bad in [
        None,
        Some(0.0),
        Some(-1.0),
        Some(MAINNET_HARD_CAP + 0.01),
        Some(f64::NAN),
        Some(f64::INFINITY),
    ] {
        assert!(!valid_mainnet_cap(bad), "{bad:?}");
    }
}

#[tokio::test]
async fn mainnet_with_cap_above_hard_limit_is_rejected_before_any_exchange_call() {
    // Arrange
    let h = Harness::new("mainnet-over", BUY_BELOW_40K).with_cap(MAINNET_HARD_CAP + 1.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Mainnet, 0.001)
        .await;

    // Assert
    assert!(matches!(result, Err(LiveLoopError::NetworkError(_))));
    assert!(h.log.lock().expect("log").is_empty());
}

#[tokio::test]
async fn mainnet_with_valid_cap_runs_and_blocks_orders_over_the_cap() {
    // Arrange: Deckel 20, Order 0.001 BTC @ 35000 = 35 > 20 → blockiert, kein Aufruf an die Börse.
    let h = Harness::new("mainnet-cap", BUY_BELOW_40K).with_cap(20.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Mainnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert!(
        h.placed.lock().expect("placed").is_empty(),
        "Order über Deckel darf nicht raus"
    );
}

#[tokio::test]
async fn testnet_cap_allows_orders_within_the_cap() {
    // Arrange: Deckel 50, Order 0.001 @ 35000 = 35 ≤ 50.
    let h = Harness::new("testnet-cap", BUY_BELOW_40K).with_cap(50.0);

    // Act
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;

    // Assert
    assert!(result.is_ok(), "{result:?}");
    assert_eq!(h.placed.lock().expect("placed").len(), 1);
}

#[tokio::test]
async fn non_positive_cap_is_rejected_even_on_testnet() {
    let h = Harness::new("cap-zero", BUY_BELOW_40K).with_cap(0.0);
    let result = h
        .run(&[35000.0], Some(1000.0), Network::Testnet, 0.001)
        .await;
    assert!(matches!(result, Err(LiveLoopError::NetworkError(_))));
}

#[test]
fn long_only_rejects_sell_without_position() {
    // Ohne Deckel und ohne vorherigen Kauf darf ein Sell keine Kurzposition oeffnen.
    let mut guard = LongOnlyGuard::new();
    let r = guard.check(Side::Sell, 0.19, 0.0);
    assert!(
        r.is_err(),
        "Sell bei flacher Position muss abgelehnt werden: {r:?}"
    );
}

#[test]
fn long_only_allows_sell_of_the_run_position() {
    // Erst kaufen (Lauf-Baseline 0), dann genau die gekaufte Menge verkaufen.
    let mut guard = LongOnlyGuard::new();
    assert!(guard.check(Side::Buy, 5.0, 0.0).is_ok());
    assert!(guard.check(Side::Sell, 5.0, 5.0).is_ok());
}

#[test]
fn liquidation_beim_preis_null_sperrt_das_nachlegen_nicht() {
    // OKX meldet liqPx "0", wenn es keine Liquidation gibt (1x isolated).
    // Vorher machte der Parser daraus None und der Guard verweigerte JEDES
    // Nachlegen ("Liquidationspreis oder Markpreis fehlt", Befund 21.10.2026).
    let ohne_liq = PositionSnapshot {
        is_perpetual: true,
        base_qty: 0.1,
        unrealized_pnl: 0.0,
        liq_price: Some(0.0),
        mark_price: Some(108.27),
    };
    assert!(
        check_liquidation_distance(&ohne_liq, Side::Buy).is_ok(),
        "ohne Liquidationspreis ist der Abstand 100 % - Nachlegen muss erlaubt sein"
    );

    // Gegenprobe: eine echte Liquidation dicht am Markpreis sperrt weiterhin.
    let nah = PositionSnapshot {
        is_perpetual: true,
        base_qty: 0.1,
        unrealized_pnl: 0.0,
        liq_price: Some(106.0),
        mark_price: Some(108.27),
    };
    assert!(
        check_liquidation_distance(&nah, Side::Buy).is_err(),
        "2,1 % Abstand zur Liquidation muss abgelehnt werden"
    );
}

#[test]
fn long_only_never_sells_pre_existing_stock() {
    // Vorbestand 3: er zaehlt nicht zur Laufposition, ein Verkauf daraus scheitert.
    let mut guard = LongOnlyGuard::new();
    assert!(guard.check(Side::Buy, 1.0, 3.0).is_ok());
    let r = guard.check(Side::Sell, 4.0, 4.0);
    assert!(r.is_err(), "Vorbestand darf nicht verkauft werden: {r:?}");
}

/// XPERP traegt im Symbol `USD`, das Konto fuehrt USDC. Wer die Quote nimmt,
/// findet keinen Bestand, bewertet das Konto mit 0 und der RiskGuard misst einen
/// Drawdown von Millionen Prozent (gemessen 21.10.2026: 4.688.672 %).
#[test]
fn equity_nimmt_die_abrechnungswaehrung_wenn_die_quote_fehlt() {
    let konto = |paare: &[(&str, f64)]| Account {
        balances: paare
            .iter()
            .map(|(ccy, wert)| {
                (
                    (*ccy).to_string(),
                    AccountBalance {
                        free: *wert,
                        locked: 0.0,
                    },
                )
            })
            .collect(),
    };
    let xperp = konto(&[("USDC", 46.43), ("SOL", 0.0)]);

    // Act: Bewertungswaehrung und Equity eines offenen Shorts.
    let waehrung = bewertungs_waehrung(&xperp, "USD", Some("USDC"));
    let equity = account_equity(
        &xperp,
        "SOL",
        waehrung,
        106.46,
        &PositionSnapshot {
            is_perpetual: true,
            base_qty: -0.1,
            unrealized_pnl: -0.13,
            liq_price: None,
            mark_price: None,
        },
    );

    // Assert: der Kontobestand zaehlt, nicht die leere Quote.
    assert_eq!(waehrung, "USDC");
    assert!(
        (equity - 46.30).abs() < 1e-9,
        "Equity muss 46.43 USDC + (-0.13) uPnL sein, war {equity}"
    );

    // Gegenprobe 1: traegt das Konto die Quote, bleibt sie die Bewertungswaehrung.
    let swap = konto(&[("USDT", 100.0)]);
    assert_eq!(bewertungs_waehrung(&swap, "USDT", Some("USDT")), "USDT");
    // Gegenprobe 2: weder Quote noch Abrechnung vorhanden -> alte Regel, kein Panikwert.
    assert_eq!(bewertungs_waehrung(&swap, "USD", Some("USDC")), "USD");
}
