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

fn ausfuehrung(fee: f64, ccy: &str) -> omnitrade_exchange::FillDetails {
    omnitrade_exchange::FillDetails {
        filled: 0.1,
        avg_price: 100.0,
        fee,
        fee_ccy: ccy.to_string(),
    }
}

#[test]
fn gebuehr_in_abrechnungs_oder_quote_waehrung_zaehlt_eins_zu_eins() {
    // XPERP: Symbol sagt USD, das Konto rechnet in USDC ab.
    let usdc = gebuehr_in_abrechnung(&ausfuehrung(-0.02, "USDC"), "SOL", "USD", Some("USDC"));
    assert_eq!(usdc, Some(-0.02));
    let quote = gebuehr_in_abrechnung(&ausfuehrung(-0.02, "usdt"), "BTC", "USDT", None);
    assert_eq!(quote, Some(-0.02));
    // Ein Rabatt (positiv) erhoeht das Laufeigenkapital.
    let rabatt = gebuehr_in_abrechnung(&ausfuehrung(0.01, "USDC"), "SOL", "USD", Some("USDC"));
    assert_eq!(rabatt, Some(0.01));
}

#[test]
fn gebuehr_in_der_basis_wird_zum_fuellpreis_umgerechnet() {
    // -0.0001 SOL zu 100 = -0.01 in der Abrechnungswaehrung.
    let wert = gebuehr_in_abrechnung(&ausfuehrung(-0.0001, "SOL"), "SOL", "USD", Some("USDC"))
        .expect("Basis ist umrechenbar");
    assert!((wert + 0.01).abs() < 1e-12, "{wert}");
}

#[test]
fn gebuehr_in_fremder_waehrung_oder_ohne_zahl_ist_unbekannt() {
    let fremd = gebuehr_in_abrechnung(&ausfuehrung(-1.0, "OKB"), "SOL", "USD", Some("USDC"));
    assert_eq!(fremd, None, "OKB laesst sich hier nicht bewerten");
    let nan = gebuehr_in_abrechnung(&ausfuehrung(f64::NAN, "USDC"), "SOL", "USD", Some("USDC"));
    assert_eq!(nan, None);
    // Gebuehr 0 ist bekannt, auch ohne Waehrung.
    let null = gebuehr_in_abrechnung(&ausfuehrung(0.0, ""), "SOL", "USD", Some("USDC"));
    assert_eq!(null, Some(0.0));
}

#[test]
fn perp_gebuehr_ohne_ausfuehrungsdaten_bucht_nichts() {
    assert_eq!(
        perp_gebuehr(None, "SOL-USD_UM_XPERP-310404", Some("USDC")),
        0.0
    );
    let fremd = ausfuehrung(-1.0, "OKB");
    assert_eq!(
        perp_gebuehr(Some(&fremd), "SOL-USD_UM_XPERP-310404", Some("USDC")),
        0.0
    );
    let ok = ausfuehrung(-0.02, "USDC");
    assert_eq!(
        perp_gebuehr(Some(&ok), "SOL-USD_UM_XPERP-310404", Some("USDC")),
        -0.02
    );
}

/// Codex-P2 auf PR #1: Gebuehren gehoeren ins Laufeigenkapital. Drei Auftraege
/// mit je 150 Gebuehr auf 1000 Laufkapital: vor dem dritten steht der Lauf bei
/// 700, das sind 30 % Drawdown gegen die 25 % des Testnets. Ohne Gebuehren
/// gehen alle drei durch.
#[tokio::test]
async fn gebuehren_senken_das_laufeigenkapital_bis_zur_drawdown_bremse() {
    use super::test_support::{Harness, SELL_ABOVE_40K};
    use omnitrade_exchange::Network;

    let lauf = |name: &str, gebuehr: f64| {
        Harness::new(name, SELL_ABOVE_40K)
            .with_allow_shorts()
            .with_symbol("SOL-USD_UM_XPERP-310404")
            .with_settle("USDC")
            .with_konto_waehrung("USDC")
            .with_gebuehr(gebuehr, "USDC")
    };
    let kerzen = [45_000.0, 46_000.0, 47_000.0];

    let ohne = lauf("gebuehr-ohne", 0.0);
    let res = ohne
        .run(&kerzen, Some(1000.0), Network::Testnet, 0.001)
        .await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(ohne.placed.lock().expect("placed").len(), 3);

    let mit = lauf("gebuehr-mit", -150.0);
    let res = mit
        .run(&kerzen, Some(1000.0), Network::Testnet, 0.001)
        .await;
    assert!(res.is_ok(), "{res:?}");
    assert_eq!(
        mit.placed.lock().expect("placed").len(),
        2,
        "der dritte Auftrag muss an der Drawdown-Bremse scheitern"
    );
}
