//! Tests der Risiko-Helfer: Eigenkapital, Liquidationsabstand, Positionsdeckel, Mainnet-Cap.

use super::risk::*;
use omnitrade_core::Side;
use omnitrade_exchange::{Account, AccountBalance, PositionSnapshot};

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

#[test]
fn realisierter_pnl_beim_schliessen_folgt_dem_geschlossenen_anteil() {
    use omnitrade_exchange::PositionSnapshot;
    let pos = |qty: f64, upl: f64| PositionSnapshot {
        is_perpetual: true,
        base_qty: qty,
        unrealized_pnl: upl,
        liq_price: None,
        mark_price: None,
    };
    // Ganz geschlossen: der gesamte uPnL ist realisiert.
    assert_eq!(
        realisiert_beim_schliessen(&pos(2.0, 20.0), &pos(0.0, 0.0)),
        20.0
    );
    // Halb geschlossen: die Haelfte.
    assert_eq!(
        realisiert_beim_schliessen(&pos(2.0, 20.0), &pos(1.0, 10.0)),
        10.0
    );
    // Vergroessert: nichts realisiert.
    assert_eq!(
        realisiert_beim_schliessen(&pos(2.0, 20.0), &pos(3.0, 30.0)),
        0.0
    );
    // Vorzeichenwechsel schliesst die alte Position ganz.
    assert_eq!(
        realisiert_beim_schliessen(&pos(2.0, 20.0), &pos(-1.0, -5.0)),
        20.0
    );
    // Flach vorher: nichts zu realisieren.
    assert_eq!(
        realisiert_beim_schliessen(&pos(0.0, 0.0), &pos(1.0, 0.0)),
        0.0
    );
}
